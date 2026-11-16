//! Every analyzed match, and a detail view for the selected one.

use chests::{same_name, PlayerChests};
use eframe::egui::{self, RichText};
use egui_extras::{Column, TableBuilder};

use crate::app::{short_name, ChestApp};
use crate::i18n::{date_format, plural, tr, trf};
use crate::record::MatchRecord;
use crate::theme;
use crate::worker::Command;

pub fn show(app: &mut ChestApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label(theme::title(tr("Matches")));
        let own = crate::stats::own(&app.store.matches, &app.settings).count();
        ui.label(theme::caption(trf("{0} analyzed", &[&own])));
    });
    ui.add_space(8.0);

    if crate::stats::own(&app.store.matches, &app.settings).next().is_none() {
        ui.label(theme::caption(tr(
            "No matches yet. New replays are picked up automatically; you can also drop .replay files here.",
        )));
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
                ui.label(theme::caption(tr("Select a match to see its chests.")));
            }
        }
    });
}

fn table(app: &mut ChestApp, ui: &mut egui::Ui) {
    // Only the rows on screen are drawn: each reads its match in place, so a long
    // history costs nothing per frame.
    let mut clicked = None;
    let view = &*app;
    let rows: Vec<&MatchRecord> = crate::stats::own(&view.store.matches, &view.settings).collect();
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
            let squad = tr("Squad");
            let titles = [tr("Date"), tr("Length"), tr("Place"), tr("Opened"), tr("Broken"), tr("Mine"), squad, ""];
            for title in titles {
                h.col(|ui| {
                    let label = ui.label(RichText::new(title).strong().color(theme::muted()));
                    if title == squad {
                        label.on_hover_text(theme::squad_nearby_note());
                    }
                });
            }
        })
        .body(|body| {
            body.rows(24.0, rows.len(), |mut row| {
                let m = rows[row.index()];
                let s = m.summary(view.settings.name_in(m));
                let counted = view.settings.counts(m.started_at);
                row.set_selected(view.selected_match.as_deref() == Some(m.id.as_str()));
                let dim = |text: String| {
                    if counted {
                        RichText::new(text)
                    } else {
                        RichText::new(text).color(theme::muted())
                    }
                };
                row.col(|ui| {
                    let when = m
                        .started_at
                        .map(|t| t.format(&date_format(&t, "%d %b %H:%M")).to_string())
                        .unwrap_or_else(|| short_name(&m.file_name).into());
                    ui.label(dim(when));
                });
                row.col(|ui| {
                    ui.label(dim(trf("{0} min", &[&format!("{:.0}", m.length_minutes())])));
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
                    ui.label(RichText::new(s.mine().to_string()).strong().color(if counted {
                        theme::gold()
                    } else {
                        theme::muted()
                    }));
                });
                row.col(|ui| {
                    ui.label(dim(if s.teammates.is_empty() { "–".into() } else { s.mates.to_string() }));
                });
                row.col(|ui| {
                    let mut notes = Vec::new();
                    if !counted {
                        notes.push(tr("not counted"));
                    }
                    if !s.found {
                        notes.push(tr("you are not in this match"));
                    }
                    if m.packet_health < 0.97 {
                        notes.push(tr("decode issues"));
                    }
                    ui.label(theme::caption(notes.join(" · ")));
                });
                if row.response().clicked() {
                    clicked = Some(m.id.clone());
                }
            });
        });
    if clicked.is_some() {
        app.selected_match = clicked;
    }
}

