//! Application state and the per-frame plumbing between the UI, the worker and the tray.

use std::collections::VecDeque;
use std::process::ExitCode;
use std::sync::mpsc::{self, Receiver};

use chrono::NaiveDateTime;
use eframe::egui::{self, ViewportCommand};

use crate::config::Settings;
use crate::store::Store;
use crate::tray::{Tray, TrayAction};
use crate::worker::{Command, Event, Worker};
use crate::{icon, platform, stats, theme, APP_NAME};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Overview,
    Matches,
    Stats,
    Map,
    Squad,
    Settings,
}

impl Page {
    const ALL: [Page; 6] = [Page::Overview, Page::Matches, Page::Stats, Page::Map, Page::Squad, Page::Settings];

    fn label(self) -> &'static str {
        match self {
            Page::Overview => "Overview",
            Page::Matches => "Matches",
            Page::Stats => "Stats & odds",
            Page::Map => "Map",
            Page::Squad => "Squad replays",
            Page::Settings => "Settings",
        }
    }

    fn icon(self) -> char {
        use theme::icon;
        match self {
            Page::Overview => icon::HOME,
            Page::Matches => icon::LIST,
            Page::Stats => icon::CHART,
            Page::Map => icon::MAP_PIN,
            Page::Squad => icon::PEOPLE,
            Page::Settings => icon::SETTINGS,
        }
    }
}

/// Sidebar widths, open and folded to icons.
const NAV_WIDE: f32 = 172.0;
const NAV_NARROW: f32 = 54.0;

#[derive(Debug, Default)]
pub struct Activity {
    pub progress: Option<(String, f32, usize)>,
    pub live_matches: usize,
    pub last_scan: Option<NaiveDateTime>,
    pub log: VecDeque<String>,
}

impl Activity {
    pub fn push_log(&mut self, line: String) {
        let stamp = chrono::Local::now().format("%H:%M:%S");
        self.log.push_front(format!("{stamp}  {line}"));
        self.log.truncate(50);
    }
}

pub struct ChestApp {
    pub settings: Settings,
    pub store: Store,
    pub worker: Worker,
    events: Receiver<Event>,
    activations: Receiver<()>,
    tray: Option<Tray>,
    pub page: Page,
    pub activity: Activity,
    pub selected_match: Option<String>,
    pub totals: stats::Totals,
    pub setup: crate::ui::setup::SetupState,
    pub settings_ui: crate::ui::settings::SettingsUi,
    pub map_ui: crate::ui::map::MapUi,
    pub quest_import: crate::ui::quest_start::QuestImport,
    pub overlay: crate::overlay::OverlayState,
    pub updater: crate::update::Updater,
    pub update: crate::update::Status,
    pub online: crate::online::Sync,
    /// Whether the store currently holds squadmates' counts from the sync server.
    online_shown: bool,
    pub stream: crate::stream::Stream,
    pub logo: egui::TextureHandle,
    /// Frames left in which to keep the window hidden after a `--minimized` start.
    /// eframe shows the window after its first frame regardless of the builder.
    hide_frames: u8,
    quitting: bool,
    tray_status: (String, bool, bool),
    /// Matches Fortnite's log saw end.
    game_log: Receiver<crate::gamelog::Played>,
    /// Recorded matches whose replay file should be in the folder by the given time.
    replay_checks: Vec<(std::time::Instant, std::time::SystemTime)>,
    /// A warning was shown and nothing has changed since: not again for every match.
    warned_unrecorded: bool,
    warned_no_file: bool,
}

pub fn run(start_minimized: bool) -> ExitCode {
    let size = 64;
    let icon = egui::IconData { rgba: icon::chest_rgba(size), width: size, height: size };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_NAME)
            .with_inner_size([900.0, 600.0])
            .with_min_inner_size([640.0, 440.0])
            .with_icon(icon)
            .with_visible(!start_minimized),
        ..Default::default()
    };
    let result = eframe::run_native(
        APP_NAME,
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(ChestApp::new(&cc.egui_ctx, start_minimized)))
        }),
    );
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            platform::notify(APP_NAME, &format!("Could not start: {e}"));
            ExitCode::FAILURE
        }
    }
}

