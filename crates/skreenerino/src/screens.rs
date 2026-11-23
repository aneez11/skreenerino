//! Screen rendering, matched to `docs/design/Skreenerino Screen Sharing UI Mockup.png`.

use egui::{Color32, RichText, Sense, Vec2};

use crate::app::{format_elapsed, quality_color, NavPage, UiCommand};
use crate::icons::{self, Icon};
use crate::theme::{self, Palette, CONTROL_H, RAIL_W, SIDEBAR_W};
use crate::widgets::{self, ButtonKind};
use skreen_app::model::Screen;

/// Draw the active screen plus any open dialogs.
pub fn draw(
    ctx: &egui::Context,
    app: &mut crate::app::App,
    commands: &mut Vec<UiCommand>,
    new_target: &mut Option<String>,
    reset_recent: &mut bool,
) {
    let p = app.palette();
    match app.screen().clone() {
        Screen::Home => draw_home(ctx, app, &p, commands, new_target, reset_recent),
        Screen::Meeting => draw_meeting(ctx, app, &p, commands),
    }
    crate::modals::draw(ctx, app, &p, commands);
}

// ---------------------------------------------------------------------------
// Home
// ---------------------------------------------------------------------------

fn draw_home(
    ctx: &egui::Context,
    app: &mut crate::app::App,
    p: &Palette,
    commands: &mut Vec<UiCommand>,
    new_target: &mut Option<String>,
    reset_recent: &mut bool,
) {
    title_bar(ctx, app, p, false);

    // Sidebar -----------------------------------------------------------------
    egui::SidePanel::left("nav")
        .exact_width(SIDEBAR_W)
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(p.window())
                .inner_margin(egui::Margin::symmetric(10, 8)),
        )
        .show(ctx, |ui| {
            let mut page = app.page();
            if widgets::nav_item(ui, p, Icon::Home, "Home", page == NavPage::Home).clicked() {
                page = NavPage::Home;
            }
            if widgets::nav_item(ui, p, Icon::Settings, "Settings", page == NavPage::Settings)
                .clicked()
            {
                page = NavPage::Settings;
            }
            if widgets::nav_item(ui, p, Icon::Info, "About", page == NavPage::About).clicked() {
                page = NavPage::About;
            }
            app.set_page(page);
        });

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(p.window())
                .inner_margin(egui::Margin::symmetric(8, 8)),
        )
        .show(ctx, |ui| {
            if app.page() != NavPage::Home {
                egui::ScrollArea::vertical().show(ui, |ui| match app.page() {
                    NavPage::Settings => crate::modals::settings_page(ui, app, p),
                    NavPage::About => crate::modals::about_page(ui, p),
                    NavPage::Home => {}
                });
                return;
            }
            egui::SidePanel::right("hero")
                .exact_width(300.0)
                .resizable(false)
                .frame(egui::Frame::new().inner_margin(egui::Margin::same(0)))
                .show_inside(ui, |ui| draw_hero(ui, p));

            let mut start_share = false;
            let mut join: Option<String> = None;
            let mut open_pair = false;
            let mut rescan = false;
            let mut clear_recent = false;

            egui::ScrollArea::vertical().show(ui, |ui| {
                // Primary + secondary actions
                if widgets::button_icon(
                    ui,
                    p,
                    ButtonKind::Primary,
                    Icon::Monitor,
                    "Share my screen",
                    Some(ui.available_width()),
                )
                .clicked()
                {
                    start_share = true;
                }
                ui.add_space(8.0);
                if widgets::button_icon(
                    ui,
                    p,
                    ButtonKind::Secondary,
                    Icon::Users,
                    "Join a room",
                    Some(ui.available_width()),
                )
                .clicked()
                {
                    if let Some(target) = app.home_mut().validate_manual() {
                        join = Some(target);
                    }
                }
                ui.add_space(8.0);
                if widgets::button_icon(
                    ui,
                    p,
                    ButtonKind::Secondary,
                    Icon::Link,
                    "Pair a device",
                    Some(ui.available_width()),
                )
                .clicked()
                {
                    open_pair = true;
                }

                ui.add_space(16.0);
                if widgets::section_header(ui, p, "Nearby devices", Some((Icon::Refresh, "Rescan")))
                {
                    rescan = true;
                }
                ui.add_space(2.0);

                let peers = app.home().peers.clone();
                if peers.is_empty() {
                    ui.label(
                        RichText::new(if app.home().scanning {
                            "Scanning..."
                        } else {
                            "No devices found. Refresh, or connect manually."
                        })
                        .size(12.0)
                        .color(p.muted()),
                    );
                } else {
                    for peer in peers {
                        let subtitle = peer.subtitle();
                        if device_row(ui, p, &peer.device, &subtitle) {
                            join = Some(peer.endpoint.clone());
                        }
                    }
                }

                ui.add_space(16.0);
                ui.label(
                    RichText::new("Connect manually")
                        .size(13.0)
                        .strong()
                        .color(p.text()),
                );
                ui.add_space(4.0);
                let home = &mut app.home_inputs().1.manual;
                let row_h = CONTROL_H;
                let (rect, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), Sense::hover());
                ui.painter().rect(
                    rect,
                    p.control_radius(),
                    p.surface(),
                    egui::Stroke::new(1.0, p.stroke()),
                    egui::StrokeKind::Inside,
                );
                let arrow_w = 28.0;
                let edit_rect = rect.shrink2(Vec2::new(8.0, 4.0));
                ui.scope_builder(egui::UiBuilder::new().max_rect(edit_rect), |ui| {
                    ui.add(
                        egui::TextEdit::singleline(home)
                            .hint_text("host:port (e.g. 192.168.1.10:8080)")
                            .frame(false)
                            .desired_width(edit_rect.width() - arrow_w),
                    );
                });
                let arrow_rect = egui::Rect::from_min_size(
                    egui::pos2(rect.right() - arrow_w - 4.0, rect.top() + 4.0),
                    Vec2::new(arrow_w, row_h - 8.0),
                );
                let arrow = ui.interact(arrow_rect, ui.id().with("manual-go"), Sense::click());
                icons::draw(
                    ui.painter(),
                    arrow_rect.shrink(7.0),
                    Icon::ArrowRight,
                    if arrow.hovered() {
                        p.accent()
                    } else {
                        p.muted()
                    },
                );
                if arrow.clicked() {
                    if let Some(target) = app.home_mut().validate_manual() {
                        join = Some(target);
                    }
                }
                if let Some(err) = app.home().manual_error.clone() {
                    ui.label(RichText::new(err).size(11.0).color(p.danger_text()));
                }

                let recent = app.settings_mut().recent.clone();
                if !recent.is_empty() {
                    ui.add_space(14.0);
                    ui.label(
                        RichText::new("Recent rooms")
                            .size(13.0)
                            .strong()
                            .color(p.text()),
                    );
                    ui.add_space(2.0);
                    for target in recent {
                        if recent_row(ui, p, &target) {
                            join = Some(target);
                        }
                    }
                    if ui
                        .add(egui::Button::new(RichText::new("Clear").size(11.0)).frame(false))
                        .clicked()
                    {
                        clear_recent = true;
                    }
                }
            });

            if start_share {
                app.set_share_picker(true);
            }
            if open_pair {
                app.set_pair_open(true);
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
        });
}

