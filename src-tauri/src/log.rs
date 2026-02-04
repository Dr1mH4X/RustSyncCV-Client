use anyhow::{Context, Result};
use simplelog::{
    ColorChoice, CombinedLogger, ConfigBuilder, LevelFilter, SharedLogger, TermLogger,
    TerminalMode, WriteLogger,
};
use std::fs::File;

pub fn setup_logger() -> Result<()> {
    let exe_path = std::env::current_exe().context("Failed to get exe path")?;
    let exe_dir = exe_path.parent().unwrap_or(std::path::Path::new("."));
    let log_dir = exe_dir.join("logs");
    std::fs::create_dir_all(&log_dir).context("Failed to create log dir")?;

    let backend_log_file =
        File::create(log_dir.join("backend.log")).context("Failed to create backend log file")?;
    let frontend_log_file =
        File::create(log_dir.join("frontend.log")).context("Failed to create frontend log file")?;

    // Backend config: ignore "frontend" target
    let backend_config = ConfigBuilder::new()
        .set_time_offset_to_local()
        .unwrap()
        .add_filter_ignore_str("fontdb")
        .add_filter_ignore_str("frontend")
        .add_filter_ignore_str("tauri")
        .build();

    // Frontend config: allow ONLY "frontend" target
    let frontend_config = ConfigBuilder::new()
        .set_time_offset_to_local()
        .unwrap()
        .add_filter_allow_str("frontend")
        .build();

    // Terminal config (shows everything)
    let term_config = ConfigBuilder::new()
        .set_time_offset_to_local()
        .unwrap()
        .add_filter_ignore_str("fontdb")
        .build();

    let mut loggers: Vec<Box<dyn SharedLogger>> = Vec::new();

    loggers.push(TermLogger::new(
        LevelFilter::Info,
        term_config,
        TerminalMode::Mixed,
        ColorChoice::Auto,
    ));

    loggers.push(WriteLogger::new(
        LevelFilter::Debug,
        backend_config,
        backend_log_file,
    ));
    loggers.push(WriteLogger::new(
        LevelFilter::Debug,
        frontend_config,
        frontend_log_file,
    ));

    CombinedLogger::init(loggers).ok();
    log::info!("Backend initialized");
    Ok(())
}

#[tauri::command]
pub fn open_log_folder() -> Result<(), String> {
    let exe_path = std::env::current_exe().map_err(|e| e.to_string())?;
    let log_dir = exe_path.parent().unwrap().join("logs");
    if log_dir.exists() {
        #[cfg(target_os = "windows")]
        std::process::Command::new("explorer")
            .arg(log_dir)
            .spawn()
            .map_err(|e| e.to_string())?;
        #[cfg(not(target_os = "windows"))]
        std::process::Command::new("xdg-open")
            .arg(log_dir)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn frontend_log(level: String, message: String) {
    let target = "frontend";
    match level.as_str() {
        "error" => log::error!(target: target, "{}", message),
        "warn" => log::warn!(target: target, "{}", message),
        "info" => log::info!(target: target, "{}", message),
        "debug" => log::debug!(target: target, "{}", message),
        _ => log::info!(target: target, "{}", message),
    }
}
