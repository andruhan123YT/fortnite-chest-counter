//! Settings page.

use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText};

use crate::app::ChestApp;
use crate::config::{data_dir, parse_since, Settings, SINCE_FORMAT};
use crate::i18n::{tr, trf};
use crate::worker::Command;
use crate::{archive, theme};

/// Text buffers for fields that are edited as strings and validated on apply.
#[derive(Debug, Clone, Default)]
pub struct SettingsUi {
    pub base: String,
    pub adjustment: String,
    pub since: String,
    pub replay_dir: String,
    pub archive_dir: String,
    pub error: Option<String>,
    pub archive_usage: Option<u64>,
    /// Names for Running total lines, one per line.
    pub online_lines: String,
    /// The stream overlay port while it is being edited.
    pub stream_port: u16,
    /// A checkbox or choice changed and is not applied yet.
    pub toggled: bool,
}

impl SettingsUi {
    pub fn from_settings(s: &Settings) -> Self {
        Self {
            base: s.base_count.to_string(),
            adjustment: signed(s.adjustment),
            since: s.count_since.map(|d| d.format(SINCE_FORMAT).to_string()).unwrap_or_default(),
            replay_dir: s.replay_dir.display().to_string(),
            archive_dir: s.archive.dir.display().to_string(),
            error: None,
            archive_usage: None,
            stream_port: s.stream.port,
            online_lines: s.online_lines.join("\n"),
            toggled: false,
        }
    }

    /// Validates the text fields into `settings`. Returns an error message on bad input.
    pub fn commit(&self, settings: &mut Settings) -> Result<(), String> {
        let base = self.base.trim().replace([',', ' '], "");
        let base = if base.is_empty() {
            0
        } else {
            base.parse::<u64>().map_err(|_| tr("The base count must be a whole number."))?
        };
        let since = if self.since.trim().is_empty() {
            None
        } else {
            Some(parse_since(&self.since).ok_or(tr("Use the format 2026-11-07 22:39 for the start time."))?)
        };
        settings.base_count = base;
        settings.adjustment = parse_adjustment(&self.adjustment)?;
        settings.count_since = since;
        settings.replay_dir = PathBuf::from(self.replay_dir.trim());
        settings.archive.dir = crate::config::archive_dir_or_default(Path::new(self.archive_dir.trim()));
        Ok(())
    }
}

/// `+25`, `-3` or `0`, the way the adjustment field shows it.
pub fn signed(n: i64) -> String {
    if n > 0 {
        format!("+{n}")
    } else {
        n.to_string()
    }
}

/// Reads the adjustment field: a whole number, optionally signed; empty means none.
fn parse_adjustment(text: &str) -> Result<i64, String> {
    let text = text.trim().replace([',', ' '], "");
    if text.is_empty() {
        return Ok(0);
    }
    text.trim_start_matches('+')
        .parse::<i64>()
        .map_err(|_| tr("Chests added by hand must be a whole number, like +25 or -3.").to_string())
}

pub fn pick_folder(current: &str) -> Option<String> {
    #[cfg(windows)]
    {
        rfd::FileDialog::new().set_directory(current).pick_folder().map(|p| p.display().to_string())
    }
    #[cfg(not(windows))]
    {
        let _ = current;
        None
    }
}

pub fn pick_file(filter_name: &str, extension: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        rfd::FileDialog::new().add_filter(filter_name, &[extension]).pick_file()
    }
    #[cfg(not(windows))]
    {
        let _ = (filter_name, extension);
        None
    }
}

pub fn pick_files(filter_name: &str, extension: &str) -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        rfd::FileDialog::new().add_filter(filter_name, &[extension]).pick_files().unwrap_or_default()
    }
    #[cfg(not(windows))]
    {
        let _ = (filter_name, extension);
        Vec::new()
    }
}

fn folder_row(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.label(label);
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(value).desired_width(420.0));
        if cfg!(windows) && ui.button(tr("Browse...")).clicked() {
            if let Some(folder) = pick_folder(value) {
                *value = folder;
            }
        }
    });
    ui.end_row();
}