fn draw_hero(ui: &mut egui::Ui, p: &Palette) {
    let rect = ui.max_rect();
    widgets::hero_gradient(ui.painter(), rect);
    let center = rect.center();
    // Monitor glyph in a translucent rounded square.
    let badge =
        egui::Rect::from_center_size(egui::pos2(center.x, center.y - 90.0), Vec2::splat(86.0));
    ui.painter().rect_filled(
        badge,
        egui::CornerRadius::same(18),
        Color32::from_white_alpha(28),
    );
    icons::draw(
        ui.painter(),
        egui::Rect::from_center_size(badge.center(), Vec2::splat(46.0)),
        Icon::Monitor,
        Color32::WHITE,
    );
    ui.painter().text(
        egui::pos2(center.x, center.y - 8.0),
        egui::Align2::CENTER_CENTER,
        "Skreenerino",
        egui::FontId::new(30.0, egui::FontFamily::Proportional),
        Color32::WHITE,
    );
    ui.painter().text(
        egui::pos2(center.x, center.y + 26.0),
        egui::Align2::CENTER_CENTER,
        "Share. Collaborate. Locally.",
        egui::FontId::new(15.0, egui::FontFamily::Proportional),
        Color32::from_white_alpha(220),
    );
    let text_rect = egui::Rect::from_center_size(
        egui::pos2(center.x, center.y + 96.0),
        Vec2::new(rect.width() - 48.0, 48.0),
    );
    ui.painter().text(
        text_rect.center(),
        egui::Align2::CENTER_CENTER,
        "Project your screen to other devices\non the same network.",
        egui::FontId::new(12.5, egui::FontFamily::Proportional),
        Color32::from_white_alpha(190),
    );
    let _ = p;
}

