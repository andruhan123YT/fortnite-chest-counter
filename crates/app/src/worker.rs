//! Background thread that watches the replay folder and analyzes replays.
//!
//! The UI talks to it through [`Command`]s and hears back through [`Event`]s, so parsing
//! a 30 MB replay never blocks a frame.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::archive;
use crate::config::Settings;
use crate::record::{MatchRecord, ANALYZER_VERSION};

const POLL_INTERVAL: Duration = Duration::from_secs(5);
/// The header (including the encryption key) always fits in the first few kilobytes.
const HEADER_PROBE_BYTES: usize = 64 * 1024;

#[derive(Debug)]
pub enum Command {
    /// Look for new replays now. `force` re-analyzes everything found.
    Scan {
        force: bool,
    },
    Analyze(Vec<PathBuf>),
    /// Analyze replays recorded by teammates. They are never archived or counted alone.
    AnalyzeSquad(Vec<PathBuf>),
    /// Re-run the analysis for these stored matches, looking in the replay folder and
    /// the archive.
    Reanalyze(Vec<String>),
    Settings(Box<Settings>),
}

#[derive(Debug)]
pub enum Event {
    Progress {
        file: String,
        fraction: f32,
        queued: usize,
    },
    Analyzed {
        record: Box<MatchRecord>,
        from_watch: bool,
    },
    SquadAnalyzed {
        record: Box<MatchRecord>,
    },
    Failed {
        file: String,
        error: String,
    },
    /// Replays of matches still being played (no encryption key yet).
    Waiting {
        live: usize,
    },
    Idle {
        last_scan: chrono::NaiveDateTime,
    },
    Info(String),
}

pub struct Worker {
    commands: Sender<Command>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Worker {
    pub fn spawn(
        settings: Settings,
        known: HashMap<(String, u64), u32>,
        events: Sender<Event>,
        wake: impl Fn() + Send + 'static,
    ) -> Self {
        let (commands, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let state = State {
            settings,
            known,
            failed: HashSet::new(),
            sizes: HashMap::new(),
            queue: VecDeque::new(),
            events,
            wake: Box::new(wake),
            stop: stop.clone(),
        };
        let handle = std::thread::Builder::new().name("replay-worker".into()).spawn(move || state.run(rx)).ok();
        Self { commands, stop, handle }
    }

    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let (tx, _) = mpsc::channel();
        drop(std::mem::replace(&mut self.commands, tx));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

struct Job {
    path: PathBuf,
    from_watch: bool,
    /// A teammate's replay.
    squad: bool,
}

struct State {
    settings: Settings,
    known: HashMap<(String, u64), u32>,
    failed: HashSet<(String, u64)>,
    /// Last seen size of replays that are not finished yet.
    sizes: HashMap<PathBuf, u64>,
    queue: VecDeque<Job>,
    events: Sender<Event>,
    wake: Box<dyn Fn() + Send>,
    stop: Arc<AtomicBool>,
}

impl State {
    fn emit(&self, event: Event) {
        let _ = self.events.send(event);
        (self.wake)();
    }

    fn run(mut self, rx: Receiver<Command>) {
        if self.settings.watch && self.settings.setup_done {
            self.scan(false, true);
        }
        loop {
            self.drain_queue();
            match rx.recv_timeout(POLL_INTERVAL) {
                Ok(command) => self.handle(command),
                Err(RecvTimeoutError::Timeout) => {
                    if self.settings.watch && self.settings.setup_done {
                        self.scan(false, true);
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return,
            }
            if self.stop.load(Ordering::Relaxed) {
                return;
            }
        }
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::Scan { force } => self.scan(force, false),
            Command::Analyze(paths) => {
                for path in expand(&paths) {
                    self.queue.push_back(Job { path, from_watch: false, squad: false });
                }
            }
            Command::AnalyzeSquad(paths) => {
                for path in expand(&paths) {
                    self.queue.push_back(Job { path, from_watch: false, squad: true });
                }
            }
            Command::Reanalyze(file_names) => {
                for name in file_names {
                    match self.locate(&name) {
                        Some(path) => self.queue.push_back(Job { path, from_watch: false, squad: false }),
                        None => self.emit(Event::Failed { file: name, error: "replay file no longer exists".into() }),
                    }
                }
            }
            Command::Settings(settings) => self.settings = *settings,
        }
    }

    fn locate(&self, file_name: &str) -> Option<PathBuf> {
        [&self.settings.replay_dir, &self.settings.archive.dir]
            .iter()
            .map(|dir| dir.join(file_name))
            .find(|p| p.is_file())
    }

    fn scan(&mut self, force: bool, from_watch: bool) {
        let mut live = 0;
        for path in list_replays(&self.settings.replay_dir) {
            let Some((name, size)) = file_key(&path) else { continue };
            let key = (name, size);
            if !force && (self.known.get(&key).is_some_and(|&v| v >= ANALYZER_VERSION) || self.failed.contains(&key)) {
                continue;
            }
            if self.queue.iter().any(|j| j.path == path) {
                continue;
            }
            match probe(&path) {
                Probe::Complete => {
                    // Wait for one quiet poll so Fortnite has finished writing the file.
                    let settled = !from_watch || self.sizes.get(&path) == Some(&size);
                    self.sizes.insert(path.clone(), size);
                    if settled {
                        self.sizes.remove(&path);
                        // Only a match that just ended deserves a notification, not the
                        // backlog found on the first scan.
                        let from_watch = from_watch && modified_recently(&path);
                        self.queue.push_back(Job { path, from_watch, squad: false });
                    }
                }
                Probe::Live => {
                    self.sizes.insert(path, size);
                    live += 1;
                }
                Probe::Invalid => {
                    self.failed.insert(key);
                }
            }
        }
        self.emit(Event::Waiting { live });
        self.emit(Event::Idle { last_scan: chrono::Local::now().naive_local() });
    }

    fn drain_queue(&mut self) {
        while let Some(job) = self.queue.pop_front() {
            if self.stop.load(Ordering::Relaxed) {
                return;
            }
            self.analyze(job);
        }
    }

    fn analyze(&mut self, job: Job) {
        let Some((name, size)) = file_key(&job.path) else { return };
        let bytes = match std::fs::read(&job.path) {
            Ok(bytes) => bytes,
            Err(e) => {
                self.emit(Event::Failed { file: name, error: e.to_string() });
                return;
            }
        };

        let queued = self.queue.len();
        let mut progress = Progress { state: self, file: &name, queued, last: Instant::now() };
        progress.send(0.0);
        // A parser bug on one odd replay must not take the watcher down with it.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            chests::analyze_replay(&bytes, &self.settings.breaks, &mut progress)
        }));
        let result = match result {
            Ok(result) => result,
            Err(_) => {
                self.failed.insert((name.clone(), size));
                self.emit(Event::Failed { file: name, error: "the parser crashed on this replay".into() });
                return;
            }
        };
        match result {
            Ok((replay, analysis)) => {
                let record = MatchRecord::from_analysis(&name, size, &replay, analysis, &self.settings.nick);
                if job.squad {
                    self.emit(Event::SquadAnalyzed { record: Box::new(record) });
                    return;
                }
                self.known.insert((name.clone(), size), ANALYZER_VERSION);
                self.failed.remove(&(name.clone(), size));
                let worth_keeping = self.settings.counts(record.started_at);
                if self.settings.archive.enabled && worth_keeping && !job.path.starts_with(&self.settings.archive.dir) {
                    if let Err(e) = archive::store_copy(&self.settings.archive.dir, &name, &bytes).and_then(|_| {
                        archive::enforce_cap(&self.settings.archive.dir, self.settings.archive.max_bytes())
                    }) {
                        self.emit(Event::Info(format!("Could not archive {name}: {e}")));
                    }
                }
                self.emit(Event::Analyzed { record: Box::new(record), from_watch: job.from_watch });
            }
            Err(fnreplay::Error::Incomplete) => {
                self.emit(Event::Info(format!("{name}: the match is still being played")));
            }
            Err(e) => {
                self.failed.insert((name.clone(), size));
                self.emit(Event::Failed { file: name, error: e.to_string() });
            }
        }
    }
}

struct Progress<'a> {
    state: &'a State,
    file: &'a str,
    queued: usize,
    last: Instant,
}

