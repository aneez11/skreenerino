//! Screen rendering: Home (discover/join/share) and Meeting (grid and controls).

use egui::{Color32, RichText, Vec2};

use crate::app::{format_elapsed, quality_color, section_label, UiCommand};
use skreen_app::model::Screen;

/// Draw whichever screen is active and report the user's intent back to `app`.
pub fn draw(
    ctx: &egui::Context,
    app: &mut crate::app::App,
    commands: &mut Vec<UiCommand>,
    new_target: &mut Option<String>,
    reset_recent: &mut bool,
) {
    match app.screen().clone() {
        Screen::Home => draw_home(ctx, app, commands, new_target, reset_recent),
        Screen::Meeting => draw_meeting(ctx, app, commands, new_target, reset_recent),
    }
}

fn draw_home(
    ctx: &egui::Context,
    app: &mut crate::app::App,
    commands: &mut Vec<UiCommand>,
    new_target: &mut Option<String>,
    reset_recent: &mut bool,
) {
    egui::TopBottomPanel::top("home-top")
        .frame(
            egui::Frame::new()
                .fill(crate::theme::SURFACE)
                .inner_margin(egui::Margin::symmetric(20, 14)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Skreenerino").size(20.0).strong());
                ui.add_space(4.0);
                ui.label(
                    RichText::new("share your screen on this network")
                        .size(12.0)
                        .color(crate::theme::TEXT_SECONDARY),
                );
            });
        });

    let mut start_share: Option<(Option<String>, u32, u32, f32, bool)> = None;
    let mut rescan = false;
    let mut join: Option<String> = None;
    let mut clear_recent = false;

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(crate::theme::SURFACE)
                .inner_margin(egui::Margin::symmetric(20, 16)),
        )
        .show(ctx, |ui| {
            ui.columns(2, |cols| {
                {
                    let left = &mut cols[0];
                    let (settings, home) = app.home_inputs();
                    let room = &mut home.room;

                    section_label(left, "Start a room");
                    left.add_space(4.0);
                    left.horizontal(|ui| {
                        ui.label("Room");
                        ui.add(
                            egui::TextEdit::singleline(room)
                                .hint_text("classroom")
                                .desired_width(180.0),
                        );
                    });
                    left.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut settings.fps)
                                .range(5..=60)
                                .prefix("fps "),
                        );
                        ui.add(
                            egui::DragValue::new(&mut settings.bitrate)
                                .range(500_000..=20_000_000)
                                .speed(100_000)
                                .prefix("bps "),
                        );
                    });
                    left.add(
                        egui::Slider::new(&mut settings.scale, 0.25..=1.0)
                            .text("scale")
                            .fixed_decimals(2),
                    );
                    left.checkbox(&mut settings.audio, "Send microphone audio");
                    left.add_space(10.0);
                    left.horizontal(|ui| {
                        if ui
                            .add(egui::Button::new(
                                RichText::new("Share my screen").color(Color32::BLACK),
                            ))
                            .highlight()
                            .clicked()
                        {
                            start_share = Some((
                                room.clone()
                                    .trim()
                                    .to_string()
                                    .is_empty()
                                    .then_some(None)
                                    .flatten()
                                    .or_else(|| Some(room.trim().to_string()))
                                    .filter(|r| !r.is_empty()),
                                settings.fps,
                                settings.bitrate,
                                settings.scale,
                                settings.audio,
                            ));
                        }
                        if ui.button("Refresh devices").clicked() {
                            rescan = true;
                        }
                    });

                    left.add_space(18.0);
                    section_label(left, "Join a room");
                    left.add_space(4.0);
                    let manual = &mut app.home_inputs().1.manual;
                    let response = left.add(
                        egui::TextEdit::singleline(manual)
                            .hint_text("host:port or room name")
                            .desired_width(220.0),
                    );
                    if let Some(err) = app.home().manual_error.as_deref() {
                        left.label(RichText::new(err).size(11.0).color(crate::theme::DANGER));
                    }
                    let submit = (response.lost_focus()
                        && left.input(|i| i.key_pressed(egui::Key::Enter)))
                        || left.button("Join").clicked();
                    if submit {
                        let target = app.home_mut().validate_manual();
                        if target.is_some() {
                            join = target;
                        }
                    }

                    left.add_space(12.0);
                    if !app.settings_mut().recent.is_empty() {
                        section_label(left, "Recent");
                        let recent = app.settings_mut().recent.clone();
                        for target in recent {
                            if left.button(RichText::new(&target).size(12.0)).clicked() {
                                join = Some(target);
                            }
                        }
                        if left.small_button("Clear recent").clicked() {
                            clear_recent = true;
                        }
                    }
                }

                {
                    let right = &mut cols[1];
                    section_label(right, "On this network");
                    right.add_space(4.0);
                    let home = app.home();
                    if home.peers.is_empty() {
                        right.label(
                            RichText::new(if home.scanning {
                                "Scanning for devices…"
                            } else {
                                "No devices found yet. Refresh, or join by address."
                            })
                            .size(12.0)
                            .color(crate::theme::TEXT_SECONDARY),
                        );
                    } else {
                        let peers = home.peers.clone();
                        egui::ScrollArea::vertical()
                            .max_height(320.0)
                            .show(right, |ui| {
                                for peer in peers {
                                    let text = RichText::new(format!(
                                        "{}  ·  {}",
                                        peer.device,
                                        peer.subtitle()
                                    ))
                                    .size(13.0);
                                    if ui.button(text).clicked() {
                                        join = Some(peer.endpoint.clone());
                                    }
                                }
                            });
                    }
                }
            });
        });

    if let Some((room, fps, bitrate, scale, audio)) = start_share {
        commands.push(UiCommand::StartShare {
            room,
            name: Some(app.settings_mut().display_name.clone()),
            fps,
            bitrate,
            scale,
            audio,
        });
    }
    if rescan {
        app.home_mut().scanning = true;
        commands.push(UiCommand::Rescan);
    }
    if let Some(target) = join {
        commands.push(UiCommand::Join {
            target: target.clone(),
        });
        *new_target = Some(target);
    }
    if clear_recent {
        *reset_recent = true;
    }
}

