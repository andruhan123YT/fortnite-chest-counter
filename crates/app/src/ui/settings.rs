//! Settings page: categories on the left, rows of switches and fields on the right. Every
//! change is saved as it is made; text fields once they are left.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Align, Layout, Margin, RichText, Stroke};

use crate::app::ChestApp;
use crate::config::{data_dir, parse_since, Settings, SINCE_FORMAT};
use crate::i18n::{tr, trf};
use crate::theme::{self, ColorTheme};
use crate::worker::Command;
use crate::{archive, platform};

/// The groups of settings in the page's own menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Category {
    #[default]
    General,
    Counting,
    Replays,
    Overlay,
    Streaming,
    Online,
    Advanced,
    About,
}

impl Category {
    const ALL: [Category; 8] = [
        Category::General,
        Category::Counting,
        Category::Replays,
        Category::Overlay,
        Category::Streaming,
        Category::Online,
        Category::Advanced,
        Category::About,
    ];

    fn label(self) -> &'static str {
        match self {
            Category::General => tr("General"),
            Category::Counting => tr("Counting"),
            Category::Replays => tr("Replays"),
            Category::Overlay => tr("Overlay"),
            Category::Streaming => tr("OBS and Discord"),
            Category::Online => tr("Online"),
            Category::Advanced => tr("Advanced"),
            Category::About => tr("About"),
        }
    }

    fn note(self) -> &'static str {
        match self {
            Category::General => tr("Language, look and how the app starts."),
            Category::Counting => tr("Which chests make up your total."),
            Category::Replays => tr("Where replays come from and the copies the app keeps."),
            Category::Overlay => tr("Your count on top of the game."),
            Category::Streaming => tr("Your count in OBS and in your Discord status."),
            Category::Online => tr("Squadmates' exact counts and the leaderboard."),
            Category::Advanced => tr("Rescans and how broken chests are found."),
            Category::About => tr("Version, updates, help and licenses."),
        }
    }

    /// Glyphs of Segoe MDL2 Assets.
    fn icon(self) -> char {
        match self {
            Category::General => '\u{E770}',
            Category::Counting => '\u{E8EF}',
            Category::Replays => '\u{E8B7}',
            Category::Overlay => '\u{E7F4}',
            Category::Streaming => '\u{E93E}',
            Category::Online => '\u{E774}',
            Category::Advanced => '\u{E90F}',
            Category::About => '\u{E946}',
        }
    }
}

/// Text buffers for fields that are edited as strings and validated when left.
#[derive(Debug, Clone, Default)]
pub struct SettingsUi {
    pub category: Category,
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
    /// A switch or choice changed and is not applied yet.
    pub toggled: bool,
}

impl SettingsUi {
    pub fn from_settings(s: &Settings) -> Self {
        Self {
            category: Category::default(),
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

/// Switches and choices take effect at once: the file, autostart and the worker all
/// follow. A drag is applied once it ends, not every frame. Text fields are applied when
/// no field has the keyboard.
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
    if ui.ctx().memory(|m| m.focused().is_none()) {
        commit_fields(app);
    }
}

/// Applies the text fields once they are left, if they changed.
fn commit_fields(app: &mut ChestApp) {
    let mut settings = app.settings.clone();
    match app.settings_ui.commit(&mut settings) {
        Ok(()) => {
            app.settings_ui.error = None;
            if settings != app.settings {
                let folder_moved = settings.replay_dir != app.settings.replay_dir;
                app.settings = settings;
                app.settings_ui.archive_usage = None;
                // An empty or relative folder was replaced with the default.
                app.settings_ui.archive_dir = app.settings.archive.dir.display().to_string();
                app.apply_settings(folder_moved);
            }
        }
        Err(message) => app.settings_ui.error = Some(message),
    }
}

const NAV_WIDTH: f32 = 176.0;

fn page(app: &mut ChestApp, ui: &mut egui::Ui) {
    egui::Panel::left("settings_nav")
        .resizable(false)
        .exact_size(NAV_WIDTH)
        .frame(egui::Frame::new().inner_margin(Margin { left: 0, right: 12, top: 0, bottom: 0 }))
        .show_separator_line(true)
        .show(ui, |ui| navigation(app, ui));
    egui::CentralPanel::default()
        .frame(egui::Frame::new().inner_margin(Margin { left: 18, ..Default::default() }))
        .show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt(app.settings_ui.category as u8).show(
                ui,
                |ui| {
                    let category = app.settings_ui.category;
                    ui.label(theme::title(category.label()));
                    ui.label(theme::caption(category.note()));
                    ui.add_space(10.0);
                    match category {
                        Category::General => general(app, ui),
                        Category::Counting => counting(app, ui),
                        Category::Replays => replays(app, ui),
                        Category::Overlay => overlay(app, ui),
                        Category::Streaming => {
                            stream(app, ui);
                            discord(app, ui);
                        }
                        Category::Online => online(app, ui),
                        Category::Advanced => advanced(app, ui),
                        Category::About => about(app, ui),
                    }
                    if category != Category::About {
                        ui.horizontal(|ui| {
                            ui.label(theme::icon_text('\u{E73E}', "", 12.0).color(theme::opened()));
                            ui.label(theme::caption(tr("Changes are saved as you make them.")));
                        });
                    }
                },
            );
        });
}

fn navigation(app: &mut ChestApp, ui: &mut egui::Ui) {
    ui.label(theme::title(tr("Settings")));
    ui.add_space(12.0);
    for category in Category::ALL {
        if category == Category::About {
            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);
        }
        let selected = app.settings_ui.category == category;
        let color = if selected { theme::gold() } else { theme::text() };
        let letter: String = category.label().chars().take(1).collect();
        let mut job = egui::text::LayoutJob::default();
        theme::icon_text(category.icon(), &letter, 14.0).color(color).append_to(
            &mut job,
            ui.style(),
            egui::FontSelection::Default,
            Align::Center,
        );
        let label = RichText::new(format!("   {}", category.label())).size(13.5).color(color);
        let label = if selected { label.strong() } else { label };
        label.append_to(&mut job, ui.style(), egui::FontSelection::Default, Align::Center);
        let button = egui::Button::selectable(selected, job).min_size(egui::vec2(ui.available_width(), 30.0));
        if ui.add(button).clicked() {
            app.settings_ui.category = category;
        }
    }
}