/// Registers the notification name and icon and, on the very first run, puts a shortcut
/// on the desktop. Off the UI thread: both touch the disk and the registry.
fn set_up_shell(first_run: bool) {
    let icon = crate::config::data_dir().join("icon.png");
    let registered = image::save_buffer(&icon, &icon::chest_rgba(128), 128, 128, image::ExtendedColorType::Rgba8)
        .map_err(std::io::Error::other)
        .and_then(|()| platform::register_notifications(&icon));
    if let Err(e) = registered {
        crate::config::log_event(&format!("notifications stay under PowerShell's name: {e}"));
    }
    if first_run {
        crate::config::log_event(&match platform::create_desktop_shortcut() {
            Ok(path) => format!("first run: desktop shortcut at {}", path.display()),
            Err(e) => format!("first run: no desktop shortcut: {e}"),
        });
    }
}

impl ChestApp {
    fn new(ctx: &egui::Context, start_minimized: bool) -> Self {
        let (mut settings, first_run) = Settings::load_first_run();
        std::thread::spawn(move || set_up_shell(first_run));
        if settings.install_id.is_empty() {
            settings.install_id = crate::online::new_install_id();
            let _ = settings.save();
        }
        if settings.setup_done && settings.start_with_windows {
            // Point autostart at this executable in case it was moved since setup.
            let _ = platform::set_autostart(true);
        }
        let store = Store::load(Store::default_path());
        let (tx, events) = mpsc::channel();
        let wake_ctx = ctx.clone();
        let worker = Worker::spawn(settings.clone(), store.known_files(), tx, move || wake_ctx.request_repaint());

        let (activate_tx, activations) = mpsc::channel();
        let activate_ctx = ctx.clone();
        platform::listen_for_activation(move || {
            let _ = activate_tx.send(());
            activate_ctx.request_repaint();
        });

        let update_ctx = ctx.clone();
        let updater = crate::update::Updater::start(settings.check_updates, move || update_ctx.request_repaint());
        let online_ctx = ctx.clone();
        let online =
            crate::online::Sync::start(crate::online::Sync::default_path(), move || online_ctx.request_repaint());

        let log_ctx = ctx.clone();
        let game_log = crate::gamelog::watch(&crate::gamelog::log_path(), move || log_ctx.request_repaint());

        let tray = crate::tray::create(ctx, settings.watch, settings.overlay.enabled);
        if start_minimized && tray.is_none() {
            ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        }

        let setup = crate::ui::setup::SetupState::from_settings(&settings);
        let settings_ui = crate::ui::settings::SettingsUi::from_settings(&settings);
        let hide_frames = if start_minimized && tray.is_some() { 5 } else { 0 };
        let mut app = Self {
            settings,
            store,
            worker,
            events,
            activations,
            tray,
            page: Page::Overview,
            activity: Activity::default(),
            selected_match: None,
            totals: stats::Totals::default(),
            setup,
            settings_ui,
            map_ui: Default::default(),
            quest_import: Default::default(),
            overlay: Default::default(),
            updater,
            update: Default::default(),
            online,
            online_shown: false,
            stream: Default::default(),
            logo: ctx.load_texture(
                "logo",
                egui::ColorImage::from_rgba_unmultiplied([128, 128], &icon::chest_rgba(128)),
                Default::default(),
            ),
            hide_frames,
            quitting: false,
            tray_status: Default::default(),
            game_log,
            replay_checks: Vec::new(),
            warned_unrecorded: false,
            warned_no_file: false,
        };
        app.refresh_totals();
        #[cfg(debug_assertions)]
        if let Ok(page) = std::env::var("FCC_DEBUG_PAGE") {
            app.page = Page::ALL.into_iter().find(|p| p.label().eq_ignore_ascii_case(&page)).unwrap_or(Page::Overview);
            app.selected_match =
                app.store.matches.iter().find(|m| m.summary(&app.settings.nick).found).map(|m| m.id.clone());
        }
        app
    }

    /// The configured name, or the recorder of the newest replay when it is automatic.
    pub fn counted_player(&self) -> String {
        if !self.settings.nick.trim().is_empty() {
            return self.settings.nick.clone();
        }
        self.store
            .matches
            .iter()
            .filter(|m| m.owner.is_some())
            .max_by_key(|m| m.started_at)
            .and_then(|m| m.owner.clone())
            .unwrap_or_else(|| "you".into())
    }

    pub fn refresh_totals(&mut self) {
        self.totals = stats::totals(&self.store.matches, &self.settings);
    }

