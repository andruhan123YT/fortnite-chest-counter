//! The landing page: running totals, Pirate Shop odds and the latest match.

use chests::odds::{any_reward_rate, chance_at_least_one, PIRATE_SHOP_ACCESS};
use eframe::egui::{self, RichText};

use crate::app::{short_name, ChestApp, Page};
use crate::theme;
use crate::worker::Command;

pub fn show(app: &mut ChestApp, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.label(theme::title("Overview"));
        ui.label(theme::caption(counting_note(app)));
        ui.add_space(12.0);

        let t = &app.totals;
        let me = t.mine();
        let squad = t.with_squad();
        theme::stat_row(
            ui,
            &[
                theme::Stat {
                    label: "My chests",
                    value: theme::thousands(me),
                    color: theme::GOLD,
                    note: Some(format!(
                        "opened {} · broken {} · base {}",
                        t.opened,
                        t.broken,
                        theme::thousands(t.base)
                    )),
                },
                theme::Stat {
                    label: "Squad chests",
                    value: theme::squad_count(t.teammates as u64),
                    color: theme::SQUAD,
                    note: Some("teammates only, seen near you · their chests far away are not in replays".into()),
                },
                theme::Stat {
                    label: "Pirate Shop Access chance",
                    value: theme::percent(chance_at_least_one(squad, PIRATE_SHOP_ACCESS.one_in)),
                    color: theme::OPENED,
                    note: Some(format!(
                        "with squad, at least · solo {} · 1 in {}",
                        theme::percent(chance_at_least_one(me, PIRATE_SHOP_ACCESS.one_in)),
                        theme::thousands(PIRATE_SHOP_ACCESS.one_in as u64)
                    )),
                },
                theme::Stat {
                    label: "Treasures",
                    value: theme::thousands(t.treasures as u64),
                    color: theme::TREASURE,
                    note: Some(format!(
                        "expected {:.1} from {} squad chests seen",
                        t.counted_squad() as f64 * any_reward_rate(),
                        theme::thousands(t.counted_squad())
                    )),
                },
            ],
        );

        ui.add_space(12.0);
        warnings(app, ui);
        latest_match(app, ui);
        ui.add_space(12.0);
        activity(app, ui);
    });
}

fn counting_note(app: &ChestApp) -> String {
    let who = app.counted_player();
    match app.settings.count_since {
        Some(since) => format!(
            "Counting {who} from {} on, plus a base of {}.",
            since.format("%d %b %Y %H:%M"),
            theme::thousands(app.settings.base_count)
        ),
        None => format!("Counting every match of {who}, plus a base of {}.", theme::thousands(app.settings.base_count)),
    }
}

fn warnings(app: &mut ChestApp, ui: &mut egui::Ui) {
    if !app.settings.replay_dir.is_dir() {
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("The replay folder does not exist.").color(theme::WARN));
            ui.label(theme::caption(format!(
                "{} was not found. Check the folder in Settings and make sure replay recording is on in Fortnite.",
                app.settings.replay_dir.display()
            )));
        });
        ui.add_space(8.0);
    }
    if app.totals.matches_without_player > 0 {
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            let who = if app.settings.nick.trim().is_empty() {
                "the replay's recorder".to_string()
            } else {
                format!("a player named \"{}\"", app.settings.nick)
            };
            ui.label(
                RichText::new(format!("{} counted match(es) do not contain {who}.", app.totals.matches_without_player))
                    .color(theme::WARN),
            );
            ui.label(theme::caption("Open the match to see who was in it, or fix your name in Settings."));
        });
        ui.add_space(8.0);
    }
    let unhealthy = app.store.matches.iter().take(5).filter(|m| m.packet_health < 0.97).count();
    if unhealthy > 0 {
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Recent replays did not decode cleanly.").color(theme::WARN));
            ui.label(theme::caption(
                "A Fortnite update probably changed the replay format. Counts may be low until the parser is updated.",
            ));
        });
        ui.add_space(8.0);
    }
}

fn latest_match(app: &mut ChestApp, ui: &mut egui::Ui) {
    let Some(m) = app.store.matches.first() else {
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("No matches yet").size(16.0).strong());
            ui.label(theme::caption(
                "Play a match with replay recording on (Settings > Game > Replays), or drop .replay files on this window.",
            ));
            if ui.button("Scan replay folder now").clicked() {
                app.worker.send(Command::Scan { force: false });
            }
        });
        return;
    };
    let s = m.summary(&app.settings.nick);
    let id = m.id.clone();
    let response = theme::card()
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(theme::caption("Latest match"));
            ui.horizontal(|ui| {
                let when = m
                    .started_at
                    .map(|t| t.format("%d %b %H:%M").to_string())
                    .unwrap_or_else(|| short_name(&m.file_name).to_string());
                ui.label(RichText::new(when).size(18.0).strong());
                if let (Some(p), Some(n)) = (m.placement, m.total_players) {
                    ui.label(theme::caption(format!("#{p} of {n}")));
                }
                ui.label(theme::caption(format!("{:.0} min", m.length_minutes())));
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("+{}", s.mine())).size(26.0).strong().color(theme::GOLD));
                ui.label(RichText::new(format!("opened {}", s.opened)).color(theme::OPENED));
                ui.label(RichText::new(format!("broken {}", s.broken)).color(theme::BROKEN));
                if !s.teammates.is_empty() {
                    ui.label(RichText::new(format!("squad +{}+", s.mates)).color(theme::SQUAD))
                        .on_hover_text(theme::SQUAD_NEARBY_NOTE);
                }
                if s.treasures > 0 {
                    ui.label(RichText::new(format!("treasure +{}", s.treasures)).color(theme::TREASURE));
                }
            });
            if !app.settings.counts(m.started_at) {
                ui.label(theme::caption("Before the counting start date, not included in the total."));
            }
        })
        .response;
    if response.interact(egui::Sense::click()).on_hover_text("Open match details").clicked() {
        app.selected_match = Some(id);
        app.page = Page::Matches;
    }
}

fn activity(app: &mut ChestApp, ui: &mut egui::Ui) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new("Activity").strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Scan now").clicked() {
                    app.worker.send(Command::Scan { force: false });
                }
                if let Some(at) = app.activity.last_scan {
                    ui.label(theme::caption(format!("last scan {}", at.format("%H:%M:%S"))));
                }
            });
        });
        if app.activity.live_matches > 0 {
            ui.label(
                RichText::new("A match is being played. It will be counted once you are back in the lobby.")
                    .color(theme::OPENED),
            );
        }
        if let Some((file, fraction, _)) = &app.activity.progress {
            ui.add(egui::ProgressBar::new(*fraction).text(format!("Analyzing {}", short_name(file))));
        }
        if app.activity.log.is_empty() {
            ui.label(theme::caption("Nothing yet."));
        }
        for line in app.activity.log.iter().take(8) {
            ui.label(theme::caption(line.clone()));
        }
    });
}