/// Rows inside one group card, with a hairline between them.
struct Rows<'a> {
    ui: &'a mut egui::Ui,
    count: usize,
}

impl Rows<'_> {
    fn divider(&mut self) {
        if self.count > 0 {
            let rect = self.ui.available_rect_before_wrap();
            self.ui.painter().hline(rect.x_range(), rect.top(), Stroke::new(1.0, theme::bg()));
            self.ui.add_space(1.0);
        }
        self.count += 1;
    }

    /// A name and an optional short note on the left, the control on the right.
    fn row<R>(
        &mut self,
        label: &str,
        note: Option<&str>,
        info: Option<&str>,
        control: impl FnOnce(&mut egui::Ui) -> R,
    ) -> R {
        self.divider();
        line(self.ui, 0.0, label, note, info, control)
    }

    /// A row that belongs to the switch above it: indented, and greyed out while that
    /// switch is off.
    fn sub<R>(
        &mut self,
        enabled: bool,
        label: &str,
        note: Option<&str>,
        info: Option<&str>,
        control: impl FnOnce(&mut egui::Ui) -> R,
    ) -> R {
        self.divider();
        self.ui.add_enabled_ui(enabled, |ui| line(ui, 26.0, label, note, info, control)).inner
    }

    /// A row that is greyed out while a switch elsewhere is off.
    fn dim<R>(
        &mut self,
        enabled: bool,
        label: &str,
        note: Option<&str>,
        info: Option<&str>,
        control: impl FnOnce(&mut egui::Ui) -> R,
    ) -> R {
        self.divider();
        self.ui.add_enabled_ui(enabled, |ui| line(ui, 0.0, label, note, info, control)).inner
    }

    /// A name and a note, with buttons under them rather than beside: for buttons too wide
    /// to share the line in every language.
    fn stacked(&mut self, label: &str, note: &str, buttons: impl FnOnce(&mut egui::Ui)) {
        self.full(|ui| {
            ui.label(RichText::new(label).size(13.5));
            ui.add(egui::Label::new(theme::caption(note)).wrap());
            ui.add_space(2.0);
            ui.horizontal_wrapped(buttons);
        });
    }

    /// Like `dim`, with the controls under the name: for controls too wide to share the line
    /// in every language.
    fn dim_stacked(&mut self, enabled: bool, label: &str, controls: impl FnOnce(&mut egui::Ui)) {
        self.divider();
        self.ui.add_space(6.0);
        self.ui.add_enabled_ui(enabled, |ui| {
            ui.label(RichText::new(label).size(13.5));
            ui.add_space(2.0);
            ui.horizontal_wrapped(controls);
        });
        self.ui.add_space(6.0);
    }

    /// A row with its own layout across the full width.
    fn full<R>(&mut self, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
        self.divider();
        self.ui.add_space(6.0);
        let r = self.ui.vertical(add).inner;
        self.ui.add_space(6.0);
        r
    }

    /// Like `full`, indented and greyed out with the switch above it.
    fn sub_full<R>(&mut self, enabled: bool, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
        self.divider();
        self.ui.add_space(6.0);
        let r = self
            .ui
            .add_enabled_ui(enabled, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(26.0);
                    ui.vertical(add).inner
                })
                .inner
            })
            .inner;
        self.ui.add_space(6.0);
        r
    }
}

fn line<R>(
    ui: &mut egui::Ui,
    indent: f32,
    label: &str,
    note: Option<&str>,
    info: Option<&str>,
    control: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let width = ui.available_width();
    ui.allocate_ui_with_layout(egui::vec2(width, 0.0), Layout::right_to_left(Align::Center), |ui| {
        ui.set_min_height(if note.is_some() { 44.0 } else { 36.0 });
        let out = control(ui);
        ui.add_space(14.0);
        ui.with_layout(Layout::top_down(Align::Min), |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add_space(indent);
                // Long names wrap rather than push the control out of the card; the info
                // icon keeps its room after them.
                let room = ui.available_width() - if info.is_some() { 24.0 } else { 0.0 };
                ui.scope(|ui| {
                    ui.set_max_width(room);
                    ui.add(egui::Label::new(RichText::new(label).size(13.5)).wrap());
                });
                if let Some(info) = info {
                    theme::info_icon(ui, info);
                }
            });
            if let Some(note) = note {
                ui.horizontal(|ui| {
                    ui.add_space(indent);
                    ui.add(egui::Label::new(theme::caption(note)).wrap());
                });
            }
            ui.add_space(4.0);
        });
        out
    })
    .inner
}

/// A titled card of rows.
fn group(ui: &mut egui::Ui, title: Option<&str>, add: impl FnOnce(&mut Rows)) {
    if let Some(title) = title {
        ui.label(RichText::new(title).size(12.5).strong().color(theme::muted()));
        ui.add_space(2.0);
    }
    egui::Frame::new()
        .fill(theme::card_color())
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(Margin::symmetric(14, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let mut rows = Rows { ui, count: 0 };
            add(&mut rows);
        });
    ui.add_space(14.0);
}

fn folder_field(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.label(RichText::new(label).size(13.5));
    ui.horizontal(|ui| {
        let browse = cfg!(windows);
        let width = ui.available_width() - if browse { 92.0 } else { 0.0 };
        ui.add(egui::TextEdit::singleline(value).desired_width(width.max(120.0)));
        if browse && ui.button(tr("Browse...")).clicked() {
            if let Some(folder) = pick_folder(value) {
                *value = folder;
            }
        }
    });
}

fn combo<T: Copy + PartialEq>(ui: &mut egui::Ui, id: &str, value: &mut T, options: &[(T, &str)]) {
    let current = options.iter().find(|(v, _)| v == value).map_or("", |(_, l)| *l);
    egui::ComboBox::from_id_salt(id).selected_text(current).show_ui(ui, |ui| {
        for (option, label) in options {
            ui.selectable_value(value, *option, *label);
        }
    });
}