/// Device row: green dot, name, room subtitle, Join button.
fn device_row(ui: &mut egui::Ui, p: &Palette, name: &str, subtitle: &str) -> bool {
    let mut join = false;
    let height = 44.0;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, p.control_radius(), p.hover());
    }
    ui.painter().circle_filled(
        egui::pos2(rect.left() + 12.0, rect.center().y),
        4.0,
        p.success(),
    );
    ui.painter().text(
        egui::pos2(rect.left() + 24.0, rect.center().y - 8.0),
        egui::Align2::LEFT_CENTER,
        name,
        egui::FontId::proportional(12.5),
        p.text(),
    );
    ui.painter().text(
        egui::pos2(rect.left() + 24.0, rect.center().y + 8.0),
        egui::Align2::LEFT_CENTER,
        format!("Room: {subtitle}"),
        egui::FontId::proportional(11.0),
        p.muted(),
    );
    let btn_rect = egui::Rect::from_min_size(
        egui::pos2(rect.right() - 62.0, rect.center().y - 13.0),
        Vec2::new(52.0, 26.0),
    );
    let btn = ui.interact(btn_rect, ui.id().with(("join", name)), Sense::click());
    let fill = if btn.hovered() {
        p.accent_hover()
    } else {
        p.accent()
    };
    ui.painter().rect_filled(btn_rect, p.control_radius(), fill);
    ui.painter().text(
        btn_rect.center(),
        egui::Align2::CENTER_CENTER,
        "Join",
        egui::FontId::proportional(11.5),
        p.on_accent(),
    );
    join = btn.clicked() || response.double_clicked();
    join
}

fn recent_row(ui: &mut egui::Ui, p: &Palette, target: &str) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, p.control_radius(), p.hover());
    }
    icons::draw(
        ui.painter(),
        egui::Rect::from_center_size(
            egui::pos2(rect.left() + 12.0, rect.center().y),
            Vec2::splat(13.0),
        ),
        Icon::ChevronRight,
        p.muted(),
    );
    ui.painter().text(
        egui::pos2(rect.left() + 26.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        target,
        egui::FontId::proportional(12.5),
        p.text(),
    );
    response.clicked()
}

// ---------------------------------------------------------------------------
// Title bar (the mockup's in-window header with the app name and window buttons)
// ---------------------------------------------------------------------------

fn title_bar(ctx: &egui::Context, app: &mut crate::app::App, p: &Palette, meeting: bool) {
    // The OS title bar already provides window controls, so this is a content
    // header: brand on the left, session state on the right.
    egui::TopBottomPanel::top("header")
        .frame(
            egui::Frame::new()
                .fill(p.surface())
                .inner_margin(egui::Margin::symmetric(14, 8)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let logo = egui::Rect::from_center_size(
                    egui::pos2(ui.cursor().left() + 9.0, ui.cursor().center().y),
                    Vec2::splat(18.0),
                );
                icons::draw(ui.painter(), logo, Icon::Monitor, p.accent());
                ui.add_space(18.0);
                ui.label(
                    RichText::new("Skreenerino")
                        .size(13.0)
                        .strong()
                        .color(p.text()),
                );

                if meeting {
                    ui.add_space(24.0);
                    ui.label(
                        RichText::new(format!("Room: {}", app.room_label()))
                            .size(12.0)
                            .color(p.text()),
                    );
                    ui.add_space(10.0);
                    let (dot_rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                    ui.painter()
                        .circle_filled(dot_rect.center(), 4.0, p.success());
                    ui.label(
                        RichText::new(plural_viewers(app.participant_count()))
                            .size(12.0)
                            .color(p.success()),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(format_elapsed(app.started_secs()))
                            .size(12.0)
                            .monospace()
                            .color(p.muted()),
                    );
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if meeting {
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
                        icons::draw(ui.painter(), rect.shrink(3.0), Icon::Signal, p.success());
                    }
                });
            });
        });
}
// Meeting
// ---------------------------------------------------------------------------

