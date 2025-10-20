#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod runtime;

use std::{path::PathBuf, sync::Arc};

#[cfg(target_os = "windows")]
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use parking_lot::Mutex;
use simplelog::{ColorChoice, ConfigBuilder, LevelFilter, TermLogger, TerminalMode};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use slint::winit_030::{winit::dpi::LogicalSize, WinitWindowAccessor};
#[cfg(target_os = "windows")]
use slint::Timer;
use tokio::runtime::Runtime;

use runtime::config::Config;
use runtime::{spawn_runtime, ConnectionStateEvent, RuntimeEvent, StartOptions};

slint::include_modules!();

mod windows_material;

fn sanitize_material_effect(value: &str) -> &'static str {
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case("acrylic") {
        "acrylic"
    } else {
        "mica"
    }
}

fn resolve_material_effect(value: &str) -> windows_material::Effect {
    let sanitized = sanitize_material_effect(value);

    windows_material::Effect::from_str(sanitized).unwrap_or(windows_material::Effect::Mica)
}

fn sanitize_theme_mode(value: &str) -> &'static str {
    match value.trim().to_ascii_lowercase().as_str() {
        "dark" => "dark",
        "light" => "light",
        _ => "system",
    }
}

fn resolve_theme_mode(value: &str) -> windows_material::ThemeMode {
    match sanitize_theme_mode(value) {
        "dark" => windows_material::ThemeMode::Dark,
        "light" => windows_material::ThemeMode::Light,
        _ => windows_material::ThemeMode::System,
    }
}

struct UiState {
    paused: bool,
}

struct SettingsFormData {
    server_url: SharedString,

    token: SharedString,

    username: SharedString,

    password: SharedString,

    max_image_kb: i32,

    material_effect: SharedString,

    theme_mode: SharedString,
}

impl Default for SettingsFormData {
    fn default() -> Self {
        Self {
            server_url: SharedString::from(""),
            token: SharedString::from(""),
            username: SharedString::from(""),
            password: SharedString::from(""),
            max_image_kb: 512,
            material_effect: SharedString::from("mica"),
            theme_mode: SharedString::from("system"),
        }
    }
}

struct AppContext {
    runtime: Arc<Runtime>,
    handle: runtime::RuntimeHandle,
    state: Mutex<UiState>,
    logs: Mutex<Vec<SharedString>>,
    config_dir: PathBuf,
}

impl AppContext {
    fn new(runtime: Arc<Runtime>, handle: runtime::RuntimeHandle, config_dir: PathBuf) -> Self {
        Self {
            runtime,
            handle,
            state: Mutex::new(UiState { paused: true }),
            logs: Mutex::new(Vec::new()),
            config_dir,
        }
    }

    fn runtime(&self) -> Arc<Runtime> {
        self.runtime.clone()
    }

    fn handle(&self) -> runtime::RuntimeHandle {
        self.handle.clone()
    }

    fn config_dir(&self) -> PathBuf {
        self.config_dir.clone()
    }

    fn update_paused(&self, paused: bool) {
        self.state.lock().paused = paused;
    }

    fn is_paused(&self) -> bool {
        self.state.lock().paused
    }

    fn push_log(&self, line: impl Into<SharedString>) -> Vec<SharedString> {
        let mut logs = self.logs.lock();
        if logs.len() > 2000 {
            let keep = 2000;
            let remove = logs.len().saturating_sub(keep);
            if remove > 0 {
                logs.drain(0..remove);
            }
        }
        logs.push(line.into());
        logs.clone()
    }

    fn clear_logs(&self) -> Vec<SharedString> {
        let mut logs = self.logs.lock();
        logs.clear();
        Vec::new()
    }
}