fn general(app: &mut ChestApp, ui: &mut egui::Ui) {
    group(ui, Some(tr("Look")), |rows| {
        let picked = rows.row(tr("Language"), None, None, |ui| language_combo(ui, "language", app.settings.language));
        if let Some(lang) = picked {
            let ctx = rows.ui.ctx().clone();
            app.set_language(&ctx, lang);
        }
        rows.full(|ui| {
            ui.label(RichText::new(tr("Theme")).size(13.5));
            ui.add_space(4.0);
            if let Some(picked) = theme_cards(ui, app.settings.theme) {
                app.set_theme(ui.ctx(), picked);
            }
        });
    });
    group(ui, Some(tr("Windows")), |rows| {
        rows.row(tr("Start by itself, in the tray"), None, None, |ui| {
            theme::toggle(ui, &mut app.settings.start_with_windows)
        });
        let start = app.settings.start_with_windows;
        let note = app.settings.start_with_fortnite.then(|| tr("Leaves the tray a minute after Fortnite closes."));
        rows.sub(
            start,
            tr("When"),
            note,
            app.settings.start_with_fortnite.then(|| {
                tr("A minute after Fortnite closes, once the last match is counted, the app leaves the tray \
                 and waits for the next game. With its window open it stays.")
            }),
            |ui| {
                combo(
                    ui,
                    "autostart",
                    &mut app.settings.start_with_fortnite,
                    &[(true, tr("When Fortnite starts")), (false, tr("When Windows starts"))],
                )
            },
        );
        rows.row(tr("Closing the window keeps the app running in the tray"), None, None, |ui| {
            theme::toggle(ui, &mut app.settings.close_to_tray)
        });
        rows.row(tr("Show a notification after each match"), None, None, |ui| {
            theme::toggle(ui, &mut app.settings.notifications)
        });
    });
    group(ui, Some(tr("Treasure Hours")), |rows| {
        let th = &mut app.settings.treasure_hours;
        rows.row(tr("Remind me 15 minutes before one starts"), None, None, |ui| theme::toggle(ui, &mut th.remind));
        rows.row(
            tr("Show the next one on Overview"),
            Some(tr("Before and during one: when, and what it boosts.")),
            None,
            |ui| theme::toggle(ui, &mut th.banner),
        );
        rows.row(tr("Show TREASURE HOUR on the in-game overlay"), None, None, |ui| theme::toggle(ui, &mut th.overlay));
    });
}

/// A drop-down of every language, each in its own name; returns the one picked. Opening it
/// loads the fonts of every script, so all the names show.
pub fn language_combo(ui: &mut egui::Ui, id: &str, current: crate::i18n::Lang) -> Option<crate::i18n::Lang> {
    let mut picked = None;
    let combo =
        egui::ComboBox::from_id_salt(id).selected_text(current.name()).width(210.0).height(460.0).show_ui(ui, |ui| {
            for lang in crate::i18n::Lang::ALL {
                if ui.selectable_label(lang == current, lang.name()).clicked() && lang != current {
                    picked = Some(lang);
                }
            }
        });
    if combo.response.clicked() || combo.inner.is_some() {
        theme::load_every_script(ui.ctx());
    }
    picked
}

const THEME_CARD: egui::Vec2 = egui::vec2(132.0, 84.0);

/// A small picture of each theme; returns the one clicked.
fn theme_cards(ui: &mut egui::Ui, current: ColorTheme) -> Option<ColorTheme> {
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
        for t in ColorTheme::ALL {
            let p = t.palette();
            let (rect, response) = ui.allocate_exact_size(THEME_CARD, egui::Sense::click());
            let response = response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(t.note());
            let painter = ui.painter();
            let radius = egui::CornerRadius::same(8);
            painter.rect_filled(rect, radius, p.bg);
            let strip = egui::Rect::from_min_max(rect.min, egui::pos2(rect.left() + 26.0, rect.bottom()));
            painter.rect_filled(strip, egui::CornerRadius { nw: 8, sw: 8, ne: 0, se: 0 }, p.panel);
            for i in 0..3 {
                let y = rect.top() + 14.0 + i as f32 * 10.0;
                let color = if i == 0 { p.gold } else { p.muted.gamma_multiply(0.6) };
                painter.rect_filled(
                    egui::Rect::from_min_size(egui::pos2(rect.left() + 7.0, y), egui::vec2(12.0, 3.0)),
                    1.0,
                    color,
                );
            }
            let inner = egui::Rect::from_min_max(
                egui::pos2(strip.right() + 8.0, rect.top() + 9.0),
                egui::pos2(rect.right() - 8.0, rect.top() + 46.0),
            );
            painter.rect_filled(inner, 5.0, p.card);
            painter.text(
                inner.left_center() + egui::vec2(7.0, 0.0),
                egui::Align2::LEFT_CENTER,
                "5,027",
                theme::font(theme::BOLD, 17.0),
                p.gold,
            );
            painter.text(
                egui::pos2(strip.right() + 8.0, rect.bottom() - 10.0),
                egui::Align2::LEFT_CENTER,
                t.name(),
                theme::font(theme::SEMIBOLD, 12.5),
                p.text,
            );
            let (width, color) = if t == current {
                (2.0, theme::gold())
            } else if response.hovered() {
                (1.5, theme::muted())
            } else {
                (1.0, theme::card_hover())
            };
            painter.rect_stroke(rect, radius, Stroke::new(width, color), egui::StrokeKind::Outside);
            if response.clicked() {
                picked = Some(t);
            }
        }
    });
    picked
}

