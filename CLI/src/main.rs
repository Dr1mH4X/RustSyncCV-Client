mod clipboard;
mod config;
mod messages;

use anyhow::{anyhow, Result};
use clipboard::{start_clipboard_monitor, start_clipboard_setter};
use config::Config;
use futures_util::{SinkExt, StreamExt};
use messages::{AuthResponse, ClipboardBroadcast, ClipboardBroadcastPayload, ClipboardUpdate};
use rustls::client::danger::{ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::ClientConfig as RustlsClientConfig;
use serde_json::json;
use std::sync::Arc as StdArc;
use std::{
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};
use tokio::{
    sync::{broadcast, mpsc},
    time::sleep,
};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::Connector;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<()> {
    // Load configuration
    let cfg = Config::load()?;
    let server_url = Url::parse(&cfg.server_url)?;
    println!("Connecting to: {}", server_url);

    // 简单命令行参数解析: 支持 --insecure
    let args: Vec<String> = std::env::args().collect();
    let cli_insecure = args.iter().any(|a| a == "--insecure");
    let insecure_effective = cfg.trust_insecure_cert || cli_insecure;
    if insecure_effective && server_url.scheme() == "wss" {
        eprintln!(
            "[WARN] TLS 证书验证已被禁用 (来源: {} ). ",
            if cli_insecure {
                "--insecure 参数"
            } else {
                "配置 trust_insecure_cert"
            }
        );
    }

    // Unique device identifier
    let device_id = Uuid::new_v4().to_string();
    // Flag to disable monitor when setting clipboard
    let disable_flag = Arc::new(AtomicBool::new(false));

    // Channels for outgoing and incoming messages
    let (tx_out, _rx_out) = broadcast::channel::<ClipboardUpdate>(100);
    let (tx_in, rx_in) = mpsc::channel::<ClipboardBroadcastPayload>(100);

    // Start clipboard monitor
    {
        let tx = tx_out.clone();
        let disable = disable_flag.clone();
        let dev = device_id.clone();
        let max_image_kb = cfg.max_image_kb;
        tokio::spawn(async move {
            start_clipboard_monitor(tx, disable, dev, max_image_kb).await;
        });
    }

    // Start clipboard setter
    {
        let disable = disable_flag.clone();
        tokio::spawn(async move {
            start_clipboard_setter(rx_in, disable).await;
        });
    }

    // Main connection loop
    loop {
        match {
            if server_url.scheme() == "wss" && insecure_effective {
                #[derive(Debug)]
                struct NoVerify;
                use rustls::client::danger::HandshakeSignatureValid;
                use rustls::{DigitallySignedStruct, SignatureScheme};
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
                let request = server_url.as_str().into_client_request().unwrap();
                tokio_tungstenite::connect_async_tls_with_config(
                    request,
                    None,
                    false,
                    Some(Connector::Rustls(StdArc::new(config))),
                )
                .await
            } else {
                connect_async(server_url.clone().to_string()).await
            }
        } {
            Ok((ws_stream, _)) => {
                println!("WebSocket connected");
                let (mut write, mut read) = ws_stream.split();

                // Send authentication message (token or username/password) as flat JSON
                let auth_json = if let Some(token) = cfg.token.clone() {
                    json!({ "token": token })
                } else {
                    // unwrap because username/password must be set when token is None
                    json!({ "username": cfg.username.clone().unwrap(), "password": cfg.password.clone().unwrap() })
                };
                let auth_text = serde_json::to_string(&auth_json)?;
                write.send(Message::Text(auth_text.into())).await?;

                // Wait for auth response, check message type before parsing
                if let Some(raw_msg) = read.next().await {
                    let raw_msg = raw_msg?;
                    if let Message::Text(text) = raw_msg {
                        println!("server reply raw: {}", text);
                        // now parse JSON and handle possible formats
                        let v: serde_json::Value = match serde_json::from_str(&text) {
                            Ok(val) => val,
                            Err(e) => {
                                println!("failed to parse JSON: {}", e);
                                return Err(anyhow!("Invalid auth response format"));
                            }
                        };
                        if v.get("payload").is_some() {
                            // full AuthResponse wrapper
                            let resp: AuthResponse = serde_json::from_value(v.clone())?;
                            if !resp.payload.success {
                                return Err(anyhow!("Auth failed: {}", resp.payload.message));
                            }
                            println!("Auth succeeded: {}", resp.payload.message);
                        } else if v.get("success").is_some() {
                            // fallback flat structure
                            let success =
                                v.get("success").and_then(|b| b.as_bool()).unwrap_or(false);
                            let message = v.get("message").and_then(|s| s.as_str()).unwrap_or("");
                            if !success {
                                return Err(anyhow!("Auth failed: {}", message));
                            }
                            println!("Auth succeeded: {}", message);
                        } else {
                            println!("unexpected auth response: {:?}", v);
                            return Err(anyhow!("Invalid auth response"));
                        }
                    } else {
                        println!("server reply not text: {:?}", raw_msg);
                        return Err(anyhow!("Invalid auth message type"));
                    }
                }

                // Clone tx_in for receive task and subscribe to broadcast channel for sending updates
                let tx_in_clone = tx_in.clone();
                let mut rx = tx_out.subscribe();

                // Task to send clipboard updates
                let send_task = tokio::spawn(async move {
                    while let Ok(msg) = rx.recv().await {
                        if let Ok(text) = serde_json::to_string(&msg) {
                            if write.send(Message::Text(text.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                });

                // Task to receive broadcasts
                let recv_task = tokio::spawn(async move {
                    while let Some(msg) = read.next().await {
                        if let Ok(Message::Text(txt)) = msg {
                            println!("recv raw broadcast: {}", txt);
                            // Try full wrapper
                            if let Ok(bc) = serde_json::from_str::<ClipboardBroadcast>(&txt) {
                                let _ = tx_in_clone.send(bc.payload).await;
                            // Fallback to flat payload
                            } else if let Ok(payload) =
                                serde_json::from_str::<ClipboardBroadcastPayload>(&txt)
                            {
                                let _ = tx_in_clone.send(payload).await;
                            } else {
                                println!("unhandled broadcast message: {}", txt);
                            }
                        }
                    }
                });

                // Wait until one task ends
                tokio::select! {
                    _ = send_task => println!("Send task ended, reconnecting"),
                    _ = recv_task => println!("Receive task ended, reconnecting"),
                }
            }
            Err(e) => {
                eprintln!("Connection error: {}", e);
            }
        }
        println!("Reconnecting in 5 seconds...");
        sleep(Duration::from_secs(5)).await;
    }
}
