//! A small always-on-top panel with the running count, drawn over the game.
//!
//! It is a separate borderless window that ignores the mouse, so clicks go to the game.
//! Only while the player moves or resizes it (Settings > Overlay) does it take the mouse.
//! Windows can only draw it over Fortnite in windowed or borderless fullscreen.

use std::time::{Duration, Instant};

use eframe::egui::{self, RichText, Sense, Stroke, StrokeKind, ViewportBuilder, ViewportCommand, ViewportId};

use crate::app::{ChestApp, Page};
use crate::config::OverlayCorner;
use crate::{platform, theme};

pub const OVERLAY_TITLE: &str = "Fortnite Chest Counter Overlay";
/// The tiny window that carries the lock icon while the overlay ignores the mouse.
const LOCK_TITLE: &str = "Fortnite Chest Counter Overlay Lock";
/// Side of the lock icon in the overlay's top right corner.
const LOCK_SIDE: f32 = 22.0;
/// Size before the first frame has measured the content, at 100%.
const BASE_SIZE: egui::Vec2 = egui::vec2(280.0, 110.0);
pub const MIN_SCALE: f32 = 0.5;
pub const MAX_SCALE: f32 = 2.5;
const SCREEN_MARGIN: f32 = 16.0;
const FORTNITE_CHECK_EVERY: Duration = Duration::from_secs(5);
/// Side of the resize handle in the bottom right corner.
const GRIP: f32 = 16.0;
/// Width of the resize strips along the right and bottom edges.
const EDGE: f32 = 6.0;

#[derive(Default)]
pub struct OverlayState {
    fortnite_running: bool,
    checked_at: Option<Instant>,
    applied_alpha: Option<u8>,
    /// The overlay takes the mouse so it can be moved and resized.
    pub editing: bool,
    /// Where the window has been dragged to. Saved on lock; kept out of the viewport
    /// builder meanwhile so egui does not move the window back while Windows drags it.
    dragged_to: Option<egui::Pos2>,
    /// What the content measured last frame; the window is sized to fit it.
    content: Option<egui::Vec2>,
    /// Pointer, scale and size when the resize handle was grabbed.
    resize_from: Option<(egui::Pos2, Axis, f32, egui::Vec2)>,
    /// Moving was unlocked from the overlay's own lock icon, so it stays unlocked until
    /// the icon locks it again, whatever page the app is on.
    from_icon: bool,
    /// How far the lock window is in showing itself; `None` until it is created.
    lock_window: Option<LockWindow>,
}

/// The lock window opens without a flash: created hidden, then shown fully transparent
/// until it has painted, then given the overlay's opacity.
#[derive(Clone, Copy)]
enum LockWindow {
    /// Shown fully transparent since then.
    Painting(Instant),
    /// Visible at this opacity.
    Up(u8),
}

/// How long the new lock window stays transparent: it is white until its first frame
/// reaches the screen, which takes longer than a couple of frames. The overlay draws the
/// same icon underneath meanwhile.
const LOCK_REVEAL_AFTER: Duration = Duration::from_millis(300);
/// Where the lock window waits while the overlay is unlocked: beyond any screen. It is
/// kept rather than closed, as a new window flashes white when it opens.
const LOCK_PARKED: egui::Pos2 = egui::pos2(-20000.0, -20000.0);

pub fn start_editing(app: &mut ChestApp) {
    app.overlay.editing = true;
    app.overlay.from_icon = false;
    app.overlay.dragged_to = None;
}

/// The lock icon: unlocks moving and resizing, or locks the overlay in place.
fn toggle_from_icon(app: &mut ChestApp) {
    if app.overlay.editing {
        lock(app);
    } else {
        start_editing(app);
        app.overlay.from_icon = true;
    }
}

/// Where the lock icon sits inside an overlay of `size`.
fn lock_offset(size: egui::Vec2) -> egui::Vec2 {
    egui::vec2(size.x - LOCK_SIDE - 4.0, 4.0)
}

/// Ends moving and resizing: saves the place and size and makes the overlay ignore the
/// mouse again.
pub fn lock(app: &mut ChestApp) {
    if !app.overlay.editing {
        return;
    }
    app.overlay.editing = false;
    app.overlay.from_icon = false;
    if let Some(at) = app.overlay.dragged_to.take() {
        app.settings.overlay.position = Some([at.x, at.y]);
    }
    let _ = app.settings.save();
}

