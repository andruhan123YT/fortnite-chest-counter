//! Players who run the app, by chests, best match and matches recorded, from the sync
//! server.

use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText};
use egui_extras::{Column, TableBuilder};

use crate::app::{ChestApp, Page};
use crate::online::{LeaderEntry, Leaderboard};
use crate::theme;

/// How often the open page asks the server again.
const REFRESH_EVERY: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Period {
    #[default]
    All,
    Week,
    Today,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Sort {
    Player,
    Matches,
    #[default]
    Chests,
    Best,
}

impl Sort {
    fn title(self) -> &'static str {
        match self {
            Sort::Player => "Player",
            Sort::Matches => "Matches",
            Sort::Chests => "Chests",
            Sort::Best => "Best match",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Sort::Player => "Name in Fortnite, or Anonymous if the player hid it",
            Sort::Matches => "Matches the app recorded and sent",
            Sort::Chests => "Chests opened and broken",
            Sort::Best => "The most chests opened and broken in one match",
        }
    }
}

pub struct LeaderboardUi {
    board: Option<Leaderboard>,
    error: Option<String>,
    fetched: Option<Instant>,
    pending: Option<Receiver<Result<Leaderboard, String>>>,
    period: Period,
    sort: Sort,
    /// Largest first.
    descending: bool,
}

impl Default for LeaderboardUi {
    fn default() -> Self {
        Self {
            board: None,
            error: None,
            fetched: None,
            pending: None,
            period: Period::All,
            sort: Sort::Chests,
            descending: true,
        }
    }
}

impl LeaderboardUi {
    fn poll(&mut self) {
        let Some(rx) = &self.pending else { return };
        match rx.try_recv() {
            Ok(result) => {
                self.pending = None;
                self.fetched = Some(Instant::now());
                match result {
                    Ok(board) => {
                        self.board = Some(board);
                        self.error = None;
                    }
                    Err(e) => self.error = Some(e),
                }
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => self.pending = None,
        }
    }

    fn fetch(&mut self, me: Option<String>, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new().name("leaderboard".into()).spawn(move || {
            let _ = tx.send(crate::online::fetch_leaderboard(me.as_deref()));
            ctx.request_repaint();
        });
        if spawned.is_ok() {
            self.pending = Some(rx);
        }
    }

    fn click(&mut self, sort: Sort) {
        if self.sort == sort {
            self.descending = !self.descending;
        } else {
            self.sort = sort;
            // Numbers start with the largest, names with A.
            self.descending = sort != Sort::Player;
        }
    }
}

fn sorted(entries: &[LeaderEntry], sort: Sort, descending: bool) -> Vec<&LeaderEntry> {
    let mut rows: Vec<&LeaderEntry> = entries.iter().collect();
    rows.sort_by(|a, b| {
        let order = match sort {
            Sort::Player => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            Sort::Matches => a.matches.cmp(&b.matches),
            Sort::Chests => a.chests.cmp(&b.chests),
            Sort::Best => a.best.cmp(&b.best),
        };
        let order = if descending { order.reverse() } else { order };
        // Ties: more chests first, then by name.
        order.then(b.chests.cmp(&a.chests)).then(a.name.cmp(&b.name))
    });
    rows
}

/// A small triangle, drawn rather than typed: the bundled fonts have no arrow glyphs.
fn sort_arrow(ui: &mut egui::Ui, down: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 12.0), egui::Sense::hover());
    let c = rect.center();
    let (tip, back) = if down { (3.5, -2.5) } else { (-3.5, 2.5) };
    let points = vec![c + egui::vec2(0.0, tip), c + egui::vec2(-4.0, back), c + egui::vec2(4.0, back)];
    ui.painter().add(egui::Shape::convex_polygon(points, theme::GOLD, egui::Stroke::NONE));
}

fn local(ts: i64, format: &str) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|t| t.with_timezone(&chrono::Local).format(format).to_string())
        .unwrap_or_default()
}

