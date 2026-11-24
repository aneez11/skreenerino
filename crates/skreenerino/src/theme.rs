//! Windows 11 Fluent design tokens, matched to `docs/design` mockup.

use egui::Color32;

/// A light/dark pair for one design token.
#[derive(Debug, Clone, Copy)]
pub struct Token {
    pub light: Color32,
    pub dark: Color32,
}

impl Token {
    pub const fn new(light: u32, dark: u32) -> Self {
        Self {
            light: Color32::from_rgb(
                ((light >> 16) & 0xFF) as u8,
                ((light >> 8) & 0xFF) as u8,
                (light & 0xFF) as u8,
            ),
            dark: Color32::from_rgb(
                ((dark >> 16) & 0xFF) as u8,
                ((dark >> 8) & 0xFF) as u8,
                (dark & 0xFF) as u8,
            ),
        }
    }

    pub fn get(&self, dark: bool) -> Color32 {
        if dark {
            self.dark
        } else {
            self.light
        }
    }
}

/// Window background (behind cards).
pub const WINDOW: Token = Token::new(0xF3F3F3, 0x1B1B1B);
/// Card / panel surface.
pub const SURFACE: Token = Token::new(0xFFFFFF, 0x242424);
/// Slightly raised surface (inputs, tiles).
pub const SURFACE_ALT: Token = Token::new(0xFAFAFA, 0x2B2B2B);
/// Hover fill for interactive rows.
pub const HOVER: Token = Token::new(0xF0F0F0, 0x333333);
/// Sidebar active pill.
pub const NAV_ACTIVE: Token = Token::new(0xE8F1FB, 0x2D4356);
/// Hairline border.
pub const STROKE: Token = Token::new(0xE5E5E5, 0x3A3A3A);
/// Accent (Win11 system blue).
pub const ACCENT: Token = Token::new(0x0F6CBD, 0x4CC2FF);
/// Accent hover.
pub const ACCENT_HOVER: Token = Token::new(0x115EA3, 0x62CDFF);
/// Text on accent fills.
pub const ON_ACCENT: Token = Token::new(0xFFFFFF, 0x000000);
/// Primary text.
pub const TEXT: Token = Token::new(0x1A1A1A, 0xFFFFFF);
/// Secondary text.
pub const TEXT_MUTED: Token = Token::new(0x616161, 0xA0A0A0);
/// Disabled / tertiary text.
pub const TEXT_FAINT: Token = Token::new(0x8A8A8A, 0x7A7A7A);
/// Success (online dot, toast check).
pub const SUCCESS: Token = Token::new(0x0F7B0F, 0x6CCB5F);
/// Warning.
pub const WARNING: Token = Token::new(0x9D5D00, 0xFFC83D);
/// Danger fill (Leave).
pub const DANGER: Token = Token::new(0xC42B1C, 0xC42B1C);
/// Danger text on transparent.
pub const DANGER_TEXT: Token = Token::new(0xC42B1C, 0xFF99A4);

/// Radius used by cards and tiles.
pub const RADIUS_CARD: u8 = 12;
/// Radius for buttons, pills and inputs.
pub const RADIUS_CONTROL: u8 = 8;
/// Height of primary controls.
pub const CONTROL_H: f32 = 34.0;
/// Sidebar width.
pub const SIDEBAR_W: f32 = 168.0;
/// Participants rail width.
pub const RAIL_W: f32 = 236.0;

/// Convenience: current palette for the window.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub dark: bool,
}

impl Palette {
    pub fn new(dark: bool) -> Self {
        Self { dark }
    }
    pub fn window(&self) -> Color32 {
        WINDOW.get(self.dark)
    }
    pub fn surface(&self) -> Color32 {
        SURFACE.get(self.dark)
    }
    pub fn surface_alt(&self) -> Color32 {
        SURFACE_ALT.get(self.dark)
    }
    pub fn hover(&self) -> Color32 {
        HOVER.get(self.dark)
    }
    pub fn nav_active(&self) -> Color32 {
        NAV_ACTIVE.get(self.dark)
    }
    pub fn stroke(&self) -> Color32 {
        STROKE.get(self.dark)
    }
    pub fn accent(&self) -> Color32 {
        ACCENT.get(self.dark)
    }
    pub fn accent_hover(&self) -> Color32 {
        ACCENT_HOVER.get(self.dark)
    }
    pub fn on_accent(&self) -> Color32 {
        ON_ACCENT.get(self.dark)
    }
    pub fn text(&self) -> Color32 {
        TEXT.get(self.dark)
    }
    pub fn muted(&self) -> Color32 {
        TEXT_MUTED.get(self.dark)
    }
    pub fn faint(&self) -> Color32 {
        TEXT_FAINT.get(self.dark)
    }
    pub fn success(&self) -> Color32 {
        SUCCESS.get(self.dark)
    }
    pub fn warning(&self) -> Color32 {
        WARNING.get(self.dark)
    }
    pub fn danger(&self) -> Color32 {
        DANGER.get(self.dark)
    }
    pub fn danger_text(&self) -> Color32 {
        DANGER_TEXT.get(self.dark)
    }
    pub fn card_radius(&self) -> egui::CornerRadius {
        egui::CornerRadius::same(RADIUS_CARD)
    }
    pub fn control_radius(&self) -> egui::CornerRadius {
        egui::CornerRadius::same(RADIUS_CONTROL)
    }
}