fn main() -> Result<()> {
    let log_config = ConfigBuilder::new().add_filter_ignore_str("fontdb").build();

    TermLogger::init(
        LevelFilter::Info,
        log_config,
        TerminalMode::Mixed,
        ColorChoice::Auto,
    )
    .ok();

    let runtime = Arc::new(Runtime::new()?);
    let config_dir = resolve_config_dir()?;
    let initial_form = Config::load_from_dir(&config_dir)
        .map(|cfg| settings_form_from_config(&cfg))
        .unwrap_or_default();

    let (handle, mut event_rx) = spawn_runtime(&runtime);
    let ctx = Arc::new(AppContext::new(
        runtime.clone(),
        handle.clone(),
        config_dir.clone(),
    ));

    let ui = MainWindow::new()?;

    ui.window().with_winit_window(|winit_window| {
        winit_window.set_resizable(true);
        winit_window.set_min_inner_size(Some(LogicalSize::new(400.0, 360.0)));
    });
    // 启动时先禁用透明，待系统材质应用后再开启，避免第一帧全透明
    ui.set_window_transparent(false);

    #[cfg(target_os = "windows")]
    {
        // 仅按配置决定材质效果（禁用环境变量覆盖）
        let effect = resolve_material_effect(initial_form.material_effect.as_str());

        let theme = resolve_theme_mode(initial_form.theme_mode.as_str());
        if let Err(err) = windows_material::apply_to_component_with_theme(&ui, effect, theme) {
            log::warn!("应用 Windows 材质失败: {err}");

            ui.set_window_transparent(false);
        } else {
            // 材质应用成功后再开启透明
            ui.set_window_transparent(true);
        }

        let retry_effect = effect;
        let retry_theme = theme;

        let ui_for_retry = ui.as_weak();

        Timer::single_shot(Duration::from_millis(60), move || {
            if let Some(ui) = ui_for_retry.upgrade() {
                if let Err(err) =
                    windows_material::apply_to_component_with_theme(&ui, retry_effect, retry_theme)
                {
                    log::warn!("二次应用 Windows 材质失败: {err}");
                } else {
                    ui.set_window_transparent(true);
                }
            }
        });

        // 第三次更晚的重试，进一步规避窗口初始化早期时序问题

        let retry_effect_3 = effect;

        let retry_theme_3 = theme;
        let ui_for_retry_3 = ui.as_weak();

        Timer::single_shot(Duration::from_millis(200), move || {
            if let Some(ui) = ui_for_retry_3.upgrade() {
                if let Err(err) = windows_material::apply_to_component_with_theme(
                    &ui,
                    retry_effect_3,
                    retry_theme_3,
                ) {
                    log::warn!("三次应用 Windows 材质失败: {err}");
                } else {
                    ui.set_window_transparent(true);
                }
            }
        });
    }

    ui.set_status_text("准备启动".into());
    ui.set_paused(true);
    ui.set_log_lines(ModelRc::new(VecModel::from(Vec::<SharedString>::new())));
    ui.set_settings_visible(false);
    ui.set_settings_error("".into());
    apply_settings_to_ui(&ui, &initial_form);

    let ui_handle = ui.as_weak();

    // 事件监听：将 RuntimeEvent 映射到 UI
    {
        let ctx = ctx.clone();
        let ui_events = ui_handle.clone();
        runtime.spawn(async move {
            while let Some(event) = event_rx.recv().await {
                process_runtime_event(&ctx, &ui_events, event);
            }
        });
    }

    let toggle_ctx = ctx.clone();
    let toggle_ui = ui_handle.clone();
    ui.on_toggle_pause(move || {
        if let Err(err) = toggle_pause(&toggle_ctx, &toggle_ui) {
            let message = format!("切换失败: {}", err);
            let snapshot = toggle_ctx.push_log(message.clone());
            update_logs(&toggle_ui, snapshot);
            update_status(&toggle_ui, message);
        }
    });

    let settings_ctx = ctx.clone();
    let settings_ui = ui_handle.clone();
    ui.on_open_settings(move || {
        if let Err(err) = open_settings_dialog(&settings_ctx, &settings_ui) {
            let message = format!("打开设置失败: {}", err);
            let snapshot = settings_ctx.push_log(message.clone());
            update_logs(&settings_ui, snapshot);
            update_status(&settings_ui, message);
        }
    });

    let save_ctx = ctx.clone();
    let save_ui = ui_handle.clone();
    ui.on_save_settings(move || {
        if let Some(ui) = save_ui.upgrade() {
            let form = collect_settings_from_ui(&ui);
            if let Err(err) = save_settings(&save_ctx, &save_ui, form) {
                let message = err.to_string();
                let weak = save_ui.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = weak.upgrade() {
                        ui.set_settings_error(message.clone().into());
                    }
                });
            }
        }
    });

    let close_ui = ui_handle.clone();
    ui.on_close_settings(move || {
        let weak = close_ui.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_settings_visible(false);
                ui.set_settings_error("".into());
                apply_settings_to_ui(&ui, &SettingsFormData::default());
            }
        });
    });

    let clear_ctx = ctx.clone();
    let clear_ui = ui_handle.clone();
    ui.on_clear_logs(move || {
        let snapshot = clear_ctx.clear_logs();
        update_logs(&clear_ui, snapshot);
        update_status(&clear_ui, "日志已清空");
    });

    // 启动运行时
    let start_ctx = ctx.clone();
    let start_ui = ui_handle.clone();
    update_status(&start_ui, "正在启动同步...");
    runtime.spawn(async move {
        let options = StartOptions {
            config_dir: start_ctx.config_dir(),
        };
        if let Err(err) = start_ctx.handle().start(options).await {
            let message = format!("启动失败: {}", err);
            let snapshot = start_ctx.push_log(message.clone());
            update_logs(&start_ui, snapshot);
            update_status(&start_ui, message);
        }
    });

    ui.run()?;

    let shutdown_runtime = ctx.runtime();
    let shutdown_handle = ctx.handle();
    shutdown_runtime.block_on(async move {
        let _ = shutdown_handle.shutdown().await;
    });

    Ok(())
}

