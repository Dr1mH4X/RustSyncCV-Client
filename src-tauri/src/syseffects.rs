use tauri::WebviewWindow;

#[cfg(target_os = "windows")]
use window_vibrancy::{apply_acrylic, clear_acrylic};

#[tauri::command]
pub fn apply_window_effects(window: WebviewWindow, effect: String) {
    #[cfg(target_os = "windows")]
    {
        let _ = window.set_skip_taskbar(false);

        if effect == "acrylic" {
            // Try to apply acrylic. If it fails (unsupported platform), clear effects.
            if apply_acrylic(&window, Some((0, 0, 0, 0))).is_err() {
                let _ = clear_acrylic(&window);
            }
        } else {
            let _ = clear_acrylic(&window);
        }
    }
}
