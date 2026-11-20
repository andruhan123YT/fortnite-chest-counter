//! Application state and the per-frame plumbing between the UI, the worker and the tray.

use std::collections::VecDeque;
use std::process::ExitCode;
use std::sync::mpsc::{self, Receiver};

use chrono::NaiveDateTime;
use eframe::egui::{self, ViewportCommand};

use crate::config::Settings;
use crate::i18n::{tr, trf};
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
    Leaderboard,
    Settings,
}

impl Page {
    const ALL: [Page; 7] =
        [Page::Overview, Page::Matches, Page::Stats, Page::Map, Page::Squad, Page::Leaderboard, Page::Settings];

    fn label(self) -> &'static str {
        match self {
            Page::Overview => tr("Overview"),
            Page::Matches => tr("Matches"),
            Page::Stats => tr("Stats & odds"),
            Page::Map => tr("Map"),
            Page::Squad => tr("Squad replays"),
            Page::Leaderboard => tr("Leaderboard"),
            Page::Settings => tr("Settings"),
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
            Page::Leaderboard => icon::STAR,
            Page::Settings => icon::SETTINGS,
        }
    }
}

/// Sidebar widths, open and folded to icons.
/// The side menu's width when open: at least this, wider when a language's page names need it.
const NAV_WIDE: f32 = 172.0;
const NAV_WIDEST: f32 = 250.0;
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
    /// Epic accounts that recorded replays here, the main one first.
    pub accounts: Vec<stats::Account>,
    pub setup: crate::ui::setup::SetupState,
    pub settings_ui: crate::ui::settings::SettingsUi,
    /// Notes shown in the "What's new" dialog until it is closed.
    pub whats_new: Vec<crate::changelog::Entry>,
    pub leaderboard: crate::ui::leaderboard::LeaderboardUi,
    pub map_ui: crate::ui::map::MapUi,
    pub quest_import: crate::ui::quest_start::QuestImport,
    pub overlay: crate::overlay::OverlayState,
    pub updater: crate::update::Updater,
    pub update: crate::update::Status,
    pub online: crate::online::Sync,
    /// Whether the store currently holds squadmates' counts from the sync server.
    online_shown: bool,
    /// The Treasure Hour the player was last reminded of, by its start.
    reminded_hour: Option<chrono::DateTime<chrono::Utc>>,
    pub stream: crate::stream::Stream,
    pub discord: crate::discord::Discord,
    pub logo: egui::TextureHandle,
    /// Frames left in which to keep the window hidden after a `--minimized` start.
    /// eframe shows the window after its first frame regardless of the builder.
    hide_frames: u8,
    quitting: bool,
    tray_status: (String, bool, bool),
    /// Matches Fortnite's log saw end.
    game_log: Receiver<crate::gamelog::LogEvent>,
    /// When the game loaded the match the player is in now, from its log.
    pub match_joined: Option<chrono::DateTime<chrono::Utc>>,
    /// Recorded matches whose replay file should be in the folder by the given time.
    replay_checks: Vec<(std::time::Instant, std::time::SystemTime)>,
    /// A warning was shown and nothing has changed since: not again for every match.
    warned_unrecorded: bool,
    warned_no_file: bool,
    sleeper: crate::sleep::Sleeper,
    /// New accounts already announced by a notification this run.
    asked_accounts: std::collections::HashSet<String>,
    /// The theme the window's title bar was last painted in.
    title_bar: Option<theme::ColorTheme>,
    /// The open side menu's width, fitted to the page names in the current language.
    nav_wide: f32,
    pub licenses: crate::licenses::LicensesUi,
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
    if crate::config::on_test_data() {
        return;
    }
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
        if first_run {
            settings.language = crate::i18n::Lang::from_windows(platform::ui_language());
        }
        crate::i18n::set(settings.language);
        theme::load_fonts(ctx);
        theme::set(settings.theme);
        theme::apply_colors(ctx);
        std::thread::spawn(move || set_up_shell(first_run));
        if settings.install_id.is_empty() {
            settings.install_id = crate::online::new_install_id();
            let _ = settings.save();
        }
        if settings.setup_done && settings.start_with_windows {
            // Point autostart at this executable in case it was moved since setup.
            if let Err(e) = platform::set_autostart(true, settings.start_with_fortnite) {
                crate::config::log_event(&format!("could not update autostart: {e}"));
            }
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

        let whats_new = if settings.setup_done { crate::changelog::unseen(&settings.notes_seen) } else { Vec::new() };
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
            accounts: Vec::new(),
            setup,
            settings_ui,
            whats_new,
            leaderboard: Default::default(),
            map_ui: Default::default(),
            quest_import: Default::default(),
            overlay: Default::default(),
            updater,
            update: Default::default(),
            online,
            online_shown: false,
            reminded_hour: None,
            stream: Default::default(),
            discord: Default::default(),
            logo: ctx.load_texture(
                "logo",
                egui::ColorImage::from_rgba_unmultiplied([128, 128], &icon::chest_rgba(128)),
                Default::default(),
            ),
            hide_frames,
            quitting: false,
            tray_status: Default::default(),
            game_log,
            match_joined: None,
            replay_checks: Vec::new(),
            warned_unrecorded: false,
            warned_no_file: false,
            sleeper: Default::default(),
            asked_accounts: Default::default(),
            title_bar: None,
            nav_wide: NAV_WIDE,
            licenses: Default::default(),
        };
        app.refresh_totals();
        app.settle_account();
        #[cfg(debug_assertions)]
        if let Ok(page) = std::env::var("FCC_DEBUG_PAGE") {
            app.page = Page::ALL.into_iter().find(|p| p.label().eq_ignore_ascii_case(&page)).unwrap_or(Page::Overview);
            app.selected_match =
                app.store.matches.iter().find(|m| m.summary(app.settings.name_in(m)).found).map(|m| m.id.clone());
        }
        app
    }

    /// The configured name, or the recorder of the newest replay when it is automatic.
    /// None before any replay.
    pub fn counted_player(&self) -> Option<String> {
        if !self.settings.nick.trim().is_empty() {
            return Some(self.settings.nick.clone());
        }
        self.store
            .matches
            .iter()
            .filter(|m| m.owner.is_some())
            .max_by_key(|m| m.started_at)
            .and_then(|m| m.owner.clone())
    }

    pub fn refresh_totals(&mut self) {
        self.totals = stats::totals(&self.store.matches, &self.settings);
        self.accounts = stats::accounts(&self.store.matches);
    }

    /// Learns which Epic account the player is, from the name they gave or, when it is
    /// automatic, from most of the replays; then follows that account's and the other
    /// profiles' renames.
    fn settle_account(&mut self) {
        let mut changed = false;
        if self.settings.account_id.is_empty() {
            let nick = self.settings.nick.trim();
            let id = if nick.is_empty() {
                self.accounts.first().map(|a| a.id.clone())
            } else {
                stats::id_of(&self.store.matches, nick)
            };
            if let Some(id) = id {
                crate::config::log_event(&format!("counting for account {id}"));
                self.settings.account_id = id;
                changed = true;
            }
        }
        let name_of = |id: &str| self.accounts.iter().find(|a| a.id == id).map(|a| a.name.clone());
        if let Some(name) = name_of(&self.settings.account_id).filter(|n| *n != self.settings.nick) {
            crate::config::log_event(&format!("{} is now called {name}", self.settings.nick));
            self.settings.nick = name;
            changed = true;
        }
        let renamed: Vec<(String, String)> = self
            .settings
            .profiles
            .iter()
            .filter_map(|(id, p)| Some((id.clone(), name_of(id).filter(|n| *n != p.name)?)))
            .collect();
        for (id, name) in renamed {
            if let Some(p) = self.settings.profiles.get_mut(&id) {
                p.name = name;
                changed = true;
            }
        }
        if changed {
            self.settings_ui = crate::ui::settings::SettingsUi::from_settings(&self.settings);
            self.apply_settings(false);
        }
    }

    /// Shows and counts the account with Epic id `id`, with its own base, start date and
    /// prizes.
    pub fn switch_account(&mut self, id: &str) {
        if id == self.settings.account_id {
            return;
        }
        let name = self
            .accounts
            .iter()
            .find(|a| a.id == id)
            .map(|a| a.name.clone())
            .or_else(|| self.settings.profiles.get(id).map(|p| p.name.clone()))
            .unwrap_or_default();
        self.settings.switch_account(id, &name);
        crate::config::log_event(&format!("counting for {}", self.settings.nick));
        self.settings_ui = crate::ui::settings::SettingsUi::from_settings(&self.settings);
        self.selected_match = None;
        self.apply_settings(false);
        self.online.nudge();
    }

    /// The first account seen in replays that the player has not said is theirs or not.
    fn unknown_account(&self) -> Option<&stats::Account> {
        stats::unknown_account(&self.accounts, &self.settings)
    }

    /// Asks whether a new account's replays are the player's own.
    fn ask_about_account(&mut self, ctx: &egui::Context) {
        let Some(account) = self.unknown_account().cloned() else { return };
        let mut answer = None;
        egui::Modal::new(egui::Id::unique("unknown-account")).show(ctx, |ui| {
            ui.set_max_width(420.0);
            ui.label(egui::RichText::new(trf("Is {0} your account?", &[&account.name])).heading().color(theme::gold()));
            ui.add_space(6.0);
            let replays = crate::i18n::plural(account.matches as u64, "replay", "replays");
            ui.add(
                egui::Label::new(trf(
                    "{0} recorded {1} {2} on this PC. If it is your second account, it gets its own total and \
                     the app switches to it when it plays. If not, its matches are not counted and not sent \
                     to the server.",
                    &[&account.name, &account.matches, &replays],
                ))
                .wrap(),
            );
            ui.add_space(10.0);
            let buttons =
                vec![egui::RichText::new(tr("Yes, it is mine")).strong(), egui::RichText::new(tr("No, not mine"))];
            answer = theme::centered_buttons(ui, buttons).map(|i| i == 0);
        });
        if let Some(mine) = answer {
            // A second account that played last is the one to show now.
            let played_last = mine && self.accounts.iter().all(|a| a.last <= account.last);
            self.set_account_mine(&account.id, &account.name, mine);
            if played_last {
                self.switch_account(&account.id);
            }
        }
    }

    /// Records whether the account `id` is one of the player's, from the question or from
    /// Settings > Counting, where an answer given by mistake can be changed. The active
    /// account is not touched.
    pub fn set_account_mine(&mut self, id: &str, name: &str, mine: bool) {
        if id.is_empty() || id == self.settings.account_id {
            return;
        }
        if mine {
            crate::config::log_event(&format!("{name} is the player's account too"));
            self.settings.mark_mine(id, name);
        } else {
            crate::config::log_event(&format!("{name} is not the player's account"));
            self.settings.mark_not_mine(id);
        }
        self.apply_settings(false);
    }

    /// Persists settings and hands them to the worker. Optionally triggers a scan.
    pub fn apply_settings(&mut self, scan: bool) {
        if let Err(e) = self.settings.save() {
            self.activity.push_log(trf("Could not save settings: {0}", &[&e]));
        }
        if let Err(e) = platform::set_autostart(self.settings.start_with_windows, self.settings.start_with_fortnite) {
            self.activity.push_log(trf("Could not update autostart: {0}", &[&e]));
        }
        self.worker.send(Command::Settings(Box::new(self.settings.clone())));
        if scan {
            self.worker.send(Command::Scan { force: false });
        }
        self.refresh_totals();
    }

    pub fn save_store(&mut self) {
        if let Err(e) = self.store.save() {
            self.activity.push_log(trf("Could not save matches: {0}", &[&e]));
        }
        self.refresh_totals();
    }

    fn process_events(&mut self, ctx: &egui::Context) {
        let mut changed = false;
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Progress { file, fraction, queued } => self.activity.progress = Some((file, fraction, queued)),
                // Only Battle Royale and Zero Build count. A replay from the player's folder
                // leaves the list and is remembered, so the scan skips it; a teammate's
                // replay added by hand is just not kept.
                Event::Analyzed { record, .. } if !record.counted_mode() => {
                    self.activity.push_log(self.other_mode_note(&record));
                    self.store.upsert(*record);
                    self.activity.progress = None;
                    changed = true;
                }
                Event::SquadAnalyzed { record } if !record.counted_mode() => {
                    self.activity.push_log(self.other_mode_note(&record));
                    self.activity.progress = None;
                }
                Event::Analyzed { record, .. } if self.is_teammates(&record) => {
                    self.activity.push_log(trf(
                        "{0}: recorded by {1}, added to Squad replays",
                        &[&record.file_name, &record.owner.as_deref().unwrap_or(tr("a teammate"))],
                    ));
                    self.store.add_squad_replay(*record);
                    self.activity.progress = None;
                    changed = true;
                }
                Event::SquadAnalyzed { record } => {
                    self.activity.push_log(trf(
                        "{0}: teammate replay by {1}",
                        &[&record.file_name, &record.owner.as_deref().unwrap_or(tr("unknown"))],
                    ));
                    self.store.add_squad_replay(*record);
                    self.activity.progress = None;
                    changed = true;
                }
                Event::Analyzed { record, from_watch } => {
                    if let Some(id) = record.recorder_id().filter(|_| from_watch).map(str::to_owned) {
                        if self.settings.profiles.contains_key(&id) {
                            // Another of the player's accounts just played: show and count that one.
                            self.switch_account(&id);
                        } else if !self.settings.account_id.is_empty()
                            && !self.settings.knows_account(&id)
                            && self.asked_accounts.insert(id)
                            && !platform::main_window_visible()
                        {
                            let name = record.owner.as_deref().unwrap_or("?");
                            platform::notify(
                                &trf("Is {0} your account?", &[&name]),
                                tr("Open Chest Counter to say whether this account is yours. Until then its matches are not counted."),
                            );
                        }
                    }
                    let summary = record.summary(self.settings.name_in(&record));
                    self.activity.push_log(trf(
                        "{0}: opened {1}, broke {2}, teammates {3}",
                        &[&record.file_name, &summary.opened, &summary.broken, &summary.mates],
                    ));
                    if record.packet_health < 0.97 {
                        self.activity.push_log(trf(
                            "{0}: only {1}% of packets decoded; a Fortnite update may have changed the replay format",
                            &[&record.file_name, &format!("{:.1}", record.packet_health * 100.0)],
                        ));
                    }
                    let notify = from_watch
                        && self.settings.notifications
                        && self.settings.counts(record.started_at)
                        && self.settings.owns(&record);
                    // A named prize goes into the notification by name.
                    let nick = self.settings.name_in(&record);
                    let named_prize = record
                        .treasures(nick)
                        .iter()
                        .any(|t| t.prize.is_some())
                        .then(|| record.prizes_note(nick))
                        .flatten();
                    self.store.upsert(*record);
                    self.online.nudge();
                    self.activity.progress = None;
                    changed = true;
                    if notify {
                        self.refresh_totals();
                        let body = if summary.found {
                            trf(
                                "You: +{0} (opened {1}, broke {2}) · squad +{3}+{4}\nTotal: {5} · squad {6}+",
                                &[
                                    &summary.mine(),
                                    &summary.opened,
                                    &summary.broken,
                                    &summary.mates,
                                    &match (summary.treasures, &named_prize) {
                                        (0, _) => String::new(),
                                        (_, Some(prize)) => trf(" · prize: {0}", &[prize]),
                                        (n, None) => trf(" · treasure +{0}", &[&n]),
                                    },
                                    &theme::thousands(self.totals.mine()),
                                    &theme::thousands(self.totals.teammates as u64),
                                ],
                            )
                        } else if self.settings.nick.trim().is_empty() {
                            tr("Could not tell who recorded this replay.").to_string()
                        } else {
                            trf("{0} was not found in this match.", &[&self.settings.nick])
                        };
                        platform::notify(tr("Match counted"), &body);
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
            self.settle_account();
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
        let status = trf(
            "{0}\nChests: {1} · squad {2}+{3}",
            &[
                &APP_NAME,
                &theme::thousands(self.totals.mine()),
                &theme::thousands(self.totals.teammates as u64),
                &if self.settings.watch { String::new() } else { format!("\n{}", tr("Watching paused")) },
            ],
        );
        let state = (status, self.settings.watch, self.settings.overlay.enabled);
        if let Some(tray) = &self.tray {
            if state != self.tray_status {
                tray.set_status(&state.0, state.1, state.2);
                self.tray_status = state;
            }
        }
    }

    /// Installs an update found by the background check, once Fortnite is closed and the
    /// window is in the tray. Each release gets one try; a failed one waits for the button.
    fn auto_update(&mut self, ctx: &egui::Context) {
        const EVERY: std::time::Duration = std::time::Duration::from_secs(20);
        let s = &self.settings;
        let Some(version) = self.update.available.as_ref().map(|r| r.version) else {
            return;
        };
        if !(s.setup_done && s.check_updates && s.auto_update)
            || self.update.installing.is_some()
            || self.update.auto_tried == Some(version)
            || self.quitting
        {
            return;
        }
        ctx.request_repaint_after(EVERY);
        let now = std::time::Instant::now();
        if self.update.auto_checked.is_some_and(|at| now.duration_since(at) < EVERY) {
            return;
        }
        self.update.auto_checked = Some(now);
        let counting = self.activity.progress.is_some();
        if !crate::update::may_install_alone(platform::fortnite_running(), platform::main_window_visible(), counting) {
            return;
        }
        crate::config::log_event(&format!("installing v{} by itself", crate::update::version_text(version)));
        self.update.auto_tried = Some(version);
        self.install_update();
        self.update.auto = true;
    }

    /// In "start when Fortnite starts" mode, hands over to a copy that waits for the next
    /// game once this one is over and counted.
    fn process_sleep(&mut self, ctx: &egui::Context) {
        use crate::sleep::CHECK_EVERY;
        let s = &self.settings;
        if !(s.setup_done && s.start_with_windows && s.start_with_fortnite && self.tray.is_some()) || self.quitting {
            return;
        }
        ctx.request_repaint_after(CHECK_EVERY);
        let now = std::time::Instant::now();
        if self.sleeper.checked_at.is_some_and(|at| now.duration_since(at) < CHECK_EVERY) {
            return;
        }
        self.sleeper.checked_at = Some(now);
        let counting = self.activity.progress.is_some();
        if !self.sleeper.observe(platform::fortnite_running(), now, counting, platform::main_window_visible()) {
            return;
        }
        let spawned = std::env::current_exe().and_then(|exe| {
            std::process::Command::new(exe).args([platform::WAIT_FOR_FORTNITE_ARG, platform::AFTER_SLEEP_ARG]).spawn()
        });
        match spawned {
            Ok(_) => {
                crate::config::log_event("Fortnite closed, going back to waiting for it");
                self.quitting = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            Err(e) => {
                crate::config::log_event(&format!("could not start the waiting copy, staying: {e}"));
                self.sleeper = crate::sleep::Sleeper::default();
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
            self.activity.push_log(trf("Analyzing {0} teammate replay(s)", &[&paths.len()]));
            self.worker.send(Command::AnalyzeSquad(paths));
        } else {
            self.activity.push_log(trf("Analyzing {0} dropped item(s)", &[&paths.len()]));
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
    fn other_mode_note(&self, record: &crate::record::MatchRecord) -> String {
        trf(
            "{0}: {1} is not Battle Royale or Zero Build, not counted",
            &[&record.file_name, &record.playlist.as_deref().unwrap_or("?")],
        )
    }

    fn is_teammates(&self, record: &crate::record::MatchRecord) -> bool {
        let (Some(owner), Some(id)) = (record.owner.as_deref(), record.match_id.as_deref()) else { return false };
        if record.recorder_id().is_some_and(|r| self.settings.my_accounts().any(|a| a == r)) {
            return false;
        }
        let Some(me) = self.usual_player() else { return false };
        !chests::same_name(owner, &me)
            && self.store.matches.iter().any(|m| m.match_id.as_deref() == Some(id) && m.id != record.id)
    }

    /// Warns, once until things look right again, when Fortnite played a match without a
    /// replay, or recorded one that never showed up in the replay folder.
    fn process_game_log(&mut self, ctx: &egui::Context) {
        /// Fortnite finishes the replay when the player is back in the lobby.
        const FILE_WAIT: std::time::Duration = std::time::Duration::from_secs(60);
        while let Ok(event) = self.game_log.try_recv() {
            let played = match event {
                crate::gamelog::LogEvent::Joined(at) => {
                    self.match_joined = Some(at);
                    continue;
                }
                crate::gamelog::LogEvent::Left(played) => {
                    self.match_joined = None;
                    played
                }
            };
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
            self.activity
                .push_log(tr("Fortnite did not record the last match, so its chests cannot be counted").into());
            if self.settings.warn_unrecorded && !self.warned_unrecorded {
                self.warned_unrecorded = true;
                platform::notify(
                    tr("Your last match was not recorded"),
                    tr("Fortnite saved no replay, so its chests can't be counted. In Fortnite turn on \
                        Settings > Game > Replays > Record Replays."),
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
            self.activity.push_log(trf("Fortnite recorded the last match, but its replay is not in {0}", &[&dir]));
            if self.settings.warn_unrecorded && !self.warned_no_file {
                self.warned_no_file = true;
                platform::notify(
                    tr("Replay not found"),
                    &trf(
                        "Fortnite recorded your last match, but it isn't in {0}. Check the replay folder in Settings > Replays.",
                        &[&dir],
                    ),
                );
            }
        }
    }

    /// Runs the online mode, and puts squadmates' counts into the matches or takes them out
    /// when it is switched off.
    /// Reminds the player of a Treasure Hour a quarter of an hour before it starts, and
    /// keeps the overlay's badge and Overview's countdown moving.
    fn process_treasure_hours(&mut self, ctx: &egui::Context) {
        const REMIND_BEFORE: i64 = 15 * 60;
        let hours = crate::prizes::treasure_hours();
        let now = chrono::Utc::now();
        let soon = crate::prizes::next_hour(&hours, now).filter(|h| (h.start - now).num_seconds() <= 86_400);
        if soon.is_none() && crate::prizes::hour_at(&hours, now).is_none() {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_secs(30));
        let Some(hour) = soon else { return };
        let left = (hour.start - now).num_seconds();
        let due = left <= REMIND_BEFORE && self.reminded_hour != Some(hour.start);
        if !due || !self.settings.setup_done {
            return;
        }
        self.reminded_hour = Some(hour.start);
        if !self.settings.treasure_hours.remind {
            return;
        }
        let (start, end) = (hour.start.with_timezone(&chrono::Local), hour.end.with_timezone(&chrono::Local));
        let title = trf("Treasure Hour in {0} min", &[&((left + 59) / 60)]);
        let body = format!("{}–{} · {}", start.format("%H:%M"), end.format("%H:%M"), hour.boosts.text());
        platform::notify(&title, &body);
    }

    fn process_online(&mut self, ctx: &egui::Context) {
        let enabled = self.settings.online && self.settings.setup_done;
        let accounts: Vec<&str> = self.settings.my_accounts().collect();
        let changed = self.online.tick(
            enabled,
            &self.store.matches,
            &self.settings.install_id,
            &accounts,
            (self.totals.mine(), !self.settings.leaderboard_name),
        );
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
                Update::Prizes(text) => {
                    if crate::prizes::accept(&text) {
                        ctx.request_repaint();
                    }
                }
                Update::Checking => self.update.checking = true,
                Update::Checked(result) => {
                    self.update.checking = false;
                    self.update.last_checked = Some(chrono::Local::now());
                    match result {
                        Ok(release) => {
                            self.update.available = release;
                            self.update.error = None;
                        }
                        Err(e) => self.update.error = Some(trf("Could not check for updates: {0}", &[&e])),
                    }
                }
                Update::Progress(p) => self.update.installing = Some(p),
                Update::Installed(exe) => {
                    crate::config::log_event("update installed, restarting");
                    let mut restart = std::process::Command::new(&exe);
                    restart.arg(AFTER_UPDATE_ARG);
                    if self.update.auto {
                        // Installed while the window was hidden: come back the same way.
                        restart.arg("--minimized");
                    }
                    match restart.spawn() {
                        Ok(_) => {
                            self.quitting = true;
                            ctx.send_viewport_cmd(ViewportCommand::Close);
                        }
                        Err(e) => {
                            self.update.installing = None;
                            self.update.error =
                                Some(trf("The update is installed; start the app again to use it ({0}).", &[&e]));
                        }
                    }
                }
                Update::Failed(e) => {
                    self.update.installing = None;
                    self.update.error = Some(trf("Update failed: {0}", &[&e]));
                }
            }
        }
    }

    pub fn install_update(&mut self) {
        if let (Some(release), None) = (self.update.available.clone(), self.update.installing) {
            self.update.auto = false;
            self.update.installing = Some(0.0);
            self.update.error = None;
            self.updater.send(crate::update::Command::Install(release));
        }
    }

    fn update_button(&mut self, ui: &mut egui::Ui, narrow: bool) {
        let width = if narrow { NAV_NARROW - 16.0 } else { self.nav_wide - 28.0 };
        if let Some(p) = self.update.installing {
            if !narrow {
                ui.label(theme::caption(tr("Updating...")));
            }
            ui.add(egui::ProgressBar::new(p).desired_width(width).show_percentage());
        } else if let Some(release) = &self.update.available {
            let version = crate::update::version_text(release.version);
            let text = if narrow {
                theme::icon_text(theme::icon::DOWNLOAD, "Up", 16.0)
            } else {
                egui::RichText::new(trf("Update to v{0}", &[&version])).strong()
            };
            let button = egui::Button::new(text.color(theme::bg())).fill(theme::gold());
            let hint = trf("Update to v{0}: downloads it from GitHub and restarts", &[&version]);
            if ui.add_sized([width, 30.0], button).on_hover_text(hint).clicked() {
                self.install_update();
            }
        }
    }

    fn navigation(&mut self, ui: &mut egui::Ui) {
        let narrow = self.settings.compact_nav;
        self.nav_wide = nav_width(ui.ctx());
        let width = if narrow { NAV_NARROW } else { self.nav_wide };
        egui::Panel::left("nav")
            .resizable(false)
            .exact_size(width)
            .frame(egui::Frame::new().fill(theme::panel()).inner_margin(egui::Margin::symmetric(8, 12)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let toggle = egui::Button::new(theme::icon_text(theme::icon::MENU, "=", 16.0)).frame(false);
                    let hint = if narrow { tr("Show the menu") } else { tr("Fold the menu to icons") };
                    if ui.add_sized([NAV_NARROW - 16.0, 28.0], toggle).on_hover_text(hint).clicked() {
                        self.settings.compact_nav = !narrow;
                        let _ = self.settings.save();
                    }
                    if !narrow {
                        ui.label(egui::RichText::new(tr("Chest Counter")).size(15.0).strong().color(theme::gold()));
                    }
                });
                ui.add_space(10.0);
                self.account_switcher(ui, narrow);
                for page in Page::ALL {
                    let selected = self.page == page;
                    let color = if selected { theme::gold() } else { theme::text() };
                    let letter: String = page.label().chars().take(1).collect();
                    let icon = theme::icon_text(page.icon(), &letter, 16.0).color(color);
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
                        ui.add_sized([self.nav_wide - 16.0, 34.0], button)
                    };
                    if response.clicked() {
                        self.page = page;
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    let version = ui
                        .add(
                            egui::Label::new(theme::caption(format!("v{}", env!("CARGO_PKG_VERSION"))))
                                .sense(egui::Sense::click()),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(tr("About this app"));
                    if version.clicked() {
                        self.page = Page::Settings;
                        self.settings_ui.category = crate::ui::settings::Category::About;
                    }
                    self.update_button(ui, narrow);
                    self.activity_badge(ui, narrow);
                    self.star_link(ui, narrow);
                });
            });
    }

    /// Switches the interface language, with the font its script needs.
    pub fn set_language(&mut self, ctx: &egui::Context, picked: crate::i18n::Lang) {
        if self.settings.language == picked {
            return;
        }
        self.settings.language = picked;
        crate::i18n::set(picked);
        theme::load_fonts(ctx);
        let _ = self.settings.save();
    }

    /// Switches the color theme and restyles everything at once.
    pub fn set_theme(&mut self, ctx: &egui::Context, picked: theme::ColorTheme) {
        if self.settings.theme == picked {
            return;
        }
        self.settings.theme = picked;
        theme::set(picked);
        theme::apply_colors(ctx);
        let _ = self.settings.save();
    }

    /// Paints Windows' title bar to match the theme, once the window exists.
    fn paint_title_bar(&mut self) {
        let current = theme::current();
        if self.title_bar == Some(current) {
            return;
        }
        let p = theme::palette();
        let rgb = |c: egui::Color32| [c.r(), c.g(), c.b()];
        if platform::color_title_bar(APP_NAME, !p.light, rgb(p.panel), rgb(p.text)) {
            self.title_bar = Some(current);
        }
    }

    /// A quiet link to the public repository, asking for a star.
    fn star_link(&mut self, ui: &mut egui::Ui, narrow: bool) {
        let response = if narrow {
            let star = theme::icon_text(theme::icon::STAR_FILLED, "*", 15.0).color(theme::gold());
            ui.add_sized([NAV_NARROW - 16.0, 26.0], egui::Button::new(star))
        } else {
            let mut job = egui::text::LayoutJob::default();
            let star = theme::icon_text(theme::icon::STAR_FILLED, "*", 11.0).color(theme::gold());
            star.append_to(&mut job, ui.style(), egui::FontSelection::Default, egui::Align::Center);
            for text in [egui::RichText::new("  "), egui::RichText::new(tr("Star on GitHub")).underline()] {
                let text = text.size(12.5).color(theme::muted());
                text.append_to(&mut job, ui.style(), egui::FontSelection::Default, egui::Align::Center);
            }
            ui.add(egui::Label::new(job).sense(egui::Sense::click()))
        };
        let response = response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(tr("Enjoying the app? Star it on GitHub so more players find it."));
        if response.clicked() {
            let url = format!("https://github.com/{}", crate::update::REPO);
            if let Err(e) = crate::platform::open_url(&url) {
                self.activity.push_log(trf("Could not open the browser: {0}", &[&e]));
            }
        }
    }

    /// Which Epic account is shown, when more than one plays on this PC.
    fn account_switcher(&mut self, ui: &mut egui::Ui, narrow: bool) {
        if self.settings.profiles.is_empty() || self.settings.account_id.is_empty() {
            return;
        }
        let current = self.settings.nick.clone();
        let mut all: Vec<(String, String)> = vec![(self.settings.account_id.clone(), current.clone())];
        all.extend(self.settings.profiles.iter().map(|(id, p)| (id.clone(), p.name.clone())));
        let active = self.settings.account_id.clone();
        let mut pick = None;
        let list = |ui: &mut egui::Ui, pick: &mut Option<String>| {
            for (id, name) in &all {
                if ui.selectable_label(*id == active, name).clicked() {
                    *pick = Some(id.clone());
                }
            }
        };
        let hint = tr("Accounts that play on this PC. Each has its own total, matches and settings; \
                       the app switches by itself to the one that played last.");
        if narrow {
            let letter: String = current.chars().take(1).collect();
            let icon = theme::icon_text(theme::icon::ACCOUNT, &letter, 16.0);
            ui.menu_button(icon, |ui| list(ui, &mut pick)).response.on_hover_text(format!("{current}\n\n{hint}"));
        } else {
            egui::ComboBox::from_id_salt("account")
                .width(self.nav_wide - 16.0)
                .selected_text(&current)
                .show_ui(ui, |ui| list(ui, &mut pick))
                .response
                .on_hover_text(hint);
        }
        ui.add_space(8.0);
        if let Some(name) = pick {
            self.switch_account(&name);
        }
    }

    fn activity_badge(&self, ui: &mut egui::Ui, narrow: bool) {
        let width = if narrow { NAV_NARROW - 16.0 } else { self.nav_wide - 28.0 };
        if let Some((file, fraction, queued)) = &self.activity.progress {
            let bar = ui.add(egui::ProgressBar::new(*fraction).desired_width(width));
            let more = if *queued > 0 { trf(" (+{0} queued)", &[queued]) } else { String::new() };
            let text = trf("Analyzing {0}{1}", &[&short_name(file), &more]);
            if narrow {
                bar.on_hover_text(text);
            } else {
                ui.label(theme::caption(text));
            }
        } else if narrow {
            // A dot: green while a match is played, gold while watching, red when paused.
            let (color, hint) = if self.activity.live_matches > 0 {
                (theme::opened(), tr("Match in progress"))
            } else if self.settings.watch {
                (theme::gold(), tr("Waiting for your next match"))
            } else {
                (theme::warn(), tr("Watching paused"))
            };
            let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 14.0), egui::Sense::hover());
            ui.painter().circle_filled(rect.center(), 4.0, color);
            response.on_hover_text(hint);
        } else if self.activity.live_matches > 0 {
            ui.label(egui::RichText::new(tr("Match in progress")).color(theme::opened()).size(12.5));
        } else if self.settings.watch {
            ui.label(theme::caption(tr("Waiting for your next match")));
        } else {
            ui.label(egui::RichText::new(tr("Watching paused")).color(theme::warn()).size(12.5));
        }
    }
}

/// Wide enough for the longest page name next to its icon, within bounds.
fn nav_width(ctx: &egui::Context) -> f32 {
    let widest = Page::ALL
        .iter()
        .map(|page| {
            let galley = ctx.fonts_mut(|f| {
                f.layout_no_wrap(format!("   {}", page.label()), egui::FontId::proportional(14.5), egui::Color32::WHITE)
            });
            galley.size().x
        })
        .fold(0.0, f32::max);
    // Icon, button padding and the panel's margins.
    (widest + 16.0 + 20.0 + 16.0 + 8.0).clamp(NAV_WIDE, NAV_WIDEST)
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
        self.process_treasure_hours(ctx);
        self.process_game_log(ctx);
        self.handle_close(ctx);
        self.auto_update(ctx);
        self.process_sleep(ctx);
        self.handle_dropped_files(ctx);
        crate::overlay::update(self, ctx);
        crate::stream::update(self);
        crate::discord::update(self, ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.paint_title_bar();
        if !self.settings.setup_done {
            egui::CentralPanel::default().show(ui, |ui| crate::ui::setup::show(self, ui));
            return;
        }
        self.navigation(ui);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::bg()).inner_margin(egui::Margin::same(20)))
            .show(ui, |ui| match self.page {
                Page::Overview => crate::ui::overview::show(self, ui),
                Page::Matches => crate::ui::matches::show(self, ui),
                Page::Stats => crate::ui::stats::show(self, ui),
                Page::Map => crate::ui::map::show(self, ui),
                Page::Squad => crate::ui::squad::show(self, ui),
                Page::Leaderboard => crate::ui::leaderboard::show(self, ui),
                Page::Settings => crate::ui::settings::show(self, ui),
            });
        crate::licenses::window(ui.ctx(), &mut self.licenses);
        let before = self.settings.auto_update;
        let switch = self.settings.check_updates.then_some(&mut self.settings.auto_update);
        let closed = !self.whats_new.is_empty() && crate::changelog::show(ui.ctx(), &self.whats_new, switch);
        if self.settings.auto_update != before {
            let _ = self.settings.save();
        }
        if closed {
            self.whats_new.clear();
            let current = crate::update::version_text(crate::update::current());
            if self.settings.notes_seen != current {
                self.settings.notes_seen = current;
                let _ = self.settings.save();
            }
        } else if self.whats_new.is_empty() && self.settings.setup_done {
            self.ask_about_account(ui.ctx());
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        crate::config::log_event("exit");
        let _ = self.store.save();
    }
}