/// Every other account that recorded replays here, with the answer to "Is it yours?" and a
/// button to change it, so a wrong answer is not for good.
fn other_accounts(app: &mut ChestApp, ui: &mut egui::Ui) {
    let others: Vec<crate::stats::Account> =
        app.accounts.iter().filter(|a| a.id != app.settings.account_id).cloned().collect();
    if others.is_empty() {
        return;
    }
    let mut change = None;
    group(ui, Some(tr("Other accounts on this PC")), |rows| {
        rows.full(|ui| {
            ui.add(
                egui::Label::new(theme::caption(tr(
                    "Accounts that recorded replays on this PC. Yours get their own total and the app switches to \
                     them when they play; the others' matches are not counted or sent.",
                )))
                .wrap(),
            )
        });
        for account in &others {
            let mine = app.settings.profiles.contains_key(&account.id);
            let not_mine = app.settings.not_mine.contains(&account.id);
            let replays = crate::i18n::plural(account.matches as u64, "replay", "replays");
            let args: [&dyn std::fmt::Display; 2] = [&account.matches, &replays];
            let note = if mine {
                trf("{0} {1} on this PC · your account", &args)
            } else if not_mine {
                trf("{0} {1} on this PC · not yours", &args)
            } else {
                trf("{0} {1} on this PC · not answered yet", &args)
            };
            rows.row(&account.name, Some(&note), None, |ui| {
                if !mine && ui.button(tr("It is mine")).clicked() {
                    change = Some((account.clone(), true));
                }
                if !not_mine && ui.button(tr("Not mine")).clicked() {
                    change = Some((account.clone(), false));
                }
            });
        }
    });
    if let Some((account, mine)) = change {
        app.set_account_mine(&account.id, &account.name, mine);
    }
}

fn counting(app: &mut ChestApp, ui: &mut egui::Ui) {
    group(ui, Some(tr("Account")), |rows| {
        let account = if app.settings.nick.trim().is_empty() {
            tr("found from your first replay").to_string()
        } else {
            app.settings.nick.clone()
        };
        rows.row(
            tr("Account"),
            None,
            Some(tr(
                "Every replay records who played it. Your account is told apart by its Epic id, so a new name follows by itself.",
            )),
            |ui| ui.label(RichText::new(account).strong()),
        );
    });
    other_accounts(app, ui);
    group(ui, Some(tr("Your total")), |rows| {
        rows.row(
            tr("Base count"),
            Some(tr("What you had when counting started, e.g. 1000 for the 1000-chest quest.")),
            None,
            |ui| ui.add(egui::TextEdit::singleline(&mut app.settings_ui.base).desired_width(110.0)),
        );
        rows.row(
            tr("Added by hand"),
            Some(tr("Chests the replays missed. A minus takes chests off.")),
            Some(tr("For chests the replays missed, say from matches whose replays Fortnite deleted \
                 (it keeps only 100) before the app saw them.")),
            |ui| {
                let field = &mut app.settings_ui.adjustment;
                let current = parse_adjustment(field).unwrap_or(0);
                if ui.small_button(" + ").on_hover_text(tr("One chest more")).clicked() {
                    *field = signed(current + 1);
                }
                ui.add(egui::TextEdit::singleline(field).hint_text("0").desired_width(70.0));
                if ui.small_button(" - ").on_hover_text(tr("One chest fewer")).clicked() {
                    *field = signed(current - 1);
                }
            },
        );
        rows.row(
            tr("Count matches from"),
            Some(tr("Only matches started at or after this time are added. Empty counts every match.")),
            None,
            |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut app.settings_ui.since)
                        .hint_text("2026-11-07 22:39")
                        .desired_width(150.0),
                )
            },
        );
        if let Some(error) = &app.settings_ui.error {
            rows.full(|ui| ui.label(RichText::new(error).color(theme::warn())));
        }
        rows.full(|ui| {
            crate::ui::quest_start::show(ui, &mut app.quest_import, &mut app.settings_ui, &app.store.matches)
        });
    });
}

fn replays(app: &mut ChestApp, ui: &mut egui::Ui) {
    group(ui, Some(tr("Replays")), |rows| {
        rows.full(|ui| folder_field(ui, tr("Replay folder"), &mut app.settings_ui.replay_dir));
        if let Some(error) = &app.settings_ui.error {
            rows.full(|ui| ui.label(RichText::new(error).color(theme::warn())));
        }
        rows.row(tr("Watch the folder and count new matches automatically"), None, None, |ui| {
            theme::toggle(ui, &mut app.settings.watch)
        });
        rows.row(tr("Warn me when Fortnite plays a match without saving its replay"), None, None, |ui| {
            theme::toggle(ui, &mut app.settings.warn_unrecorded)
        });
    });
    group(ui, Some(tr("Archive")), |rows| {
        rows.row(
            tr("Keep a copy of every counted replay"),
            Some(tr("Fortnite keeps only the latest 100 replays on PC and deletes older ones.")),
            None,
            |ui| theme::toggle(ui, &mut app.settings.archive.enabled),
        );
        let on = app.settings.archive.enabled;
        rows.sub_full(on, |ui| folder_field(ui, tr("Archive folder"), &mut app.settings_ui.archive_dir));
        let usage =
            *app.settings_ui.archive_usage.get_or_insert_with(|| archive::usage_bytes(&app.settings.archive.dir));
        let note = trf(
            "Using {0} GB · about {1} matches fit. The oldest copies are deleted when the limit is reached.",
            &[
                &format!("{:.2}", usage as f64 / 1024f64.powi(3)),
                &((app.settings.archive.max_gb as f64 * 1024.0 / 26.0) as u64),
            ],
        );
        rows.sub(on, tr("Maximum size"), Some(&note), None, |ui| {
            ui.add(egui::Slider::new(&mut app.settings.archive.max_gb, 1.0..=200.0).logarithmic(true).suffix(" GB"))
        });
    });
}

