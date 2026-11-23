//! Dialogs from the mockup: share picker, pair device, consent request, toast,
//! and the stop-sharing confirmation.

use egui::{Color32, RichText, Sense, Vec2};

use crate::app::{NavPage, UiCommand};
use crate::icons::{self, Icon};
use crate::theme::{Palette, CONTROL_H};
use crate::widgets::{self, ButtonKind};

/// Draw any open dialog on top of the current screen.
pub fn draw(
    ctx: &egui::Context,
    app: &mut crate::app::App,
    p: &Palette,
    commands: &mut Vec<UiCommand>,
) {
    if app.share_picker_open() {
        share_picker(ctx, app, p, commands);
    }
    if app.pair_open() {
        pair_device(ctx, app, p);
    }
    if app.confirm_exit() {
        confirm_exit(ctx, app, p, commands);
    }
}

/// Common dialog chrome: title row with a close button and a card body.
fn dialog(
    ctx: &egui::Context,
    p: &Palette,
    title: &str,
    width: f32,
    body: impl FnOnce(&mut egui::Ui),
) -> bool {
    let mut closed = false;
    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .title_bar(false)
        .default_width(width)
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .frame(
            egui::Frame::new()
                .fill(p.surface())
                .stroke(egui::Stroke::new(1.0, p.stroke()))
                .corner_radius(p.card_radius())
                .inner_margin(egui::Margin::same(18)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).size(17.0).strong().color(p.text()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (rect, response) =
                        ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
                    icons::draw(
                        ui.painter(),
                        rect.shrink(6.0),
                        Icon::Close,
                        if response.hovered() {
                            p.text()
                        } else {
                            p.muted()
                        },
                    );
                    if response.clicked() {
                        closed = true;
                    }
                });
            });
            ui.add_space(10.0);
            body(ui);
        });
    closed
}

// ---------------------------------------------------------------------------
// Share picker
// ---------------------------------------------------------------------------