/// Read `AppsUseLightTheme`; any failure falls back to dark.
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
            if let Some(value) = text.split_whitespace().last() {
                return !value.eq_ignore_ascii_case("0x1");
            }
        }
    }
    true
}

/// Apply the Fluent palette, metrics and the Segoe UI Variable font.
pub fn apply(ctx: &egui::Context, dark: bool) {
    load_system_font(ctx);

    let p = Palette::new(dark);
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.panel_fill = p.window();
    visuals.window_fill = p.surface();
    visuals.extreme_bg_color = p.surface_alt();
    visuals.faint_bg_color = p.surface_alt();
    visuals.override_text_color = Some(p.text());
    visuals.hyperlink_color = p.accent();
    visuals.selection.bg_fill = p.accent().gamma_multiply(0.30);
    visuals.selection.stroke.color = p.accent();
    visuals.window_stroke.color = p.stroke();
    visuals.widgets.noninteractive.bg_fill = p.surface();
    visuals.widgets.noninteractive.bg_stroke.color = p.stroke();
    visuals.widgets.noninteractive.fg_stroke.color = p.text();
    visuals.widgets.inactive.bg_fill = p.surface_alt();
    visuals.widgets.inactive.weak_bg_fill = p.surface_alt();
    visuals.widgets.inactive.bg_stroke.color = p.stroke();
    visuals.widgets.inactive.fg_stroke.color = p.text();
    visuals.widgets.hovered.bg_fill = p.hover();
    visuals.widgets.hovered.weak_bg_fill = p.hover();
    visuals.widgets.hovered.bg_stroke.color = p.accent();
    visuals.widgets.hovered.fg_stroke.color = p.text();
    visuals.widgets.active.bg_fill = p.accent().gamma_multiply(0.25);
    visuals.widgets.active.weak_bg_fill = p.accent().gamma_multiply(0.25);
    visuals.widgets.active.fg_stroke.color = p.text();
    visuals.widgets.open.bg_fill = p.surface_alt();
    visuals.window_corner_radius = p.card_radius();
    visuals.menu_corner_radius = p.control_radius();
    visuals.window_shadow = egui::epaint::Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 120 } else { 40 }),
    };
    visuals.popup_shadow = visuals.window_shadow;
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    style.spacing.interact_size.y = CONTROL_H;
    style.spacing.window_margin = egui::Margin::same(16);
    style.spacing.menu_margin = egui::Margin::same(6);
    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::new(13.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::new(13.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::new(20.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Small,
        egui::FontId::new(11.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Monospace,
        egui::FontId::new(13.0, egui::FontFamily::Monospace),
    );
    ctx.set_style(style);
}

/// Prefer Segoe UI Variable (Windows 11) when present, else Segoe UI.
fn load_system_font(ctx: &egui::Context) {
    const CANDIDATES: [&str; 3] = [
        r"C:\Windows\Fonts\SegUIVar.ttf",
        r"C:\Windows\Fonts\segoeui.ttf",
        r"C:\Windows\Fonts\segoeuib.ttf",
    ];
    for path in CANDIDATES {
        if let Ok(bytes) = std::fs::read(path) {
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert(
                "system".to_owned(),
                std::sync::Arc::new(egui::FontData::from_owned(bytes)),
            );
            if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                family.insert(1, "system".to_owned());
            }
            ctx.set_fonts(fonts);
            return;
        }
    }
}

/// Best-effort Windows 11 window effects: Mica backdrop, rounded corners, dark titlebar.
pub fn apply_window_effects(window: &winit::window::Window, dark: bool) {
    #[cfg(target_os = "windows")]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};

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
        let _ = (window, dark, mica);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_parsing_matches_hex() {
        let t = Token::new(0x0F6CBD, 0x4CC2FF);
        assert_eq!(t.light, Color32::from_rgb(0x0F, 0x6C, 0xBD));
        assert_eq!(t.dark, Color32::from_rgb(0x4C, 0xC2, 0xFF));
    }

    #[test]
    fn palette_selects_by_mode() {
        let light = Palette::new(false);
        let dark = Palette::new(true);
        assert_ne!(light.surface(), dark.surface());
        assert_ne!(light.text(), dark.text());
        assert_eq!(light.accent(), ACCENT.light);
        assert_eq!(dark.accent(), ACCENT.dark);
    }

    #[test]
    fn detect_dark_mode_does_not_panic() {
        let _ = detect_dark_mode();
    }
}