fn overlay(app: &mut ChestApp, ui: &mut egui::Ui) {
    let editing = app.overlay.editing;
    let mut start_editing = false;
    let mut lock = false;
    {
        let o = &mut app.settings.overlay;
        group(ui, Some(tr("In game")), |rows| {
            rows.row(
                tr("Show your count on top of the game"),
                Some(tr("Needs Windowed Fullscreen in Fortnite.")),
                Some(tr(
                    "Set Fortnite to Windowed Fullscreen (Settings > Video > Window Mode): Windows cannot draw over exclusive fullscreen. \
                     The overlay ignores the mouse, so clicks go to the game; it only takes the mouse while you move it. \
                     The lock in its top right corner unlocks it for moving too, and locks it again; \
                     while it is unlocked, the cross next to it hides the overlay.",
                )),
                |ui| theme::toggle(ui, &mut o.enabled),
            );
            let on = o.enabled;
            rows.sub(on, tr("Only while Fortnite is running"), None, None, |ui| {
                theme::toggle(ui, &mut o.only_with_fortnite)
            });
            rows.sub(on, tr("Hide it while another window is in front of Fortnite"), None, None, |ui| {
                theme::toggle(ui, &mut o.only_over_fortnite)
            });
        });
        let on = o.enabled;
        group(ui, Some(tr("Look")), |rows| {
            let place = if o.position.is_some() { tr("Where you moved it") } else { o.corner.label() };
            rows.dim_stacked(on, tr("Position and size"), |ui| {
                egui::ComboBox::from_id_salt("overlay_corner").selected_text(place).show_ui(ui, |ui| {
                    for corner in crate::config::OverlayCorner::ALL {
                        let selected = o.position.is_none() && o.corner == corner;
                        if ui.selectable_label(selected, corner.label()).clicked() {
                            o.corner = corner;
                            o.position = None;
                        }
                    }
                });
                if editing {
                    lock = ui.button(RichText::new(tr("Lock")).strong().color(theme::gold())).clicked();
                } else {
                    start_editing = ui
                        .button(tr("Move and resize"))
                        .on_hover_text(tr(
                            "Shows the overlay so you can drag it anywhere and pull its corner to resize it",
                        ))
                        .clicked();
                }
            });
            if editing {
                rows.full(|ui| {
                    ui.label(
                        RichText::new(tr(
                            "Drag the overlay where you want it and pull its bottom right corner to resize it, then press Lock. \
                             It locks by itself when you leave this page.",
                        ))
                        .color(theme::gold()),
                    )
                });
            }
            rows.dim(on, tr("Opacity"), None, None, |ui| {
                ui.add(
                    egui::Slider::new(&mut o.opacity, 0.3..=1.0).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
                )
            });
            rows.dim(on, tr("Size"), None, None, |ui| {
                ui.add(
                    egui::Slider::new(&mut o.scale, crate::overlay::MIN_SCALE..=crate::overlay::MAX_SCALE)
                        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
                )
            });
        });
    }
    if start_editing {
        crate::overlay::start_editing(app);
    }
    if lock {
        crate::overlay::lock(app);
    }
}

fn stream(app: &mut ChestApp, ui: &mut egui::Ui) {
    let link = crate::stream::link(app.settings.stream.port);
    let mut retry = false;
    let mut open_error = None;
    let error = app.stream.error.clone();
    let running = app.stream.running_port().is_some();
    let s = &mut app.settings.stream;
    let ui_port = &mut app.settings_ui.stream_port;
    group(ui, Some(tr("Stream overlay (OBS)")), |rows| {
        rows.row(
            tr("Show your count in OBS or other streaming apps"),
            Some(tr("Add it to OBS as a browser source.")),
            Some(tr(
                "In OBS: Sources > + > Browser, paste the link, set the width to 400 and the height to 220. The background \
                 is transparent. Only this PC can open the page, and nothing goes online.",
            )),
            |ui| theme::toggle(ui, &mut s.enabled),
        );
        let on = s.enabled;
        let status = match (&error, running) {
            (Some(e), _) => Some(e.clone()),
            (None, true) => Some(tr("Running. The page picks up new numbers within two seconds.").to_string()),
            _ => None,
        };
        rows.sub_full(on, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&link).monospace().color(theme::gold()));
                if ui.button(tr("Copy link")).clicked() {
                    ui.ctx().copy_text(link.clone());
                }
                if ui.button(tr("Open in browser")).clicked() {
                    if let Err(e) = platform::open_url(&link) {
                        open_error = Some(e);
                    }
                }
            });
            if let Some(status) = &status {
                let color = if error.is_some() { theme::warn() } else { theme::muted() };
                ui.label(RichText::new(status).size(12.5).color(color));
            }
        });
        rows.sub(on, tr("Port"), None, None, |ui| {
            let port = ui.add(egui::DragValue::new(ui_port).range(1024..=65535));
            // Moves the server once the number is set, not on every step of a drag.
            if !port.dragged() && !port.has_focus() && *ui_port != s.port {
                s.port = *ui_port;
                retry = true;
            }
        });
        rows.sub_full(on, |ui| {
            ui.label(RichText::new(tr("Show")).size(13.5));
            ui.horizontal_wrapped(|ui| {
                let show = &mut s.show;
                ui.checkbox(&mut show.last_match, tr("Last match"));
                ui.checkbox(&mut show.today, tr("Today"));
                ui.checkbox(&mut show.squad, tr("Squad"));
                ui.checkbox(&mut show.treasures, tr("Treasures"));
                ui.checkbox(&mut show.in_match, tr("IN MATCH badge"));
            });
        });
        rows.sub(on, tr("Background"), None, None, |ui| {
            ui.add(egui::Slider::new(&mut s.background, 0..=100).suffix("%"));
        });
    });
    if retry {
        app.stream.retry();
    }
    if let Some(e) = open_error {
        app.activity.push_log(trf("Could not open the browser: {0}", &[&e]));
    }
}

fn discord(app: &mut ChestApp, ui: &mut egui::Ui) {
    let status = match app.discord.status() {
        crate::discord::Status::Off => None,
        crate::discord::Status::Idle => Some((tr("Shows up once Fortnite starts.").to_string(), theme::muted())),
        crate::discord::Status::NoDiscord => Some((
            tr("The Discord app is not running on this PC. The status shows up once it is.").to_string(),
            theme::muted(),
        )),
        crate::discord::Status::Shown => Some((tr("Shown in Discord.").to_string(), theme::opened())),
        crate::discord::Status::Error(e) => Some((trf("Discord did not take the status: {0}", &[&e]), theme::warn())),
    };
    let s = &mut app.settings.discord;
    group(ui, Some("Discord"), |rows| {
        rows.row(
            tr("Show my chests in my Discord status"),
            None,
            Some(tr(
                "Only the Discord app on this PC gets these numbers, the same way games show what you play. Friends see \
                 them on your profile. The app never signs in to Discord and cannot see your messages.",
            )),
            |ui| theme::toggle(ui, &mut s.enabled),
        );
        if let Some((text, color)) = &status {
            rows.full(|ui| ui.label(RichText::new(text).size(12.5).color(*color)));
        }
        let on = s.enabled;
        rows.sub(on, tr("Total chests"), None, None, |ui| theme::toggle(ui, &mut s.total));
        rows.sub(on, tr("Today and last match"), None, None, |ui| theme::toggle(ui, &mut s.today));
        rows.sub(on, tr("Match timer"), None, None, |ui| theme::toggle(ui, &mut s.timer));
        rows.sub(on, tr("Treasures and Pirate Shop chance, on the picture's tooltip"), None, None, |ui| {
            theme::toggle(ui, &mut s.odds)
        });
        rows.sub(on, tr("A \"Get Chest Counter\" button for friends"), None, None, |ui| {
            theme::toggle(ui, &mut s.button)
        });
        rows.sub(on, tr("When"), None, None, |ui| {
            combo(
                ui,
                "discord_when",
                &mut s.only_with_fortnite,
                &[(true, tr("Only while Fortnite is running")), (false, tr("Whenever the app is running"))],
            )
        });
        rows.sub(on, tr("Under your name in the member list"), None, None, |ui| {
            combo(
                ui,
                "discord_list",
                &mut s.count_in_list,
                &[(true, tr("Your chest count")), (false, tr("The app's name"))],
            )
        });
    });
}

