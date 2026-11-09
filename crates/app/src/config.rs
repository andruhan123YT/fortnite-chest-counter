//! User settings and well-known locations.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chests::BreakConfig;
use chrono::NaiveDateTime;
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
    pub notifications: bool,
    pub start_with_windows: bool,
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
            notifications: true,
            start_with_windows: true,
            close_to_tray: true,
            archive: ArchiveSettings::default(),
            overlay: OverlaySettings::default(),
            received: BTreeMap::new(),
            breaks: BreakConfig::default(),
            dim_map: false,
            check_updates: true,
            compact_nav: false,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        std::fs::read_to_string(settings_path())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
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

/// Writes through a temporary file so a crash never leaves half a file behind.
pub fn write_json_atomically<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(value).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
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

    #[test]
    fn counts_only_matches_after_since() {
        let s = Settings { count_since: parse_since("2026-11-07 22:39"), ..Default::default() };
        assert!(s.counts(parse_since("2026-11-07 22:39")));
        assert!(!s.counts(parse_since("2026-11-07 22:38")));
        assert!(!s.counts(None));
    }
}
