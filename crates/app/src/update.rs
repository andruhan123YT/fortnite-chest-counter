//! Updates from the app's GitHub releases: a check every few minutes, and on request a
//! download that replaces the running exe, after which the app restarts itself.
//!
//! A release must carry the exe and a `.sha256` file with its checksum; an exe that does
//! not match is thrown away and the installed one stays.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

pub const REPO: &str = "andruhan123YT/fortnite-chest-counter";
const EXE_ASSET: &str = "fortnite-chest-counter.exe";
const SUM_ASSET: &str = "fortnite-chest-counter.exe.sha256";
pub const CHECK_EVERY: Duration = Duration::from_secs(10 * 60);
/// Replaces the GitHub address of the latest release, to try updates against a local copy.
const FEED_ENV: &str = "FCC_UPDATE_FEED";
/// Passed to the new exe so it waits for the old one to quit instead of handing over to it.
pub const AFTER_UPDATE_ARG: &str = "--after-update";

pub type Version = (u32, u32, u32);

/// `v1.2.3` or `1.2.3`; anything after a `-` (like `-beta`) is ignored.
pub fn parse_version(text: &str) -> Option<Version> {
    let text = text.trim().trim_start_matches(['v', 'V']);
    let core = text.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u32>().ok());
    let version = (parts.next()??, parts.next().flatten().unwrap_or(0), parts.next().flatten().unwrap_or(0));
    Some(version)
}

pub fn current() -> Version {
    parse_version(env!("CARGO_PKG_VERSION")).unwrap_or((0, 0, 0))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub tag: String,
    exe_url: String,
    sum_url: String,
}

/// The newest release if it is newer than `than` and carries both files an update needs.
fn newer_release(json: &serde_json::Value, than: Version) -> Option<Release> {
    let tag = json.get("tag_name")?.as_str()?;
    let version = parse_version(tag)?;
    if version <= than {
        return None;
    }
    let asset = |name: &str| {
        json.get("assets")?
            .as_array()?
            .iter()
            .find(|a| a.get("name").and_then(|n| n.as_str()) == Some(name))?
            .get("browser_download_url")?
            .as_str()
            .map(str::to_owned)
    };
    Some(Release { version, tag: tag.to_owned(), exe_url: asset(EXE_ASSET)?, sum_url: asset(SUM_ASSET)? })
}