fn online(app: &mut ChestApp, ui: &mut egui::Ui) {
    let status = &app.online.status;
    let note = if !app.settings.online {
        Some((
            tr("Off: nothing is sent, and squadmates count as far as your replays saw them.").to_string(),
            theme::muted(),
        ))
    } else if let Some(error) = &status.error {
        Some((trf("Could not reach the server: {0}. Trying again every minute.", &[error]), theme::warn()))
    } else {
        status.last_ok.map(|at| {
            (
                trf(
                    "In sync at {0}. Squadmates with the app found so far: {1}.",
                    &[&at.format("%H:%M"), &app.online.squadmates()],
                ),
                theme::muted(),
            )
        })
    };
    let privacy = trf(
        "Squadmates who run the app too get each other's own chests, so their chests count in full and show on \
         your map, not only the ones near you. Sent to {0}: your Epic account id and name, and for each match its \
         id, start time and your opened and broken chests: when, what kind and where on the island. Someone gets \
         them only for a match they played in your squad.",
        &[&crate::online::SERVER.trim_start_matches("https://")],
    );
    let mut name_changed = false;
    group(ui, Some(tr("Online")), |rows| {
        rows.row(tr("Share my chests and get squadmates' exact counts"), None, Some(&privacy), |ui| {
            theme::toggle(ui, &mut app.settings.online)
        });
        if let Some((text, color)) = &note {
            rows.full(|ui| ui.label(RichText::new(text).size(12.5).color(*color)));
        }
        rows.row(
            tr("Show my name in the leaderboard"),
            Some(tr("Off: the leaderboard shows Anonymous instead of your name; your numbers stay in it.")),
            None,
            |ui| name_changed = theme::toggle(ui, &mut app.settings.leaderboard_name).changed(),
        );
    });
    group(ui, Some(tr("Running total")), |rows| {
        rows.row(tr("A line for every squadmate who runs the app"), None, None, |ui| {
            theme::toggle(ui, &mut app.settings.online_lines_all)
        });
        let all = app.settings.online_lines_all;
        rows.sub_full(!all, |ui| {
            ui.label(theme::caption(tr("Lines only for these names, one per line:")));
            let edit =
                egui::TextEdit::multiline(&mut app.settings_ui.online_lines).desired_rows(3).desired_width(260.0);
            if ui.add(edit).changed() {
                app.settings.online_lines = app
                    .settings_ui
                    .online_lines
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(str::to_owned)
                    .collect();
            }
        });
    });
    if name_changed {
        app.online.nudge();
    }
}

fn advanced(app: &mut ChestApp, ui: &mut egui::Ui) {
    group(ui, Some(tr("Tools")), |rows| {
        rows.row(tr("Look for new replays"), None, None, |ui| {
            if ui.button(tr("Scan replay folder now")).clicked() {
                app.worker.send(Command::Scan { force: false });
            }
        });
        rows.row(tr("Count every match again"), Some(tr("Uses the replay folder and the archive.")), None, |ui| {
            if ui.button(tr("Re-analyze all matches")).clicked() {
                let files: Vec<String> = app.store.matches.iter().map(|m| m.file_name.clone()).collect();
                app.activity.push_log(trf("Re-analyzing {0} matches", &[&files.len()]));
                app.worker.send(Command::Reanalyze(files));
            }
        });
    });
    let b = &mut app.settings.breaks;
    group(ui, Some(tr("Broken chest detection")), |rows| {
        rows.row(
            tr("Also guess breaks from loot bursts"),
            Some(tr("Only for older replays: it over-counts on current ones.")),
            Some(tr(
                "When a chest is destroyed the server records who did it, so breaks are exact. The loot-burst guess below \
                 is only for older replays; it over-counts on current ones. Re-analyze after changing anything here.",
            )),
            |ui| theme::toggle(ui, &mut b.use_loot_heuristic),
        );
        let on = b.use_loot_heuristic;
        let value =
            |rows: &mut Rows, label: &str, value: &mut f64, range: std::ops::RangeInclusive<f64>, suffix: &str| {
                rows.sub(on, label, None, None, |ui| {
                    ui.add(egui::DragValue::new(value).range(range).speed(0.05).suffix(suffix));
                });
            };
        value(rows, tr("Max distance on foot"), &mut b.max_distance_on_foot_m, 1.0..=20.0, " m");
        value(rows, tr("Max distance in a vehicle"), &mut b.max_distance_in_vehicle_m, 1.0..=30.0, " m");
        value(rows, tr("Despawn delay, min"), &mut b.despawn_delay_min_s, 0.5..=10.0, " s");
        value(rows, tr("Despawn delay, max"), &mut b.despawn_delay_max_s, 0.5..=15.0, " s");
        value(rows, tr("Despawn delay, typical"), &mut b.despawn_delay_typical_s, 0.5..=10.0, " s");
        value(rows, tr("Health zero, max gap"), &mut b.health_zero_window_s, 0.0..=5.0, " s");
        value(rows, tr("Loot burst radius"), &mut b.burst_radius_m, 0.5..=10.0, " m");
        value(rows, tr("Open radius"), &mut b.open_radius_m, 1.0..=20.0, " m");
        rows.sub(on, tr("Back to the defaults"), None, None, |ui| {
            if ui.button(tr("Reset to defaults")).clicked() {
                *b = chests::BreakConfig::default();
            }
        });
    });
}

