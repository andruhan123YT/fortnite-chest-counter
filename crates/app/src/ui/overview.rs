//! The landing page: running totals, Pirate Shop odds and the latest match.

use chests::odds::{any_reward_rate, chance_at_least_one, PIRATE_SHOP_ACCESS};
use eframe::egui::{self, RichText};

use crate::app::{short_name, ChestApp, Page};
use crate::i18n::{date_format, plural, tr, trf};
use crate::theme;
use crate::worker::Command;

/// Content may stick out of the window by this much before the page scrolls, so a page
/// that fits does not wobble by a few pixels.
const SCROLL_SLACK: f32 = 8.0;

pub fn show(app: &mut ChestApp, ui: &mut egui::Ui) {
    // Whether the page overflowed last frame decides whether it scrolls this frame.
    let overflow_id = ui.make_persistent_id("overview_overflows");
    let overflows = ui.ctx().data(|d| d.get_temp::<bool>(overflow_id)).unwrap_or(false);
    let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]).vscroll(overflows);
    if !overflows {
        area = area.vertical_scroll_offset(0.0);
    }
    let shown = area.show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(theme::title(tr("Overview")));
            ui.add(egui::Label::new(theme::caption(counting_note(app))).truncate());
        });
        ui.add_space(8.0);
        warnings(app, ui);

        let days = crate::stats::by_day(&app.store.matches, &app.settings);
        let today = chrono::Local::now().date_naive();
        ui.columns(2, |cols| {
            hero(app, &mut cols[0]);
            cols[0].add_space(8.0);
            last_days(&mut cols[0], &days, today);
            dashboard_tiles(app, &mut cols[1], &days, today);
        });

        ui.add_space(8.0);
        latest_match(app, ui);
        ui.add_space(8.0);
        activity(app, ui);
    });
    let now = shown.content_size.y > shown.inner_rect.height() + SCROLL_SLACK;
    if now != overflows {
        ui.ctx().data_mut(|d| d.insert_temp(overflow_id, now));
        ui.ctx().request_repaint();
    }
}

/// The total, and the last match next to it.
fn hero(app: &ChestApp, ui: &mut egui::Ui) {
    let t = &app.totals;
    let last =
        crate::stats::own(&app.store.matches, &app.settings).next().map(|m| m.summary(app.settings.name_in(m)).mine());
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(theme::caption(tr("CHESTS")));
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(
                    RichText::new(theme::thousands(t.mine())).font(theme::font(theme::BOLD, 40.0)).color(theme::gold()),
                );
                ui.label(theme::caption(tr("in total")));
            });
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(10.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let value = last.map_or_else(|| "-".to_string(), |n| n.to_string());
                ui.label(RichText::new(value).font(theme::font(theme::BOLD, 40.0)).color(theme::text()));
                ui.label(theme::caption(tr("last match")));
            });
        });
        let by_hand = if t.adjustment == 0 {
            String::new()
        } else {
            trf(" · by hand {0}", &[&crate::ui::settings::signed(t.adjustment)])
        };
        ui.add(
            egui::Label::new(theme::caption(trf(
                "opened {0} · broken {1} · base {2}{3}",
                &[
                    &theme::thousands(t.opened as u64),
                    &theme::thousands(t.broken as u64),
                    &theme::thousands(t.base),
                    &by_hand,
                ],
            )))
            .truncate(),
        );
    });
}

/// Bars for the last seven days, today on the right.
fn last_days(ui: &mut egui::Ui, days: &std::collections::BTreeMap<chrono::NaiveDate, u64>, today: chrono::NaiveDate) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(theme::caption(tr("Last 7 days")));
        let week: Vec<(chrono::NaiveDate, u64)> = (0..7)
            .rev()
            .map(|back| {
                let day = today - chrono::Duration::days(back);
                (day, days.get(&day).copied().unwrap_or(0))
            })
            .collect();
        let most = week.iter().map(|&(_, n)| n).max().unwrap_or(0).max(1) as f32;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 120.0), egui::Sense::hover());
        let painter = ui.painter();
        let slot = rect.width() / 7.0;
        let bar_w = (slot * 0.55).min(34.0);
        let (top, bottom) = (rect.top() + 16.0, rect.bottom() - 16.0);
        for (i, &(day, n)) in week.iter().enumerate() {
            let x = rect.left() + slot * (i as f32 + 0.5);
            let height = (bottom - top) * n as f32 / most;
            let bar = egui::Rect::from_min_max(
                egui::pos2(x - bar_w / 2.0, bottom - height.max(2.0)),
                egui::pos2(x + bar_w / 2.0, bottom),
            );
            let color = if day == today { theme::gold() } else { theme::gold().gamma_multiply(0.45) };
            painter.rect_filled(bar, egui::CornerRadius::same(4), color);
            painter.text(
                egui::pos2(x, bar.top() - 2.0),
                egui::Align2::CENTER_BOTTOM,
                n.to_string(),
                theme::font(theme::SEMIBOLD, 11.5),
                theme::text(),
            );
            painter.text(
                egui::pos2(x, rect.bottom()),
                egui::Align2::CENTER_BOTTOM,
                day.format(&date_format(&day, "%a")).to_string(),
                egui::FontId::proportional(11.0),
                theme::muted(),
            );
        }
    });
}