fn detail(app: &mut ChestApp, ui: &mut egui::Ui, m: &MatchRecord) {
    let nick = app.settings.name_in(m).to_owned();
    let s = m.summary(&nick);
    let logo = app.logo.clone();
    ui.horizontal(|ui| {
        let when =
            m.started_at.map(|t| t.format(&date_format(&t, "%A, %d %B %Y, %H:%M")).to_string()).unwrap_or_default();
        ui.label(RichText::new(when).size(18.0).strong());
        if let (Some(p), Some(n)) = (m.placement, m.total_players) {
            ui.label(theme::caption(trf("placed #{0} of {1}", &[&p, &n])));
        }
        if let Some(e) = m.eliminations {
            let word = plural(u64::from(e), "elimination", "eliminations");
            ui.label(theme::caption(format!("{e} {word}")));
        }
        ui.label(theme::caption(trf("{0} min", &[&format!("{:.1}", m.length_minutes())])));
    });
    ui.label(theme::caption(trf(
        "{0}{1} · packets decoded {2}%",
        &[
            &m.file_name,
            &m.season.as_ref().map(|v| format!(" · v{v}")).unwrap_or_default(),
            &format!("{:.2}", m.packet_health * 100.0),
        ],
    )));
    ui.add_space(6.0);

    ui.horizontal(|ui| {
        if ui.button(tr("Re-analyze")).clicked() {
            app.worker.send(Command::Reanalyze(vec![m.file_name.clone()]));
        }
        if ui.button(tr("Show replay file")).clicked() {
            reveal(app, &m.file_name);
        }
        if ui.button(RichText::new(tr("Remove from list")).color(theme::warn())).clicked() {
            app.store.remove(&m.id);
            app.selected_match = None;
            app.save_store();
        }
    });
    ui.add_space(8.0);

    if !s.found {
        let message = if nick.trim().is_empty() {
            tr("Could not tell who recorded this replay.").to_string()
        } else {
            trf("No player named \"{0}\" in this match.", &[&nick])
        };
        ui.label(RichText::new(message).color(theme::warn()));
        return;
    }

    ui.columns(2, |cols| {
        let me = m.me(&nick);
        cols[0].label(RichText::new(trf("You: {0} chests", &[&s.mine()])).strong().color(theme::gold()));
        cols[0].label(theme::caption(trf("Times are {0}.", &[&m.clock_origin()])));
        if let Some(me) = me {
            timeline(&mut cols[0], m, me);
        }
        let treasures = m.treasures(&nick);
        if !treasures.is_empty() {
            cols[0].add_space(6.0);
            cols[0].label(RichText::new(trf("Treasures: {0}", &[&treasures.len()])).strong().color(theme::treasure()));
            for t in treasures {
                let how = |broken: bool| if broken { tr("broken") } else { tr("opened") };
                let from = match &t.source {
                    Some(src) if me.is_some_and(|me| same_name(&me.name, &src.player)) => {
                        trf("from your {0} ({1})", &[&theme::chest_phrase(src.kind), &how(src.broken)])
                    }
                    Some(src) => {
                        trf("from {0}'s {1} ({2})", &[&src.player, &theme::chest_phrase(src.kind), &how(src.broken)])
                    }
                    None => tr("from a squad chest out of view").to_string(),
                };
                cols[0].label(format!("{}  {from}", m.clock(t.time)));
            }
        }
        let all_exact = !s.teammates.is_empty() && s.teammates.iter().all(|t| t.with_app);
        let heading = if all_exact {
            trf("Teammates: {0} chests", &[&s.mates])
        } else {
            trf("Teammates: at least {0} chests", &[&s.mates])
        };
        cols[1].label(RichText::new(heading).strong().color(theme::squad()));
        if s.teammates.is_empty() {
            cols[1].label(theme::caption(tr("Solo, or teammates were never near you.")));
        }
        for t in &s.teammates {
            cols[1].horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(&t.name);
                if t.with_app {
                    app_badge(ui, &logo);
                }
                ui.label(trf(": opened {0}, broke {1}", &[&t.opened, &t.broken]));
            });
        }
        cols[1].add_space(6.0);
        if s.teammates.iter().any(|t| t.with_app) {
            cols[1].horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                app_badge(ui, &logo);
                ui.label(theme::caption(tr("runs the app too: their own count, from their own replay.")));
            });
        }
        // Teammates' replays only matter for those without the app.
        if !m.linked_from.is_empty() {
            cols[1].label(
                RichText::new(trf("Filled in from {0}'s replay of this match.", &[&m.linked_from.join(", ")]))
                    .color(theme::opened()),
            );
        } else if !all_exact {
            cols[1].label(theme::caption(theme::squad_nearby_note()));
            cols[1]
                .label(theme::caption(tr("Add a teammate's replay of this match under Squad replays to fill it in.")));
        }
    });
}

/// The app's chest next to a squadmate who runs the app too.
fn app_badge(ui: &mut egui::Ui, logo: &egui::TextureHandle) {
    ui.add(egui::Image::new(logo).fit_to_exact_size(egui::vec2(14.0, 14.0))).on_hover_text(tr(
        "Runs Fortnite Chest Counter too, so these are their own numbers from their own replay, not only the chests \
         seen near you.",
    ));
}

fn timeline(ui: &mut egui::Ui, m: &MatchRecord, player: &PlayerChests) {
    egui::Grid::new(("timeline", &player.name)).num_columns(3).spacing([12.0, 2.0]).show(ui, |ui| {
        for (e, opened) in player.in_order() {
            ui.monospace(m.clock(e.time));
            if opened {
                let label = ui.label(
                    RichText::new(if e.from_loot { tr("opened*") } else { tr("opened") }).color(theme::opened()),
                );
                if e.from_loot {
                    label.on_hover_text(tr(
                        "The replay lost this open: its loot landed at your feet with no chest recorded there, \
                         so it was found from the loot. It counts like any other.",
                    ));
                }
            } else {
                ui.label(RichText::new(if e.in_vehicle { tr("rammed") } else { tr("broke") }).color(theme::broken()));
            }
            ui.label(theme::chest_kind(e.kind));
            ui.end_row();
        }
    });
}

fn reveal(app: &mut ChestApp, file_name: &str) {
    let candidates = [app.settings.replay_dir.join(file_name), app.settings.archive.dir.join(file_name)];
    let Some(path) = candidates.into_iter().find(|p| p.is_file()) else {
        app.activity.push_log(trf("{0} is gone from the replay folder and the archive", &[&file_name]));
        return;
    };
    if crate::platform::reveal_in_explorer(&path).is_err() {
        app.activity.push_log(trf("Replay file: {0}", &[&path.display()]));
    }
}