fn about(app: &mut ChestApp, ui: &mut egui::Ui) {
    let repo = format!("https://github.com/{}", crate::update::REPO);
    let mut open = None;
    group(ui, None, |rows| {
        rows.full(|ui| {
            ui.horizontal(|ui| {
                ui.add(egui::Image::new(&app.logo).fit_to_exact_size(egui::vec2(52.0, 52.0)));
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(format!("Fortnite Chest Counter v{}", env!("CARGO_PKG_VERSION")))
                            .size(17.0)
                            .strong(),
                    );
                    ui.label(theme::caption(tr("Free and open source, under the Apache License 2.0.")));
                    ui.horizontal(|ui| {
                        if ui.button("GitHub").clicked() {
                            open = Some(repo.clone());
                        }
                        let star = theme::icon_text(theme::icon::STAR_FILLED, "*", 12.0).color(theme::gold());
                        let mut job = egui::text::LayoutJob::default();
                        star.append_to(&mut job, ui.style(), egui::FontSelection::Default, Align::Center);
                        RichText::new(format!("  {}", tr("Star on GitHub"))).append_to(
                            &mut job,
                            ui.style(),
                            egui::FontSelection::Default,
                            Align::Center,
                        );
                        if ui
                            .button(job)
                            .on_hover_text(tr("Enjoying the app? Star it on GitHub so more players find it."))
                            .clicked()
                        {
                            open = Some(repo.clone());
                        }
                    });
                });
            });
        });
    });
    updates(app, ui);
    group(ui, Some(tr("Replays")), |rows| {
        let newest = crate::stats::own(&app.store.matches, &app.settings).next();
        let (note, status) = match newest {
            Some(m) => {
                let when = m
                    .started_at
                    .map(|t| t.format(&crate::i18n::date_format(&t, "%d %b %H:%M")).to_string())
                    .unwrap_or_default();
                let note = trf("Fortnite {0} · {1}", &[&m.season.clone().unwrap_or_else(|| "?".into()), &when]);
                let status = if m.packet_health >= 0.995 {
                    (tr("Read in full").to_string(), theme::opened())
                } else {
                    (trf("Read {0}%", &[&format!("{:.0}", m.packet_health * 100.0)]), theme::warn())
                };
                (note, Some(status))
            }
            None => (tr("No replays read yet.").to_string(), None),
        };
        rows.row(
            tr("Last replay"),
            Some(&note),
            Some(tr("The app reads replays with its own parser. A Fortnite update can change their format: if a \
                 replay is not read in full, counts may be low until the app is updated.")),
            |ui| {
                if let Some((text, color)) = status {
                    ui.label(RichText::new(text).color(color));
                }
            },
        );
    });
    group(ui, Some(tr("Help")), |rows| {
        rows.stacked(tr("Something wrong?"), tr("Copy the app info and paste it into your report."), |ui| {
            if ui.button(tr("Report a problem")).clicked() {
                open = Some(format!("{repo}/issues/new"));
            }
            if ui
                .button(tr("Copy app info"))
                .on_hover_text(tr("Version, Windows, last replay and recent log lines"))
                .clicked()
            {
                ui.ctx().copy_text(app_info(app));
                app.activity.push_log(tr("App info copied").into());
            }
            if ui.button(tr("Open log")).clicked() {
                open = Some(data_dir().join("app.log").display().to_string());
            }
        });
    });
    group(ui, Some(tr("Your data")), |rows| {
        let note = if app.settings.online {
            tr("Matches and settings stay on this PC. Online mode sends your chests per match: how many, when, what \
                kind and where.")
        } else {
            tr("Matches and settings stay on this PC. Nothing about your matches goes online.")
        };
        rows.stacked(tr("Where your data is"), note, |ui| {
            if ui.button(tr("Privacy details")).clicked() {
                open = Some(format!("{repo}/blob/main/docs/guide.md#privacy"));
            }
            if ui.button(tr("Open data folder")).clicked() {
                open = Some(data_dir().display().to_string());
            }
        });
    });
    group(ui, Some(tr("Credits")), |rows| {
        let note = trf("Uses {0} open-source libraries. Map picture: fortnite.gg.", &[&crate::licenses::count()]);
        rows.row(tr("Made with open source"), Some(&note), None, |ui| {
            if ui.button(tr("Show licenses")).clicked() {
                app.licenses.open = true;
            }
        });
    });
    ui.label(theme::caption(tr(
        "Not affiliated with or endorsed by Epic Games. Fortnite is a trademark of Epic Games, Inc.",
    )));
    ui.add_space(8.0);
    if let Some(target) = open {
        if let Err(e) = platform::open_url(&target) {
            app.activity.push_log(trf("Could not open the browser: {0}", &[&e]));
        }
    }
}

