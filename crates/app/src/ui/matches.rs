//! Every analyzed match, and a detail view for the selected one.

use chests::{same_name, PlayerChests};
use eframe::egui::{self, RichText};
use egui_extras::{Column, TableBuilder};

use crate::app::{short_name, ChestApp};
use crate::record::MatchRecord;
use crate::theme;
use crate::worker::Command;

pub fn show(app: &mut ChestApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label(theme::title("Matches"));
        ui.label(theme::caption(format!("{} analyzed", app.store.matches.len())));
    });
    ui.add_space(8.0);

    if app.store.matches.is_empty() {
        ui.label(theme::caption(
            "No matches yet. New replays are picked up automatically; you can also drop .replay files here.",
        ));
        return;
    }

    let table_height = (ui.available_height() * 0.45).max(180.0);
    ui.allocate_ui(egui::vec2(ui.available_width(), table_height), |ui| table(app, ui));
    ui.separator();
    egui::ScrollArea::vertical().id_salt("match_detail").auto_shrink([false, false]).show(ui, |ui| {
        let selected = app.selected_match.clone().and_then(|id| app.store.matches.iter().find(|m| m.id == id).cloned());
        match selected {
            Some(m) => detail(app, ui, &m),
            None => {
                ui.label(theme::caption("Select a match to see its chests."));
            }
        }
    });
}

fn table(app: &mut ChestApp, ui: &mut egui::Ui) {
    let nick = app.settings.nick.clone();
    let rows: Vec<_> = app
        .store
        .matches
        .iter()
        .map(|m| (m.id.clone(), m.summary(&nick), app.settings.counts(m.started_at), m.clone()))
        .collect();
    // Selectable labels take the click for text selection, so a click on a cell's
    // text would never reach the row.
    ui.style_mut().interaction.selectable_labels = false;
    TableBuilder::new(ui)
        .striped(true)
        .sense(egui::Sense::click())
        .column(Column::auto().at_least(130.0))
        .column(Column::auto().at_least(50.0))
        .column(Column::auto().at_least(60.0))
        .column(Column::auto().at_least(60.0))
        .column(Column::auto().at_least(60.0))
        .column(Column::auto().at_least(50.0))
        .column(Column::auto().at_least(60.0))
        .column(Column::remainder())
        .header(24.0, |mut h| {
            for title in ["Date", "Length", "Place", "Opened", "Broken", "Mine", "Squad", ""] {
                h.col(|ui| {
                    let label = ui.label(RichText::new(title).strong().color(theme::MUTED));
                    if title == "Squad" {
                        label.on_hover_text(theme::SQUAD_NEARBY_NOTE);
                    }
                });
            }
        })
        .body(|body| {
            body.rows(24.0, rows.len(), |mut row| {
                let (id, s, counted, m) = &rows[row.index()];
                row.set_selected(app.selected_match.as_deref() == Some(id.as_str()));
                let dim =
                    |text: String| if *counted { RichText::new(text) } else { RichText::new(text).color(theme::MUTED) };
                row.col(|ui| {
                    let when = m
                        .started_at
                        .map(|t| t.format("%d %b %H:%M").to_string())
                        .unwrap_or_else(|| short_name(&m.file_name).into());
                    ui.label(dim(when));
                });
                row.col(|ui| {
                    ui.label(dim(format!("{:.0} min", m.length_minutes())));
                });
                row.col(|ui| {
                    ui.label(dim(m.placement.map(|p| format!("#{p}")).unwrap_or_default()));
                });
                row.col(|ui| {
                    ui.label(dim(s.opened.to_string()));
                });
                row.col(|ui| {
                    ui.label(dim(s.broken.to_string()));
                });
                row.col(|ui| {
                    ui.label(RichText::new(s.mine().to_string()).strong().color(if *counted {
                        theme::GOLD
                    } else {
                        theme::MUTED
                    }));
                });
                row.col(|ui| {
                    ui.label(dim(if s.teammates.is_empty() { "–".into() } else { s.mates.to_string() }));
                });
                row.col(|ui| {
                    let mut notes = Vec::new();
                    if !counted {
                        notes.push("not counted");
                    }
                    if !s.found {
                        notes.push("you are not in this match");
                    }
                    if m.packet_health < 0.97 {
                        notes.push("decode issues");
                    }
                    ui.label(theme::caption(notes.join(" · ")));
                });
                if row.response().clicked() {
                    app.selected_match = Some(id.clone());
                }
            });
        });
}

