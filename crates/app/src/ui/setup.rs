//! First-run wizard: three short steps, each with one question, and everything else left
//! at its default for Settings.

use eframe::egui::{self, Margin, RichText, Stroke};

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
    /// The player already has chests from the quest to start from.
    quest: bool,
    /// Replays in the folder last looked at: the folder, and their count, or None when the
    /// folder is missing.
    replays: Option<(String, Option<usize>)>,
}

impl SetupState {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            step: 0,
            fields: SettingsUi::from_settings(settings),
            error: None,
            quest: settings.base_count > 0 || settings.count_since.is_some(),
            replays: None,
        }
    }
}

const STEPS: usize = 3;

pub fn show(app: &mut ChestApp, ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(30.0);
        ui.add(egui::Image::new(&app.logo).fit_to_exact_size(egui::vec2(72.0, 72.0)));
        ui.label(RichText::new(tr("Fortnite Chest Counter")).size(28.0).strong().color(theme::gold()));
        ui.label(theme::caption(trf("Step {0} of {1}", &[&(app.setup.step + 1), &STEPS])));
        ui.add_space(12.0);
    });

    ui.vertical_centered(|ui| {
        ui.set_max_width(560.0);
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            // The card's text reads from the left; the welcome step centers its own lines.
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| match app.setup.step {
                0 => welcome_step(app, ui),
                1 => counting_step(app, ui),
                _ => background_step(app, ui),
            });
            if let Some(error) = &app.setup.error {
                ui.add_space(6.0);
                ui.label(RichText::new(error).color(theme::warn()));
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
                        .add(egui::Button::new(RichText::new(label).strong().color(theme::bg())).fill(theme::gold()))
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
    if !app.setup.quest {
        // Every match counts from zero.
        app.setup.fields.base = "0".into();
        app.setup.fields.since.clear();
    }
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

/// `.replay` files in `dir`, or None when the folder does not exist.
fn count_replays(dir: &str) -> Option<usize> {
    let entries = std::fs::read_dir(dir).ok()?;
    Some(
        entries
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("replay")))
            .count(),
    )
}

/// What the app does, the language, and whether Fortnite is recording replays: the one
/// setting outside the app that it cannot work without.
fn welcome_step(app: &mut ChestApp, ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.label(tr("Counts every chest you open and break, from your Fortnite replays."));
        ui.add_space(8.0);
        if let Some(lang) = crate::ui::settings::language_combo(ui, "setup_language", app.settings.language) {
            app.set_language(ui.ctx(), lang);
        }
    });
    ui.add_space(12.0);

    let dir = app.setup.fields.replay_dir.trim().to_owned();
    let count = match &app.setup.replays {
        Some((seen, count)) if *seen == dir => *count,
        _ => {
            let count = count_replays(&dir);
            app.setup.replays = Some((dir, count));
            count
        }
    };
    let (text, color, fill) = match count {
        Some(n) if n > 0 => {
            let replays = crate::i18n::plural(n as u64, "replay", "replays");
            (trf("Found {0} {1} in the replay folder.", &[&n, &replays]), theme::opened(), theme::opened())
        }
        Some(_) => (tr("No replays here yet.").to_string(), theme::warn(), theme::warn()),
        None => (tr("The replay folder does not exist.").to_string(), theme::warn(), theme::warn()),
    };
    egui::Frame::new()
        .fill(fill.gamma_multiply(0.12))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(text).color(color));
            if !count.is_some_and(|n| n > 0) {
                ui.label(theme::caption(tr(
                    "Replays must be on: Fortnite > Settings > Game > Replays > Record Replays.",
                )));
            }
        });
    if !count.is_some_and(|n| n > 0) {
        // Fortnite may keep its replays elsewhere on this PC.
        ui.add_space(6.0);
        ui.label(tr("Replay folder"));
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut app.setup.fields.replay_dir).desired_width(380.0));
            if cfg!(windows) && ui.button(tr("Browse...")).clicked() {
                if let Some(folder) = pick_folder(&app.setup.fields.replay_dir) {
                    app.setup.fields.replay_dir = folder;
                }
            }
        });
    }
}

/// A choice drawn as a card; returns true when clicked.
fn choice(ui: &mut egui::Ui, selected: bool, title: &str, note: &str) -> bool {
    let stroke = if selected { Stroke::new(2.0, theme::gold()) } else { Stroke::new(1.0, theme::card_hover()) };
    let frame = egui::Frame::new()
        .stroke(stroke)
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let radio = ui.radio(selected, RichText::new(title).strong()).clicked();
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                ui.add(egui::Label::new(theme::caption(note)).wrap());
            });
            radio
        });
    frame.inner || frame.response.interact(egui::Sense::click()).clicked()
}

fn counting_step(app: &mut ChestApp, ui: &mut egui::Ui) {
    ui.label(RichText::new(tr("Where do you start counting?")).size(18.0).strong());
    ui.add_space(8.0);
    if choice(ui, !app.setup.quest, tr("Count every match"), tr("From your first replay on.")) {
        app.setup.quest = false;
    }
    ui.add_space(6.0);
    if choice(
        ui,
        app.setup.quest,
        tr("I already have chests from the quest"),
        tr("Enter what the game already counted and when it stopped counting."),
    ) {
        app.setup.quest = true;
    }
    if app.setup.quest {
        ui.add_space(10.0);
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
}

/// A name with a switch on the right.
fn switch_row(ui: &mut egui::Ui, label: &str, on: &mut bool) {
    // The switch takes its place first, so a long name wraps instead of pushing it out.
    let width = ui.available_width();
    ui.allocate_ui_with_layout(egui::vec2(width, 0.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
        theme::toggle(ui, on);
        ui.add_space(12.0);
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.add(egui::Label::new(label).wrap());
        });
    });
}

fn background_step(app: &mut ChestApp, ui: &mut egui::Ui) {
    ui.label(RichText::new(tr("Let it run in the background")).size(18.0).strong());
    ui.label(theme::caption(tr(
        "The app sits in the tray, counts every match when you return to the lobby and tells you the result.",
    )));
    ui.add_space(10.0);
    let s = &mut app.settings;
    switch_row(ui, tr("Start by itself when Fortnite starts"), &mut s.start_with_windows);
    ui.add_space(4.0);
    switch_row(ui, tr("Notify me after each match"), &mut s.notifications);
    ui.add_space(4.0);
    switch_row(ui, tr("Swap exact chest counts with squadmates who run the app (online)"), &mut s.online);
    ui.label(theme::caption(tr(
        "Sends your Epic account id and name and your chests per match; squadmates get them only for matches \
         you played together.",
    )));
    ui.add_space(10.0);
    ui.label(theme::caption(tr("You can change the rest in Settings.")));
}