fn share_picker(
    ctx: &egui::Context,
    app: &mut crate::app::App,
    p: &Palette,
    commands: &mut Vec<UiCommand>,
) {
    let mut share = false;
    let mut cancel = false;
    let mut chosen = app.share_monitor();
    let mut fps = app.settings_mut().fps;
    let mut scale = app.settings_mut().scale;
    let mut audio = app.settings_mut().audio;
    let room = app.home().room_value();
    let name = app.settings_mut().display_name.clone();

    let closed = dialog(ctx, p, "Share Picker", 430.0, |ui| {
        ui.label(
            RichText::new("Select a screen to share")
                .size(13.0)
                .strong()
                .color(p.text()),
        );
        ui.add_space(6.0);

        // Two monitor thumbnails, as in the mockup.
        ui.horizontal(|ui| {
            for (index, label) in [(0usize, "Monitor 1"), (1usize, "Monitor 2")] {
                let selected = chosen == index;
                let (rect, response) =
                    ui.allocate_exact_size(Vec2::new(190.0, 118.0), Sense::click());
                ui.painter().rect(
                    rect,
                    p.card_radius(),
                    p.surface_alt(),
                    if selected {
                        egui::Stroke::new(2.0, p.accent())
                    } else {
                        egui::Stroke::new(1.0, p.stroke())
                    },
                    egui::StrokeKind::Inside,
                );
                // Miniature "screen" content.
                let inner = rect.shrink(10.0);
                ui.painter().rect_filled(
                    inner,
                    egui::CornerRadius::same(4),
                    if index == 0 {
                        Color32::from_rgb(0x1E, 0x2A, 0x38)
                    } else {
                        Color32::from_rgb(0x2A, 0x38, 0x4A)
                    },
                );
                for line in 0..5 {
                    let y = inner.top() + 8.0 + line as f32 * 7.0;
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(
                            egui::pos2(inner.left() + 8.0, y),
                            Vec2::new(inner.width() * 0.6, 3.0),
                        ),
                        1.0,
                        Color32::from_rgb(0x59, 0x8C, 0xC0),
                    );
                }
                if selected {
                    ui.painter().circle_filled(
                        egui::pos2(rect.right() - 14.0, rect.top() + 14.0),
                        8.0,
                        p.accent(),
                    );
                    icons::draw(
                        ui.painter(),
                        egui::Rect::from_center_size(
                            egui::pos2(rect.right() - 14.0, rect.top() + 14.0),
                            Vec2::splat(11.0),
                        ),
                        Icon::Check,
                        p.on_accent(),
                    );
                }
                ui.painter().text(
                    egui::pos2(rect.left(), rect.bottom() + 4.0),
                    egui::Align2::LEFT_TOP,
                    label,
                    egui::FontId::proportional(12.0),
                    p.text(),
                );
                ui.painter().text(
                    egui::pos2(rect.left(), rect.bottom() + 19.0),
                    egui::Align2::LEFT_TOP,
                    "1920 x 1080",
                    egui::FontId::proportional(10.5),
                    p.muted(),
                );
                if response.clicked() {
                    chosen = index;
                }
            }
        });
        ui.add_space(26.0);

        ui.horizontal(|ui| {
            ui.label(RichText::new("Quality Preset").size(12.0).color(p.text()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                widgets::segmented(
                    ui,
                    p,
                    &[(15u32, "15 FPS"), (30, "30 FPS"), (60, "60 FPS")],
                    &mut fps,
                );
            });
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Resolution Scale").size(12.0).color(p.text()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut percent = (scale * 100.0).round() as u32;
                if widgets::segmented(
                    ui,
                    p,
                    &[(100u32, "100%"), (75, "75%"), (50, "50%")],
                    &mut percent,
                ) {
                    scale = percent as f32 / 100.0;
                }
            });
        });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            widgets::toggle(ui, p, &mut audio);
            ui.label(
                RichText::new("Share microphone audio")
                    .size(12.0)
                    .color(p.text()),
            );
        });
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::button(ui, p, ButtonKind::Primary, "Share").clicked() {
                    share = true;
                }
                if widgets::button(ui, p, ButtonKind::Ghost, "Cancel").clicked() {
                    cancel = true;
                }
            });
        });
    });

    if closed || cancel {
        app.set_share_picker(false);
    }
    if share {
        app.set_share_monitor(chosen);
        let settings = app.settings_mut();
        settings.fps = fps;
        settings.scale = scale;
        settings.audio = audio;
        commands.push(UiCommand::StartShare {
            room,
            name: Some(name),
            fps,
            bitrate: settings.bitrate,
            scale,
            audio,
        });
        app.set_share_picker(false);
        app.set_screen(skreen_app::model::Screen::Meeting);
    }
}

// ---------------------------------------------------------------------------
// Pair a device
// ---------------------------------------------------------------------------

fn pair_device(ctx: &egui::Context, app: &mut crate::app::App, p: &Palette) {
    let mut close = false;
    let code = app.pair_code().to_string();
    let closed = dialog(ctx, p, "Pair a Device", 330.0, |ui| {
        ui.label(
            RichText::new("Enter this 6-digit code on the other device to pair and join.")
                .size(12.0)
                .color(p.muted()),
        );
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            for ch in code.chars() {
                widgets::digit_box(ui, p, ch);
            }
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if widgets::button(ui, p, ButtonKind::Ghost, "Copy Code").clicked() {
                ui.ctx().copy_text(code.clone());
            }
            if widgets::button(ui, p, ButtonKind::Ghost, "Regenerate").clicked() {
                // Pseudorandom 6-digit code; good enough for a local pairing hint.
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos())
                    .unwrap_or(0);
                app.set_pair_code(format!("{:06}", nanos % 1_000_000));
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::button(ui, p, ButtonKind::Secondary, "Close").clicked() {
                    close = true;
                }
            });
        });
    });
    if closed || close {
        app.set_pair_open(false);
    }
}

// ---------------------------------------------------------------------------
// Stop-sharing confirmation
// ---------------------------------------------------------------------------

