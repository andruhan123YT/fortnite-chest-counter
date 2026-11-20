//! Reads Fortnite's own log to tell when a match was played without a replay, so the
//! player hears about it after one match instead of finding a hole in the count later.
//!
//! Each match shows up as: `LoadMap` to a server address, `playlistid="..."`,
//! `RecordReplay: Starting recording` (only when replays are on), and `LoadMap` back to
//! the front end once the player is in the lobby again.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, NaiveDateTime, Utc};

const POLL: Duration = Duration::from_secs(3);
/// How much of the log already there is read at start to find a match in progress: a
/// whole match fits many times over.
const RECENT_BYTES: u64 = 16 * 1024 * 1024;
/// Recording starts some seconds into a match, so a shorter visit proves nothing.
const MIN_MATCH: chrono::Duration = chrono::Duration::seconds(90);

pub fn log_path() -> PathBuf {
    dirs::data_local_dir().unwrap_or_default().join("FortniteGame").join("Saved").join("Logs").join("FortniteGame.log")
}

/// A match the log showed from start to the return to the lobby.
#[derive(Debug, Clone, PartialEq)]
pub struct Played {
    pub playlist: Option<String>,
    pub started: Option<DateTime<Utc>>,
    pub ended: Option<DateTime<Utc>>,
    pub recorded: bool,
}

impl Played {
    /// Battle Royale and Zero Build, ranked or not: they record replays when Fortnite's
    /// replay setting is on, and they are the only modes whose chests count. Other modes
    /// (Reload, Creative, LEGO, events) are left out rather than risk a false warning.
    pub fn records_replays(&self) -> bool {
        self.playlist.as_deref().is_some_and(chests::is_counted_playlist)
    }

    pub fn long_enough(&self) -> bool {
        matches!((self.started, self.ended), (Some(s), Some(e)) if e - s >= MIN_MATCH)
    }

    /// A Battle Royale match long enough to have a replay, played without one.
    pub fn missed_replay(&self) -> bool {
        self.records_replays() && self.long_enough() && !self.recorded
    }
}

/// Follows log lines and reports each match once the player is back in the lobby.
#[derive(Debug, Default)]
pub struct Tracker {
    current: Option<Played>,
}

/// What the game log tells the app.
#[derive(Debug, Clone, PartialEq)]
pub enum LogEvent {
    /// The game loaded a match server: the player is on the starting island.
    Joined(DateTime<Utc>),
    /// The player is back in the lobby.
    Left(Played),
}

/// The time at the start of a log line, `[2026.11.10-18.14.30:009]`, which is UTC.
fn stamp(line: &str) -> Option<DateTime<Utc>> {
    let text = line.strip_prefix('[')?.get(..19)?;
    NaiveDateTime::parse_from_str(text, "%Y.%m.%d-%H.%M.%S").ok().map(|t| t.and_utc())
}

impl Tracker {
    /// When the match the player is in now was loaded.
    pub fn joined(&self) -> Option<DateTime<Utc>> {
        self.current.as_ref().and_then(|p| p.started)
    }

    pub fn line(&mut self, line: &str) -> Option<Played> {
        if let Some(rest) = line.split("LoadMap: ").nth(1) {
            if rest.starts_with("/Game/Maps/Frontend") {
                let mut done = self.current.take()?;
                done.ended = stamp(line);
                return Some(done);
            }
            // A server address: a match starts. One still open was left without a
            // lobby in between, so nothing is known about its replay.
            if rest.starts_with(|c: char| c.is_ascii_digit()) {
                self.current = Some(Played { playlist: None, started: stamp(line), ended: None, recorded: false });
            }
            return None;
        }
        let current = self.current.as_mut()?;
        // Fortnite names the playlist in one of two lines, depending on the match.
        let playlist = line
            .split("playlistid=\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .or_else(|| line.split("PlaylistName is ").nth(1).and_then(|rest| rest.split_whitespace().next()));
        if let Some(id) = playlist {
            if !id.eq_ignore_ascii_case("Playlist_FrontEnd") {
                current.playlist = Some(id.trim_end_matches([',', '.']).to_owned());
            }
        } else if line.contains("RecordReplay: Starting recording") {
            current.recorded = true;
        }
        None
    }
}

/// Reads what a growing log file gained since the last call. A new file (the game
/// started again) is read from its start.
struct Tail {
    path: PathBuf,
    created: Option<SystemTime>,
    offset: u64,
    partial: Vec<u8>,
}

impl Tail {
    /// Starts at the end of the log that is there now: its matches are already over.
    fn new(path: PathBuf) -> Self {
        let meta = std::fs::metadata(&path).ok();
        let created = meta.as_ref().and_then(|m| m.created().ok());
        let offset = meta.map_or(0, |m| m.len());
        Self { path, created, offset, partial: Vec::new() }
    }