/// Checkboxes and choices take effect at once: the file, autostart and the worker all
/// follow. Text fields wait for Save. A drag is applied once it ends, not every frame.
pub fn show(app: &mut ChestApp, ui: &mut egui::Ui) {
    let before = app.settings.clone();
    page(app, ui);
    if app.settings != before {
        app.settings_ui.toggled = true;
    }
    if app.settings_ui.toggled && !ui.ctx().input(|i| i.pointer.any_down()) {
        app.settings_ui.toggled = false;
        app.apply_settings(false);
    }
}

fn page(app: &mut ChestApp, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.label(theme::title(tr("Settings")));
        ui.add_space(10.0);

        section(ui, tr("Language"), |ui| {
            ui.horizontal(|ui| {
                let mut lang = app.settings.language;
                for option in crate::i18n::Lang::ALL {
                    ui.selectable_value(&mut lang, option, option.name());
                }
                if lang != app.settings.language {
                    app.settings.language = lang;
                    crate::i18n::set(lang);
                    let _ = app.settings.save();
                }
            });
        });

        section(ui, tr("Counting"), |ui| {
            egui::Grid::new("counting").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
                ui.label(tr("Account"));
                let account = if app.settings.nick.trim().is_empty() {
                    tr("found from your first replay").to_string()
                } else {
                    app.settings.nick.clone()
                };
                ui.label(RichText::new(account).strong())
                    .on_hover_text(tr("Every replay records who played it. Your account is told apart by its Epic id, so a new name follows by itself."));
                ui.end_row();
                ui.label(tr("Base count"));
                ui.add(egui::TextEdit::singleline(&mut app.settings_ui.base).desired_width(120.0));
                ui.end_row();
                ui.label(tr("Added by hand"));
                ui.horizontal(|ui| {
                    let field = &mut app.settings_ui.adjustment;
                    let current = parse_adjustment(field).unwrap_or(0);
                    if ui.small_button(" - ").on_hover_text(tr("One chest fewer")).clicked() {
                        *field = signed(current - 1);
                    }
                    ui.add(egui::TextEdit::singleline(field).hint_text("0").desired_width(80.0));
                    if ui.small_button(" + ").on_hover_text(tr("One chest more")).clicked() {
                        *field = signed(current + 1);
                    }
                    ui.label(theme::caption(tr("then Save")));
                });
                ui.end_row();
                ui.label(tr("Count matches from"));
                ui.add(egui::TextEdit::singleline(&mut app.settings_ui.since).hint_text("2026-11-07 22:39").desired_width(160.0));
                ui.end_row();
            });
            ui.label(theme::caption(
                tr("The base is what you already had when you started counting, e.g. 1000 for a finished 1000-chest quest. \
                 Only matches started at or after the start time are added to it. Leave the time empty to count every match. \
                 Added by hand is for chests the replays missed, say from matches whose replays Fortnite deleted \
                 (it keeps only 100) before the app saw them; a minus takes chests off."),
            ));
            ui.add_space(6.0);
            crate::ui::quest_start::show(ui, &mut app.quest_import, &mut app.settings_ui, &app.store.matches);
        });

        section(ui, tr("Replays"), |ui| {
            egui::Grid::new("replays").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
                folder_row(ui, tr("Replay folder"), &mut app.settings_ui.replay_dir);
            });
            ui.checkbox(&mut app.settings.watch, tr("Watch the folder and count new matches automatically"));
            ui.checkbox(&mut app.settings.warn_unrecorded, tr("Warn me when Fortnite plays a match without saving its replay"));
            ui.label(theme::caption(tr("Fortnite keeps only the latest 100 replays on PC and deletes older ones.")));
            ui.add_space(6.0);
            ui.checkbox(&mut app.settings.archive.enabled, tr("Keep a copy of every counted replay"));
            ui.add_enabled_ui(app.settings.archive.enabled, |ui| {
                egui::Grid::new("archive").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
                    folder_row(ui, tr("Archive folder"), &mut app.settings_ui.archive_dir);
                    ui.label(tr("Maximum size"));
                    ui.add(egui::Slider::new(&mut app.settings.archive.max_gb, 1.0..=200.0).logarithmic(true).suffix(" GB"));
                    ui.end_row();
                });
                let usage = *app.settings_ui.archive_usage.get_or_insert_with(|| archive::usage_bytes(&app.settings.archive.dir));
                ui.label(theme::caption(trf(
                    "Using {0} GB · about {1} matches fit. The oldest copies are deleted when the limit is reached.",
                    &[
                        &format!("{:.2}", usage as f64 / 1024f64.powi(3)),
                        &((app.settings.archive.max_gb as f64 * 1024.0 / 26.0) as u64),
                    ],
                )));
            });
        });

        section(ui, tr("Overlay"), |ui| {
            let editing = app.overlay.editing;
            let o = &mut app.settings.overlay;
            ui.checkbox(&mut o.enabled, tr("Show your count on top of the game"));
            let mut start_editing = false;
            let mut lock = false;
            ui.add_enabled_ui(o.enabled, |ui| {
                ui.checkbox(&mut o.only_with_fortnite, tr("Only while Fortnite is running"));
                ui.checkbox(&mut o.only_over_fortnite, tr("Hide it while another window is in front of Fortnite"));
                egui::Grid::new("overlay").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
                    ui.label(tr("Position"));
                    ui.horizontal(|ui| {
                        let current = if o.position.is_some() { tr("Where you moved it") } else { o.corner.label() };
                        egui::ComboBox::from_id_salt("overlay_corner").selected_text(current).show_ui(ui, |ui| {
                            for corner in crate::config::OverlayCorner::ALL {
                                let selected = o.position.is_none() && o.corner == corner;
                                if ui.selectable_label(selected, corner.label()).clicked() {
                                    o.corner = corner;
                                    o.position = None;
                                }
                            }
                        });
                        if editing {
                            lock = ui.button(RichText::new(tr("Lock")).strong().color(theme::GOLD)).clicked();
                        } else {
                            start_editing = ui
                                .button(tr("Move and resize"))
                                .on_hover_text(tr("Shows the overlay so you can drag it anywhere and pull its corner to resize it"))
                                .clicked();
                        }
                    });
                    ui.end_row();
                    ui.label(tr("Opacity"));
                    ui.add(egui::Slider::new(&mut o.opacity, 0.3..=1.0).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
                    ui.end_row();
                    ui.label(tr("Size"));
                    ui.add(
                        egui::Slider::new(&mut o.scale, crate::overlay::MIN_SCALE..=crate::overlay::MAX_SCALE)
                            .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
                    );
                    ui.end_row();
                });
                if editing {
                    ui.label(
                        RichText::new(
                            tr("Drag the overlay where you want it and pull its bottom right corner to resize it, then press Lock. \
                             It locks by itself when you leave this page."),
                        )
                        .color(theme::GOLD),
                    );
                }
            });
            if start_editing {
                crate::overlay::start_editing(app);
            }
            if lock {
                crate::overlay::lock(app);
            }
            ui.label(theme::caption(
                tr("Set Fortnite to Windowed Fullscreen (Settings > Video > Window Mode): Windows cannot draw over exclusive fullscreen. \
                 The overlay ignores the mouse, so clicks go to the game; it only takes the mouse while you move it. \
                 The lock in its top right corner unlocks it for moving too, and locks it again; \
                 while it is unlocked, the cross next to it hides the overlay."),
            ));
        });

        section(ui, tr("Stream overlay (OBS)"), |ui| stream(app, ui));

        section(ui, tr("Windows"), |ui| {
            ui.checkbox(&mut app.settings.start_with_windows, tr("Start by itself, in the tray"));
            ui.add_enabled_ui(app.settings.start_with_windows, |ui| {
                ui.indent("autostart", |ui| {
                    ui.radio_value(&mut app.settings.start_with_fortnite, true, tr("When Fortnite starts"));
                    ui.radio_value(&mut app.settings.start_with_fortnite, false, tr("When Windows starts"));
                    if app.settings.start_with_fortnite {
                        ui.label(theme::caption(
                            tr("A minute after Fortnite closes, once the last match is counted, the app leaves the tray \
                             and waits for the next game. With its window open it stays."),
                        ));
                    }
                });
            });
            ui.checkbox(&mut app.settings.close_to_tray, tr("Closing the window keeps the app running in the tray"));
            ui.checkbox(&mut app.settings.notifications, tr("Show a notification after each match"));
        });

        section(ui, tr("Online"), |ui| online(app, ui));

        section(ui, tr("Updates"), |ui| updates(app, ui));

        ui.horizontal(|ui| {
            if ui.add(egui::Button::new(RichText::new(tr("Save")).strong()).fill(theme::CARD_HOVER)).clicked() {
                let mut settings = app.settings.clone();
                match app.settings_ui.commit(&mut settings) {
                    Ok(()) => {
                        app.settings = settings;
                        app.settings_ui.error = None;
                        app.settings_ui.archive_usage = None;
                        // An empty or relative folder was replaced with the default.
                        app.settings_ui.archive_dir = app.settings.archive.dir.display().to_string();
                        app.apply_settings(true);
                        app.activity.push_log(tr("Settings saved").into());
                    }
                    Err(message) => app.settings_ui.error = Some(message),
                }
            }
            if ui.button(tr("Discard changes")).clicked() {
                app.settings = crate::config::Settings::load();
                app.settings_ui = SettingsUi::from_settings(&app.settings);
            }
        });
        if let Some(error) = &app.settings_ui.error {
            ui.label(RichText::new(error).color(theme::WARN));
        }

        ui.add_space(16.0);
        section(ui, tr("Tools"), |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button(tr("Scan replay folder now")).clicked() {
                    app.worker.send(Command::Scan { force: false });
                }
                if ui.button(tr("Re-analyze all matches")).on_hover_text(tr("Uses the replay folder and the archive.")).clicked() {
                    let files: Vec<String> = app.store.matches.iter().map(|m| m.file_name.clone()).collect();
                    app.activity.push_log(trf("Re-analyzing {0} matches", &[&files.len()]));
                    app.worker.send(Command::Reanalyze(files));
                }
                if cfg!(windows) && ui.button(tr("Open data folder")).clicked() {
                    let _ = std::process::Command::new("explorer").arg(data_dir()).spawn();
                }
            });
        });

        ui.add_space(8.0);
        ui.collapsing(tr("Advanced: broken chest detection"), |ui| {
            ui.label(theme::caption(
                tr("When a chest is destroyed the server records who did it, so breaks are exact. The loot-burst guess below \
                 is only for older replays; it over-counts on current ones. Re-analyze after changing anything here."),
            ));
            let b = &mut app.settings.breaks;
            ui.checkbox(&mut b.use_loot_heuristic, tr("Also guess breaks from loot bursts"));
            ui.add_enabled_ui(b.use_loot_heuristic, |ui| {
                egui::Grid::new("breaks").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
                    let row = |ui: &mut egui::Ui,
                               label: &str,
                               value: &mut f64,
                               range: std::ops::RangeInclusive<f64>,
                               suffix: &str| {
                        ui.label(label);
                        ui.add(egui::DragValue::new(value).range(range).speed(0.05).suffix(suffix));
                        ui.end_row();
                    };
                    row(ui, tr("Max distance on foot"), &mut b.max_distance_on_foot_m, 1.0..=20.0, " m");
                    row(ui, tr("Max distance in a vehicle"), &mut b.max_distance_in_vehicle_m, 1.0..=30.0, " m");
                    row(ui, tr("Despawn delay, min"), &mut b.despawn_delay_min_s, 0.5..=10.0, " s");
                    row(ui, tr("Despawn delay, max"), &mut b.despawn_delay_max_s, 0.5..=15.0, " s");
                    row(ui, tr("Despawn delay, typical"), &mut b.despawn_delay_typical_s, 0.5..=10.0, " s");
                    row(ui, tr("Health zero, max gap"), &mut b.health_zero_window_s, 0.0..=5.0, " s");
                    row(ui, tr("Loot burst radius"), &mut b.burst_radius_m, 0.5..=10.0, " m");
                    row(ui, tr("Open radius"), &mut b.open_radius_m, 1.0..=20.0, " m");
                });
            });
            if ui.button(tr("Reset to defaults")).clicked() {
                app.settings.breaks = chests::BreakConfig::default();
            }
        });
    });
}