fn toggle_pause(ctx: &Arc<AppContext>, ui: &slint::Weak<MainWindow>) -> Result<()> {
    let paused = ctx.is_paused();
    let handle = ctx.handle();
    let runtime = ctx.runtime();
    let ctx_clone = ctx.clone();
    let ui_clone = ui.clone();

    if paused {
        update_status(ui, "正在恢复同步...");
        runtime.spawn(async move {
            if let Err(err) = handle.resume().await {
                let message = format!("恢复失败: {}", err);
                let snapshot = ctx_clone.push_log(message.clone());
                update_logs(&ui_clone, snapshot);
                update_status(&ui_clone, message);
            }
        });
    } else {
        update_status(ui, "正在暂停...");
        runtime.spawn(async move {
            if let Err(err) = handle.pause().await {
                let message = format!("暂停失败: {}", err);
                let snapshot = ctx_clone.push_log(message.clone());
                update_logs(&ui_clone, snapshot);
                update_status(&ui_clone, message);
            }
        });
    }

    Ok(())
}

fn process_runtime_event(ctx: &Arc<AppContext>, ui: &slint::Weak<MainWindow>, event: RuntimeEvent) {
    match event {
        RuntimeEvent::Status(text) => update_status(ui, text),
        RuntimeEvent::Connection(state) => {
            let paused = matches!(
                state,
                ConnectionStateEvent::Paused | ConnectionStateEvent::Idle
            );
            ctx.update_paused(paused);
            set_paused_flag(ui, paused);
        }
        RuntimeEvent::Log(record) => {
            let line = format!(
                "[{level}] {msg}",
                level = record.level,
                msg = record.message
            );
            let snapshot = ctx.push_log(line);
            update_logs(ui, snapshot);
        }
        RuntimeEvent::ClipboardSent { content_type } => {
            update_status(ui, format!("已广播剪贴板 ({})", content_type));
        }
        RuntimeEvent::ClipboardReceived { content_type } => {
            update_status(ui, format!("已同步远端剪贴板 ({})", content_type));
        }
        RuntimeEvent::Error(message) => {
            update_status(ui, format!("错误: {}", message));
        }
    }
}

fn update_status(ui: &slint::Weak<MainWindow>, text: impl Into<SharedString>) {
    let text = text.into();
    let weak = ui.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_status_text(text.clone());
        }
    });
}

fn update_logs(ui: &slint::Weak<MainWindow>, lines: Vec<SharedString>) {
    let weak = ui.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_log_lines(ModelRc::new(VecModel::from(lines)));
        }
    });
}

fn set_paused_flag(ui: &slint::Weak<MainWindow>, paused: bool) {
    let weak = ui.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_paused(paused);
        }
    });
}

fn resolve_config_dir() -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("获取工作目录失败")?;
    if cwd.join("config.toml").exists() {
        return Ok(cwd);
    }
    if let Some(parent) = cwd.parent() {
        let parent = parent.to_path_buf();
        if parent.join("config.toml").exists() {
            return Ok(parent);
        }
    }
    Ok(cwd)
}

fn open_settings_dialog(ctx: &Arc<AppContext>, ui: &slint::Weak<MainWindow>) -> Result<()> {
    let config_dir = ctx.config_dir();
    let cfg = Config::load_from_dir(&config_dir)?;

    let data = settings_form_from_config(&cfg);

    let weak = ui.clone();
    slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_settings_error("".into());
            apply_settings_to_ui(&ui, &data);
            ui.set_settings_visible(true);
        }
    })
    .map_err(|e| anyhow!("无法打开设置窗口: {}", e))?;

    Ok(())
}