/// The first SHA-256 in a checksum file, which may also name the file after it.
fn expected_sum(text: &str) -> Option<String> {
    let first = text.split_whitespace().next()?.to_ascii_lowercase();
    (first.len() == 64 && first.bytes().all(|b| b.is_ascii_hexdigit())).then_some(first)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .user_agent(format!("fortnite-chest-counter/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn feed_url() -> String {
    std::env::var(FEED_ENV).unwrap_or_else(|_| format!("https://api.github.com/repos/{REPO}/releases/latest"))
}

/// Asks GitHub for the latest release. `Ok(None)`: nothing newer, or no release yet.
pub fn check() -> Result<Option<Release>, String> {
    let response = agent().get(feed_url()).header("Accept", "application/vnd.github+json").call();
    let mut response = match response {
        Ok(r) => r,
        // A repository without releases answers 404.
        Err(ureq::Error::StatusCode(404)) => return Ok(None),
        Err(ureq::Error::StatusCode(403 | 429)) => return Err("GitHub is limiting requests, try later".into()),
        Err(e) => return Err(e.to_string()),
    };
    let text = response.body_mut().read_to_string().map_err(|e| e.to_string())?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    Ok(newer_release(&json, current()))
}

fn sibling(exe: &Path, suffix: &str) -> PathBuf {
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    exe.with_file_name(name)
}

/// Downloads the release, checks it and puts it in place of the running exe, which moves
/// aside to `.old` (Windows lets a running exe be renamed, not overwritten). Returns the
/// path to start.
pub fn install(release: &Release, mut progress: impl FnMut(f32)) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let agent = agent();
    let sums =
        agent.get(&release.sum_url).call().map_err(|e| format!("checksum file: {e}"))?.body_mut().read_to_string();
    let expected = expected_sum(&sums.map_err(|e| e.to_string())?).ok_or("the checksum file is not valid")?;

    let mut response = agent.get(&release.exe_url).call().map_err(|e| format!("download: {e}"))?;
    let total =
        response.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
    let staged = sibling(&exe, ".new");
    let downloaded = (|| -> Result<String, String> {
        let mut file = File::create(&staged).map_err(|e| e.to_string())?;
        let mut reader = response.body_mut().with_config().limit(512 * 1024 * 1024).reader();
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 64 * 1024];
        let mut done = 0u64;
        loop {
            let n = reader.read(&mut buf).map_err(|e| format!("download: {e}"))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            done += n as u64;
            if let Some(total) = total.filter(|&t| t > 0) {
                progress((done as f32 / total as f32).min(1.0));
            }
        }
        file.sync_all().map_err(|e| e.to_string())?;
        Ok(hex(&hasher.finalize()))
    })();
    let actual = match downloaded {
        Ok(sum) => sum,
        Err(e) => {
            let _ = std::fs::remove_file(&staged);
            return Err(e);
        }
    };
    if actual != expected {
        let _ = std::fs::remove_file(&staged);
        return Err("the download does not match its checksum".into());
    }

    let old = sibling(&exe, ".old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| format!("could not move the old version aside: {e}"))?;
    if let Err(e) = std::fs::rename(&staged, &exe) {
        let _ = std::fs::rename(&old, &exe);
        return Err(format!("could not put the new version in place: {e}"));
    }
    Ok(exe)
}

/// Deletes the exe an update left behind. Retries for a while: the old process may still
/// be on its way out.
pub fn remove_leftover() {
    let Ok(exe) = std::env::current_exe() else { return };
    let old = sibling(&exe, ".old");
    for _ in 0..20 {
        if !old.exists() || std::fs::remove_file(&old).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

pub fn version_text((major, minor, patch): Version) -> String {
    format!("{major}.{minor}.{patch}")
}

/// What the app knows about updates, for the sidebar and Settings.
#[derive(Default)]
pub struct Status {
    pub available: Option<Release>,
    pub checking: bool,
    pub last_checked: Option<chrono::DateTime<chrono::Local>>,
    pub error: Option<String>,
    /// Download progress while an update is being installed.
    pub installing: Option<f32>,
}

pub enum Command {
    CheckNow,
    SetEnabled(bool),
    Install(Release),
}

pub enum Event {
    Checking,
    Checked(Result<Option<Release>, String>),
    Progress(f32),
    Installed(PathBuf),
    Failed(String),
}

/// Runs checks and installs on a background thread.
pub struct Updater {
    tx: Sender<Command>,
    rx: Receiver<Event>,
}

impl Updater {
    pub fn start(enabled: bool, repaint: impl Fn() + Send + 'static) -> Self {
        let (tx, commands) = mpsc::channel();
        let (events, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("updater".into())
            .spawn(move || run(enabled, commands, events, repaint))
            .expect("spawn the updater thread");
        Self { tx, rx }
    }

    pub fn send(&self, command: Command) {
        let _ = self.tx.send(command);
    }

    pub fn events(&self) -> impl Iterator<Item = Event> + '_ {
        self.rx.try_iter()
    }
}

fn run(mut enabled: bool, commands: Receiver<Command>, events: Sender<Event>, repaint: impl Fn()) {
    // The first check waits a little so it does not compete with the startup scan.
    let mut next = Instant::now() + Duration::from_secs(5);
    let emit = |e: Event| {
        let _ = events.send(e);
        repaint();
    };
    let checked = |emit: &dyn Fn(Event)| {
        emit(Event::Checking);
        emit(Event::Checked(check()));
    };
    loop {
        let wait = if enabled { next.saturating_duration_since(Instant::now()) } else { CHECK_EVERY };
        match commands.recv_timeout(wait) {
            Ok(Command::CheckNow) => {
                checked(&emit);
                next = Instant::now() + CHECK_EVERY;
            }
            Ok(Command::SetEnabled(on)) => {
                if on && !enabled {
                    next = Instant::now();
                }
                enabled = on;
            }
            Ok(Command::Install(release)) => match install(&release, |p| emit(Event::Progress(p))) {
                Ok(path) => emit(Event::Installed(path)),
                Err(e) => emit(Event::Failed(e)),
            },
            Err(RecvTimeoutError::Timeout) => {
                if enabled {
                    checked(&emit);
                    next = Instant::now() + CHECK_EVERY;
                }
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_versions() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.10"), Some((1, 10, 0)));
        assert_eq!(parse_version("v2.0.0-beta"), Some((2, 0, 0)));
        assert_eq!(parse_version("latest"), None);
        assert!(parse_version("v1.10.0") > parse_version("v1.9.9"));
    }

    fn release(tag: &str, assets: &[&str]) -> serde_json::Value {
        let assets: Vec<_> = assets
            .iter()
            .map(|n| serde_json::json!({"name": n, "browser_download_url": format!("https://x/{n}")}))
            .collect();
        serde_json::json!({"tag_name": tag, "assets": assets})
    }

    #[test]
    fn offers_only_newer_complete_releases() {
        let both = [EXE_ASSET, SUM_ASSET];
        let got = newer_release(&release("v1.1.0", &both), (1, 0, 0)).unwrap();
        assert_eq!((got.version, got.exe_url.as_str()), ((1, 1, 0), "https://x/fortnite-chest-counter.exe"));
        assert!(newer_release(&release("v1.0.0", &both), (1, 0, 0)).is_none());
        assert!(newer_release(&release("v0.9.0", &both), (1, 0, 0)).is_none());
        // Without a checksum there is nothing to check the download against.
        assert!(newer_release(&release("v1.1.0", &[EXE_ASSET]), (1, 0, 0)).is_none());
    }

    #[test]
    fn reads_checksum_files() {
        let sum = "a".repeat(64);
        assert_eq!(expected_sum(&format!("{sum}  fortnite-chest-counter.exe\n")), Some(sum.clone()));
        assert_eq!(expected_sum(&sum.to_uppercase()), Some(sum));
        assert_eq!(expected_sum("not a sum"), None);
        assert_eq!(hex(&[0, 15, 255]), "000fff");
    }
}