fn corner_position(corner: OverlayCorner, size: egui::Vec2, screen: egui::Vec2) -> egui::Pos2 {
    // Room for the taskbar at the bottom.
    let bottom = screen.y - size.y - SCREEN_MARGIN - 48.0;
    let right = screen.x - size.x - SCREEN_MARGIN;
    match corner {
        OverlayCorner::TopLeft => egui::pos2(SCREEN_MARGIN, SCREEN_MARGIN),
        OverlayCorner::TopRight => egui::pos2(right, SCREEN_MARGIN),
        OverlayCorner::BottomLeft => egui::pos2(SCREEN_MARGIN, bottom),
        OverlayCorner::BottomRight => egui::pos2(right, bottom),
    }
}

/// Which way a resize handle pulls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    Both,
    Width,
    Height,
}

/// The scale after pulling a resize handle by `moved` from where it was grabbed, when the
/// overlay had `size` at `scale`. The text keeps its proportions, so an edge scales the
/// whole overlay by how far that edge moved; the corner by whichever side moved further.
fn resized_scale(scale: f32, size: egui::Vec2, moved: egui::Vec2, axis: Axis) -> f32 {
    let wide = (size.x + moved.x) / size.x;
    let tall = (size.y + moved.y) / size.y;
    let grow = match axis {
        Axis::Both => wide.max(tall),
        Axis::Width => wide,
        Axis::Height => tall,
    };
    (scale * grow).clamp(MIN_SCALE, MAX_SCALE)
}

/// A small green "live" mark next to the title while a match is being played.
fn live_badge(ui: &mut egui::Ui, scale: f32) {
    egui::Frame::new()
        .fill(egui::Color32::from_rgba_unmultiplied(94, 194, 122, 36))
        .corner_radius(egui::CornerRadius::same((8.0 * scale) as u8))
        .inner_margin(egui::Margin::symmetric((6.0 * scale) as i8, (1.0 * scale) as i8))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 4.0 * scale;
            ui.horizontal(|ui| {
                // Drawn: the bundled fonts have no bullet glyph.
                let (dot, _) = ui.allocate_exact_size(egui::vec2(6.0, 10.0) * scale, Sense::hover());
                ui.painter().circle_filled(dot.center(), 3.0 * scale, theme::OPENED);
                ui.add(
                    egui::Label::new(
                        RichText::new("IN MATCH").font(theme::font(theme::BOLD, 9.5 * scale)).color(theme::OPENED),
                    )
                    .extend(),
                );
            });
        });
}

/// Window size around text of `text` size, kept within what the scale allows so a bad
/// measurement can never blow the overlay up.
fn fit(text: egui::Vec2, margin: f32, scale: f32) -> egui::Vec2 {
    let size = text + egui::Vec2::splat(margin * 2.0);
    let most = BASE_SIZE * scale * 2.0;
    egui::vec2(size.x.clamp(40.0, most.x), size.y.clamp(20.0, most.y))
}

/// What happened inside the overlay window this frame.
#[derive(Default)]
struct Interaction {
    lock: bool,
    at: Option<egui::Pos2>,
    content: Option<egui::Vec2>,
    resize_grabbed: Option<(egui::Pos2, Axis)>,
    resize_pointer: Option<egui::Pos2>,
    resize_released: bool,
}

