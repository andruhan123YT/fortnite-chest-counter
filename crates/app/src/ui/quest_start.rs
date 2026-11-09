//! "When did my quest stop counting?": a guide to the profile script and the import of
//! the file it saves. Fills in the base count and start time; saving stays with the page.

use std::path::Path;

use eframe::egui::{self, RichText};

use crate::config::SINCE_FORMAT;
use crate::quest::{self, QuestStart, PROFILE_FILE};
use crate::record::MatchRecord;
use crate::theme;
use crate::ui::settings::SettingsUi;

#[derive(Debug, Default)]
pub struct QuestImport {
    result: Option<Result<QuestStart, String>>,
}

pub fn show(ui: &mut egui::Ui, state: &mut QuestImport, fields: &mut SettingsUi, matches: &[MatchRecord]) {
    egui::CollapsingHeader::new(
        RichText::new("Not sure when your quest stopped counting? Get the exact time").strong(),
    )
    .id_salt("quest_start_guide")
    .show(ui, |ui| guide(ui, state, fields, matches));
}

fn guide(ui: &mut egui::Ui, state: &mut QuestImport, fields: &mut SettingsUi, matches: &[MatchRecord]) {
    ui.label(theme::caption(
        "Fortnite shows a finished quest but not when it finished. Your Epic profile has the exact time, and a \
         small script can download it for you. It takes about a minute.",
    ));
    ui.add_space(6.0);

    step(ui, 1, "Open the script folder and double-click get-epic-profile.bat.");
    if ui.button("Open the script folder").clicked() {
        match quest::script_folder() {
            Ok(dir) => reveal(&dir.join("get-epic-profile.bat")),
            Err(e) => state.result = Some(Err(format!("Could not write the script: {e}"))),
        }
    }
    step(
        ui,
        2,
        "Your browser opens an Epic Games page. Sign in if it asks you to. The page shows one line of text \
         with \"authorizationCode\" in it: copy it all (Ctrl+A, Ctrl+C).",
    );
    step(
        ui,
        3,
        "Paste it into the script window and press Enter. The script saves epic-profile.json next to itself \
         and closes the Epic session straight away.",
    );
    step(ui, 4, "Import the file here. The base count and start time below are filled in for you; then save.");
    ui.horizontal(|ui| {
        let saved = quest::saved_profile();
        if ui.add_enabled(saved.is_some(), egui::Button::new(format!("Import {PROFILE_FILE}"))).clicked() {
            if let Some(path) = &saved {
                import(state, fields, path);
            }
        }
        if cfg!(windows) && ui.button("Choose a file...").clicked() {
            if let Some(path) = super::settings::pick_file("Epic profile", "json") {
                import(state, fields, &path);
            }
        }
    });

    match &state.result {
        Some(Ok(start)) => outcome(ui, start, matches),
        Some(Err(message)) => {
            ui.label(RichText::new(message).color(theme::BROKEN));
        }
        None => {}
    }

    ui.add_space(8.0);
    ui.label(theme::caption(
        "epic-profile.json holds your account's game data (cosmetics, stats, quests). Keep it to yourself and delete \
         it when you are done. The script uses the same sign-in as the game, which Epic does not officially \
         support for other programs; it only reads your profile.",
    ));
    ui.add_space(4.0);
    ui.label(
        RichText::new(
            "Fortnite Chest Counter never gets access to your Epic account. It does not see your password, the code \
             or the session: it only reads the file you give it, on your PC.",
        )
        .strong()
        .color(theme::GOLD),
    );
}

fn step(ui: &mut egui::Ui, n: u32, text: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(format!("{n}.")).strong());
        ui.label(text);
    });
}

fn import(state: &mut QuestImport, fields: &mut SettingsUi, path: &Path) {
    let result = quest::read_profile(path);
    if let Ok(start) = &result {
        fields.base = start.progress.to_string();
        fields.since = start.at.format(SINCE_FORMAT).to_string();
    }
    state.result = Some(result);
}

fn outcome(ui: &mut egui::Ui, start: &QuestStart, matches: &[MatchRecord]) {
    let when = start.at.format("%d %b %Y, %H:%M:%S");
    let text = if start.finished {
        format!("Your quest finished on {when} at {} chests.", start.progress)
    } else {
        format!("Your quest is still running: {} chests as of {when}.", start.progress)
    };
    ui.label(RichText::new(text).strong().color(theme::OPENED));
    if start.finished {
        let note = match quest::match_at(matches, start.at) {
            Some(m) => format!(
                "It finished in your match of {}. That match stays out of the count: the quest already took its chests, \
                 though any you looted after the last one the quest needed are lost.",
                m.started_at.map(|t| t.format("%d %b %H:%M").to_string()).unwrap_or_default()
            ),
            None => "The match it finished in is not among your replays; counting starts right after it.".to_string(),
        };
        ui.label(theme::caption(note));
    }
    ui.label(theme::caption("Base count and start time are filled in."));
}

fn reveal(path: &Path) {
    let _ = crate::platform::reveal_in_explorer(path);
}