fn confirm_exit(
    ctx: &egui::Context,
    app: &mut crate::app::App,
    p: &Palette,
    commands: &mut Vec<UiCommand>,
) {
    let mut close = false;
    let mut stop = false;
    let mut keep = false;
    let closed = dialog(ctx, p, "Stop sharing and exit?", 420.0, |ui| {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::hover());
            icons::draw(ui.painter(), rect.shrink(4.0), Icon::Warning, p.warning());
            ui.label(
                RichText::new("Are you sure you want to stop sharing and close?")
                    .size(12.5)
                    .color(p.text()),
            );
        });
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if widgets::button(ui, p, ButtonKind::Ghost, "Keep sharing").clicked() {
                keep = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::button(ui, p, ButtonKind::Primary, "Stop & Exit").clicked() {
                    stop = true;
                }
                if widgets::button(ui, p, ButtonKind::Secondary, "Cancel").clicked() {
                    close = true;
                }
            });
        });
    });

    if closed || close || keep {
        app.set_confirm_exit(false);
    }
    if stop {
        commands.push(UiCommand::StopShare);
        app.set_confirm_exit(false);
        app.set_screen(skreen_app::model::Screen::Home);
    }
}

/// A celebration/toast card, used when a share starts.
pub fn toast(ctx: &egui::Context, p: &Palette, text: &str) {
    egui::Area::new(egui::Id::new("toast"))
        .anchor(egui::Align2::CENTER_TOP, Vec2::new(0.0, 56.0))
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(p.surface())
                .stroke(egui::Stroke::new(1.0, p.stroke()))
                .corner_radius(p.card_radius())
                .inner_margin(egui::Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
                        ui.painter().circle_filled(rect.center(), 10.0, p.success());
                        icons::draw(ui.painter(), rect.shrink(5.0), Icon::Check, Color32::WHITE);
                        ui.label(RichText::new(text).size(12.5).color(p.text()));
                    });
                });
        });
}

/// Sidebar page body for Settings/About, kept here to avoid another module.
pub fn settings_page(ui: &mut egui::Ui, app: &mut crate::app::App, p: &Palette) {
    let settings = app.settings_mut();
    ui.heading(RichText::new("Settings").size(18.0).color(p.text()));
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Display name").size(12.5).color(p.text()));
        ui.add(egui::TextEdit::singleline(&mut settings.display_name).desired_width(220.0));
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("Default FPS").size(12.5).color(p.text()));
        ui.add(egui::DragValue::new(&mut settings.fps).range(5..=60));
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("Default bitrate").size(12.5).color(p.text()));
        ui.add(
            egui::DragValue::new(&mut settings.bitrate)
                .range(500_000..=20_000_000)
                .speed(100_000),
        );
    });
    ui.add(egui::Slider::new(&mut settings.scale, 0.25..=1.0).text("default scale"));
    ui.checkbox(&mut settings.audio, "Share microphone audio by default");
    ui.add_space(12.0);
    if widgets::button(ui, p, ButtonKind::Primary, "Save").clicked() {
        let _ = app
            .settings_mut()
            .save(&skreen_app::model::Settings::default_path());
    }
    let _ = NavPage::Home;
    let _ = CONTROL_H;
}

/// About body.
pub fn about_page(ui: &mut egui::Ui, p: &Palette) {
    ui.heading(
        RichText::new("About Skreenerino")
            .size(18.0)
            .color(p.text()),
    );
    ui.add_space(8.0);
    ui.label(
        RichText::new("Local screen sharing for Windows.")
            .size(13.0)
            .color(p.text()),
    );
    ui.add_space(6.0);
    for line in [
        "No cloud. No accounts. No internet.",
        "Works on your local network.",
        "Fast. Simple. Reliable.",
        "Designed for Windows 11.",
    ] {
        ui.label(
            RichText::new(format!("- {line}"))
                .size(12.0)
                .color(p.muted()),
        );
    }
    ui.add_space(10.0);
    ui.label(
        RichText::new(format!("version {}", env!("CARGO_PKG_VERSION")))
            .size(11.0)
            .color(p.faint()),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_constants_are_reasonable() {
        assert!(CONTROL_H > 20.0);
        let _ = NavPage::About;
        let _ = Color32::WHITE;
    }
}