fn stream(app: &mut ChestApp, ui: &mut egui::Ui) {
    let mut changed =
        ui.checkbox(&mut app.settings.stream.enabled, tr("Show your count in OBS or other streaming apps")).changed();
    ui.add_enabled_ui(app.settings.stream.enabled, |ui| {
        let link = crate::stream::link(app.settings.stream.port);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(&link).monospace().color(theme::GOLD));
            if ui.button(tr("Copy link")).clicked() {
                ui.ctx().copy_text(link.clone());
            }
            if ui.button(tr("Open in browser")).clicked() {
                if let Err(e) = crate::platform::open_url(&link) {
                    app.activity.push_log(trf("Could not open the browser: {0}", &[&e]));
                }
            }
            ui.label(tr("Port"));
            let port = ui.add(egui::DragValue::new(&mut app.settings_ui.stream_port).range(1024..=65535));
            // Moves the server once the number is set, not on every step of a drag.
            if !port.dragged() && !port.has_focus() && app.settings_ui.stream_port != app.settings.stream.port {
                app.settings.stream.port = app.settings_ui.stream_port;
                app.stream.retry();
                changed = true;
            }
        });
        if let Some(error) = &app.stream.error {
            ui.label(RichText::new(error).color(theme::WARN));
        } else if app.stream.running_port().is_some() {
            ui.label(theme::caption(tr("Running. The page picks up new numbers within two seconds.")));
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(tr("Show"));
            let show = &mut app.settings.stream.show;
            changed |= ui.checkbox(&mut show.last_match, tr("Last match")).changed();
            changed |= ui.checkbox(&mut show.today, tr("Today")).changed();
            changed |= ui.checkbox(&mut show.squad, tr("Squad")).changed();
            changed |= ui.checkbox(&mut show.treasures, tr("Treasures")).changed();
            changed |= ui.checkbox(&mut show.in_match, tr("IN MATCH badge")).changed();
        });
        ui.horizontal(|ui| {
            ui.label(tr("Background"));
            let slider = egui::Slider::new(&mut app.settings.stream.background, 0..=100).suffix("%");
            let r = ui.add(slider);
            // The page follows the slider live; the file is written once it is let go.
            changed |= r.drag_stopped() || (r.changed() && !r.dragged());
        });
    });
    if changed {
        let _ = app.settings.save();
    }
    ui.label(theme::caption(tr(
        "In OBS: Sources > + > Browser, paste the link, set the width to 400 and the height to 220. The background \
         is transparent. Only this PC can open the page, and nothing goes online.",
    )));
}

