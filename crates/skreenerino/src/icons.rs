//! Minimal procedural line icons drawn with the egui painter.
//!
//! A real icon font would be nicer, but this keeps the build dependency-free and
//! renders crisply at any DPI.

use egui::{Color32, Pos2, Rect, Stroke, Vec2};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Home,
    Settings,
    Info,
    Monitor,
    Users,
    Link,
    Refresh,
    Mic,
    MicrophoneMuted,
    Share,
    Leave,
    Check,
    Warning,
    Close,
    ChevronRight,
    ArrowRight,
    Copy,
    Signal,
}

/// Draw `icon` centred in `rect` using `color`.
pub fn draw(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let size = rect.width().min(rect.height());
    let s = size * 0.5;
    let stroke = Stroke::new((size * 0.09).clamp(1.0, 2.0), color);

    // Local helper: map unit coords (-1..1) to screen space.
    let p = |x: f32, y: f32| Pos2::new(c.x + x * s, c.y + y * s);

    match icon {
        Icon::Home => {
            painter.line_segment([p(-0.7, 0.0), p(0.0, -0.7)], stroke);
            painter.line_segment([p(0.0, -0.7), p(0.7, 0.0)], stroke);
            painter.line_segment([p(-0.45, -0.05), p(-0.45, 0.6)], stroke);
            painter.line_segment([p(0.45, -0.05), p(0.45, 0.6)], stroke);
            painter.line_segment([p(-0.45, 0.6), p(0.45, 0.6)], stroke);
        }
        Icon::Settings => {
            painter.circle_stroke(c, s * 0.42, stroke);
            painter.circle_stroke(c, s * 0.13, stroke);
            for i in 0..8 {
                let angle = i as f32 * std::f32::consts::TAU / 8.0;
                let inner = 0.58;
                let outer = 0.82;
                painter.line_segment(
                    [
                        p(angle.cos() * inner, angle.sin() * inner),
                        p(angle.cos() * outer, angle.sin() * outer),
                    ],
                    stroke,
                );
            }
        }
        Icon::Info => {
            painter.circle_stroke(c, s * 0.75, stroke);
            painter.line_segment([p(0.0, -0.25), p(0.0, -0.15)], stroke);
            painter.line_segment([p(0.0, 0.0), p(0.0, 0.45)], stroke);
        }
        Icon::Monitor => {
            painter.rect_stroke(
                Rect::from_center_size(c, Vec2::new(s * 1.5, s * 1.1)),
                egui::CornerRadius::same(3),
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.line_segment([p(0.0, 0.55), p(0.0, 0.78)], stroke);
            painter.line_segment([p(-0.35, 0.8), p(0.35, 0.8)], stroke);
        }
        Icon::Users | Icon::Share => {
            painter.circle_stroke(p(-0.3, -0.25), s * 0.28, stroke);
            painter.circle_stroke(p(0.35, -0.05), s * 0.22, stroke);
            painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                [p(-0.75, 0.65), p(-0.5, 0.15), p(0.1, 0.15), p(0.25, 0.65)],
                false,
                Color32::TRANSPARENT,
                stroke,
            ));
            painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                [p(0.15, 0.62), p(0.3, 0.28), p(0.7, 0.28), p(0.82, 0.62)],
                false,
                Color32::TRANSPARENT,
                stroke,
            ));
        }
        Icon::Link => {
            painter.circle_stroke(p(-0.3, 0.0), s * 0.3, stroke);
            painter.circle_stroke(p(0.3, 0.0), s * 0.3, stroke);
            painter.line_segment([p(-0.1, 0.0), p(0.1, 0.0)], stroke);
        }
        Icon::Refresh => {
            let r = s * 0.6;
            let start = -0.6f32;
            let end = 2.4f32;
            let steps = 18;
            for i in 0..steps {
                let a0 = start + (end - start) * i as f32 / steps as f32;
                let a1 = start + (end - start) * (i + 1) as f32 / steps as f32;
                painter.line_segment(
                    [
                        p(a0.cos() * r / s, a0.sin() * r / s),
                        p(a1.cos() * r / s, a1.sin() * r / s),
                    ],
                    stroke,
                );
            }
            let tip = start + (end - start);
            let tx = tip.cos() * r / s;
            let ty = tip.sin() * r / s;
            painter.line_segment([p(tx, ty), p(tx - 0.18, ty + 0.2)], stroke);
            painter.line_segment([p(tx, ty), p(tx + 0.12, ty + 0.24)], stroke);
        }
        Icon::Mic | Icon::MicrophoneMuted => {
            painter.rect_stroke(
                Rect::from_center_size(p(0.0, -0.15), Vec2::new(s * 0.42, s * 0.72)),
                egui::CornerRadius::same(6),
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                [p(-0.42, -0.05), p(-0.42, 0.4), p(0.42, 0.4), p(0.42, -0.05)],
                false,
                Color32::TRANSPARENT,
                stroke,
            ));
            painter.line_segment([p(0.0, 0.4), p(0.0, 0.72)], stroke);
            painter.line_segment([p(-0.3, 0.74), p(0.3, 0.74)], stroke);
            if icon == Icon::MicrophoneMuted {
                painter.line_segment([p(-0.72, -0.72), p(0.72, 0.72)], stroke);
            }
        }
        Icon::Leave => {
            // Handset rotated 135 degrees, as the mockup's red Leave button.
            painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                [p(-0.6, -0.6), p(-0.15, -0.55), p(0.1, -0.3), p(0.15, 0.0)],
                false,
                Color32::TRANSPARENT,
                stroke,
            ));
            painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                [p(-0.6, -0.6), p(-0.55, -0.1), p(-0.3, 0.15), p(0.0, 0.2)],
                false,
                Color32::TRANSPARENT,
                stroke,
            ));
            painter.line_segment([p(-0.1, 0.15), p(0.35, 0.62)], stroke);
            painter.line_segment([p(0.1, 0.4), p(0.55, 0.55)], stroke);
        }
        Icon::Check => {
            painter.line_segment([p(-0.45, 0.05), p(-0.1, 0.42)], stroke);
            painter.line_segment([p(-0.1, 0.42), p(0.5, -0.4)], stroke);
        }
        Icon::Warning => {
            painter.add(egui::epaint::PathShape::convex_polygon(
                vec![p(0.0, -0.7), p(0.78, 0.62), p(-0.78, 0.62)],
                Color32::TRANSPARENT,
                Stroke::NONE,
            ));
            painter.line_segment([p(0.0, -0.7), p(0.78, 0.62)], stroke);
            painter.line_segment([p(0.78, 0.62), p(-0.78, 0.62)], stroke);
            painter.line_segment([p(-0.78, 0.62), p(0.0, -0.7)], stroke);
            painter.line_segment([p(0.0, -0.15), p(0.0, 0.25)], stroke);
            painter.line_segment([p(0.0, 0.45), p(0.0, 0.5)], stroke);
        }
        Icon::Close => {
            painter.line_segment([p(-0.45, -0.45), p(0.45, 0.45)], stroke);
            painter.line_segment([p(0.45, -0.45), p(-0.45, 0.45)], stroke);
        }
        Icon::ChevronRight => {
            painter.line_segment([p(-0.25, -0.45), p(0.28, 0.0)], stroke);
            painter.line_segment([p(0.28, 0.0), p(-0.25, 0.45)], stroke);
        }
        Icon::ArrowRight => {
            painter.line_segment([p(-0.6, 0.0), p(0.55, 0.0)], stroke);
            painter.line_segment([p(0.2, -0.35), p(0.55, 0.0)], stroke);
            painter.line_segment([p(0.55, 0.0), p(0.2, 0.35)], stroke);
        }
        Icon::Copy => {
            painter.rect_stroke(
                Rect::from_center_size(p(-0.15, -0.15), Vec2::new(s * 0.9, s * 0.9)),
                egui::CornerRadius::same(3),
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.rect_stroke(
                Rect::from_center_size(p(0.2, 0.2), Vec2::new(s * 0.9, s * 0.9)),
                egui::CornerRadius::same(3),
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        Icon::Signal => {
            for (i, h) in [0.35f32, 0.55, 0.75, 0.95].iter().enumerate() {
                let x = -0.6 + i as f32 * 0.4;
                painter.line_segment([p(x, 0.6), p(x, 0.6 - h)], stroke);
            }
        }
    }
}

/// Draw an icon badge: a filled circle with the glyph centred.
pub fn circle_button(
    painter: &egui::Painter,
    rect: Rect,
    icon: Icon,
    fill: Color32,
    glyph: Color32,
) {
    painter.circle_filled(rect.center(), rect.width() * 0.5, fill);
    draw(painter, rect.shrink(rect.width() * 0.28), icon, glyph);
}