fn draw_meeting(
    ctx: &egui::Context,
    app: &mut crate::app::App,
    p: &Palette,
    commands: &mut Vec<UiCommand>,
) {
    title_bar(ctx, app, p, true);

    let stats = app.stats();
    let streams = app.streams();
    let sharing = app.sharing();

    // Participants rail -------------------------------------------------------
    egui::SidePanel::right("participants")
        .exact_width(RAIL_W)
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(p.surface())
                .inner_margin(egui::Margin::symmetric(12, 10)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Participants")
                        .size(13.0)
                        .strong()
                        .color(p.text()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("{}", app.participant_count()))
                            .size(12.0)
                            .color(p.muted()),
                    );
                });
            });
            ui.add_space(6.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                // Presenter first, then each received stream.
                participant_row(
                    ui,
                    p,
                    "You",
                    if sharing { "Presenter" } else { "Viewer" },
                    0,
                    0,
                    true,
                );
                for (name, _, _) in &streams {
                    participant_row(ui, p, name, "Viewer", stats.rtt_ms, 0, false);
                }
            });
        });

    // Status strip ------------------------------------------------------------
    egui::TopBottomPanel::bottom("status")
        .frame(
            egui::Frame::new()
                .fill(p.surface())
                .inner_margin(egui::Margin::symmetric(16, 7)),
        )
        .show(ctx, |ui| {
            let (w, h) = app.resolution().unwrap_or((1920, 1080));
            let mbps = app.settings_mut().bitrate as f32 / 1_000_000.0;
            let parts = [
                format!("H.264"),
                format!("{w}x{h}"),
                format!("{mbps:.1} Mbps"),
                format!("{:.0} FPS", app.settings_mut().fps as f32),
                format!("{}: {:.0}ms", app.timing_label(), app.encode_ms()),
                format!("Dropped: {}", app.dropped()),
            ];
            ui.horizontal(|ui| {
                for (i, part) in parts.iter().enumerate() {
                    if i > 0 {
                        ui.label(RichText::new(" - ").size(11.0).color(p.faint()));
                    }
                    ui.label(RichText::new(part).size(11.5).color(p.muted()));
                }
            });
        });

    // Control bar -------------------------------------------------------------
    egui::TopBottomPanel::bottom("controls")
        .frame(
            egui::Frame::new()
                .fill(p.window())
                .inner_margin(egui::Margin::symmetric(16, 10)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.add_space((ui.available_width() - 300.0).max(0.0) * 0.5);
                let mic = circle_control(ui, p, Icon::MicrophoneMuted, true);
                let _ = mic;
                let share = circle_control(ui, p, Icon::Share, false);
                let people = circle_control(ui, p, Icon::Users, false);
                let gear = circle_control(ui, p, Icon::Settings, false);
                ui.add_space(8.0);
                // Leave pill
                let label = "Leave";
                let galley = ui.painter().layout_no_wrap(
                    label.into(),
                    egui::FontId::proportional(13.0),
                    Color32::WHITE,
                );
                let size = Vec2::new(galley.size().x + 70.0, 36.0);
                let (rect, response) = ui.allocate_exact_size(size, Sense::click());
                let fill = if response.hovered() {
                    Color32::from_rgb(0xA8, 0x24, 0x18)
                } else {
                    p.danger()
                };
                ui.painter()
                    .rect_filled(rect, egui::CornerRadius::same(18), fill);
                icons::draw(
                    ui.painter(),
                    egui::Rect::from_center_size(
                        egui::pos2(rect.left() + 22.0, rect.center().y),
                        Vec2::splat(16.0),
                    ),
                    Icon::Leave,
                    Color32::WHITE,
                );
                ui.painter().text(
                    egui::pos2(rect.center().x + 8.0, rect.center().y),
                    egui::Align2::CENTER_CENTER,
                    label,
                    egui::FontId::proportional(13.0),
                    Color32::WHITE,
                );
                if response.clicked() {
                    if sharing {
                        app.set_confirm_exit(true);
                    } else {
                        commands.push(UiCommand::Leave);
                        app.set_screen(Screen::Home);
                    }
                }
                let _ = (share, people, gear);
            });
        });

    // Video grid --------------------------------------------------------------
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(p.window())
                .inner_margin(egui::Margin::same(8)),
        )
        .show(ctx, |ui| {
            let area = ui.max_rect();
            if streams.is_empty() {
                ui.painter()
                    .rect_filled(area, p.card_radius(), p.surface_alt());
                icons::draw(
                    ui.painter(),
                    egui::Rect::from_center_size(
                        egui::pos2(area.center().x, area.center().y - 22.0),
                        Vec2::splat(38.0),
                    ),
                    Icon::Monitor,
                    p.faint(),
                );
                ui.painter().text(
                    egui::pos2(area.center().x, area.center().y + 22.0),
                    egui::Align2::CENTER_CENTER,
                    "Waiting for the first frame...",
                    egui::FontId::proportional(13.0),
                    p.muted(),
                );
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
                ui.painter()
                    .rect_filled(rect, p.card_radius(), p.surface_alt());
                let image = letterbox(rect, size.width, size.height);
                ui.painter().image(
                    *id,
                    image,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
                // Name chip, top-left, like the mockup.
                let label = if streams.len() == 1 {
                    format!("{} (Presenter)", tile.stream)
                } else {
                    tile.stream.clone()
                };
                let galley = ui.painter().layout_no_wrap(
                    label,
                    egui::FontId::proportional(11.5),
                    Color32::WHITE,
                );
                let chip = egui::Rect::from_min_size(
                    egui::pos2(rect.left() + 8.0, rect.top() + 8.0),
                    Vec2::new(galley.size().x + 16.0, 22.0),
                );
                ui.painter().rect_filled(
                    chip,
                    egui::CornerRadius::same(6),
                    Color32::from_black_alpha(140),
                );
                ui.painter().galley(
                    egui::pos2(chip.left() + 8.0, chip.center().y - galley.size().y * 0.5),
                    galley,
                    Color32::WHITE,
                );
            }
        });

    let _ = quality_color(stats.fps);
}

