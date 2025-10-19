use std::{
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc},
};

use anyhow::{anyhow, Context, Result};
use futures_util::{SinkExt, StreamExt};
use log::Level;
use tokio::{
    sync::{broadcast, mpsc},
    task::JoinHandle,
    time::{sleep, Duration},
};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};
use tokio_tungstenite::{connect_async, Connector, MaybeTlsStream};
use tokio_util::sync::CancellationToken;
use url::Url;
use uuid::Uuid;

type WsStream = tokio_tungstenite::WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

pub mod clipboard;
pub mod config;
pub mod messages;

use clipboard::{start_clipboard_monitor, start_clipboard_setter};
use config::Config;
use messages::{AuthResponse, ClipboardBroadcast, ClipboardBroadcastPayload, ClipboardUpdate};

#[derive(Debug, Clone)]
pub enum ConnectionStateEvent {
    Idle,
    Connecting,
    Connected,
    Reconnecting,
    Disconnected,
    Paused,
}

#[derive(Debug, Clone)]
pub struct RuntimeLogEvent {
    pub level: Level,
    pub message: String,
}

impl RuntimeLogEvent {
    pub fn new(level: Level, message: impl Into<String>) -> Self {
        Self {
            level,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    Status(String),
    Connection(ConnectionStateEvent),
    Log(RuntimeLogEvent),
    ClipboardSent { content_type: String },
    ClipboardReceived { content_type: String },
    Error(String),
}

#[derive(Debug, Clone)]
pub struct StartOptions {
    pub config_dir: PathBuf,
    pub insecure_override: bool,
}

enum RuntimeCommand {
    Start(StartOptions),
    Reload(StartOptions),
    Pause,
    Resume,

    Shutdown,
}

#[derive(Clone)]
pub struct RuntimeHandle {
    command_tx: mpsc::Sender<RuntimeCommand>,
}

impl RuntimeHandle {
    pub async fn start(&self, options: StartOptions) -> Result<()> {
        self.command_tx
            .send(RuntimeCommand::Start(options))
            .await
            .context("发送启动命令失败")
    }

    pub async fn pause(&self) -> Result<()> {
        self.command_tx
            .send(RuntimeCommand::Pause)
            .await
            .context("发送暂停命令失败")
    }

    pub async fn resume(&self) -> Result<()> {
        self.command_tx
            .send(RuntimeCommand::Resume)
            .await
            .context("发送恢复命令失败")
    }

    pub async fn reload(&self, options: StartOptions) -> Result<()> {
        self.command_tx
            .send(RuntimeCommand::Reload(options))
            .await
            .context("发送重新加载命令失败")
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.command_tx
            .send(RuntimeCommand::Shutdown)
            .await
            .context("发送关闭命令失败")
    }
}

pub fn spawn_runtime(
    runtime: &Arc<tokio::runtime::Runtime>,
) -> (RuntimeHandle, mpsc::Receiver<RuntimeEvent>) {
    let (command_tx, command_rx) = mpsc::channel(32);
    let (event_tx, event_rx) = mpsc::channel(512);

    let mut worker = RuntimeWorker::new(event_tx);
    runtime.spawn(async move {
        worker.run(command_rx).await;
    });

    (RuntimeHandle { command_tx }, event_rx)
}

struct RuntimeWorker {
    events: mpsc::Sender<RuntimeEvent>,
    active: Option<ActiveTasks>,
    last_options: Option<StartOptions>,
    paused: bool,
}

struct ActiveTasks {
    cancel: CancellationToken,
    monitor_handle: JoinHandle<()>,
    setter_handle: JoinHandle<()>,
    connection_handle: JoinHandle<()>,
}

impl RuntimeWorker {
    fn new(events: mpsc::Sender<RuntimeEvent>) -> Self {
        Self {
            events,
            active: None,
            last_options: None,
            paused: true,
        }
    }

    async fn run(&mut self, mut commands: mpsc::Receiver<RuntimeCommand>) {
        self.emit_connection(ConnectionStateEvent::Idle).await;
        while let Some(cmd) = commands.recv().await {
            match cmd {
                RuntimeCommand::Start(options) => {
                    self.last_options = Some(options.clone());
                    if let Err(err) = self.start_tasks(options).await {
                        self.emit_error(format!("启动失败: {}", err)).await;
                    }
                }
                RuntimeCommand::Pause => {
                    self.paused = true;
                    self.stop_tasks(false).await;
                    self.emit_status("已暂停").await;
                    self.emit_connection(ConnectionStateEvent::Paused).await;
                }
                RuntimeCommand::Resume => {
                    if self.active.is_some() {
                        self.emit_status("已在运行").await;
                        continue;
                    }
                    if let Some(options) = self.last_options.clone() {
                        if let Err(err) = self.start_tasks(options).await {
                            self.emit_error(format!("恢复失败: {}", err)).await;
                        }
                    } else {
                        self.emit_error("尚未配置，无法恢复".into()).await;
                    }
                }

                RuntimeCommand::Reload(options) => {
                    self.emit_status("正在应用新配置").await;
                    self.stop_tasks(true).await;
                    self.last_options = Some(options.clone());
                    if let Err(err) = self.start_tasks(options).await {
                        self.emit_error(format!("重新加载失败: {}", err)).await;
                    }
                }
                RuntimeCommand::Shutdown => {
                    self.stop_tasks(true).await;
                    break;
                }
            }
        }
    }

    async fn start_tasks(&mut self, options: StartOptions) -> Result<()> {
        if self.active.is_some() {
            return Ok(());
        }
        self.paused = false;

        let cfg = Config::load_from_dir(&options.config_dir)?;
        let server_url = Url::parse(&cfg.server_url)
            .with_context(|| format!("无法解析服务器地址: {}", cfg.server_url))?;
        let insecure_effective = cfg.trust_insecure_cert || options.insecure_override;
        if insecure_effective && server_url.scheme() == "wss" {
            self.emit_log(Level::Warn, "TLS 证书验证已禁用").await;
        }

        self.emit_status("正在连接...").await;
        self.emit_connection(ConnectionStateEvent::Connecting).await;

        let disable_flag = Arc::new(AtomicBool::new(false));
        let device_id = Uuid::new_v4().to_string();
        let (tx_out, _) = broadcast::channel::<ClipboardUpdate>(100);
        let (tx_in, rx_in) = mpsc::channel::<ClipboardBroadcastPayload>(100);
        let cancel = CancellationToken::new();

        let monitor_events = self.events.clone();
        let monitor_cancel = cancel.clone();
        let monitor_disable = disable_flag.clone();
        let monitor_device = device_id.clone();
        let monitor_cfg = cfg.max_image_kb;
        let tx_out_for_monitor = tx_out.clone();
        let monitor_handle = tokio::spawn(async move {
            start_clipboard_monitor(
                tx_out_for_monitor,
                monitor_disable,
                monitor_device,
                monitor_cfg,
                monitor_events,
                monitor_cancel,
            )
            .await;
        });

        let setter_events = self.events.clone();
        let setter_cancel = cancel.clone();
        let setter_disable = disable_flag.clone();
        let setter_handle = tokio::spawn(async move {
            start_clipboard_setter(rx_in, setter_disable, setter_events, setter_cancel).await;
        });

        let connection_events = self.events.clone();
        let connection_cancel = cancel.clone();
        let cfg_clone = cfg.clone();
        let server_url_clone = server_url.clone();
        let connection_handle = tokio::spawn(async move {
            run_connection_loop(
                cfg_clone,
                server_url_clone,
                tx_out,
                tx_in,
                connection_events,
                connection_cancel,
                insecure_effective,
            )
            .await;
        });

        self.active = Some(ActiveTasks {
            cancel,
            monitor_handle,
            setter_handle,
            connection_handle,
        });
        Ok(())
    }

    async fn stop_tasks(&mut self, hard: bool) {
        if let Some(active) = self.active.take() {
            let ActiveTasks {
                cancel,
                monitor_handle,
                setter_handle,
                connection_handle,
            } = active;
            cancel.cancel();
            if hard {
                monitor_handle.abort();
                setter_handle.abort();
                connection_handle.abort();
            } else {
                let _ = monitor_handle.await;
                let _ = setter_handle.await;
                let _ = connection_handle.await;
            }
        }
    }

    async fn emit_status(&self, text: impl Into<String>) {
        let _ = self.events.send(RuntimeEvent::Status(text.into())).await;
    }

    async fn emit_connection(&self, state: ConnectionStateEvent) {
        let _ = self.events.send(RuntimeEvent::Connection(state)).await;
    }

    async fn emit_error(&self, message: String) {
        let _ = self.events.send(RuntimeEvent::Error(message.clone())).await;
        self.emit_log(Level::Error, message).await;
    }

    async fn emit_log(&self, level: Level, message: impl Into<String>) {
        let _ = self
            .events
            .send(RuntimeEvent::Log(RuntimeLogEvent::new(level, message)))
            .await;
    }
}

async fn run_connection_loop(
    cfg: Config,
    server_url: Url,
    tx_out: broadcast::Sender<ClipboardUpdate>,
    tx_in: mpsc::Sender<ClipboardBroadcastPayload>,
    events: mpsc::Sender<RuntimeEvent>,
    cancel: CancellationToken,
    insecure_effective: bool,
) {
    let reconnect_delay = Duration::from_secs(5);
    let _ = events
        .send(RuntimeEvent::Log(RuntimeLogEvent::new(
            Level::Info,
            format!("目标服务器: {}", server_url),
        )))
        .await;

    while !cancel.is_cancelled() {
        if cancel.is_cancelled() {
            break;
        }
        let _ = events
            .send(RuntimeEvent::Connection(ConnectionStateEvent::Connecting))
            .await;
        let _ = events
            .send(RuntimeEvent::Status("正在连接服务器".into()))
            .await;

        let connection = if server_url.scheme() == "wss" && insecure_effective {
            connect_insecure(server_url.clone()).await
        } else {
            connect_async(server_url.as_str())
                .await
                .map(|(stream, _)| stream)
        };

        match connection {
            Ok(mut ws_stream) => {
                let _ = events
                    .send(RuntimeEvent::Log(RuntimeLogEvent::new(
                        Level::Info,
                        "WebSocket 已连接",
                    )))
                    .await;
                let _ = events
                    .send(RuntimeEvent::Connection(ConnectionStateEvent::Connected))
                    .await;
                let _ = events.send(RuntimeEvent::Status("已连接".into())).await;

                if let Err(err) = authenticate_stream(&cfg, &mut ws_stream, &events).await {
                    let _ = events
                        .send(RuntimeEvent::Error(format!("认证失败: {}", err)))
                        .await;
                    let _ = events
                        .send(RuntimeEvent::Connection(ConnectionStateEvent::Disconnected))
                        .await;
                    if cancel.is_cancelled() {
                        break;
                    }
                    sleep(Duration::from_secs(3)).await;
                    continue;
                }

                let (mut write, mut read) = ws_stream.split();

                let mut rx_updates = tx_out.subscribe();
                let tx_in_clone = tx_in.clone();

                loop {
                    tokio::select! {
                        _ = cancel.cancelled() => {
                            let _ = events.send(RuntimeEvent::Log(RuntimeLogEvent::new(Level::Debug, String::from("连接任务取消")))).await;
                            break;
                        }
                        outbound = rx_updates.recv() => {
                            match outbound {
                                Ok(update) => {
                                    if let Ok(text) = serde_json::to_string(&update) {
                                        if let Err(err) = write.send(Message::Text(text.into())).await {
                                            let _ = events.send(RuntimeEvent::Log(RuntimeLogEvent::new(Level::Error, format!("发送失败: {}", err)))).await;
                                            break;
                                        }
                                    }
                                }
                                Err(_) => break,
                            }
                        }
                        incoming = read.next() => {
                            match incoming {
                                Some(Ok(Message::Text(text))) => {
                                    let handled = if let Ok(broadcast) = serde_json::from_str::<ClipboardBroadcast>(&text) {
                                        let _ = tx_in_clone.send(broadcast.payload).await;
                                        true
                                    } else if let Ok(payload) = serde_json::from_str::<ClipboardBroadcastPayload>(&text) {
                                        let _ = tx_in_clone.send(payload).await;
                                        true
                                    } else {
                                        false
                                    };
                                    if !handled {
                                        let _ = events.send(RuntimeEvent::Log(RuntimeLogEvent::new(Level::Warn, format!("未识别的广播: {}", text)))).await;
                                    } else {
                                        let _ = events.send(RuntimeEvent::Log(RuntimeLogEvent::new(Level::Debug, format!("收到广播: {}", text.len())))).await;
                                    }
                                }
                                Some(Ok(other)) => {
                                    let _ = events.send(RuntimeEvent::Log(RuntimeLogEvent::new(Level::Warn, format!("忽略非文本消息: {:?}", other)))).await;
                                }
                                Some(Err(err)) => {
                                    let _ = events.send(RuntimeEvent::Log(RuntimeLogEvent::new(Level::Error, format!("读取失败: {}", err)))).await;
                                    break;
                                }
                                None => {
                                    break;
                                }
                            }
                        }
                    }
                }

                let _ = events
                    .send(RuntimeEvent::Connection(ConnectionStateEvent::Disconnected))
                    .await;
                let _ = events
                    .send(RuntimeEvent::Status("连接已断开，准备重试".into()))
                    .await;
            }
            Err(err) => {
                let _ = events
                    .send(RuntimeEvent::Log(RuntimeLogEvent::new(
                        Level::Error,
                        format!("连接失败: {}", err),
                    )))
                    .await;
                let _ = events
                    .send(RuntimeEvent::Connection(ConnectionStateEvent::Disconnected))
                    .await;
            }
        }

        if cancel.is_cancelled() {
            break;
        }

        let _ = events
            .send(RuntimeEvent::Connection(ConnectionStateEvent::Reconnecting))
            .await;
        let _ = events
            .send(RuntimeEvent::Status(format!(
                "{} 秒后重连",
                reconnect_delay.as_secs()
            )))
            .await;

        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = sleep(reconnect_delay) => {}
        }
    }

    if cancel.is_cancelled() {
        let _ = events
            .send(RuntimeEvent::Connection(ConnectionStateEvent::Paused))
            .await;
        let _ = events.send(RuntimeEvent::Status("已暂停".into())).await;
    } else {
        let _ = events
            .send(RuntimeEvent::Connection(ConnectionStateEvent::Disconnected))
            .await;
    }
}

async fn authenticate_stream(
    cfg: &Config,
    stream: &mut WsStream,
    events: &mpsc::Sender<RuntimeEvent>,
) -> Result<()> {
    let auth_json = if let Some(token) = cfg.token.clone() {
        serde_json::json!({ "token": token })
    } else {
        serde_json::json!({
            "username": cfg.username.clone().ok_or_else(|| anyhow!("缺少用户名"))?,
            "password": cfg.password.clone().ok_or_else(|| anyhow!("缺少密码"))?,
        })
    };

    let auth_text = serde_json::to_string(&auth_json)?;
    stream.send(Message::Text(auth_text.into())).await?;
    events
        .send(RuntimeEvent::Log(RuntimeLogEvent::new(
            Level::Debug,
            "认证请求已发送",
        )))
        .await
        .ok();

    if let Some(reply) = stream.next().await {
        match reply {
            Ok(Message::Text(text)) => {
                let value: serde_json::Value = serde_json::from_str(&text)
                    .map_err(|err| anyhow!("认证响应格式错误: {}", err))?;
                if value.get("payload").is_some() {
                    let resp: AuthResponse = serde_json::from_value(value)
                        .map_err(|err| anyhow!("认证响应解析失败: {}", err))?;
                    if !resp.payload.success {
                        return Err(anyhow!("认证失败: {}", resp.payload.message));
                    }
                    events
                        .send(RuntimeEvent::Log(RuntimeLogEvent::new(
                            Level::Info,
                            format!("认证成功: {}", resp.payload.message),
                        )))
                        .await
                        .ok();
                    events
                        .send(RuntimeEvent::Status("认证成功".into()))
                        .await
                        .ok();
                } else if value.get("success").is_some() {
                    let success = value
                        .get("success")
                        .and_then(|b| b.as_bool())
                        .unwrap_or(false);
                    let message = value.get("message").and_then(|s| s.as_str()).unwrap_or("");
                    if !success {
                        return Err(anyhow!("认证失败: {}", message));
                    }
                    events
                        .send(RuntimeEvent::Log(RuntimeLogEvent::new(
                            Level::Info,
                            format!("认证成功: {}", message),
                        )))
                        .await
                        .ok();
                    events
                        .send(RuntimeEvent::Status("认证成功".into()))
                        .await
                        .ok();
                } else {
                    return Err(anyhow!("认证响应无法识别"));
                }
            }
            Ok(other) => {
                return Err(anyhow!("认证响应类型错误: {:?}", other));
            }
            Err(err) => {
                return Err(anyhow!("读取认证响应失败: {}", err));
            }
        }
    } else {
        return Err(anyhow!("服务器未返回认证结果"));
    }

    Ok(())
}

async fn connect_insecure(url: Url) -> Result<WsStream, tokio_tungstenite::tungstenite::Error> {
    #[derive(Debug)]
    struct NoVerify;
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
    use rustls::{ClientConfig as RustlsClientConfig, DigitallySignedStruct, SignatureScheme};
    use std::sync::Arc as StdArc;

    impl ServerCertVerifier for NoVerify {
        fn verify_server_cert(
            &self,
            _end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _server_name: &ServerName<'_>,
            _ocsp_response: &[u8],
            _now: UnixTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            Ok(ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer<'_>,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer<'_>,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            vec![
                SignatureScheme::ECDSA_NISTP256_SHA256,
                SignatureScheme::ED25519,
                SignatureScheme::RSA_PKCS1_SHA256,
            ]
        }
    }

    let builder = RustlsClientConfig::builder().dangerous();
    let config = builder
        .with_custom_certificate_verifier(StdArc::new(NoVerify))
        .with_no_client_auth();
    let request = url.as_str().into_client_request().unwrap();
    tokio_tungstenite::connect_async_tls_with_config(
        request,
        None,
        false,
        Some(Connector::Rustls(StdArc::new(config))),
    )
    .await
    .map(|(stream, _)| stream)
}