fn draw_meeting(
    ctx: &egui::Context,
    app: &mut crate::app::App,
    commands: &mut Vec<UiCommand>,
    new_target: &mut Option<String>,
    reset_recent: &mut bool,
) {
    let _ = (new_target, reset_recent);
    let stats = app.stats();
    let streams = app.streams();
    let sharing = app.sharing();

    egui::TopBottomPanel::top("meeting-top")
        .frame(
            egui::Frame::new()
                .fill(crate::theme::SURFACE)
                .inner_margin(egui::Margin::symmetric(18, 12)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Skreenerino").size(15.0).strong());
                ui.label(
                    RichText::new(if sharing { "sharing" } else { "watching" })
                        .size(12.0)
                        .color(crate::theme::TEXT_SECONDARY),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let dot = quality_color(stats.fps);
                    ui.label(RichText::new("â—").color(dot));
                    ui.label(
                        RichText::new(format!("{:.1} fps", stats.fps))
                            .size(12.0)
                            .color(crate::theme::TEXT_SECONDARY),
                    );
                    ui.label(
                        RichText::new(format!("{} stream(s)", streams.len()))
                            .size(12.0)
                            .color(crate::theme::TEXT_SECONDARY),
                    );
                });
            });
        });

    egui::TopBottomPanel::bottom("meeting-controls")
        .frame(
            egui::Frame::new()
                .fill(crate::theme::SURFACE_RAISED)
                .inner_margin(egui::Margin::symmetric(18, 12)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Back").clicked() {
                    app.set_screen(Screen::Home);
                }
                ui.separator();
                ui.label(
                    RichText::new(format!(
                        "{} frames Â· {} datagrams Â· decode {} ms Â· rtt {} ms",
                        stats.frames, stats.datagrams, stats.decode_ms, stats.rtt_ms
                    ))
                    .size(12.0)
                    .color(crate::theme::TEXT_SECONDARY),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let leave = egui::Button::new(RichText::new("Leave").color(Color32::WHITE))
                        .fill(crate::theme::DANGER);
                    if ui.add(leave).clicked() {
                        commands.push(UiCommand::Leave);
                        app.set_screen(Screen::Home);
                    }
                    if sharing && ui.button("Stop sharing").clicked() {
                        commands.push(UiCommand::StopShare);
                        app.set_screen(Screen::Home);
                    }
                    ui.label(
                        RichText::new(format_elapsed(app.started_secs()))
                            .size(12.0)
                            .monospace()
                            .color(crate::theme::TEXT_SECONDARY),
                    );
                });
            });
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(Color32::from_rgb(0x0C, 0x0C, 0x0E)))
        .show(ctx, |ui| {
            let area = ui.max_rect();
            if streams.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        RichText::new("Waiting for the first frameâ€¦")
                            .color(crate::theme::TEXT_SECONDARY),
                    );
                });
                return;
            }

            let layout = if streams.len() == 1 {
                skreen_render::layout::Layout::Single
            } else {
                skreen_render::layout::Layout::Grid
            };
            let names: Vec<String> = streams.iter().map(|(n, _, _)| n.clone()).collect();
            let tiles = skreen_render::layout::TileLayout::compute(layout, &names);
            let gap = 8.0;

            for tile in &tiles.tiles {
                let Some((_, id, size)) = streams.iter().find(|(n, _, _)| *n == tile.stream) else {
                    continue;
                };
                let rect = egui::Rect::from_min_size(
                    egui::pos2(
                        area.left() + tile.rect.x * area.width() + gap * 0.5,
                        area.top() + tile.rect.y * area.height() + gap * 0.5,
                    ),
                    Vec2::new(
                        (tile.rect.w * area.width() - gap).max(1.0),
                        (tile.rect.h * area.height() - gap).max(1.0),
                    ),
                );
                ui.painter().rect_filled(
                    rect,
                    egui::CornerRadius::same(6),
                    crate::theme::SURFACE_RAISED,
                );
                let image = letterbox(rect, size.width, size.height);
                ui.painter().image(
                    *id,
                    image,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
                if streams.len() > 1 {
                    ui.painter().text(
                        egui::pos2(rect.left() + 8.0, rect.top() + 6.0),
                        egui::Align2::LEFT_TOP,
                        &tile.stream,
                        egui::FontId::proportional(11.0),
                        crate::theme::TEXT_SECONDARY,
                    );
                }
            }
        });
}

