//! User settings and well-known locations.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chests::BreakConfig;
use chrono::NaiveDateTime;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::APP_NAME;

pub const SINCE_FORMAT: &str = "%Y-%m-%d %H:%M";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ArchiveSettings {
    pub enabled: bool,
    pub dir: PathBuf,
    pub max_gb: f32,
}

impl Default for ArchiveSettings {
    fn default() -> Self {
        Self { enabled: true, dir: default_archive_dir(), max_gb: 10.0 }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum OverlayCorner {
    TopLeft,
    #[default]
    TopRight,
    BottomLeft,
    BottomRight,
}

impl OverlayCorner {
    pub const ALL: [OverlayCorner; 4] = [Self::TopLeft, Self::TopRight, Self::BottomLeft, Self::BottomRight];

    pub fn label(self) -> &'static str {
        match self {
            Self::TopLeft => "Top left",
            Self::TopRight => "Top right",
            Self::BottomLeft => "Bottom left",
            Self::BottomRight => "Bottom right",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlaySettings {
    pub enabled: bool,
    /// Hide the overlay while Fortnite is not running.
    pub only_with_fortnite: bool,
    pub corner: OverlayCorner,
    /// Top left corner in screen points where the player dragged the overlay. Replaces
    /// `corner` while set.
    pub position: Option<[f32; 2]>,
    /// 0.3 to 1.0.
    pub opacity: f32,
    /// 0.5 to 2.5.
    pub scale: f32,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            only_with_fortnite: true,
            corner: OverlayCorner::TopRight,
            position: None,
            opacity: 0.85,
            scale: 1.0,
        }
    }
}

impl ArchiveSettings {
    pub fn max_bytes(&self) -> u64 {
        (f64::from(self.max_gb) * 1024.0 * 1024.0 * 1024.0) as u64
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub setup_done: bool,
    pub nick: String,
    /// Chests counted before the first replay, e.g. a finished in-game quest.
    pub base_count: u64,
    /// Chests the player added (or, below zero, took off) by hand, for matches the
    /// replays missed.
    pub adjustment: i64,
    /// Only matches started at or after this local time add to the total.
    pub count_since: Option<NaiveDateTime>,
    pub replay_dir: PathBuf,
    pub watch: bool,
    /// Warn when Fortnite's log shows a match played without a replay.
    pub warn_unrecorded: bool,
    pub notifications: bool,
    /// Start by itself, in the tray, when the user signs in to Windows.
    pub start_with_windows: bool,
    /// With `start_with_windows`: sit idle after sign-in and open only once Fortnite runs.
    pub start_with_fortnite: bool,
    pub close_to_tray: bool,
    pub archive: ArchiveSettings,
    pub overlay: OverlaySettings,
    /// Drops the player reported receiving, keyed by reward name.
    pub received: BTreeMap<String, u32>,
    pub breaks: BreakConfig,
    /// Darken the map picture so chest markers stand out.
    pub dim_map: bool,
    /// Look for a newer release on GitHub every few minutes.
    pub check_updates: bool,
    /// The sidebar shows icons only.
    pub compact_nav: bool,
    /// Share the player's own chest counts with the sync server and get squadmates' back.
    pub online: bool,
    /// Running total draws a line for every squadmate met who runs the app; when off,
    /// only for the names in `online_lines`.
    pub online_lines_all: bool,
    pub online_lines: Vec<String>,
    /// The leaderboard shows the player's name; off, it shows "Anonymous".
    pub leaderboard_name: bool,
    /// Random id of this installation, sent with uploads.
    pub install_id: String,
    /// The overlay page for OBS.
    pub stream: crate::stream::StreamSettings,
    /// The version whose "What's new" notes the player last closed.
    pub notes_seen: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            setup_done: false,
            nick: String::new(),
            base_count: 0,
            adjustment: 0,
            count_since: None,
            replay_dir: default_replay_dir(),
            watch: true,
            warn_unrecorded: true,
            notifications: true,
            start_with_windows: true,
            start_with_fortnite: true,
            close_to_tray: true,
            archive: ArchiveSettings::default(),
            overlay: OverlaySettings::default(),
            received: BTreeMap::new(),
            breaks: BreakConfig::default(),
            dim_map: false,
            check_updates: true,
            compact_nav: false,
            online: true,
            online_lines_all: true,
            online_lines: Vec::new(),
            leaderboard_name: true,
            install_id: String::new(),
            stream: Default::default(),
            notes_seen: String::new(),
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        Self::load_first_run().0
    }

    /// The settings, and whether there were none yet: the first run of the app.
    pub fn load_first_run() -> (Self, bool) {
        let Loaded { mut value, fresh } = load_json::<Self>(&settings_path());
        value.archive.dir = archive_dir_or_default(&value.archive.dir);
        (value, fresh)
    }

    pub fn save(&self) -> std::io::Result<()> {
        write_json_atomically(&settings_path(), self)
    }

    pub fn counts(&self, started_at: Option<NaiveDateTime>) -> bool {
        match (self.count_since, started_at) {
            (None, _) => true,
            (Some(since), Some(at)) => at >= since,
            (Some(_), None) => false,
        }
    }
}

pub fn data_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join(APP_NAME)
}

/// Appends a timestamped line to `app.log` in the data folder, so starts and exits can be
/// traced after the fact. Starts over once the file passes 256 KB.
pub fn log_event(message: &str) {
    use std::io::Write;
    if cfg!(test) {
        return;
    }
    let dir = data_dir();
    let path = dir.join("app.log");
    let _ = std::fs::create_dir_all(&dir);
    let too_big = std::fs::metadata(&path).is_ok_and(|m| m.len() > 256 * 1024);
    let file = std::fs::OpenOptions::new().create(true).write(true).append(!too_big).truncate(too_big).open(&path);
    if let Ok(mut file) = file {
        let _ = writeln!(file, "{} {message}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
    }
}

pub fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}

pub fn default_replay_dir() -> PathBuf {
    dirs::data_local_dir().unwrap_or_default().join("FortniteGame").join("Saved").join("Demos")
}

pub fn default_archive_dir() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join(APP_NAME).join("Replays")
}

