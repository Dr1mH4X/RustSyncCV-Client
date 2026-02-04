use tauri::WebviewWindow;

#[cfg(target_os = "windows")]
use window_vibrancy::{apply_acrylic, clear_acrylic};

#[cfg(target_os = "macos")]
use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState};

#[tauri::command]
pub fn apply_window_effects(window: WebviewWindow, effect: String) {
    #[cfg(target_os = "windows")]
    {
        let _ = window.set_skip_taskbar(false);

        if effect == "acrylic" {
            if apply_acrylic(&window, Some((0, 0, 0, 0))).is_err() {
                let _ = clear_acrylic(&window);
            }
        } else {
            let _ = clear_acrylic(&window);
        }
    }

    #[cfg(target_os = "macos")]
    {
        if effect == "acrylic" {
            let _ = apply_vibrancy(
                &window,
                NSVisualEffectMaterial::UnderWindowBackground,
                Some(NSVisualEffectState::Active),
                None,
            );
        } else {
            let _ = window_vibrancy::clear_vibrancy(&window);
        }
    }
}