/// Fit a source aspect ratio inside `outer`, centred.
fn letterbox(outer: egui::Rect, width: u32, height: u32) -> egui::Rect {
    let aspect = if height == 0 {
        16.0 / 9.0
    } else {
        width as f32 / height as f32
    };
    let current = outer.width() / outer.height().max(1.0);
    if current > aspect {
        let w = outer.height() * aspect;
        let x = outer.left() + (outer.width() - w) * 0.5;
        egui::Rect::from_min_size(egui::pos2(x, outer.top()), Vec2::new(w, outer.height()))
    } else {
        let h = outer.width() / aspect;
        let y = outer.top() + (outer.height() - h) * 0.5;
        egui::Rect::from_min_size(egui::pos2(outer.left(), y), Vec2::new(outer.width(), h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letterbox_pads_wide_rects() {
        let outer = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 100.0));
        let fitted = letterbox(outer, 1920, 1080);
        assert!(fitted.width() < outer.width());
        assert!((fitted.center().x - outer.center().x).abs() < 0.01);
    }

    #[test]
    fn letterbox_pads_tall_rects() {
        let outer = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(160.0, 400.0));
        let fitted = letterbox(outer, 1920, 1080);
        assert!(fitted.height() < outer.height());
        assert!((fitted.center().y - outer.center().y).abs() < 0.01);
    }
}