fn online(app: &mut ChestApp, ui: &mut egui::Ui) {
    if ui.checkbox(&mut app.settings.online, tr("Share my chest counts and get squadmates' exact counts")).changed() {
        let _ = app.settings.save();
    }
    let status = &app.online.status;
    if !app.settings.online {
        ui.label(theme::caption(tr("Off: nothing is sent, and squadmates count as far as your replays saw them.")));
    } else if let Some(error) = &status.error {
        ui.label(
            RichText::new(trf("Could not reach the server: {0}. Trying again every minute.", &[error]))
                .color(theme::WARN),
        );
    } else if let Some(at) = status.last_ok {
        let found = app.online.squadmates();
        ui.label(theme::caption(trf(
            "In sync at {0}. Squadmates with the app found so far: {1}.",
            &[&at.format("%H:%M"), &found],
        )));
    }
    ui.add_space(4.0);
    if ui.checkbox(&mut app.settings.leaderboard_name, tr("Show my name in the leaderboard")).changed() {
        let _ = app.settings.save();
        app.online.nudge();
    }
    ui.label(theme::caption(tr("Off: the leaderboard shows Anonymous instead of your name; your numbers stay in it.")));
    ui.add_space(4.0);
    if ui
        .checkbox(&mut app.settings.online_lines_all, tr("Running total: a line for every squadmate who runs the app"))
        .changed()
    {
        let _ = app.settings.save();
    }
    if !app.settings.online_lines_all {
        ui.label(theme::caption(tr("Lines only for these names, one per line:")));
        let edit = egui::TextEdit::multiline(&mut app.settings_ui.online_lines).desired_rows(3).desired_width(260.0);
        if ui.add(edit).changed() {
            app.settings.online_lines = app
                .settings_ui
                .online_lines
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_owned)
                .collect();
            let _ = app.settings.save();
        }
    }
    ui.label(theme::caption(trf(
        "Squadmates who run the app too get each other's own counts, so their chests count in full, not only the \
         ones near you. Sent to {0}: your Epic account id and name, and for each match its id, start time and your \
         opened and broken chests. Someone gets your numbers only for a match they played in your squad.",
        &[&crate::online::SERVER.trim_start_matches("https://")],
    )));
}

