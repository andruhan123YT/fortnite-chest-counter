//! Replays recorded by teammates. Each is tied to the user's match with the same server
//! match id and fills in the chests the user's own replay could not see.

use eframe::egui::{self, RichText};
use egui_extras::{Column, TableBuilder};

use crate::app::{short_name, ChestApp, Page};
use crate::record::ANALYZER_VERSION;
use crate::store::SquadLink;
use crate::theme;
use crate::worker::Command;

pub fn show(app: &mut ChestApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label(theme::title("Squad replays"));
        ui.label(theme::caption(format!("{} added", app.store.squad_replays.len())));
    });
    ui.label(theme::caption(
        "Add replays your teammates recorded of matches you played together. Each one is tied to your match \
         by the match id the server gives every player in it, never by date, and fills in what your own replay \
         could not see: teammates' chests far from you, and yours when they saw more of them. \
         They never count as matches of their own.",
    ));
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if cfg!(windows) && ui.button("Add replays...").clicked() {
            let files = super::settings::pick_files("Fortnite replay", "replay");
            if !files.is_empty() {
                app.activity.push_log(format!("Analyzing {} teammate replay(s)", files.len()));
                app.worker.send(Command::AnalyzeSquad(files));
            }
        }
        ui.label(theme::caption("or drop .replay files on this page."));
    });
    if let Some((file, fraction, _)) = &app.activity.progress {
        ui.label(theme::caption(format!("Analyzing {file}: {:.0}%", fraction * 100.0)));
    }
    ui.add_space(10.0);

    if app.store.squad_replays.is_empty() {
        ui.label(RichText::new("No teammate replays yet.").color(theme::MUTED));
        ui.label(theme::caption(
            "Teammates find theirs in %LOCALAPPDATA%\\FortniteGame\\Saved\\Demos. The newest ones are the last matches.",
        ));
        return;
    }

    let rows: Vec<(String, String, String, SquadLink, bool)> = app
        .store
        .squad_replays
        .iter()
        .map(|r| {
            let when = r.started_at.map(|t| t.format("%d %b %H:%M").to_string()).unwrap_or_default();
            let owner = r.owner.clone().unwrap_or_else(|| short_name(&r.file_name).to_string());
            let stale = r.analyzer_version < ANALYZER_VERSION;
            (r.id.clone(), when, owner, app.store.squad_link(r), stale)
        })
        .collect();
    let mut remove = None;
    let mut open = None;
    TableBuilder::new(ui)
        .striped(true)
        .column(Column::auto().at_least(110.0))
        .column(Column::auto().at_least(150.0))
        .column(Column::remainder().at_least(240.0))
        .column(Column::auto())
        .header(24.0, |mut h| {
            for title in ["Recorded", "By", "Your match", ""] {
                h.col(|ui| {
                    ui.label(RichText::new(title).strong().color(theme::MUTED));
                });
            }
        })
        .body(|mut body| {
            for (id, when, owner, link, stale) in &rows {
                body.row(24.0, |mut row| {
                    row.col(|ui| {
                        ui.label(when);
                    });
                    row.col(|ui| {
                        ui.label(owner);
                        if *stale {
                            ui.label(RichText::new("outdated").color(theme::WARN)).on_hover_text(
                                "Analyzed by an older version of the app. Add the file again to update it: \
                                 counts and chest times may be off until then.",
                            );
                        }
                    });
                    row.col(|ui| match link {
                        SquadLink::Linked { match_record, added } => {
                            let text = match added {
                                0 => "linked, your replay already saw everything".to_string(),
                                n => format!("linked, +{n} chests your replay missed"),
                            };
                            if ui.link(RichText::new(text).color(theme::OPENED)).clicked() {
                                open = Some(match_record.clone());
                            }
                        }
                        SquadLink::Unmatched => {
                            ui.label(RichText::new("no replay of yours from this match").color(theme::MUTED));
                        }
                        SquadLink::Own => {
                            ui.label(RichText::new("this is your own replay").color(theme::WARN));
                        }
                        SquadLink::NoMatchId => {
                            ui.label(RichText::new("no match id in this replay").color(theme::WARN));
                        }
                    });
                    row.col(|ui| {
                        if ui.small_button("Remove").clicked() {
                            remove = Some(id.clone());
                        }
                    });
                });
            }
        });
    if let Some(id) = remove {
        app.store.remove_squad_replay(&id);
        app.save_store();
    }
    if let Some(id) = open {
        app.selected_match = Some(id);
        app.page = Page::Matches;
    }
}