fn updates(app: &mut ChestApp, ui: &mut egui::Ui) {
    use crate::update::{version_text, Command};
    let current = version_text(crate::update::current());
    let busy = app.update.checking || app.update.installing.is_some();
    let (note, color) = if app.update.checking {
        (tr("Checking...").to_string(), theme::muted())
    } else if let Some(p) = app.update.installing {
        (trf("Updating... {0}%", &[&format!("{:.0}", p * 100.0)]), theme::muted())
    } else if let Some(release) = &app.update.available {
        (trf("v{0} is available (you have v{1}).", &[&version_text(release.version), &current]), theme::gold())
    } else if let Some(error) = &app.update.error {
        (error.clone(), theme::warn())
    } else if let Some(at) = app.update.last_checked {
        (trf("You have the latest version, v{0}. Checked at {1}.", &[&current, &at.format("%H:%M")]), theme::muted())
    } else {
        (trf("You have v{0}.", &[&current]), theme::muted())
    };
    let available = app.update.available.is_some();
    let mut check = false;
    let mut install = false;
    let mut notes = false;
    let mut enabled_changed = false;
    group(ui, Some(tr("Updates")), |rows| {
        rows.full(|ui| {
            ui.horizontal_wrapped(|ui| {
                if available {
                    install = ui.button(RichText::new(tr("Update now")).strong().color(theme::gold())).clicked();
                }
                check = ui.add_enabled(!busy, egui::Button::new(tr("Check for updates"))).clicked();
                notes = ui.button(tr("What's new")).clicked();
            });
            ui.label(RichText::new(&note).size(12.5).color(color));
        });
        rows.row(
            tr("Check for updates on GitHub every 10 minutes"),
            None,
            Some(&trf(
                "The app asks github.com/{0} for its latest release and nothing else. Nothing about you or your matches is sent. \
                 An update is checked against its published checksum before it replaces this one.",
                &[&crate::update::REPO],
            )),
            |ui| enabled_changed = theme::toggle(ui, &mut app.settings.check_updates).changed(),
        );
        rows.sub(
            app.settings.check_updates,
            tr("Install updates automatically"),
            Some(tr("Only while Fortnite is closed.")),
            None,
            |ui| theme::toggle(ui, &mut app.settings.auto_update),
        );
    });
    if enabled_changed {
        app.updater.send(Command::SetEnabled(app.settings.check_updates));
    }
    if check {
        app.updater.send(Command::CheckNow);
        app.update.checking = true;
    }
    if install {
        app.install_update();
    }
    if notes {
        app.whats_new = crate::changelog::recent();
    }
}

/// What a bug report needs: versions, the last replay, settings that matter and the
/// newest lines of the logs.
fn app_info(app: &ChestApp) -> String {
    let mut out = format!(
        "Fortnite Chest Counter v{}\nWindows: {}\nAnalysis version: {}\n",
        env!("CARGO_PKG_VERSION"),
        windows_version(),
        crate::record::ANALYZER_VERSION,
    );
    if let Some(m) = crate::stats::own(&app.store.matches, &app.settings).next() {
        out += &format!(
            "Last replay: {} · read {:.1}% cleanly · {} chests\n",
            m.season.as_deref().unwrap_or("?"),
            m.packet_health * 100.0,
            m.summary(app.settings.name_in(m)).mine(),
        );
    }
    let s = &app.settings;
    out += &format!(
        "Matches: {} · language {:?} · theme {}\nOnline {} · Discord {} · overlay {} · OBS {} · watch {}\n",
        app.store.matches.len(),
        s.language,
        s.theme.name(),
        on_off(s.online),
        on_off(s.discord.enabled),
        on_off(s.overlay.enabled),
        on_off(s.stream.enabled),
        on_off(s.watch),
    );
    if let Ok(text) = std::fs::read_to_string(data_dir().join("app.log")) {
        let lines: Vec<&str> = text.lines().rev().take(15).collect();
        out += &format!("\n--- app.log, last {} lines ---\n", lines.len());
        for line in lines.into_iter().rev() {
            out += line;
            out.push('\n');
        }
    }
    if let Ok(text) = std::fs::read_to_string(data_dir().join("crash.log")) {
        let crashes = crash_summary(&text, 3);
        if !crashes.is_empty() {
            out += &format!("\n--- crash.log, last {} ---\n", crashes.len());
            out += &crashes.join("\n");
        }
    }
    out
}

/// The newest `count` crashes of a crash log, oldest first: when and where each happened,
/// its message and the app's own frames of the backtrace. The full backtrace is mostly the
/// standard library and stays in the file.
fn crash_summary(log: &str, count: usize) -> Vec<String> {
    let starts_entry = |line: &str| {
        let b = line.as_bytes();
        b.len() > 20 && b[4] == b'-' && b[7] == b'-' && b[13] == b':' && line.contains(" thread '")
    };
    let mut entries: Vec<String> = Vec::new();
    let mut in_trace = false;
    for line in log.lines() {
        if starts_entry(line) {
            entries.push(format!("{line}\n"));
            in_trace = false;
            continue;
        }
        let Some(entry) = entries.last_mut() else { continue };
        let trimmed = line.trim_start();
        let frame = trimmed.split_once(": ").filter(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
        if let Some((_, frame)) = frame {
            in_trace = true;
            if frame.starts_with("fortnite_chest_counter::") || frame.starts_with("chests::") {
                *entry += &format!("  {frame}\n");
            }
        } else if !in_trace && !trimmed.is_empty() {
            *entry += &format!("{line}\n");
        }
    }
    let skip = entries.len().saturating_sub(count);
    entries.split_off(skip)
}

#[cfg(test)]
mod tests {
    use super::crash_summary;

    #[test]
    fn crash_summary_keeps_the_message_and_own_frames() {
        let log = "2026-11-09 00:34:04 v1.0.0 thread 'main': panicked at src\\a.rs:1:2:\n\
                   first message\n\
                   \x20  0: std::backtrace_rs::backtrace::win64::trace\n\
                   \x20            at /rustc/x/library\\std\\src\\backtrace.rs:85\n\
                   \n\
                   2026-11-11 22:45:47 v1.1.2 thread 'main': panicked at egui\\context.rs:4714:17:\n\
                   egui backend is implemented incorrectly\n\
                   \x20  0: std::backtrace::Backtrace::create\n\
                   \x20  7: fortnite_chest_counter::overlay::show\n\
                   \x20            at crates\\app\\src\\overlay.rs:40\n\
                   \x20 28: main\n";
        let all = crash_summary(log, 3);
        assert_eq!(all.len(), 2);
        assert!(all[0].contains("first message"));
        assert!(!all[0].contains("win64"));
        assert!(all[1].starts_with("2026-11-11 22:45:47 v1.1.2"));
        assert!(all[1].contains("egui backend is implemented incorrectly"));
        assert!(all[1].contains("  fortnite_chest_counter::overlay::show\n"));
        assert!(!all[1].contains("main\n  "));
        assert_eq!(crash_summary(log, 1).len(), 1);
    }
}

fn on_off(on: bool) -> &'static str {
    if on {
        "on"
    } else {
        "off"
    }
}

/// "Microsoft Windows [Version 10.0.26300.1234]" as `ver` prints it.
fn windows_version() -> String {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new("cmd")
            .args(["/c", "ver"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "unknown".into())
    }
    #[cfg(not(windows))]
    {
        std::env::consts::OS.to_string()
    }
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
