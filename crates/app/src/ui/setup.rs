//! First-run wizard.

use eframe::egui::{self, RichText};

use crate::app::ChestApp;
use crate::config::Settings;
use crate::i18n::{tr, trf};
use crate::theme;
use crate::ui::settings::{pick_folder, SettingsUi};

#[derive(Debug, Clone, Default)]
pub struct SetupState {
    step: usize,
    fields: SettingsUi,
    error: Option<String>,
}

impl SetupState {
    pub fn from_settings(settings: &Settings) -> Self {
        Self { step: 0, fields: SettingsUi::from_settings(settings), error: None }
    }
}

const STEPS: usize = 3;

pub fn show(app: &mut ChestApp, ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(30.0);
        ui.add(egui::Image::new(&app.logo).fit_to_exact_size(egui::vec2(72.0, 72.0)));
        ui.label(RichText::new(tr("Fortnite Chest Counter")).size(28.0).strong().color(theme::GOLD));
        ui.label(theme::caption(trf("Step {0} of {1}", &[&(app.setup.step + 1), &STEPS])));
        ui.add_space(8.0);
        language_picker(app, ui);
        ui.add_space(12.0);
    });

    ui.vertical_centered(|ui| {
        ui.set_max_width(560.0);
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            match app.setup.step {
                0 => name_step(app, ui),
                1 => counting_step(app, ui),
                _ => background_step(app, ui),
            }
            if let Some(error) = &app.setup.error {
                ui.add_space(6.0);
                ui.label(RichText::new(error).color(theme::WARN));
            }
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if app.setup.step > 0 && ui.button(tr("Back")).clicked() {
                    app.setup.step -= 1;
                    app.setup.error = None;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let last = app.setup.step + 1 == STEPS;
                    let label = if last { tr("Start counting") } else { tr("Next") };
                    if ui
                        .add(egui::Button::new(RichText::new(label).strong().color(theme::BG)).fill(theme::GOLD))
                        .clicked()
                    {
                        next(app, last);
                    }
                });
            });
        });
    });
}

fn next(app: &mut ChestApp, last: bool) {
    let mut settings = app.settings.clone();
    if let Err(message) = app.setup.fields.commit(&mut settings) {
        app.setup.error = Some(message);
        return;
    }
    app.setup.error = None;
    if !last {
        app.setup.step += 1;
        return;
    }
    settings.setup_done = true;
    // A new install has nothing to catch up on.
    settings.notes_seen = crate::update::version_text(crate::update::current());
    app.settings = settings;
    app.settings_ui = SettingsUi::from_settings(&app.settings);
    app.apply_settings(true);
}

/// English or Russian, switched on the spot: the first thing a new player sees.
fn language_picker(app: &mut ChestApp, ui: &mut egui::Ui) {
    let mut lang = app.settings.language;
    let width = 220.0;
    ui.allocate_ui_with_layout(egui::vec2(width, 26.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        for option in crate::i18n::Lang::ALL {
            ui.selectable_value(&mut lang, option, option.name());
        }
    });
    if lang != app.settings.language {
        app.settings.language = lang;
        crate::i18n::set(lang);
        let _ = app.settings.save();
    }
}

fn name_step(_app: &mut ChestApp, ui: &mut egui::Ui) {
    ui.label(RichText::new(tr("Who are you in Fortnite?")).size(18.0).strong());
    ui.label(theme::caption(tr(
        "Nothing to type: every replay records who played it, and that is who gets counted. If several \
         accounts play on this PC, each gets its own count, and the menu switches between them.",
    )));
    ui.add_space(8.0);
    ui.label(theme::caption(tr("Replays must be on: Fortnite > Settings > Game > Replays > Record Replays.")));
}

fn counting_step(app: &mut ChestApp, ui: &mut egui::Ui) {
    ui.label(RichText::new(tr("Where do you start counting?")).size(18.0).strong());
    ui.label(theme::caption(
        tr("If the game already counted chests for you, enter that number as the base and the moment it stopped counting. \
         Example: the 1000-chest quest finished on 7 Nov at 22:39 > base 1000, start 2026-11-07 22:39. \
         Still working on the quest? Enter its current progress and the current time."),
    ));
    ui.add_space(8.0);
    egui::Grid::new("setup_counting").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
        ui.label(tr("Base count"));
        ui.add(egui::TextEdit::singleline(&mut app.setup.fields.base).desired_width(120.0));
        ui.end_row();
        ui.label(tr("Start time"));
        ui.add(
            egui::TextEdit::singleline(&mut app.setup.fields.since)
                .hint_text(tr("empty = all matches"))
                .desired_width(180.0),
        );
        ui.end_row();
    });
    ui.add_space(8.0);
    crate::ui::quest_start::show(ui, &mut app.quest_import, &mut app.setup.fields, &app.store.matches);
}

fn background_step(app: &mut ChestApp, ui: &mut egui::Ui) {
    ui.label(RichText::new(tr("Let it run in the background")).size(18.0).strong());
    ui.label(theme::caption(tr(
        "The app sits in the tray, counts every match when you return to the lobby and tells you the result.",
    )));
    ui.add_space(8.0);
    ui.label(tr("Replay folder"));
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut app.setup.fields.replay_dir).desired_width(380.0));
        if cfg!(windows) && ui.button(tr("Browse...")).clicked() {
            if let Some(folder) = pick_folder(&app.setup.fields.replay_dir) {
                app.setup.fields.replay_dir = folder;
            }
        }
    });
    ui.add_space(6.0);
    ui.checkbox(&mut app.settings.watch, tr("Count new matches automatically"));
    ui.checkbox(&mut app.settings.start_with_windows, tr("Start by itself when Fortnite starts"));
    ui.checkbox(&mut app.settings.notifications, tr("Notify me after each match"));
    ui.checkbox(&mut app.settings.online, tr("Swap exact chest counts with squadmates who run the app (online)"));
    ui.label(theme::caption(tr(
        "Sends your Epic account id and name and your chests per match; squadmates get them only for matches \
         you played together.",
    )));
    ui.checkbox(&mut app.settings.archive.enabled, tr("Keep copies of replays (Fortnite deletes old ones)"));
    ui.add_enabled_ui(app.settings.archive.enabled, |ui| {
        ui.add(
            egui::Slider::new(&mut app.settings.archive.max_gb, 1.0..=200.0)
                .logarithmic(true)
                .suffix(" GB")
                .text(tr("max archive size")),
        );
    });
}
