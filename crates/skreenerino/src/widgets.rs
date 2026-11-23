//! Fluent controls from the mockup's component sheet, built on egui primitives.

use egui::{Color32, Rect, Response, RichText, Sense, Ui, Vec2};

use crate::icons::{self, Icon};
use crate::theme::{Palette, CONTROL_H};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Primary,
    Secondary,
    Ghost,
    Danger,
}

/// A full-width or content-sized button matching the component sheet.
pub fn button(ui: &mut Ui, p: &Palette, kind: ButtonKind, label: &str) -> Response {
    let (fill, text, stroke) = match kind {
        ButtonKind::Primary => (p.accent(), p.on_accent(), egui::Stroke::NONE),
        ButtonKind::Secondary => (p.surface(), p.text(), egui::Stroke::new(1.0, p.stroke())),
        ButtonKind::Ghost => (Color32::TRANSPARENT, p.text(), egui::Stroke::NONE),
        ButtonKind::Danger => (p.danger(), Color32::WHITE, egui::Stroke::NONE),
    };
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_owned(), egui::FontId::proportional(13.0), text);
    let size = Vec2::new(galley.size().x + 28.0, CONTROL_H);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = response.hovered();
    let fill = match kind {
        ButtonKind::Primary if hovered => p.accent_hover(),
        ButtonKind::Secondary if hovered => p.hover(),
        ButtonKind::Ghost if hovered => p.hover(),
        ButtonKind::Danger if hovered => Color32::from_rgb(0xA8, 0x24, 0x18),
        _ => fill,
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        p.control_radius(),
        fill,
        stroke,
        egui::StrokeKind::Inside,
    );
    painter.galley(rect.center() - galley.size() * 0.5, galley, text);
    response
}

/// Button with a leading icon, as used by "Share my screen" and the sidebar actions.
pub fn button_icon(
    ui: &mut Ui,
    p: &Palette,
    kind: ButtonKind,
    icon: Icon,
    label: &str,
    width: Option<f32>,
) -> Response {
    let (fill, text, stroke) = match kind {
        ButtonKind::Primary => (p.accent(), p.on_accent(), egui::Stroke::NONE),
        ButtonKind::Secondary => (p.surface(), p.text(), egui::Stroke::new(1.0, p.stroke())),
        ButtonKind::Ghost => (Color32::TRANSPARENT, p.text(), egui::Stroke::NONE),
        ButtonKind::Danger => (p.danger(), Color32::WHITE, egui::Stroke::NONE),
    };
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_owned(), egui::FontId::proportional(13.0), text);
    let icon_size = 16.0;
    let content_w = icon_size + 8.0 + galley.size().x;
    let size = Vec2::new(width.unwrap_or(content_w + 28.0), CONTROL_H);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let fill = if response.hovered() {
        match kind {
            ButtonKind::Primary => p.accent_hover(),
            ButtonKind::Danger => Color32::from_rgb(0xA8, 0x24, 0x18),
            _ => p.hover(),
        }
    } else {
        fill
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        p.control_radius(),
        fill,
        stroke,
        egui::StrokeKind::Inside,
    );
    let total = icon_size + 8.0 + galley.size().x;
    let start = rect.center().x - total * 0.5;
    icons::draw(
        painter,
        egui::Rect::from_center_size(
            egui::pos2(start + icon_size * 0.5, rect.center().y),
            Vec2::splat(icon_size),
        ),
        icon,
        text,
    );
    painter.galley(
        egui::pos2(
            start + icon_size + 8.0,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        text,
    );
    response
}

/// Segmented control (15/30/60 FPS, 100/75/50 %).
pub fn segmented<T: Copy + PartialEq>(
    ui: &mut Ui,
    p: &Palette,
    options: &[(T, &str)],
    current: &mut T,
) -> bool {
    let mut changed = false;
    let seg_h = 30.0;
    let widths: Vec<f32> = options
        .iter()
        .map(|(_, label)| {
            ui.painter()
                .layout_no_wrap(
                    label.to_string(),
                    egui::FontId::proportional(12.0),
                    p.text(),
                )
                .size()
                .x
                + 22.0
        })
        .collect();
    let total: f32 = widths.iter().sum::<f32>() + 4.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(total, seg_h), Sense::hover());
    ui.painter().rect(
        rect,
        p.control_radius(),
        p.surface_alt(),
        egui::Stroke::new(1.0, p.stroke()),
        egui::StrokeKind::Inside,
    );

    let mut x = rect.left() + 2.0;
    for (i, (value, label)) in options.iter().enumerate() {
        let w = widths[i];
        let cell =
            egui::Rect::from_min_size(egui::pos2(x, rect.top() + 2.0), Vec2::new(w, seg_h - 4.0));
        let id = ui.id().with(("seg", label));
        let response = ui.interact(cell, id, Sense::click());
        let selected = *value == *current;
        let bg = if selected {
            p.accent()
        } else if response.hovered() {
            p.hover()
        } else {
            Color32::TRANSPARENT
        };
        let fg = if selected { p.on_accent() } else { p.muted() };
        ui.painter().rect_filled(cell, p.control_radius(), bg);
        let galley =
            ui.painter()
                .layout_no_wrap(label.to_string(), egui::FontId::proportional(12.0), fg);
        ui.painter()
            .galley(cell.center() - galley.size() * 0.5, galley, fg);
        if response.clicked() && !selected {
            *current = *value;
            changed = true;
        }
        x += w;
    }
    changed
}

