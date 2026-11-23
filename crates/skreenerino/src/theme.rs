//! Windows 11 Fluent design tokens and window effects.

use egui::Color32;

pub const SURFACE: Color32 = Color32::from_rgb(0x20, 0x20, 0x20);
pub const SURFACE_RAISED: Color32 = Color32::from_rgb(0x2B, 0x2B, 0x2B);
pub const SURFACE_HOVER: Color32 = Color32::from_rgb(0x33, 0x33, 0x33);
pub const SURFACE_ACTIVE: Color32 = Color32::from_rgb(0x3D, 0x3D, 0x3D);
pub const STROKE: Color32 = Color32::from_rgb(0x3A, 0x3A, 0x3A);
pub const ACCENT: Color32 = Color32::from_rgb(0x4C, 0xC2, 0xFF);
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(0xA0, 0xA0, 0xA0);
pub const DANGER: Color32 = Color32::from_rgb(0xC4, 0x2B, 0x1C);
pub const SURFACE_LIGHT: Color32 = Color32::from_rgb(0xF3, 0xF3, 0xF3);
pub const SURFACE_RAISED_LIGHT: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
pub const STROKE_LIGHT: Color32 = Color32::from_rgb(0xE5, 0xE5, 0xE5);
pub const ACCENT_LIGHT: Color32 = Color32::from_rgb(0x0F, 0x6C, 0xBD);
pub const TEXT_PRIMARY_LIGHT: Color32 = Color32::from_rgb(0x1A, 0x1A, 0x1A);
pub const TEXT_SECONDARY_LIGHT: Color32 = Color32::from_rgb(0x61, 0x61, 0x61);

/// Read `AppsUseLightTheme` from the registry; defaults to dark on any failure.
pub fn detect_dark_mode() -> bool {
    #[cfg(target_os = "windows")]
    {
        let output = std::process::Command::new("reg")
            .args([
                "query",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
                "/v",
                "AppsUseLightTheme",
            ])
            .output();
        if let Ok(output) = output {
            let text = String::from_utf8_lossy(&output.stdout);
            // `AppsUseLightTheme    REG_DWORD    0x1` means light mode.
            if let Some(value) = text.split_whitespace().last() {
                return !value.eq_ignore_ascii_case("0x1");
            }
        }
    }
    true
}

/// Apply the Fluent palette and metrics to the egui context.
pub fn apply(ctx: &egui::Context, dark: bool) {
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.panel_fill = if dark { SURFACE } else { SURFACE_LIGHT };
    visuals.window_fill = if dark {
        SURFACE_RAISED
    } else {
        SURFACE_RAISED_LIGHT
    };
    visuals.extreme_bg_color = if dark {
        Color32::from_rgb(0x1A, 0x1A, 0x1A)
    } else {
        Color32::from_rgb(0xFA, 0xFA, 0xFA)
    };
    visuals.override_text_color = Some(if dark {
        TEXT_PRIMARY
    } else {
        TEXT_PRIMARY_LIGHT
    });
    visuals.hyperlink_color = if dark { ACCENT } else { ACCENT_LIGHT };
    visuals.selection.bg_fill = (if dark { ACCENT } else { ACCENT_LIGHT }).gamma_multiply(0.35);
    let raised = if dark {
        SURFACE_RAISED
    } else {
        SURFACE_RAISED_LIGHT
    };
    let hover = if dark {
        SURFACE_HOVER
    } else {
        Color32::from_rgb(0xF0, 0xF0, 0xF0)
    };
    let active = if dark {
        SURFACE_ACTIVE
    } else {
        Color32::from_rgb(0xE5, 0xE5, 0xE5)
    };
    visuals.widgets.noninteractive.bg_fill = visuals.panel_fill;
    visuals.widgets.noninteractive.bg_stroke.color = if dark { STROKE } else { STROKE_LIGHT };
    visuals.widgets.inactive.bg_fill = raised;
    visuals.widgets.inactive.weak_bg_fill = raised;
    visuals.widgets.hovered.bg_fill = hover;
    visuals.widgets.hovered.weak_bg_fill = hover;
    visuals.widgets.active.bg_fill = active;
    visuals.widgets.active.weak_bg_fill = active;
    visuals.window_corner_radius = egui::CornerRadius::same(8);
    visuals.menu_corner_radius = egui::CornerRadius::same(8);
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(14.0, 7.0);
    style.spacing.interact_size.y = 32.0;
    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::new(13.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::new(20.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::new(13.0, egui::FontFamily::Proportional),
    );
    ctx.set_style(style);
}

/// Apply Windows 11 DWM effects: Mica backdrop, rounded corners, dark titlebar.
///
/// Every call is best-effort: on Windows 10 or an unsupported build the window
/// simply keeps the default opaque frame.
pub fn apply_window_effects(window: &winit::window::Window, dark: bool) {
    #[cfg(target_os = "windows")]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};

        // DWMWINDOWATTRIBUTE values (undocumented-but-stable ids).
        const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
        const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
        const DWMWA_SYSTEMBACKDROP_TYPE: u32 = 38;
        const DWMWCP_ROUND: u32 = 2;
        const DWMSBT_MAINWINDOW: u32 = 2;

        let Ok(handle) = window.window_handle() else {
            return;
        };
        let RawWindowHandle::Win32(win32) = handle.as_raw() else {
            return;
        };
        let hwnd = win32.hwnd.get() as *mut core::ffi::c_void;

        #[link(name = "dwmapi")]
        extern "system" {
            fn DwmSetWindowAttribute(
                hwnd: *mut core::ffi::c_void,
                attr: u32,
                value: *const core::ffi::c_void,
                size: u32,
            ) -> i32;
        }

        unsafe {
            let dark_flag: i32 = if dark { 1 } else { 0 };
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark_flag as *const i32 as *const core::ffi::c_void,
                std::mem::size_of::<i32>() as u32,
            );

            let corner: i32 = DWMWCP_ROUND as i32;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &corner as *const i32 as *const core::ffi::c_void,
                std::mem::size_of::<i32>() as u32,
            );

            // Mica; ignored on Windows 10 and on builds without the attribute.
            let backdrop: i32 = DWMSBT_MAINWINDOW as i32;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_SYSTEMBACKDROP_TYPE,
                &backdrop as *const i32 as *const core::ffi::c_void,
                std::mem::size_of::<i32>() as u32,
            );
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (window, dark);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_dark_mode_returns_a_bool_without_panicking() {
        let _ = detect_dark_mode();
    }

    #[test]
    fn theme_tokens_are_distinct() {
        assert_ne!(SURFACE, SURFACE_RAISED);
        assert_ne!(TEXT_PRIMARY, TEXT_SECONDARY);
        assert_ne!(ACCENT, DANGER);
    }

    #[test]
    fn light_and_dark_palettes_differ() {
        assert_ne!(SURFACE, SURFACE_LIGHT);
        assert_ne!(ACCENT, ACCENT_LIGHT);
    }
}