/// Shows or hides the overlay. Called every frame.
pub fn update(app: &mut ChestApp, ctx: &egui::Context) {
    // Never leave the overlay taking the mouse once the player is done with its settings.
    // Unlocked from its own icon, it stays so until the icon locks it.
    let minimized = ctx.input(|i| i.viewport().minimized).unwrap_or(false);
    // A frame that does not draw the lock window closes it, so it is set up afresh.
    let lock_window = app.overlay.lock_window.take();
    let left_settings = app.page != Page::Settings || minimized;
    if app.overlay.editing && (!app.settings.overlay.enabled || (left_settings && !app.overlay.from_icon)) {
        lock(app);
    }
    let editing = app.overlay.editing;
    let settings = app.settings.overlay.clone();
    if !settings.enabled || !app.settings.setup_done {
        app.overlay.applied_alpha = None;
        return;
    }
    if settings.only_with_fortnite && !editing {
        let due = app.overlay.checked_at.is_none_or(|at| at.elapsed() >= FORTNITE_CHECK_EVERY);
        if due {
            app.overlay.fortnite_running = platform::fortnite_running();
            app.overlay.checked_at = Some(Instant::now());
        }
        // Keep the root window ticking so the overlay appears when the game starts.
        ctx.request_repaint_after(FORTNITE_CHECK_EVERY);
        if !app.overlay.fortnite_running {
            app.overlay.applied_alpha = None;
            return;
        }
    }

    let scale = settings.scale.clamp(MIN_SCALE, MAX_SCALE);
    let size = app.overlay.content.unwrap_or(BASE_SIZE * scale);
    let screen = platform::primary_screen_size()
        .map(|(w, h)| egui::vec2(w, h) / ctx.pixels_per_point())
        .unwrap_or(egui::vec2(1920.0, 1080.0));
    let pos = match settings.position {
        Some([x, y]) => egui::pos2(x, y),
        None => corner_position(settings.corner, size, screen),
    };

    let t = &app.totals;
    let mine = theme::thousands(t.mine());
    let nick = &app.settings.nick;
    let today = chrono::Local::now().date_naive();
    let today_count = crate::stats::by_day(&app.store.matches, &app.settings).get(&today).copied().unwrap_or(0);
    let last = app.store.matches.first().map_or(0, |m| m.summary(nick).mine()).to_string();
    let stats = [
        ("TODAY", theme::thousands(today_count), theme::TEXT),
        ("SQUAD", theme::thousands(t.teammates as u64), theme::SQUAD),
        ("TREASURES", t.treasures.to_string(), theme::TREASURE),
    ];
    let live = app.activity.live_matches > 0;

    let builder = ViewportBuilder::default()
        .with_title(OVERLAY_TITLE)
        .with_decorations(false)
        .with_always_on_top()
        .with_taskbar(false)
        .with_resizable(false)
        .with_active(false)
        .with_mouse_passthrough(!editing)
        .with_inner_size(size)
        .with_position(pos);
    // While being moved the overlay looks exactly as it will in game, so it is sized
    // by what it really shows.
    let margin = 10.0 * scale;
    let done = ctx.show_viewport_immediate(ViewportId::from_hash_of("overlay"), builder, |ui, _class| {
        let mut done = Interaction::default();
        let whole = ui.ctx().viewport_rect();
        if editing {
            // Anywhere on the overlay starts a window drag; the handle and the button
            // added later sit on top of it.
            let drag = ui.interact(whole, ui.make_persistent_id("overlay_drag"), Sense::drag());
            if drag.drag_started() {
                ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
            }
            done.at = ui.ctx().input(|i| i.viewport().outer_rect).map(|r| r.min);
        }
        ui.style_mut().interaction.selectable_labels = false;
        let frame = egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::same(margin as i8));
        egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            // Measured in its own block: the panel itself always fills the window, so its
            // size says nothing about the text and would grow the window every frame.
            ui.style_mut().interaction.selectable_labels = false;
            let text = ui.vertical(|ui| {
                // Spacing shrinks and grows with the text, or small sizes run out of room.
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 2.0) * scale;
                let text = |t: &str, family: &str, size: f32, color: egui::Color32| {
                    egui::Label::new(RichText::new(t).font(theme::font(family, size * scale)).color(color)).extend()
                };
                let caption = |t: &str| text(t, theme::SEMIBOLD, 9.5, theme::MUTED);
                ui.horizontal(|ui| {
                    ui.add(caption("CHESTS"));
                    if live {
                        live_badge(ui, scale);
                    }
                });
                ui.add_space(2.0 * scale);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 18.0 * scale;
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.add(text(&mine, theme::BOLD, 32.0, theme::GOLD));
                        ui.add(caption("TOTAL"));
                    });
                    // The last match in a tile of its own.
                    egui::Frame::new()
                        .fill(theme::CARD)
                        .stroke(Stroke::new(1.0, theme::CARD_HOVER))
                        .corner_radius(egui::CornerRadius::same((6.0 * scale) as u8))
                        .inner_margin(egui::Margin::symmetric((10.0 * scale) as i8, (4.0 * scale) as i8))
                        .show(ui, |ui| {
                            // As wide as its text: centered layouts take all the room they get.
                            let width = |t: &str, family: &str, size: f32| {
                                ui.painter()
                                    .layout_no_wrap(t.to_owned(), theme::font(family, size * scale), theme::TEXT)
                                    .size()
                                    .x
                            };
                            ui.set_width(width(&last, theme::BOLD, 22.0).max(width(
                                "LAST MATCH",
                                theme::SEMIBOLD,
                                9.5,
                            )));
                            ui.vertical_centered(|ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                ui.add(text(&last, theme::BOLD, 22.0, theme::TEXT));
                                ui.add(caption("LAST MATCH"));
                            });
                        });
                });
                ui.add_space(6.0 * scale);
                let row = ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 16.0 * scale;
                    for (name, value, color) in &stats {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            ui.add(caption(name));
                            ui.add(text(value, theme::BOLD, 14.0, *color));
                        });
                    }
                });
                // A hairline above the small numbers.
                let r = row.response.rect;
                let y = r.top() - 3.0 * scale;
                ui.painter().line_segment(
                    [egui::pos2(r.left(), y), egui::pos2(ui.min_rect().right(), y)],
                    Stroke::new(1.0, theme::CARD_HOVER),
                );
            });
            done.content = Some(fit(text.response.rect.size(), margin, scale));
        });
        let painter = ui.painter();
        // A thin border; while it is being moved, a gold frame.
        painter.rect_stroke(whole, 0.0, Stroke::new(1.0, theme::CARD_HOVER), StrokeKind::Inside);
        if editing {
            painter.rect_stroke(whole, 0.0, Stroke::new(2.0, theme::GOLD), StrokeKind::Inside);
            let grip = egui::Rect::from_min_size(whole.max - egui::vec2(GRIP, GRIP), egui::vec2(GRIP, GRIP));
            for i in 1..=3 {
                let d = i as f32 * 4.0;
                painter.line_segment(
                    [egui::pos2(grip.max.x - d, grip.max.y - 2.0), egui::pos2(grip.max.x - 2.0, grip.max.y - d)],
                    Stroke::new(1.5, theme::GOLD),
                );
            }
            let right_edge = egui::Rect::from_min_max(
                egui::pos2(whole.right() - EDGE, whole.top() + 28.0),
                egui::pos2(whole.right(), whole.bottom() - GRIP),
            );
            let bottom_edge = egui::Rect::from_min_max(
                egui::pos2(whole.left(), whole.bottom() - EDGE),
                egui::pos2(whole.right() - GRIP, whole.bottom()),
            );
            let handles = [
                (grip, Axis::Both, egui::CursorIcon::ResizeNwSe, "overlay_resize"),
                (right_edge, Axis::Width, egui::CursorIcon::ResizeHorizontal, "overlay_resize_x"),
                (bottom_edge, Axis::Height, egui::CursorIcon::ResizeVertical, "overlay_resize_y"),
            ];
            for (rect, axis, cursor, id) in handles {
                let resize = ui.interact(rect, ui.make_persistent_id(id), Sense::drag());
                if resize.hovered() || resize.dragged() {
                    ui.ctx().set_cursor_icon(cursor);
                }
                if resize.drag_started() {
                    done.resize_grabbed = resize.interact_pointer_pos().map(|p| (p, axis));
                }
                if resize.dragged() {
                    done.resize_pointer = resize.interact_pointer_pos();
                }
                done.resize_released |= resize.drag_stopped();
            }
        }
        // The lock icon. Only clickable here while unlocked; locked, the overlay lets the
        // mouse through and the small lock window below takes the click instead.
        let lock_at = egui::Rect::from_min_size(whole.min + lock_offset(whole.size()), egui::Vec2::splat(LOCK_SIDE));
        let (glyph, color) =
            if editing { (theme::icon::UNLOCK, theme::GOLD) } else { (theme::icon::LOCK, theme::MUTED) };
        let icon =
            egui::Button::new(theme::icon_text(glyph, if editing { "U" } else { "L" }, 14.0).color(color)).frame(false);
        if ui.put(lock_at, icon).on_hover_text("Lock it in place").clicked() {
            done.lock = true;
        }
        done
    });

    if let Some(at) = done.at {
        app.overlay.dragged_to = Some(at);
    }
    if let Some(content) = done.content {
        app.overlay.content = Some(content);
    }
    // The window keeps its top left corner while it is resized, so pointer positions
    // inside it stay comparable from frame to frame.
    if let Some((grabbed, axis)) = done.resize_grabbed {
        app.overlay.resize_from = Some((grabbed, axis, scale, size));
    }
    if let (Some(pointer), Some((grabbed, axis, from_scale, from_size))) =
        (done.resize_pointer, app.overlay.resize_from)
    {
        app.settings.overlay.scale = resized_scale(from_scale, from_size, pointer - grabbed, axis);
    }
    if done.resize_released || !editing {
        app.overlay.resize_from = None;
    }
    if done.lock {
        toggle_from_icon(app);
    }

    let alpha = if editing { 255 } else { (settings.opacity.clamp(0.3, 1.0) * 255.0).round() as u8 };
    if app.overlay.applied_alpha != Some(alpha) && platform::set_window_opacity(OVERLAY_TITLE, alpha) {
        app.overlay.applied_alpha = Some(alpha);
    }

    // Locked, the overlay lets the mouse through, so a tiny window of its own sits on the
    // lock icon to catch a click. Everything else still goes to the game.
    // Unlocked, it is parked off screen, and also in the frame that locks the overlay:
    // `pos` and `size` still hold the old place there.
    let parked = app.overlay.editing || editing;
    if parked && !app.overlay.editing {
        ctx.request_repaint();
    }
    let lock_builder = ViewportBuilder::default()
        .with_title(LOCK_TITLE)
        .with_decorations(false)
        .with_always_on_top()
        .with_taskbar(false)
        .with_resizable(false)
        .with_active(false)
        .with_visible(lock_window.is_some())
        .with_inner_size(egui::Vec2::splat(LOCK_SIDE))
        .with_position(if parked { LOCK_PARKED } else { pos + lock_offset(size) });
    let unlock = ctx.show_viewport_immediate(ViewportId::from_hash_of("overlay_lock"), lock_builder, |ui, _class| {
        let mut clicked = false;
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::BG)).show(ui, |ui| {
            let icon =
                egui::Button::new(theme::icon_text(theme::icon::LOCK, "L", 14.0).color(theme::MUTED)).frame(false);
            clicked = ui.put(ui.max_rect(), icon).on_hover_text("Unlock to move and resize the overlay").clicked();
        });
        clicked
    });
    if unlock {
        toggle_from_icon(app);
    }
    app.overlay.lock_window = match lock_window {
        // Windows plays an animation when a window is shown, which flies the icon in from
        // aside, so it is turned off while the window is still hidden.
        None => {
            ctx.request_repaint();
            (platform::disable_window_animation(LOCK_TITLE) && platform::set_window_opacity(LOCK_TITLE, 0))
                .then_some(LockWindow::Painting(Instant::now()))
        }
        Some(LockWindow::Painting(since)) => {
            ctx.request_repaint_after(Duration::from_millis(50));
            if since.elapsed() >= LOCK_REVEAL_AFTER && platform::set_window_opacity(LOCK_TITLE, alpha) {
                Some(LockWindow::Up(alpha))
            } else {
                Some(LockWindow::Painting(since))
            }
        }
        // Parked, it keeps its opacity: changing it in the frame it comes back would show.
        Some(LockWindow::Up(applied)) if parked => Some(LockWindow::Up(applied)),
        Some(LockWindow::Up(applied)) => {
            let changed = applied != alpha && platform::set_window_opacity(LOCK_TITLE, alpha);
            Some(LockWindow::Up(if changed { alpha } else { applied }))
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_follows_how_far_the_handle_moved() {
        let size = egui::vec2(200.0, 100.0);
        // Grabbing without moving changes nothing.
        let both = |moved| resized_scale(1.0, size, moved, Axis::Both);
        assert_eq!(both(egui::Vec2::ZERO), 1.0);
        assert_eq!(both(egui::vec2(200.0, 0.0)), 2.0);
        assert_eq!(both(egui::vec2(0.0, 50.0)), 1.5);
        assert_eq!(both(egui::vec2(-100.0, -50.0)), 0.5);
        assert_eq!(both(egui::vec2(-190.0, -90.0)), MIN_SCALE);
        assert_eq!(resized_scale(2.0, size, egui::vec2(400.0, 0.0), Axis::Both), MAX_SCALE);
        // An edge goes by its own direction only: pulling the right edge in shrinks it
        // even though the height did not change.
        assert_eq!(resized_scale(1.0, size, egui::vec2(-50.0, 0.0), Axis::Width), 0.75);
        assert_eq!(resized_scale(1.0, size, egui::vec2(-50.0, 0.0), Axis::Both), 1.0);
        assert_eq!(resized_scale(1.0, size, egui::vec2(80.0, 25.0), Axis::Height), 1.25);
    }

    #[test]
    fn window_never_outgrows_the_scale() {
        let huge = egui::vec2(5000.0, 3000.0);
        let size = fit(huge, 10.0, 1.0);
        assert!(size.x <= BASE_SIZE.x * 2.0 && size.y <= BASE_SIZE.y * 2.0);
        assert_eq!(fit(egui::vec2(200.0, 70.0), 10.0, 1.0), egui::vec2(220.0, 90.0));
    }

    #[test]
    fn corners_stay_on_screen() {
        let screen = egui::vec2(1920.0, 1080.0);
        let size = BASE_SIZE;
        let br = corner_position(OverlayCorner::BottomRight, size, screen);
        assert!(br.x + size.x <= screen.x && br.y + size.y <= screen.y);
        assert_eq!(corner_position(OverlayCorner::TopLeft, size, screen), egui::pos2(SCREEN_MARGIN, SCREEN_MARGIN));
    }
}