fn save_settings(
    ctx: &Arc<AppContext>,
    ui: &slint::Weak<MainWindow>,
    form: SettingsFormData,
) -> Result<()> {
    let server_url = form.server_url.trim();
    if server_url.is_empty() {
        bail!("服务器地址不能为空");
    }
    let server_url = server_url.to_string();

    let token_str = form.token.trim();
    let username_str = form.username.trim();
    let password_str = form.password.trim();

    let (token_opt, username_opt, password_opt) = if !token_str.is_empty() {
        (Some(token_str.to_string()), None, None)
    } else {
        if username_str.is_empty() || password_str.is_empty() {
            bail!("请填写 Token 或用户名/密码");
        }
        (
            None,
            Some(username_str.to_string()),
            Some(password_str.to_string()),
        )
    };

    let max_image_kb = form.max_image_kb.clamp(32, 8192) as u64;
    let material_effect = sanitize_material_effect(form.material_effect.as_str());
    let updated_config = Config {
        server_url: server_url.clone(),

        token: token_opt.clone(),

        username: username_opt.clone(),

        password: password_opt.clone(),

        max_image_kb,

        material_effect: material_effect.to_string(),

        theme_mode: sanitize_theme_mode(form.theme_mode.as_str()).to_string(),
    };

    let config_dir = ctx.config_dir();
    updated_config.save_to_dir(&config_dir)?;

    let sanitized_data = SettingsFormData {
        server_url: server_url.clone().into(),

        token: token_opt.clone().unwrap_or_default().into(),

        username: username_opt.clone().unwrap_or_default().into(),

        password: password_opt.clone().unwrap_or_default().into(),

        max_image_kb: max_image_kb as i32,

        material_effect: SharedString::from(material_effect),

        theme_mode: SharedString::from(sanitize_theme_mode(form.theme_mode.as_str())),
    };

    #[cfg(target_os = "windows")]
    let effect_to_apply = resolve_material_effect(material_effect);

    #[cfg(target_os = "windows")]
    let theme_to_apply = resolve_theme_mode(form.theme_mode.as_str());

    let weak = ui.clone();
    slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_settings_visible(false);
            ui.set_settings_error("".into());
            apply_settings_to_ui(&ui, &sanitized_data);

            #[cfg(target_os = "windows")]
            if let Err(err) = windows_material::apply_to_component_with_theme(
                &ui,
                effect_to_apply,
                theme_to_apply,
            ) {
                log::warn!("应用 Windows 材质失败: {err}");

                ui.set_window_transparent(false);
            } else {
                ui.set_window_transparent(true);
            }
        }
    })
    .map_err(|e| anyhow!("更新设置窗口失败: {}", e))?;

    let message = String::from("配置已保存，正在重新连接...");
    let snapshot = ctx.push_log(message.clone());
    update_logs(ui, snapshot);
    update_status(ui, message);

    let options = StartOptions {
        config_dir: config_dir.clone(),
    };

    let reload_handle = ctx.handle();
    let runtime = ctx.runtime();
    let reload_ctx = ctx.clone();
    let reload_ui = ui.clone();
    runtime.spawn(async move {
        if let Err(err) = reload_handle.reload(options).await {
            let message = format!("重新加载配置失败: {}", err);
            let snapshot = reload_ctx.push_log(message.clone());
            update_logs(&reload_ui, snapshot);
            update_status(&reload_ui, message);
        }
    });

    Ok(())
}

fn apply_settings_to_ui(ui: &MainWindow, data: &SettingsFormData) {
    ui.set_settings_server_url(data.server_url.clone());
    ui.set_settings_token(data.token.clone());
    ui.set_settings_username(data.username.clone());
    ui.set_settings_password(data.password.clone());
    ui.set_settings_max_image_kb(data.max_image_kb);

    ui.set_settings_use_acrylic(
        sanitize_material_effect(data.material_effect.as_str()) == "acrylic",
    );
    ui.set_settings_theme_mode(SharedString::from(sanitize_theme_mode(
        data.theme_mode.as_str(),
    )));
}

fn collect_settings_from_ui(ui: &MainWindow) -> SettingsFormData {
    SettingsFormData {
        server_url: ui.get_settings_server_url(),

        token: ui.get_settings_token(),

        username: ui.get_settings_username(),

        password: ui.get_settings_password(),

        max_image_kb: ui.get_settings_max_image_kb(),

        material_effect: SharedString::from(if ui.get_settings_use_acrylic() {
            "acrylic"
        } else {
            "mica"
        }),

        theme_mode: ui.get_settings_theme_mode(),
    }
}

fn settings_form_from_config(cfg: &Config) -> SettingsFormData {
    let max_image = cfg.max_image_kb.clamp(32, 8192);

    SettingsFormData {
        server_url: cfg.server_url.clone().into(),
        token: cfg.token.clone().unwrap_or_default().into(),
        username: cfg.username.clone().unwrap_or_default().into(),
        password: cfg.password.clone().unwrap_or_default().into(),
        max_image_kb: max_image as i32,

        material_effect: SharedString::from(sanitize_material_effect(&cfg.material_effect)),
        theme_mode: SharedString::from(sanitize_theme_mode(&cfg.theme_mode)),
    }
}