fn participant_row(
    ui: &mut egui::Ui,
    p: &Palette,
    name: &str,
    role: &str,
    rtt: u32,
    loss: u32,
    presenter: bool,
) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::hover());
    ui.painter().circle_filled(
        egui::pos2(rect.left() + 8.0, rect.center().y),
        4.0,
        p.success(),
    );
    ui.painter().text(
        egui::pos2(rect.left() + 20.0, rect.center().y - 1.0),
        egui::Align2::LEFT_CENTER,
        name,
        egui::FontId::proportional(12.5),
        p.text(),
    );
    // Role badge
    let badge_x = rect.left()
        + 20.0
        + ui.painter()
            .layout_no_wrap(name.to_string(), egui::FontId::proportional(12.5), p.text())
            .size()
            .x
        + 8.0;
    let (fill, fg) = if presenter {
        (p.accent().gamma_multiply(0.18), p.accent())
    } else {
        (p.surface_alt(), p.muted())
    };
    let galley =
        ui.painter()
            .layout_no_wrap(role.to_string(), egui::FontId::proportional(10.5), fg);
    let badge = egui::Rect::from_min_size(
        egui::pos2(badge_x, rect.center().y - 9.0),
        Vec2::new(galley.size().x + 12.0, 18.0),
    );
    ui.painter()
        .rect_filled(badge, egui::CornerRadius::same(9), fill);
    ui.painter().galley(
        egui::pos2(badge.left() + 6.0, badge.center().y - galley.size().y * 0.5),
        galley,
        fg,
    );
    ui.painter().text(
        egui::pos2(rect.right() - 6.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        format!("{rtt}ms  -  {loss}%"),
        egui::FontId::proportional(10.5),
        p.muted(),
    );
}

/// Circular icon control, filled red when `alert` (as the muted mic is).
fn circle_control(ui: &mut egui::Ui, p: &Palette, icon: Icon, alert: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::click());
    let fill = if alert {
        p.danger().gamma_multiply(0.25)
    } else if response.hovered() {
        p.hover()
    } else {
        p.surface()
    };
    ui.painter().circle_filled(rect.center(), 18.0, fill);
    icons::draw(
        ui.painter(),
        rect.shrink(10.0),
        icon,
        if alert { p.danger_text() } else { p.text() },
    );
    response.clicked()
}

fn plural_viewers(n: usize) -> String {
    if n == 1 {
        "1 viewer".to_string()
    } else {
        format!("{n} viewers")
    }
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

    #[test]
    fn letterbox_handles_zero_height() {
        let outer = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(160.0, 90.0));
        let fitted = letterbox(outer, 100, 0);
        assert!(fitted.width() <= outer.width() + 0.01);
    }
}
