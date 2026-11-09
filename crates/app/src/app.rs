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
}

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
    pub logo: egui::TextureHandle,
    /// Frames left in which to keep the window hidden after a `--minimized` start.
    /// eframe shows the window after its first frame regardless of the builder.
    hide_frames: u8,
    quitting: bool,
    tray_status: (String, bool, bool),
}

pub fn run(start_minimized: bool) -> ExitCode {
    let size = 64;
    let icon = egui::IconData { rgba: icon::chest_rgba(size), width: size, height: size };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_NAME)
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([820.0, 560.0])
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

impl ChestApp {
    fn new(ctx: &egui::Context, start_minimized: bool) -> Self {
        let settings = Settings::load();
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
            logo: ctx.load_texture(
                "logo",
                egui::ColorImage::from_rgba_unmultiplied([128, 128], &icon::chest_rgba(128)),
                Default::default(),
            ),
            hide_frames,
            quitting: false,
            tray_status: Default::default(),
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

    fn update_button(&mut self, ui: &mut egui::Ui) {
        if let Some(p) = self.update.installing {
            ui.label(theme::caption("Updating..."));
            ui.add(egui::ProgressBar::new(p).desired_width(160.0).show_percentage());
        } else if let Some(release) = &self.update.available {
            let text = egui::RichText::new(format!("Update to v{}", crate::update::version_text(release.version)))
                .strong()
                .color(theme::BG);
            let button = egui::Button::new(text).fill(theme::GOLD);
            if ui.add_sized([160.0, 30.0], button).on_hover_text("Downloads it from GitHub and restarts").clicked() {
                self.install_update();
            }
        }
    }

    fn navigation(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("nav")
            .resizable(false)
            .exact_size(190.0)
            .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::same(14)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::Image::new(&self.logo).fit_to_exact_size(egui::vec2(26.0, 26.0)));
                    ui.label(egui::RichText::new("Chest Counter").size(17.0).strong().color(theme::GOLD));
                });
                ui.add_space(14.0);
                for page in Page::ALL {
                    let selected = self.page == page;
                    let text = egui::RichText::new(page.label()).size(15.0);
                    let text = if selected { text.color(theme::GOLD).strong() } else { text };
                    if ui.add_sized([160.0, 32.0], egui::Button::selectable(selected, text)).clicked() {
                        self.page = page;
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.label(theme::caption(format!("v{}", env!("CARGO_PKG_VERSION"))));
                    self.update_button(ui);
                    self.activity_badge(ui);
                });
            });
    }

    fn activity_badge(&self, ui: &mut egui::Ui) {
        if let Some((file, fraction, queued)) = &self.activity.progress {
            ui.add(egui::ProgressBar::new(*fraction).desired_width(160.0).show_percentage());
            let more = if *queued > 0 { format!(" (+{queued} queued)") } else { String::new() };
            ui.label(theme::caption(format!("Analyzing {}{more}", short_name(file))));
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
        self.handle_close(ctx);
        self.handle_dropped_files(ctx);
        crate::overlay::update(self, ctx);
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