pub fn show(app: &mut ChestApp, ui: &mut egui::Ui) {
    ui.label(theme::title("Leaderboard"));
    ui.add_space(4.0);
    if !app.settings.online {
        ui.label(theme::caption(
            "The leaderboard comes from the sync server, so it needs Online mode: Settings > Online.",
        ));
        if ui.button("Open Settings").clicked() {
            app.page = Page::Settings;
        }
        return;
    }

    let state = &mut app.leaderboard;
    state.poll();
    let due = state.fetched.is_none_or(|at| at.elapsed() >= REFRESH_EVERY);
    if state.pending.is_none() && due {
        let me = crate::online::my_player_id(&app.store.matches);
        app.leaderboard.fetch(me, ui.ctx());
    }
    ui.ctx().request_repaint_after(REFRESH_EVERY);
    let state = &mut app.leaderboard;

    ui.horizontal(|ui| {
        for (period, label) in [(Period::All, "All time"), (Period::Week, "This week"), (Period::Today, "Today")] {
            ui.selectable_value(&mut state.period, period, label);
        }
        ui.add_space(8.0);
        let busy = state.pending.is_some();
        if ui.add_enabled(!busy, egui::Button::new(if busy { "Updating..." } else { "Refresh" })).clicked() {
            state.fetched = None;
        }
    });

    let Some(board) = &state.board else {
        ui.add_space(8.0);
        match &state.error {
            Some(e) => ui.label(RichText::new(format!("Could not reach the server: {e}")).color(theme::WARN)),
            None => ui.label(theme::caption("Loading...")),
        };
        return;
    };
    let (entries, since) = match state.period {
        Period::All => (&board.all, "Every match the app has sent.".to_owned()),
        Period::Week => (
            &board.week,
            format!("Since {} your time. Weeks start on Sunday.", local(board.week_start, "%a %d %b, %H:%M")),
        ),
        Period::Today => (
            &board.today,
            format!("Since {} your time: days start at 00:00 UTC, as Fortnite's.", local(board.day_start, "%H:%M")),
        ),
    };
    ui.label(theme::caption(since));
    if let Some(e) = &state.error {
        ui.label(RichText::new(format!("Could not update: {e}")).color(theme::WARN));
    }
    ui.add_space(6.0);
    if entries.is_empty() {
        ui.label(theme::caption("Nobody has played a match in this period yet."));
        return;
    }

    let rows = sorted(entries, state.sort, state.descending);
    let mut clicked = None;
    ui.style_mut().interaction.selectable_labels = false;
    let columns = [Sort::Player, Sort::Matches, Sort::Chests, Sort::Best];
    TableBuilder::new(ui)
        .striped(true)
        .column(Column::exact(40.0))
        .column(Column::auto().at_least(180.0))
        .columns(Column::auto().at_least(90.0), 3)
        .column(Column::remainder())
        .header(26.0, |mut h| {
            h.col(|ui| {
                ui.label(RichText::new("#").strong().color(theme::MUTED));
            });
            for sort in columns {
                h.col(|ui| {
                    let active = state.sort == sort;
                    let color = if active { theme::TEXT } else { theme::MUTED };
                    let text = RichText::new(sort.title()).strong().color(color);
                    // Numbers line up on the right, so their arrow goes left of the title.
                    let layout = if sort == Sort::Player {
                        egui::Layout::left_to_right(egui::Align::Center)
                    } else {
                        egui::Layout::right_to_left(egui::Align::Center)
                    };
                    ui.with_layout(layout, |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        let response = ui.add(egui::Button::new(text).frame(false)).on_hover_text(sort.hint());
                        if response.clicked() {
                            clicked = Some(sort);
                        }
                        if active {
                            sort_arrow(ui, state.descending);
                        }
                    });
                });
            }
            h.col(|_| {});
        })
        .body(|body| {
            body.rows(26.0, rows.len(), |mut row| {
                let i = row.index();
                let e = rows[i];
                let paint = |text: String| {
                    let text = RichText::new(text);
                    if e.me {
                        text.strong().color(theme::GOLD)
                    } else {
                        text
                    }
                };
                let number = |ui: &mut egui::Ui, text: String| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(paint(text)));
                };
                row.col(|ui| {
                    ui.label(RichText::new(format!("{}", i + 1)).color(theme::MUTED));
                });
                row.col(|ui| {
                    let name = if e.me { format!("{} (you)", e.name) } else { e.name.clone() };
                    ui.add(egui::Label::new(paint(name)).truncate());
                });
                row.col(|ui| number(ui, theme::thousands(u64::from(e.matches))));
                row.col(|ui| number(ui, theme::thousands(e.chests)));
                row.col(|ui| number(ui, e.best.to_string()));
                row.col(|_| {});
            });
        });
    if let Some(sort) = clicked {
        app.leaderboard.click(sort);
    }
    if !app.settings.leaderboard_name {
        ui.add_space(6.0);
        ui.label(theme::caption("Others see you as Anonymous: Settings > Online."));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, matches: u32, chests: u64, best: u32) -> LeaderEntry {
        LeaderEntry { name: name.into(), matches, chests, best, me: false }
    }

    #[test]
    fn sorts_by_any_column_both_ways() {
        let entries = [entry("bob", 10, 300, 40), entry("Ann", 30, 500, 35), entry("Cid", 5, 300, 60)];
        let names = |sort, descending| -> Vec<&str> {
            sorted(&entries, sort, descending).into_iter().map(|e| e.name.as_str()).collect()
        };
        // Equal chests fall back to the name.
        assert_eq!(names(Sort::Chests, true), ["Ann", "Cid", "bob"]);
        assert_eq!(names(Sort::Best, true), ["Cid", "bob", "Ann"]);
        assert_eq!(names(Sort::Matches, false), ["Cid", "bob", "Ann"]);
        // Names ignore case.
        assert_eq!(names(Sort::Player, false), ["Ann", "bob", "Cid"]);
    }

    #[test]
    fn a_second_click_flips_the_order() {
        let mut ui = LeaderboardUi::default();
        assert!(ui.descending && ui.sort == Sort::Chests);
        ui.click(Sort::Best);
        assert!(ui.descending);
        ui.click(Sort::Best);
        assert!(!ui.descending);
        ui.click(Sort::Player);
        assert!(!ui.descending);
    }
}
