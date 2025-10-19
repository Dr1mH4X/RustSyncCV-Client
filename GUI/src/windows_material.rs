#[cfg(target_os = "windows")]

mod imp {

    use std::{ffi::c_void, mem::size_of};

    use anyhow::{anyhow, Result};

    use slint::winit_030::{winit, WinitWindowAccessor};

    use slint::ComponentHandle;

    use windows::Win32::Foundation::{BOOL, HWND};

    use windows::Win32::Graphics::Dwm::{
        DwmGetWindowAttribute, DwmSetWindowAttribute, DWMSBT_MAINWINDOW, DWMSBT_TRANSIENTWINDOW,
        DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
        DWMWCP_ROUND, DWM_SYSTEMBACKDROP_TYPE, DWM_WINDOW_CORNER_PREFERENCE,
    };

    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    pub fn apply_to_component_with_theme<C: ComponentHandle>(
        component: &C,

        effect: super::Effect,

        theme: super::ThemeMode,
    ) -> Result<()> {
        match component.window().with_winit_window(|winit_window| {
            let handle = winit_window
                .window_handle()
                .map_err(|err| anyhow!("获取窗口句柄失败: {err:?}"))?;

            match handle.as_raw() {
                RawWindowHandle::Win32(win32) => unsafe {
                    apply_to_hwnd(HWND(win32.hwnd.get()), effect, theme)
                },

                _ => Err(anyhow!("当前窗口不是 Win32 句柄，无法应用特效")),
            }
        }) {
            Some(result) => result,

            None => Ok(()),
        }
    }

    pub fn apply_to_component<C: ComponentHandle>(
        component: &C,
        effect: super::Effect,
    ) -> Result<()> {
        apply_to_component_with_theme(component, effect, super::ThemeMode::Dark)
    }

    unsafe fn apply_to_hwnd(
        hwnd: HWND,
        effect: super::Effect,
        theme: super::ThemeMode,
    ) -> Result<()> {
        if hwnd.0 == 0 {
            return Err(anyhow!("窗口句柄无效"));
        }

        // 1) 设置主题模式（暗/亮/系统）。系统模式下不强制设置，由系统决定。
        match theme {
            super::ThemeMode::Dark | super::ThemeMode::Light => {
                let enable_dark = BOOL(matches!(theme, super::ThemeMode::Dark) as i32);
                let dark_ptr = &enable_dark as *const _ as *const c_void;

                let _ = DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_USE_IMMERSIVE_DARK_MODE,
                    dark_ptr,
                    size_of::<BOOL>() as u32,
                );
            }
            super::ThemeMode::System => {
                // 不设置，遵循系统默认
            }
        }

        // 2) 设置圆角偏好（不影响后续流程）
        let corner_pref: DWM_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUND;
        let corner_ptr = &corner_pref as *const _ as *const c_void;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            corner_ptr,
            size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
        );

        // 3) 应用系统背景材质（Windows 11）
        let backdrop = match effect {
            super::Effect::Mica => DWMSBT_MAINWINDOW,
            super::Effect::Acrylic => DWMSBT_TRANSIENTWINDOW,
        };

        let backdrop_ptr = &backdrop as *const _ as *const c_void;

        DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            backdrop_ptr,
            size_of::<DWM_SYSTEMBACKDROP_TYPE>() as u32,
        )?;

        // 读回验证系统背景材质是否真正生效，避免误判成功后提前开启透明
        let mut current: DWM_SYSTEMBACKDROP_TYPE = DWM_SYSTEMBACKDROP_TYPE(0);
        let current_ptr = &mut current as *mut _ as *mut c_void;
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            current_ptr,
            size_of::<DWM_SYSTEMBACKDROP_TYPE>() as u32,
        )?;
        if current != backdrop {
            return Err(anyhow!("系统背景未生效，读回值与设置不一致"));
        }

        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    use anyhow::Result;
    use slint::ComponentHandle;

    pub fn apply_to_component_with_theme<C: ComponentHandle>(
        _component: &C,

        _effect: super::Effect,

        _theme: super::ThemeMode,
    ) -> Result<()> {
        Ok(())
    }

    pub fn apply_to_component<C: ComponentHandle>(
        component: &C,
        effect: super::Effect,
    ) -> Result<()> {
        apply_to_component_with_theme(component, effect, super::ThemeMode::System)
    }
}

#[derive(Clone, Copy, Debug)]

pub enum ThemeMode {
    Dark,
    Light,
    System,
}

#[derive(Clone, Copy, Debug)]
pub enum Effect {
    Mica,

    Acrylic,
}

impl Effect {
    pub fn from_str(value: &str) -> Option<Self> {
        let normalized = value.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "mica" => Some(Self::Mica),
            "acrylic" => Some(Self::Acrylic),
            _ => None,
        }
    }
}

use slint::ComponentHandle;

pub fn apply_to_component_with_theme<C: ComponentHandle>(
    component: &C,
    effect: Effect,
    theme: ThemeMode,
) -> anyhow::Result<()> {
    imp::apply_to_component_with_theme(component, effect, theme)
}

pub fn apply_to_component<C: ComponentHandle>(component: &C, effect: Effect) -> anyhow::Result<()> {
    imp::apply_to_component_with_theme(component, effect, ThemeMode::Dark)
}