    /// Persists settings and hands them to the worker. Optionally triggers a scan.
    pub fn apply_settings(&mut self, scan: bool) {
        if let Err(e) = self.settings.save() {
            self.activity.push_log(format!("Could not save settings: {e}"));
        }
        if let Err(e) = platform::set_autostart(self.settings.start_with_windows) {
            self.activity.push_log(format!("Could not update autostart: {e}"));
        }
        self.worker.send(Command::Settings(Box::new(self.settings.clone())));
        if scan {
            self.worker.send(Command::Scan { force: false });
        }
        self.refresh_totals();
    }

    pub fn save_store(&mut self) {
        if let Err(e) = self.store.save() {
            self.activity.push_log(format!("Could not save matches: {e}"));
        }
        self.refresh_totals();
    }

    fn process_events(&mut self, ctx: &egui::Context) {
        let mut changed = false;
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Progress { file, fraction, queued } => self.activity.progress = Some((file, fraction, queued)),
                Event::Analyzed { record, .. } if self.is_teammates(&record) => {
                    self.activity.push_log(format!(
                        "{}: recorded by {}, added to Squad replays",
                        record.file_name,
                        record.owner.as_deref().unwrap_or("a teammate")
                    ));
                    self.store.add_squad_replay(*record);
                    self.activity.progress = None;
                    changed = true;
                }
                Event::SquadAnalyzed { record } => {
                    self.activity.push_log(format!(
                        "{}: teammate replay by {}",
                        record.file_name,
                        record.owner.as_deref().unwrap_or("unknown")
                    ));
                    self.store.add_squad_replay(*record);
                    self.activity.progress = None;
                    changed = true;
                }
                Event::Analyzed { record, from_watch } => {
                    let summary = record.summary(&self.settings.nick);
                    self.activity.push_log(format!(
                        "{}: opened {}, broke {}, teammates {}",
                        record.file_name, summary.opened, summary.broken, summary.mates
                    ));
                    if record.packet_health < 0.97 {
                        self.activity.push_log(format!(
                            "{}: only {:.1}% of packets decoded; a Fortnite update may have changed the replay format",
                            record.file_name,
                            record.packet_health * 100.0
                        ));
                    }
                    let notify = from_watch && self.settings.notifications && self.settings.counts(record.started_at);
                    self.store.upsert(*record);
                    self.online.nudge();
                    self.activity.progress = None;
                    changed = true;
                    if notify {
                        self.refresh_totals();
                        let body = if summary.found {
                            format!(
                                "You: +{} (opened {}, broke {}) · squad +{}+{}\nTotal: {} · squad {}+",
                                summary.mine(),
                                summary.opened,
                                summary.broken,
                                summary.mates,
                                match summary.treasures {
                                    0 => String::new(),
                                    n => format!(" · treasure +{n}"),
                                },
                                theme::thousands(self.totals.mine()),
                                theme::thousands(self.totals.teammates as u64)
                            )
                        } else if self.settings.nick.trim().is_empty() {
                            "Could not tell who recorded this replay.".to_string()
                        } else {
                            format!("{} was not found in this match. Check your name in Settings.", self.settings.nick)
                        };
                        platform::notify("Match counted", &body);
                    }
                }
                Event::Failed { file, error } => {
                    self.activity.progress = None;
                    self.activity.push_log(format!("{file}: {error}"));
                }
                Event::Waiting { live } => self.activity.live_matches = live,
                Event::Idle { last_scan } => {
                    self.activity.progress = None;
                    self.activity.last_scan = Some(last_scan);
                }
                Event::Info(line) => self.activity.push_log(line),
            }
        }
        if changed {
            self.save_store();
            ctx.request_repaint();
        }
    }

    fn process_tray(&mut self, ctx: &egui::Context) {
        let mut actions: Vec<TrayAction> =
            std::iter::from_fn(|| self.activations.try_recv().ok()).map(|()| TrayAction::Open).collect();
        if let Some(tray) = &self.tray {
            actions.extend(std::iter::from_fn(|| tray.poll()));
        }
        for action in actions {
            match action {
                TrayAction::Open => {
                    ctx.send_viewport_cmd(ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(ViewportCommand::Focus);
                }
                TrayAction::ScanNow => self.worker.send(Command::Scan { force: false }),
                TrayAction::ToggleWatch => {
                    self.settings.watch = !self.settings.watch;
                    self.settings_ui = crate::ui::settings::SettingsUi::from_settings(&self.settings);
                    self.apply_settings(false);
                }
                TrayAction::ToggleOverlay => {
                    self.settings.overlay.enabled = !self.settings.overlay.enabled;
                    self.apply_settings(false);
                }
                TrayAction::Quit => {
                    crate::config::log_event("quit from the tray");
                    self.quitting = true;
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
            }
        }
        let status = format!(
            "{APP_NAME}\nChests: {} · squad {}+{}",
            theme::thousands(self.totals.mine()),
            theme::thousands(self.totals.teammates as u64),
            if self.settings.watch { "" } else { "\nWatching paused" }
        );
        let state = (status, self.settings.watch, self.settings.overlay.enabled);
        if let Some(tray) = &self.tray {
            if state != self.tray_status {
                tray.set_status(&state.0, state.1, state.2);
                self.tray_status = state;
            }
        }
    }

    fn handle_close(&mut self, ctx: &egui::Context) {
        let close_requested = ctx.input(|i| i.viewport().close_requested());
        if !close_requested {
            return;
        }
        let to_tray = !self.quitting && self.settings.close_to_tray && self.tray.is_some();
        crate::config::log_event(if to_tray { "window closed to the tray" } else { "window closed, exiting" });
        crate::overlay::lock(self);
        if to_tray {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let paths: Vec<_> = ctx.input(|i| {
            i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect()
        });
        if paths.is_empty() {
            return;
        }
        if self.page == Page::Squad {
            self.activity.push_log(format!("Analyzing {} teammate replay(s)", paths.len()));
            self.worker.send(Command::AnalyzeSquad(paths));
        } else {
            self.activity.push_log(format!("Analyzing {} dropped item(s)", paths.len()));
            self.worker.send(Command::Analyze(paths));
        }
    }

    /// The name the user plays under: the configured one, or whoever recorded most of
    /// their replays.
    fn usual_player(&self) -> Option<String> {
        if !self.settings.nick.trim().is_empty() {
            return Some(self.settings.nick.clone());
        }
        let mut counts: Vec<(&str, usize)> = Vec::new();
        for owner in self.store.matches.iter().filter_map(|m| m.owner.as_deref()) {
            match counts.iter_mut().find(|(name, _)| chests::same_name(name, owner)) {
                Some((_, n)) => *n += 1,
                None => counts.push((owner, 1)),
            }
        }
        counts.into_iter().max_by_key(|&(_, n)| n).map(|(name, _)| name.to_string())
    }

    /// A replay dropped among the user's own that a teammate recorded of a match the user
    /// already has: it belongs with the squad replays, not the user's matches.
    fn is_teammates(&self, record: &crate::record::MatchRecord) -> bool {
        let (Some(owner), Some(id)) = (record.owner.as_deref(), record.match_id.as_deref()) else { return false };
        let Some(me) = self.usual_player() else { return false };
        !chests::same_name(owner, &me)
            && self.store.matches.iter().any(|m| m.match_id.as_deref() == Some(id) && m.id != record.id)
    }

    /// Warns, once until things look right again, when Fortnite played a match without a
    /// replay, or recorded one that never showed up in the replay folder.
    fn process_game_log(&mut self, ctx: &egui::Context) {
        /// Fortnite finishes the replay when the player is back in the lobby.
        const FILE_WAIT: std::time::Duration = std::time::Duration::from_secs(60);
        while let Ok(played) = self.game_log.try_recv() {
            if !played.missed_replay() {
                // A recorded Battle Royale match: its replay should show up in the folder.
                if played.recorded && played.records_replays() && played.long_enough() {
                    self.warned_unrecorded = false;
                    if let Some(started) = played.started {
                        self.replay_checks.push((std::time::Instant::now() + FILE_WAIT, started.into()));
                        ctx.request_repaint_after(FILE_WAIT);
                    }
                }
                continue;
            }
            self.activity.push_log("Fortnite did not record the last match, so its chests cannot be counted".into());
            if self.settings.warn_unrecorded && !self.warned_unrecorded {
                self.warned_unrecorded = true;
                platform::notify(
                    "Your last match was not recorded",
                    "Fortnite saved no replay, so its chests can't be counted. In Fortnite turn on \
                     Settings > Game > Replays > Record Replays.",
                );
            }
        }

        let now = std::time::Instant::now();
        let (due, waiting): (Vec<_>, Vec<_>) = self.replay_checks.drain(..).partition(|(at, _)| *at <= now);
        self.replay_checks = waiting;
        for (_, started) in due {
            let since = started - std::time::Duration::from_secs(60);
            if crate::gamelog::replay_written_since(&self.settings.replay_dir, since) {
                self.warned_no_file = false;
                continue;
            }
            let dir = self.settings.replay_dir.display().to_string();
            self.activity.push_log(format!("Fortnite recorded the last match, but its replay is not in {dir}"));
            if self.settings.warn_unrecorded && !self.warned_no_file {
                self.warned_no_file = true;
                platform::notify(
                    "Replay not found",
                    &format!("Fortnite recorded your last match, but it isn't in {dir}. Check the replay folder in Settings > Replays."),
                );
            }
        }
    }

    /// Runs the online mode, and puts squadmates' counts into the matches or takes them out
    /// when it is switched off.
    fn process_online(&mut self, ctx: &egui::Context) {
        let enabled = self.settings.online && self.settings.setup_done;
        let changed = self.online.tick(enabled, &self.store.matches, &self.settings.install_id);
        if changed || enabled != self.online_shown {
            self.online_shown = enabled;
            self.store.set_online(if enabled { self.online.found() } else { Vec::new() });
            self.refresh_totals();
        }
        if enabled {
            ctx.request_repaint_after(crate::online::TICK);
        }
    }

    fn process_updates(&mut self, ctx: &egui::Context) {
        use crate::update::{Event as Update, AFTER_UPDATE_ARG};
        for event in self.updater.events() {
            match event {
                Update::Checking => self.update.checking = true,
                Update::Checked(result) => {
                    self.update.checking = false;
                    self.update.last_checked = Some(chrono::Local::now());
                    match result {
                        Ok(release) => {
                            self.update.available = release;
                            self.update.error = None;
                        }
                        Err(e) => self.update.error = Some(format!("Could not check for updates: {e}")),
                    }
                }
                Update::Progress(p) => self.update.installing = Some(p),
                Update::Installed(exe) => {
                    crate::config::log_event("update installed, restarting");
                    match std::process::Command::new(&exe).arg(AFTER_UPDATE_ARG).spawn() {
                        Ok(_) => {
                            self.quitting = true;
                            ctx.send_viewport_cmd(ViewportCommand::Close);
                        }
                        Err(e) => {
                            self.update.installing = None;
                            self.update.error =
                                Some(format!("The update is installed; start the app again to use it ({e})."));
                        }
                    }
                }
                Update::Failed(e) => {
                    self.update.installing = None;
                    self.update.error = Some(format!("Update failed: {e}"));
                }
            }
        }
    }

    pub fn install_update(&mut self) {
        if let (Some(release), None) = (self.update.available.clone(), self.update.installing) {
            self.update.installing = Some(0.0);
            self.update.error = None;
            self.updater.send(crate::update::Command::Install(release));
        }
    }

    fn update_button(&mut self, ui: &mut egui::Ui, narrow: bool) {
        let width = if narrow { NAV_NARROW - 16.0 } else { NAV_WIDE - 28.0 };
        if let Some(p) = self.update.installing {
            if !narrow {
                ui.label(theme::caption("Updating..."));
            }
            ui.add(egui::ProgressBar::new(p).desired_width(width).show_percentage());
        } else if let Some(release) = &self.update.available {
            let version = crate::update::version_text(release.version);
            let text = if narrow {
                theme::icon_text(theme::icon::DOWNLOAD, "Up", 16.0)
            } else {
                egui::RichText::new(format!("Update to v{version}")).strong()
            };
            let button = egui::Button::new(text.color(theme::BG)).fill(theme::GOLD);
            let hint = format!("Update to v{version}: downloads it from GitHub and restarts");
            if ui.add_sized([width, 30.0], button).on_hover_text(hint).clicked() {
                self.install_update();
            }
        }
    }

    fn navigation(&mut self, ui: &mut egui::Ui) {
        let narrow = self.settings.compact_nav;
        let width = if narrow { NAV_NARROW } else { NAV_WIDE };
        egui::Panel::left("nav")
            .resizable(false)
            .exact_size(width)
            .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::symmetric(8, 12)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let toggle = egui::Button::new(theme::icon_text(theme::icon::MENU, "=", 16.0)).frame(false);
                    let hint = if narrow { "Show the menu" } else { "Fold the menu to icons" };
                    if ui.add_sized([NAV_NARROW - 16.0, 28.0], toggle).on_hover_text(hint).clicked() {
                        self.settings.compact_nav = !narrow;
                        let _ = self.settings.save();
                    }
                    if !narrow {
                        ui.label(egui::RichText::new("Chest Counter").size(15.0).strong().color(theme::GOLD));
                    }
                });
                ui.add_space(10.0);
                for page in Page::ALL {
                    let selected = self.page == page;
                    let color = if selected { theme::GOLD } else { theme::TEXT };
                    let icon = theme::icon_text(page.icon(), &page.label()[..1], 16.0).color(color);
                    let response = if narrow {
                        ui.add_sized([NAV_NARROW - 16.0, 34.0], egui::Button::selectable(selected, icon))
                            .on_hover_text(page.label())
                    } else {
                        let mut job = egui::text::LayoutJob::default();
                        icon.append_to(&mut job, ui.style(), egui::FontSelection::Default, egui::Align::Center);
                        let label = egui::RichText::new(format!("   {}", page.label())).size(14.5).color(color);
                        let label = if selected { label.strong() } else { label };
                        label.append_to(&mut job, ui.style(), egui::FontSelection::Default, egui::Align::Center);
                        let button = egui::Button::selectable(selected, job);
                        ui.add_sized([NAV_WIDE - 16.0, 34.0], button)
                    };
                    if response.clicked() {
                        self.page = page;
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.label(theme::caption(format!("v{}", env!("CARGO_PKG_VERSION"))));
                    self.update_button(ui, narrow);
                    self.activity_badge(ui, narrow);
                });
            });
    }

    fn activity_badge(&self, ui: &mut egui::Ui, narrow: bool) {
        let width = if narrow { NAV_NARROW - 16.0 } else { NAV_WIDE - 28.0 };
        if let Some((file, fraction, queued)) = &self.activity.progress {
            let bar = ui.add(egui::ProgressBar::new(*fraction).desired_width(width));
            let more = if *queued > 0 { format!(" (+{queued} queued)") } else { String::new() };
            let text = format!("Analyzing {}{more}", short_name(file));
            if narrow {
                bar.on_hover_text(text);
            } else {
                ui.label(theme::caption(text));
            }
        } else if narrow {
            // A dot: green while a match is played, gold while watching, red when paused.
            let (color, hint) = if self.activity.live_matches > 0 {
                (theme::OPENED, "Match in progress")
            } else if self.settings.watch {
                (theme::GOLD, "Watching for new matches")
            } else {
                (theme::WARN, "Watching paused")
            };
            let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 14.0), egui::Sense::hover());
            ui.painter().circle_filled(rect.center(), 4.0, color);
            response.on_hover_text(hint);
        } else if self.activity.live_matches > 0 {
            ui.label(egui::RichText::new("Match in progress").color(theme::OPENED).size(12.5));
        } else if self.settings.watch {
            ui.label(theme::caption("Watching for new matches"));
        } else {
            ui.label(egui::RichText::new("Watching paused").color(theme::WARN).size(12.5));
        }
    }
}