/// Windows 11 style toggle switch.
pub fn toggle(ui: &mut Ui, p: &Palette, on: &mut bool) -> Response {
    let size = Vec2::new(40.0, 22.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
    }
    let track = if *on { p.accent() } else { p.faint() };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(11), track);
    let knob_x = if *on {
        rect.right() - 11.0
    } else {
        rect.left() + 11.0
    };
    ui.painter()
        .circle_filled(egui::pos2(knob_x, rect.center().y), 8.0, Color32::WHITE);
    response
}

/// Role / state badge, e.g. Presenter / Viewer / Baton.
pub fn badge(ui: &mut Ui, p: &Palette, label: &str, accent: bool) {
    let (fill, fg) = if accent {
        (p.accent().gamma_multiply(0.20), p.accent())
    } else {
        (p.surface_alt(), p.muted())
    };
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_owned(), egui::FontId::proportional(11.0), fg);
    let size = Vec2::new(galley.size().x + 14.0, 18.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(9), fill);
    ui.painter()
        .galley(rect.center() - galley.size() * 0.5, galley, fg);
}

/// Small filled status dot.
pub fn dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
}

/// A card surface with optional padding; returns the inner rect for custom painting.
pub fn card(ui: &mut Ui, p: &Palette, add: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(p.surface())
        .stroke(egui::Stroke::new(1.0, p.stroke()))
        .corner_radius(p.card_radius())
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| add(ui));
}

/// Section heading with an optional trailing action (e.g. refresh devices).
pub fn section_header(ui: &mut Ui, p: &Palette, title: &str, action: Option<(Icon, &str)>) -> bool {
    let mut clicked = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).size(13.0).strong().color(p.text()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some((icon, tip)) = action {
                let (rect, response) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
                let color = if response.hovered() {
                    p.accent()
                } else {
                    p.muted()
                };
                icons::draw(ui.painter(), rect.shrink(5.0), icon, color);
                if response.clicked() {
                    clicked = true;
                }
                response.on_hover_text(tip);
            }
        });
    });
    clicked
}

/// Sidebar navigation entry with icon and label; draws an accent pill when active.
pub fn nav_item(ui: &mut Ui, p: &Palette, icon: Icon, label: &str, active: bool) -> Response {
    let height = 36.0;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());
    if active {
        ui.painter()
            .rect_filled(rect, p.control_radius(), p.nav_active());
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, p.control_radius(), p.hover());
    }
    let fg = if active { p.accent() } else { p.muted() };
    icons::draw(
        ui.painter(),
        egui::Rect::from_center_size(
            egui::pos2(rect.left() + 20.0, rect.center().y),
            Vec2::splat(16.0),
        ),
        icon,
        fg,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 36.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0),
        if active { p.text() } else { p.muted() },
    );
    response
}

/// One of the six large digit boxes in the pair dialog.
pub fn digit_box(ui: &mut Ui, p: &Palette, digit: char) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(44.0, 56.0), Sense::hover());
    ui.painter().rect(
        rect,
        p.control_radius(),
        p.surface_alt(),
        egui::Stroke::new(1.0, p.stroke()),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        digit,
        egui::FontId::new(24.0, egui::FontFamily::Proportional),
        p.text(),
    );
}

/// Rounded video tile background with a centred placeholder glyph.
pub fn video_placeholder(ui: &mut Ui, rect: egui::Rect, p: &Palette) {
    ui.painter()
        .rect_filled(rect, p.card_radius(), p.surface_alt());
    icons::draw(
        ui.painter(),
        egui::Rect::from_center_size(rect.center(), Vec2::splat(36.0)),
        Icon::Monitor,
        p.faint(),
    );
}

/// Draw a soft blue hero gradient, standing in for the mockup artwork.
pub fn hero_gradient(painter: &egui::Painter, rect: Rect) {
    let dark = Color32::from_rgb(0x0B, 0x2E, 0x5C);
    let mid = Color32::from_rgb(0x1B, 0x5F, 0xB0);
    let light = Color32::from_rgb(0x7C, 0xB8, 0xF0);
    let steps = 48;
    for i in 0..steps {
        let t = i as f32 / steps as f32;
        // Diagonal blend, dark bottom-left to light top-right.
        let c = if t < 0.5 {
            blend(dark, mid, t * 2.0)
        } else {
            blend(mid, light, (t - 0.5) * 2.0)
        };
        let y0 = rect.top() + rect.height() * t;
        let y1 = rect.top() + rect.height() * (t + 1.0 / steps as f32);
        painter.rect_filled(
            Rect::from_min_max(egui::pos2(rect.left(), y0), egui::pos2(rect.right(), y1)),
            0.0,
            c,
        );
    }
}

fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let lerp = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color32::from_rgb(lerp(a.r(), b.r()), lerp(a.g(), b.g()), lerp(a.b(), b.b()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_endpoints_are_exact() {
        let a = Color32::from_rgb(0, 0, 0);
        let b = Color32::from_rgb(255, 128, 64);
        assert_eq!(blend(a, b, 0.0), a);
        assert_eq!(blend(a, b, 1.0), b);
    }

    #[test]
    fn blend_midpoint_is_between() {
        let a = Color32::from_rgb(0, 0, 0);
        let b = Color32::from_rgb(200, 100, 50);
        let mid = blend(a, b, 0.5);
        assert!(mid.r() > 90 && mid.r() < 110);
    }
}
