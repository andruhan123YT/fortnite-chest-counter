//! "When did my quest stop counting?": a guide to the profile script and the import of
//! the file it saves. Fills in the base count and start time; saving stays with the page.

use std::path::Path;

use eframe::egui::{self, RichText};

use crate::config::SINCE_FORMAT;
use crate::i18n::{date_format, tr, trf};
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
        RichText::new(tr("Not sure when your quest stopped counting? Get the exact time")).strong(),
    )
    .id_salt("quest_start_guide")
    .show(ui, |ui| guide(ui, state, fields, matches));
}

fn guide(ui: &mut egui::Ui, state: &mut QuestImport, fields: &mut SettingsUi, matches: &[MatchRecord]) {
    ui.label(theme::caption(tr(
        "Fortnite shows a finished quest but not when it finished. Your Epic profile has the exact time, and a \
         small script can download it for you. It takes about a minute.",
    )));
    ui.add_space(6.0);

    step(ui, 1, tr("Open the script folder and double-click get-epic-profile.bat."));
    if ui.button(tr("Open the script folder")).clicked() {
        match quest::script_folder() {
            Ok(dir) => reveal(&dir.join("get-epic-profile.bat")),
            Err(e) => state.result = Some(Err(trf("Could not write the script: {0}", &[&e]))),
        }
    }
    step(
        ui,
        2,
        tr("Your browser opens an Epic Games page. Sign in if it asks you to. The page shows one line of text \
            with \"authorizationCode\" in it: copy it all (Ctrl+A, Ctrl+C)."),
    );
    step(
        ui,
        3,
        tr("Paste it into the script window and press Enter. The script saves epic-profile.json next to itself \
            and closes the Epic session straight away."),
    );
    step(ui, 4, tr("Import the file here. The base count and start time below are filled in for you; then save."));
    ui.horizontal(|ui| {
        let saved = quest::saved_profile();
        if ui.add_enabled(saved.is_some(), egui::Button::new(trf("Import {0}", &[&PROFILE_FILE]))).clicked() {
            if let Some(path) = &saved {
                import(state, fields, path);
            }
        }
        if cfg!(windows) && ui.button(tr("Choose a file...")).clicked() {
            if let Some(path) = super::settings::pick_file(tr("Epic profile"), "json") {
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
    ui.label(theme::caption(tr(
        "epic-profile.json holds your account's game data (cosmetics, stats, quests). Keep it to yourself and delete \
         it when you are done. The script uses the same sign-in as the game, which Epic does not officially \
         support for other programs; it only reads your profile.",
    )));
    ui.add_space(4.0);
    ui.label(
        RichText::new(tr(
            "Fortnite Chest Counter never gets access to your Epic account. It does not see your password, the code \
             or the session: it only reads the file you give it, on your PC.",
        ))
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
    let when = start.at.format(&date_format(&start.at, "%d %b %Y, %H:%M:%S")).to_string();
    let text = if start.finished {
        trf("Your quest finished on {0} at {1} chests.", &[&when, &start.progress])
    } else {
        trf("Your quest is still running: {0} chests as of {1}.", &[&start.progress, &when])
    };
    ui.label(RichText::new(text).strong().color(theme::OPENED));
    if start.finished {
        let note = match quest::match_at(matches, start.at) {
            Some(m) => trf(
                "It finished in your match of {0}. That match stays out of the count: the quest already took its chests, \
                 though any you looted after the last one the quest needed are lost.",
                &[&m.started_at.map(|t| t.format(&date_format(&t, "%d %b %H:%M")).to_string()).unwrap_or_default()],
            ),
            None => tr("The match it finished in is not among your replays; counting starts right after it.").to_string(),
        };
        ui.label(theme::caption(note));
    }
    ui.label(theme::caption(tr("Base count and start time are filled in.")));
}

fn reveal(path: &Path) {
    let _ = crate::platform::reveal_in_explorer(path);
}