pub fn short_name(file: &str) -> &str {
    file.strip_prefix("UnsavedReplay-").and_then(|s| s.strip_suffix(".replay")).unwrap_or(file)
}

impl eframe::App for ChestApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.hide_frames > 0 {
            self.hide_frames -= 1;
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
            ctx.request_repaint();
        }
        self.process_events(ctx);
        self.process_tray(ctx);
        self.process_updates(ctx);
        self.process_online(ctx);
        self.process_game_log(ctx);
        self.handle_close(ctx);
        self.handle_dropped_files(ctx);
        crate::overlay::update(self, ctx);
        crate::stream::update(self);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if !self.settings.setup_done {
            egui::CentralPanel::default().show(ui, |ui| crate::ui::setup::show(self, ui));
            return;
        }
        self.navigation(ui);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::same(20)))
            .show(ui, |ui| match self.page {
                Page::Overview => crate::ui::overview::show(self, ui),
                Page::Matches => crate::ui::matches::show(self, ui),
                Page::Stats => crate::ui::stats::show(self, ui),
                Page::Map => crate::ui::map::show(self, ui),
                Page::Squad => crate::ui::squad::show(self, ui),
                Page::Settings => crate::ui::settings::show(self, ui),
            });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        crate::config::log_event("exit");
        let _ = self.store.save();
    }
}