fn updates(app: &mut ChestApp, ui: &mut egui::Ui) {
    use crate::update::{version_text, Command};
    if ui.checkbox(&mut app.settings.check_updates, tr("Check for updates on GitHub every 10 minutes")).changed() {
        app.updater.send(Command::SetEnabled(app.settings.check_updates));
        let _ = app.settings.save();
    }
    ui.horizontal(|ui| {
        let busy = app.update.checking || app.update.installing.is_some();
        if ui.add_enabled(!busy, egui::Button::new(tr("Check for updates"))).clicked() {
            app.updater.send(Command::CheckNow);
            app.update.checking = true;
        }
        let current = version_text(crate::update::current());
        if ui.button(tr("What's new")).clicked() {
            app.whats_new = crate::changelog::recent();
        }
        if app.update.checking {
            ui.label(theme::caption(tr("Checking...")));
        } else if let Some(p) = app.update.installing {
            ui.label(theme::caption(trf("Updating... {0}%", &[&format!("{:.0}", p * 100.0)])));
        } else if let Some(release) = app.update.available.clone() {
            ui.label(
                RichText::new(trf("v{0} is available (you have v{1}).", &[&version_text(release.version), &current]))
                    .color(theme::GOLD),
            );
            if ui.button(RichText::new(tr("Update now")).strong()).clicked() {
                app.install_update();
            }
        } else if let Some(at) = app.update.last_checked {
            ui.label(theme::caption(trf(
                "You have the latest version, v{0}. Checked at {1}.",
                &[&current, &at.format("%H:%M")],
            )));
        }
    });
    if let Some(error) = &app.update.error {
        ui.label(RichText::new(error).color(theme::WARN));
    }
    ui.label(theme::caption(trf(
        "The app asks github.com/{0} for its latest release and nothing else. Nothing about you or your matches is sent. \
         An update is checked against its published checksum before it replaces this one.",
        &[&crate::update::REPO],
    )));
}

fn section(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title).size(16.0).strong().color(theme::GOLD));
        ui.add_space(4.0);
        add(ui);
    });
    ui.add_space(10.0);
}

#[cfg(test)]
mod adjustment_tests {
    use super::{parse_adjustment, signed};

    #[test]
    fn reads_signed_whole_numbers() {
        assert_eq!(parse_adjustment("+25"), Ok(25));
        assert_eq!(parse_adjustment(" -3 "), Ok(-3));
        assert_eq!(parse_adjustment("1,200"), Ok(1200));
        assert_eq!(parse_adjustment(""), Ok(0));
        assert!(parse_adjustment("lots").is_err());
        assert!(parse_adjustment("2.5").is_err());
        assert_eq!((signed(25), signed(0), signed(-3)), ("+25".to_string(), "0".to_string(), "-3".to_string()));
    }
}