fn detail(app: &mut ChestApp, ui: &mut egui::Ui, m: &MatchRecord) {
    let nick = app.settings.nick.clone();
    let s = m.summary(&nick);
    ui.horizontal(|ui| {
        let when = m.started_at.map(|t| t.format("%A, %d %B %Y, %H:%M").to_string()).unwrap_or_default();
        ui.label(RichText::new(when).size(18.0).strong());
        if let (Some(p), Some(n)) = (m.placement, m.total_players) {
            ui.label(theme::caption(format!("placed #{p} of {n}")));
        }
        if let Some(e) = m.eliminations {
            ui.label(theme::caption(format!("{e} elimination{}", if e == 1 { "" } else { "s" })));
        }
        ui.label(theme::caption(format!("{:.1} min", m.length_minutes())));
    });
    ui.label(theme::caption(format!(
        "{}{} · packets decoded {:.2}%",
        m.file_name,
        m.season.as_ref().map(|v| format!(" · v{v}")).unwrap_or_default(),
        m.packet_health * 100.0
    )));
    ui.add_space(6.0);

    ui.horizontal(|ui| {
        if ui.button("Re-analyze").clicked() {
            app.worker.send(Command::Reanalyze(vec![m.file_name.clone()]));
        }
        if ui.button("Show replay file").clicked() {
            reveal(app, &m.file_name);
        }
        if ui.button(RichText::new("Remove from list").color(theme::WARN)).clicked() {
            app.store.remove(&m.id);
            app.selected_match = None;
            app.save_store();
        }
    });
    ui.add_space(8.0);

    if !s.found {
        let message = if nick.trim().is_empty() {
            "Could not tell who recorded this replay.".to_string()
        } else {
            format!("No player named \"{nick}\" in this match.")
        };
        ui.label(RichText::new(message).color(theme::WARN));
        let names: Vec<&PlayerChests> = m.roster().filter(|p| !p.is_bot).collect();
        if !names.is_empty() {
            ui.label(theme::caption("Players with chests in this match (click to use as your name):"));
            ui.horizontal_wrapped(|ui| {
                for p in names {
                    if ui.button(&p.name).clicked() {
                        app.settings.nick = p.name.clone();
                        app.settings_ui = crate::ui::settings::SettingsUi::from_settings(&app.settings);
                        app.apply_settings(false);
                    }
                }
            });
        }
        return;
    }

    ui.columns(2, |cols| {
        let me = m.me(&nick);
        cols[0].label(RichText::new(format!("You: {} chests", s.mine())).strong().color(theme::GOLD));
        cols[0].label(theme::caption(format!("Times are {}.", m.clock_origin())));
        if let Some(me) = me {
            timeline(&mut cols[0], m, me);
        }
        let treasures = m.treasures(&nick);
        if !treasures.is_empty() {
            cols[0].add_space(6.0);
            cols[0].label(RichText::new(format!("Treasures: {}", treasures.len())).strong().color(theme::TREASURE));
            for t in treasures {
                let from = match &t.source {
                    Some(src) if me.is_some_and(|me| same_name(&me.name, &src.player)) => {
                        format!(
                            "from your {} chest ({})",
                            src.kind.label(),
                            if src.broken { "broken" } else { "opened" }
                        )
                    }
                    Some(src) => format!(
                        "from {}'s {} chest ({})",
                        src.player,
                        src.kind.label(),
                        if src.broken { "broken" } else { "opened" }
                    ),
                    None => "from a squad chest out of view".to_string(),
                };
                cols[0].label(format!("{}  {from}", m.clock(t.time)));
            }
        }
        cols[1].label(RichText::new(format!("Teammates: at least {} chests", s.mates)).strong().color(theme::SQUAD));
        if s.teammates.is_empty() {
            cols[1].label(theme::caption("Solo, or teammates were never near you."));
        }
        for p in m
            .roster()
            .filter(|p| me.is_some_and(|me| !same_name(&p.name, &me.name) && me.team.is_some() && p.team == me.team))
        {
            cols[1].label(format!("{}: opened {}, broke {}", p.name, p.opened.len(), p.broken.len()));
        }
        cols[1].add_space(6.0);
        if m.linked_from.is_empty() {
            cols[1].label(theme::caption(theme::SQUAD_NEARBY_NOTE));
            cols[1].label(theme::caption("Add a teammate's replay of this match under Squad replays to fill it in."));
        } else {
            cols[1].label(
                RichText::new(format!("Filled in from {}'s replay of this match.", m.linked_from.join(", ")))
                    .color(theme::OPENED),
            );
        }
    });
}

fn timeline(ui: &mut egui::Ui, m: &MatchRecord, player: &PlayerChests) {
    egui::Grid::new(("timeline", &player.name)).num_columns(3).spacing([12.0, 2.0]).show(ui, |ui| {
        for (e, opened) in player.in_order() {
            ui.monospace(m.clock(e.time));
            if opened {
                ui.label(RichText::new("opened").color(theme::OPENED));
            } else {
                ui.label(RichText::new(if e.in_vehicle { "rammed" } else { "broke" }).color(theme::BROKEN));
            }
            ui.label(e.kind.label());
            ui.end_row();
        }
    });
}

fn reveal(app: &mut ChestApp, file_name: &str) {
    let candidates = [app.settings.replay_dir.join(file_name), app.settings.archive.dir.join(file_name)];
    let Some(path) = candidates.into_iter().find(|p| p.is_file()) else {
        app.activity.push_log(format!("{file_name} is gone from the replay folder and the archive"));
        return;
    };
    if crate::platform::reveal_in_explorer(&path).is_err() {
        app.activity.push_log(format!("Replay file: {}", path.display()));
    }
}