impl Progress<'_> {
    fn send(&mut self, fraction: f32) {
        self.last = Instant::now();
        self.state.emit(Event::Progress { file: self.file.to_string(), fraction, queued: self.queued });
    }
}

impl fnreplay::Visitor for Progress<'_> {
    fn on_progress(&mut self, fraction: f32) {
        if self.last.elapsed() > Duration::from_millis(100) || fraction >= 1.0 {
            self.send(fraction);
        }
    }

    fn should_stop(&self) -> bool {
        self.state.stop.load(Ordering::Relaxed)
    }
}

const RECENT: Duration = Duration::from_secs(15 * 60);

fn modified_recently(path: &Path) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|m| m.elapsed().ok())
        .is_some_and(|age| age < RECENT)
}

enum Probe {
    Complete,
    Live,
    Invalid,
}

fn probe(path: &Path) -> Probe {
    use std::io::Read;
    let Ok(file) = std::fs::File::open(path) else { return Probe::Live };
    let mut head = Vec::with_capacity(HEADER_PROBE_BYTES);
    if file.take(HEADER_PROBE_BYTES as u64).read_to_end(&mut head).is_err() {
        return Probe::Live;
    }
    match fnreplay::read_info(&head) {
        Ok(info) if info.is_complete() => Probe::Complete,
        Ok(_) => Probe::Live,
        Err(fnreplay::Error::NotAReplay) => Probe::Invalid,
        Err(_) => Probe::Live,
    }
}

pub fn file_key(path: &Path) -> Option<(String, u64)> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let size = std::fs::metadata(path).ok()?.len();
    Some((name, size))
}

pub fn list_replays(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("replay")))
        .collect();
    files.sort();
    files
}

/// Files stay as they are, folders become the replays inside them.
pub fn expand(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for path in paths {
        if path.is_dir() {
            out.extend(list_replays(path));
        } else if path.extension().is_some_and(|x| x.eq_ignore_ascii_case("replay")) {
            out.push(path.clone());
        }
    }
    out.dedup();
    out
}