/// Label, value, value color, note.
type Tile = (String, String, egui::Color32, Option<String>);

/// Small cards with the rest of the numbers.
fn dashboard_tiles(
    app: &ChestApp,
    ui: &mut egui::Ui,
    days: &std::collections::BTreeMap<chrono::NaiveDate, u64>,
    today: chrono::NaiveDate,
) {
    let t = &app.totals;
    let today_n = days.get(&today).copied().unwrap_or(0);
    let best_day = days.iter().max_by_key(|&(day, n)| (*n, *day));
    let best = t.best_match.as_ref().map_or(0, |(_, n)| *n);
    let squad = t.with_squad();
    let rows: [[Tile; 2]; 3] = [
        [
            (tr("Today").into(), theme::thousands(today_n), theme::opened(), None),
            (
                best_day.map_or_else(
                    || tr("Best day").into(),
                    |(d, _)| trf("Best day · {0}", &[&d.format(&date_format(d, "%d %b"))]),
                ),
                theme::thousands(best_day.map_or(0, |(_, n)| *n)),
                theme::gold(),
                None,
            ),
        ],
        [
            (
                tr("Per match").into(),
                format!("{:.1}", t.per_match()),
                theme::text(),
                Some(format!("{} {}", t.counted_matches, plural(t.counted_matches as u64, "match", "matches"))),
            ),
            (tr("Best match").into(), best.to_string(), theme::text(), None),
        ],
        [
            (
                tr("Squad").into(),
                theme::squad_count(t.teammates as u64),
                theme::squad(),
                Some(tr("seen near you").into()),
            ),
            (
                tr("Treasures").into(),
                theme::thousands(t.treasures as u64),
                theme::treasure(),
                Some(trf("expected {0}", &[&format!("{:.1}", t.counted_squad() as f64 * any_reward_rate())])),
            ),
        ],
    ];
    for row in rows {
        ui.columns(2, |cols| {
            for (col, (label, value, color, note)) in cols.iter_mut().zip(row) {
                theme::tile(col, &label, &value, color, note.as_deref());
            }
        });
        ui.add_space(4.0);
    }
    theme::tile(
        ui,
        tr("Pirate Shop Access chance"),
        &theme::percent(chance_at_least_one(squad, PIRATE_SHOP_ACCESS.one_in)),
        theme::opened(),
        Some(&trf(
            "with squad · solo {0} · 1 in {1}",
            &[
                &theme::percent(chance_at_least_one(t.mine(), PIRATE_SHOP_ACCESS.one_in)),
                &theme::thousands(PIRATE_SHOP_ACCESS.one_in as u64),
            ],
        )),
    )
    .on_hover_text(theme::squad_nearby_note());
}

fn counting_note(app: &ChestApp) -> String {
    let base = theme::thousands(app.settings.base_count);
    match (app.counted_player(), app.settings.count_since) {
        (Some(who), Some(since)) => trf(
            "Counting {0} from {1} on, plus a base of {2}.",
            &[&who, &since.format(&date_format(&since, "%d %b %Y %H:%M")), &base],
        ),
        (None, Some(since)) => trf(
            "Counting your matches from {0} on, plus a base of {1}.",
            &[&since.format(&date_format(&since, "%d %b %Y %H:%M")), &base],
        ),
        (Some(who), None) => trf("Counting every match of {0}, plus a base of {1}.", &[&who, &base]),
        (None, None) => trf("Counting all your matches, plus a base of {0}.", &[&base]),
    }
}