    /// The last part of the log that was there when following began, whole lines only.
    fn before(&self) -> Vec<String> {
        let Ok(mut file) = std::fs::File::open(&self.path) else { return Vec::new() };
        let from = self.offset.saturating_sub(RECENT_BYTES);
        if file.seek(SeekFrom::Start(from)).is_err() {
            return Vec::new();
        }
        let mut chunk = Vec::new();
        if file.take(self.offset - from).read_to_end(&mut chunk).is_err() {
            return Vec::new();
        }
        // Reading from the middle of the file cuts the first line.
        let start = if from > 0 { chunk.iter().position(|&b| b == b'\n').map_or(chunk.len(), |i| i + 1) } else { 0 };
        String::from_utf8_lossy(&chunk[start..]).lines().map(str::to_owned).collect()
    }

    fn lines(&mut self) -> Vec<String> {
        let Ok(meta) = std::fs::metadata(&self.path) else { return Vec::new() };
        let created = meta.created().ok();
        if created != self.created || meta.len() < self.offset {
            self.created = created;
            self.offset = 0;
            self.partial.clear();
        }
        if meta.len() == self.offset {
            return Vec::new();
        }
        let Ok(mut file) = std::fs::File::open(&self.path) else { return Vec::new() };
        if file.seek(SeekFrom::Start(self.offset)).is_err() {
            return Vec::new();
        }
        let mut chunk = Vec::new();
        let Ok(read) = file.read_to_end(&mut chunk) else { return Vec::new() };
        self.offset += read as u64;
        self.partial.extend_from_slice(&chunk);
        let Some(end) = self.partial.iter().rposition(|&b| b == b'\n') else { return Vec::new() };
        let complete: Vec<u8> = self.partial.drain(..=end).collect();
        String::from_utf8_lossy(&complete).lines().map(str::to_owned).collect()
    }
}

/// Follows the log on a thread of its own and sends every match it sees start and end.
pub fn watch(path: &Path, wake: impl Fn() + Send + 'static) -> Receiver<LogEvent> {
    let (tx, rx) = mpsc::channel();
    let mut tail = Tail::new(path.to_path_buf());
    let _ = std::thread::Builder::new().name("game-log".into()).spawn(move || {
        // Started in the middle of a match, the app learns when it began from the log so
        // far; the matches already over there were handled by the run that saw them.
        let mut tracker = Tracker::default();
        for line in tail.before() {
            tracker.line(&line);
        }
        if let Some(at) = tracker.joined() {
            if tx.send(LogEvent::Joined(at)).is_err() {
                return;
            }
            wake();
        }
        loop {
            for line in tail.lines() {
                let before = tracker.joined();
                let mut events = Vec::new();
                if let Some(played) = tracker.line(&line) {
                    events.push(LogEvent::Left(played));
                }
                match tracker.joined() {
                    Some(at) if Some(at) != before => events.push(LogEvent::Joined(at)),
                    _ => {}
                }
                for event in events {
                    if tx.send(event).is_err() {
                        return;
                    }
                    wake();
                }
            }
            std::thread::sleep(POLL);
        }
    });
    rx
}

/// Whether `dir` holds a replay written since `since`.
pub fn replay_written_since(dir: &Path, since: SystemTime) -> bool {
    std::fs::read_dir(dir).into_iter().flatten().filter_map(Result::ok).any(|e| {
        e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("replay"))
            && e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t >= since)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: &str =
        "[2026.11.10-18.14.30:009][551]LogLoad: LoadMap: 10.0.0.1:9022/Delilah/Maps/Delilah_Terrain?EncryptionToken=x";
    const PLAYLIST: &str =
        "[2026.11.10-18.14.35:673][589]LogCsvProfiler: Display: Metadata set : playlistid=\"Playlist_NoBuildBR_Squad\"";
    const RECORDING: &str = "[2026.11.10-18.14.45:069][819]LogDemo: UReplaySubsystem::RecordReplay: Starting recording with demo driver.  Name:  FriendlyName: Unsaved Replay";
    const LOBBY: &str = "[2026.11.10-18.29.15:469][213]LogLoad: LoadMap: /Game/Maps/Frontend?closed";
    const LOBBY_PLAYLIST: &str =
        "[2026.11.10-18.29.20:451][251]LogCsvProfiler: Display: Metadata set : playlistid=\"Playlist_FrontEnd\"";

    fn run(lines: &[&str]) -> Vec<Played> {
        let mut t = Tracker::default();
        lines.iter().filter_map(|l| t.line(l)).collect()
    }

    #[test]
    fn a_recorded_match() {
        let played = run(&[START, PLAYLIST, RECORDING, LOBBY, LOBBY_PLAYLIST]);
        assert_eq!(played.len(), 1);
        assert_eq!(played[0].playlist.as_deref(), Some("Playlist_NoBuildBR_Squad"));
        assert!(played[0].recorded && played[0].long_enough() && !played[0].missed_replay());
    }

    #[test]
    fn a_match_without_recording_is_a_missed_replay() {
        let played = run(&[START, PLAYLIST, LOBBY]);
        assert!(played[0].missed_replay());
    }

    #[test]
    fn short_visits_and_other_modes_are_not_reported_as_missed() {
        let quick = "[2026.11.10-18.15.10:000][1]LogLoad: LoadMap: /Game/Maps/Frontend?closed";
        assert!(!run(&[START, PLAYLIST, quick])[0].missed_replay());
        let creative = START.replace("Delilah/Maps/Delilah_Terrain", "Creative/Maps/Island");
        let playground = PLAYLIST.replace("Playlist_NoBuildBR_Squad", "Playlist_PlaygroundV2");
        assert!(!run(&[&creative, &playground, LOBBY])[0].missed_replay());
        // No playlist seen: unknown mode, no warning.
        assert!(!run(&[START, LOBBY])[0].missed_replay());
        // Default Battle Royale counts.
        let builds = PLAYLIST.replace("Playlist_NoBuildBR_Squad", "Playlist_DefaultDuo");
        assert!(run(&[START, &builds, LOBBY])[0].missed_replay());
    }

    #[test]
    fn the_playlist_also_comes_from_the_replicated_playlist_line() {
        let line = "[2026.11.06-01.27.07:145][ 60]LogFort: PLAYLIST: Playlist Object replicated to client in \
                    AFortGameStateAthena::OnRep_CurrentPlaylistInfo() PlaylistName is Playlist_DefaultSquad";
        let played = run(&[START, line, LOBBY]);
        assert_eq!(played[0].playlist.as_deref(), Some("Playlist_DefaultSquad"));
        assert!(played[0].missed_replay());
    }

    #[test]
    fn the_lobby_alone_is_no_match() {
        assert!(run(&[LOBBY, LOBBY_PLAYLIST, RECORDING]).is_empty());
    }

    #[test]
    fn a_match_left_without_a_lobby_is_dropped() {
        let played = run(&[START, PLAYLIST, START, PLAYLIST, RECORDING, LOBBY]);
        assert_eq!(played.len(), 1);
        assert!(played[0].recorded);
    }

    #[test]
    fn a_match_in_progress_at_start_is_found_in_the_log_so_far() {
        let dir = std::env::temp_dir().join(format!("fcc-gamelog-start-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("FortniteGame.log");
        let later = START.replace("18.14.30", "18.40.00");
        std::fs::write(&path, format!("{START}\n{RECORDING}\n{LOBBY}\n{later}\n")).unwrap();
        let tail = Tail::new(path.clone());
        let mut tracker = Tracker::default();
        for line in tail.before() {
            tracker.line(&line);
        }
        assert_eq!(tracker.joined(), stamp(&later));

        std::fs::write(&path, format!("{START}\n{LOBBY}\n")).unwrap();
        let mut tracker = Tracker::default();
        for line in Tail::new(path).before() {
            tracker.line(&line);
        }
        assert_eq!(tracker.joined(), None, "back in the lobby: no match running");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn tail_reads_only_whole_new_lines_and_restarts_on_a_new_file() {
        let dir = std::env::temp_dir().join(format!("fcc-gamelog-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("FortniteGame.log");
        std::fs::write(&path, "old line\n").unwrap();
        let mut tail = Tail::new(path.clone());
        assert!(tail.lines().is_empty(), "what was there at start is skipped");

        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        write!(f, "first\nsecond, half").unwrap();
        assert_eq!(tail.lines(), vec!["first"]);
        writeln!(f, " done").unwrap();
        assert_eq!(tail.lines(), vec!["second, half done"]);

        // The game started again with a shorter log.
        drop(f);
        std::fs::write(&path, "new\n").unwrap();
        assert_eq!(tail.lines(), vec!["new"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn finds_replays_written_since() {
        let dir = std::env::temp_dir().join(format!("fcc-gamelog-demos-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let before = SystemTime::now() - Duration::from_secs(5);
        assert!(!replay_written_since(&dir, before));
        std::fs::write(dir.join("UnsavedReplay-2026.11.11-12.00.00.replay"), b"x").unwrap();
        assert!(replay_written_since(&dir, before));
        assert!(!replay_written_since(&dir, SystemTime::now() + Duration::from_secs(60)));
        let _ = std::fs::remove_dir_all(dir);
    }
}