/// An empty or relative archive folder would put copies wherever the app happens to run.
pub fn archive_dir_or_default(dir: &Path) -> PathBuf {
    if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        default_archive_dir()
    }
}

/// Writes through a temporary file that reaches the disk before it replaces the old one,
/// so a crash or power cut never leaves half a file behind. The old file is kept as
/// `<name>.bak` for `load_json`.
pub fn write_json_atomically<T: Serialize + ?Sized>(path: &Path, value: &T) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(&text)?;
        file.sync_all()?;
    }
    if path.exists() {
        std::fs::copy(path, backup_path(path))?;
    }
    std::fs::rename(&tmp, path)
}

fn backup_path(path: &Path) -> PathBuf {
    path.with_extension("json.bak")
}

/// A JSON file read by `load_json`.
pub struct Loaded<T> {
    pub value: T,
    /// The file did not exist: the app runs for the first time, or it was deleted.
    pub fresh: bool,
}

enum Read<T> {
    Ok(T),
    Missing,
    Bad(String),
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Read<T> {
    let mut attempt = 0;
    let bytes = loop {
        match std::fs::read(path) {
            Ok(bytes) => break bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Read::Missing,
            // Another program, say a virus scanner, may hold the file for a moment.
            Err(_) if attempt < 5 => {
                attempt += 1;
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            Err(e) => return Read::Bad(e.to_string()),
        }
    };
    match serde_json::from_slice(&bytes) {
        Ok(value) => Read::Ok(value),
        Err(e) => Read::Bad(e.to_string()),
    }
}

/// Moves a file that cannot be read out of the way, so the next save does not replace it.
fn set_aside(path: &Path, why: &str) {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let aside = path.with_file_name(format!("{name}.corrupt-{}", chrono::Local::now().format("%Y%m%d-%H%M%S")));
    let moved = std::fs::rename(path, &aside).or_else(|_| std::fs::copy(path, &aside).map(drop));
    log_event(&format!(
        "cannot read {name} ({why}); {}",
        match moved {
            Ok(()) => format!("kept it as {}", aside.display()),
            Err(e) => format!("could not keep a copy: {e}"),
        }
    ));
}

/// Reads a file written by `write_json_atomically`. One that does not parse is moved
/// aside as `<name>.corrupt-<time>` and the copy from the save before is used instead,
/// so a damaged file is never overwritten with defaults.
pub fn load_json<T: DeserializeOwned + Default>(path: &Path) -> Loaded<T> {
    match read_json(path) {
        Read::Ok(value) => return Loaded { value, fresh: false },
        Read::Missing => return Loaded { value: T::default(), fresh: true },
        Read::Bad(why) => set_aside(path, &why),
    }
    let backup = backup_path(path);
    match read_json(&backup) {
        Read::Ok(value) => {
            log_event(&format!("restored {} from {}", path.display(), backup.display()));
            if std::fs::copy(&backup, path).is_err() {
                log_event(&format!("could not write {} back", path.display()));
            }
            Loaded { value, fresh: false }
        }
        Read::Missing => Loaded { value: T::default(), fresh: false },
        Read::Bad(why) => {
            set_aside(&backup, &why);
            Loaded { value: T::default(), fresh: false }
        }
    }
}

pub fn parse_since(text: &str) -> Option<NaiveDateTime> {
    let text = text.trim();
    NaiveDateTime::parse_from_str(text, SINCE_FORMAT)
        .ok()
        .or_else(|| chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").ok().and_then(|d| d.and_hms_opt(0, 0, 0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_since_with_and_without_time() {
        assert_eq!(parse_since("2026-11-07 22:39").unwrap().to_string(), "2026-11-07 22:39:00");
        assert_eq!(parse_since(" 2026-11-07 ").unwrap().to_string(), "2026-11-07 00:00:00");
        assert!(parse_since("yesterday").is_none());
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fcc-config-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn corrupt_copies(dir: &Path) -> usize {
        std::fs::read_dir(dir)
            .unwrap()
            .filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().contains(".corrupt-"))
            .count()
    }

    #[test]
    fn a_missing_file_is_a_fresh_start() {
        let dir = temp_dir("missing");
        let loaded = load_json::<Settings>(&dir.join("settings.json"));
        assert!(loaded.fresh);
        assert_eq!(loaded.value, Settings::default());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_damaged_file_is_kept_and_the_backup_used() {
        let dir = temp_dir("damaged");
        let path = dir.join("settings.json");
        let first = Settings { base_count: 1000, ..Default::default() };
        write_json_atomically(&path, &first).unwrap();
        let second = Settings { base_count: 1200, ..Default::default() };
        write_json_atomically(&path, &second).unwrap();
        // A power cut can leave the file empty.
        std::fs::write(&path, "").unwrap();

        let loaded = load_json::<Settings>(&path);
        assert!(!loaded.fresh);
        assert_eq!(loaded.value.base_count, 1000, "the save before the damaged one");
        assert_eq!(corrupt_copies(&dir), 1);
        // The backup went back in place, so a crash before the next save loses nothing.
        assert_eq!(load_json::<Settings>(&path).value.base_count, 1000);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_damaged_file_without_backup_is_not_overwritten() {
        let dir = temp_dir("nobackup");
        let path = dir.join("matches.json");
        std::fs::write(&path, "{\"matches\": [").unwrap();
        let loaded = load_json::<Settings>(&path);
        assert!(!loaded.fresh, "a damaged file is no first run");
        assert!(!path.exists());
        assert_eq!(corrupt_copies(&dir), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn relative_archive_folders_fall_back_to_the_default() {
        assert_eq!(archive_dir_or_default(Path::new("")), default_archive_dir());
        assert_eq!(archive_dir_or_default(Path::new("replays")), default_archive_dir());
        let absolute = std::env::temp_dir().join("archive");
        assert_eq!(archive_dir_or_default(&absolute), absolute);
    }

    #[test]
    fn counts_only_matches_after_since() {
        let s = Settings { count_since: parse_since("2026-11-07 22:39"), ..Default::default() };
        assert!(s.counts(parse_since("2026-11-07 22:39")));
        assert!(!s.counts(parse_since("2026-11-07 22:38")));
        assert!(!s.counts(None));
    }
}