fn warnings(app: &mut ChestApp, ui: &mut egui::Ui) {
    if !app.settings.replay_dir.is_dir() {
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(tr("The replay folder does not exist.")).color(theme::warn()));
            ui.label(theme::caption(trf(
                "{0} was not found. Check the folder in Settings and make sure replay recording is on in Fortnite.",
                &[&app.settings.replay_dir.display()],
            )));
        });
        ui.add_space(8.0);
    }
    if app.totals.matches_without_player > 0 {
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            let who = if app.settings.nick.trim().is_empty() {
                tr("the replay's recorder").to_string()
            } else {
                trf("a player named \"{0}\"", &[&app.settings.nick])
            };
            ui.label(
                RichText::new(trf(
                    "{0} counted match(es) do not contain {1}.",
                    &[&app.totals.matches_without_player, &who],
                ))
                .color(theme::warn()),
            );
            ui.label(theme::caption(tr("Open the match to see who was in it.")));
        });
        ui.add_space(8.0);
    }
    let unhealthy =
        crate::stats::own(&app.store.matches, &app.settings).take(5).filter(|m| m.packet_health < 0.97).count();
    if unhealthy > 0 {
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(tr("Recent replays did not decode cleanly.")).color(theme::warn()));
            ui.label(theme::caption(tr(
                "A Fortnite update probably changed the replay format. Counts may be low until the parser is updated.",
            )));
        });
        ui.add_space(8.0);
    }
}

fn latest_match(app: &mut ChestApp, ui: &mut egui::Ui) {
    let Some(m) = crate::stats::own(&app.store.matches, &app.settings).next() else {
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(tr("No matches yet")).size(16.0).strong());
            ui.label(theme::caption(
                tr("Play a match with replay recording on (Settings > Game > Replays), or drop .replay files on this window."),
            ));
            if ui.button(tr("Scan replay folder now")).clicked() {
                app.worker.send(Command::Scan { force: false });
            }
        });
        return;
    };
    let s = m.summary(app.settings.name_in(m));
    let id = m.id.clone();
    let response = theme::card()
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(theme::caption(tr("Latest match")));
            ui.horizontal(|ui| {
                let when = m
                    .started_at
                    .map(|t| t.format(&date_format(&t, "%d %b %H:%M")).to_string())
                    .unwrap_or_else(|| short_name(&m.file_name).to_string());
                ui.label(RichText::new(when).size(18.0).strong());
                if let (Some(p), Some(n)) = (m.placement, m.total_players) {
                    ui.label(theme::caption(trf("#{0} of {1}", &[&p, &n])));
                }
                ui.label(theme::caption(trf("{0} min", &[&format!("{:.0}", m.length_minutes())])));
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("+{}", s.mine())).size(26.0).strong().color(theme::gold()));
                ui.label(RichText::new(trf("opened {0}", &[&s.opened])).color(theme::opened()));
                ui.label(RichText::new(trf("broken {0}", &[&s.broken])).color(theme::broken()));
                if !s.teammates.is_empty() {
                    ui.label(RichText::new(trf("squad +{0}+", &[&s.mates])).color(theme::squad()))
                        .on_hover_text(theme::squad_nearby_note());
                }
                if s.treasures > 0 {
                    ui.label(RichText::new(trf("treasure +{0}", &[&s.treasures])).color(theme::treasure()));
                }
            });
            if !app.settings.counts(m.started_at) {
                ui.label(theme::caption(tr("Before the counting start date, not included in the total.")));
            }
        })
        .response;
    if response.interact(egui::Sense::click()).on_hover_text(tr("Open match details")).clicked() {
        app.selected_match = Some(id);
        app.page = Page::Matches;
    }
}

fn activity(app: &mut ChestApp, ui: &mut egui::Ui) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("Activity")).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tr("Scan now")).clicked() {
                    app.worker.send(Command::Scan { force: false });
                }
                if let Some(at) = app.activity.last_scan {
                    ui.label(theme::caption(trf("last scan {0}", &[&at.format("%H:%M:%S")])));
                }
            });
        });
        if app.activity.live_matches > 0 {
            ui.label(
                RichText::new(tr("A match is being played. It will be counted once you are back in the lobby."))
                    .color(theme::opened()),
            );
        }
        if let Some((file, fraction, _)) = &app.activity.progress {
            ui.add(egui::ProgressBar::new(*fraction).text(trf("Analyzing {0}", &[&short_name(file)])));
        }
        if app.activity.log.is_empty() {
            ui.label(theme::caption(tr("Nothing yet.")));
        }
        for line in app.activity.log.iter().take(8) {
            ui.label(theme::caption(line.clone()));
        }
    });
}
