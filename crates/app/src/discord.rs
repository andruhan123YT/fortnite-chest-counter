//! Discord status (Rich Presence): the chest count on the player's Discord profile.
//!
//! The app talks only to the Discord app on this PC, over the local pipe games use for
//! their status. It never signs in, never sees a token and cannot read or send messages.

use std::io::{Read, Write};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::app::ChestApp;
use crate::i18n::{native, tr, trf};

/// The Discord application whose name follows "Playing" on the profile.
pub const APP_ID: &str = "1571314481320173609";
/// Art asset of that application shown as the big picture.
const LOGO: &str = "discord-logo";
/// Art asset of a green dot, drawn on the picture's corner during a match.
const LIVE_DOT: &str = "in-match";

/// How often the status is worked out again.
const REFRESH: Duration = Duration::from_secs(1);
/// How often to look for Fortnite while the status depends on it.
const FORTNITE_CHECK_EVERY: Duration = Duration::from_secs(5);
/// Discord takes 5 status updates per 20 seconds.
const MIN_GAP: Duration = Duration::from_secs(5);
/// How long to wait before trying a Discord app that was not there again.
const RETRY: Duration = Duration::from_secs(15);
/// How often an unchanged status is sent again, for a Discord app that restarted.
const KEEP_ALIVE: Duration = Duration::from_secs(60);
/// A match that seems to run longer than this was left without the log saying so.
const LONGEST_MATCH: i64 = 3 * 3600;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiscordSettings {
    pub enabled: bool,
    pub total: bool,
    /// Today's chests and the last match, outside a match.
    pub today: bool,
    /// "In a match" with the time since the match loaded.
    pub timer: bool,
    /// Treasures and the Pirate Shop chance, on the big picture's tooltip.
    pub odds: bool,
    /// A button that takes friends to the app's GitHub page.
    pub button: bool,
    pub only_with_fortnite: bool,
    /// The member list shows the chest count rather than the app's name.
    pub count_in_list: bool,
}

impl Default for DiscordSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            total: true,
            today: true,
            timer: true,
            odds: false,
            button: true,
            only_with_fortnite: true,
            count_in_list: true,
        }
    }
}

/// What the profile shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Presence {
    pub details: Option<String>,
    pub state: Option<String>,
    /// Unix milliseconds the current match loaded at.
    pub start: Option<i64>,
    pub picture_hint: String,
    /// A green dot on the picture while a match is played.
    pub live: bool,
    pub button: bool,
    pub count_in_list: bool,
}

/// The numbers a status is made from.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Inputs {
    pub total: u64,
    pub today: u64,
    pub last_match: u64,
    pub treasures: u64,
    /// Chance of the Pirate Shop Access token so far, 0 to 1.
    pub pirate_shop: f64,
    pub in_match: bool,
    /// Unix milliseconds the current match loaded at, from the game's log.
    pub match_start: Option<i64>,
}

pub fn presence(n: &Inputs, s: &DiscordSettings) -> Presence {
    let in_match = n.in_match || n.match_start.is_some();
    let details = s.total.then(|| trf("{0} chests", &[&crate::theme::thousands(n.total)]));
    let state = if in_match {
        s.timer.then(|| tr("In a match").to_string())
    } else {
        s.today.then(|| {
            trf(
                "Today {0} · last match +{1}",
                &[&crate::theme::thousands(n.today), &crate::theme::thousands(n.last_match)],
            )
        })
    };
    let picture_hint = if s.odds {
        trf(
            "Treasures {0} · Pirate Shop chance {1}",
            &[&crate::theme::thousands(n.treasures), &crate::theme::percent(n.pirate_shop)],
        )
    } else {
        format!("Fortnite Chest Counter v{}", env!("CARGO_PKG_VERSION"))
    };
    // Discord lays out Arabic itself: plain text, not what the app draws.
    let (details, state, picture_hint) =
        (details.map(|d| native(&d)), state.map(|d| native(&d)), native(&picture_hint));
    Presence {
        start: if in_match && s.timer { n.match_start } else { None },
        live: in_match && s.timer,
        count_in_list: s.count_in_list && details.is_some(),
        details,
        state,
        picture_hint,
        button: s.button,
    }
}

/// The activity as Discord's `SET_ACTIVITY` takes it.
fn activity(p: &Presence) -> Value {
    let mut a = json!({
        "assets": { "large_image": LOGO, "large_text": p.picture_hint },
        // 2 shows `details` under the player's name in member lists; 0 the app's name.
        "status_display_type": if p.count_in_list { 2 } else { 0 },
    });
    if let Some(details) = &p.details {
        a["details"] = json!(details);
    }
    if let Some(state) = &p.state {
        a["state"] = json!(state);
    }
    if let Some(start) = p.start {
        a["timestamps"] = json!({ "start": start });
    }
    if p.live {
        a["assets"]["small_image"] = json!(LIVE_DOT);
        a["assets"]["small_text"] = json!(native(tr("In a match")));
    }
    if p.button {
        let url = format!("https://github.com/{}", crate::update::REPO);
        a["buttons"] = json!([{ "label": native(tr("Get Chest Counter")), "url": url }]);
    }
    a
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Status {
    #[default]
    Off,
    /// Nothing to show right now, e.g. Fortnite is not running.
    Idle,
    /// The Discord app is not running on this PC.
    NoDiscord,
    Shown,
    Error(String),
}

/// The status worker and what was last handed to it.
#[derive(Default)]
pub struct Discord {
    tx: Option<Sender<Option<Presence>>>,
    pub status: Arc<Mutex<Status>>,
    sent: Option<Option<Presence>>,
    refreshed: Option<Instant>,
    fortnite: bool,
    fortnite_checked: Option<Instant>,
}

impl Discord {
    pub fn status(&self) -> Status {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn send(&mut self, wanted: Option<Presence>) {
        if self.sent.as_ref() == Some(&wanted) {
            return;
        }
        if self.tx.is_none() && wanted.is_some() {
            let (tx, rx) = std::sync::mpsc::channel();
            let status = self.status.clone();
            if std::thread::Builder::new().name("discord".into()).spawn(move || work(rx, status)).is_ok() {
                self.tx = Some(tx);
            }
        }
        if let Some(tx) = &self.tx {
            if tx.send(wanted.clone()).is_err() {
                self.tx = None;
                return;
            }
        }
        self.sent = Some(wanted);
    }
}

pub fn update(app: &mut ChestApp, ctx: &eframe::egui::Context) {
    let s = app.settings.discord.clone();
    let on = s.enabled && app.settings.setup_done;
    if !on {
        app.discord.send(None);
        set(&app.discord.status, Status::Off);
        return;
    }
    ctx.request_repaint_after(FORTNITE_CHECK_EVERY);
    if app.discord.refreshed.is_some_and(|at| at.elapsed() < REFRESH) {
        return;
    }
    app.discord.refreshed = Some(Instant::now());
    if s.only_with_fortnite {
        let due = app.discord.fortnite_checked.is_none_or(|at| at.elapsed() >= FORTNITE_CHECK_EVERY);
        if due {
            app.discord.fortnite = crate::platform::fortnite_running();
            app.discord.fortnite_checked = Some(Instant::now());
        }
    }
    let wanted = (!s.only_with_fortnite || app.discord.fortnite).then(|| presence(&inputs(app), &s));
    if wanted.is_none() {
        set(&app.discord.status, Status::Idle);
    }
    app.discord.send(wanted);
}

fn inputs(app: &ChestApp) -> Inputs {
    use chests::odds::{chance_at_least_one, PIRATE_SHOP_ACCESS};
    let n = crate::overlay::numbers(app);
    let now = chrono::Utc::now();
    Inputs {
        total: n.total,
        today: n.today,
        last_match: n.last_match,
        treasures: n.treasures,
        pirate_shop: chance_at_least_one(app.totals.pirate_shop_chests(true), PIRATE_SHOP_ACCESS.one_in),
        in_match: n.in_match,
        match_start: app
            .match_joined
            .filter(|at| (0..LONGEST_MATCH).contains(&(now - *at).num_seconds()))
            .map(|at| at.timestamp_millis()),
    }
}

fn failure(e: &std::io::Error) -> Status {
    use std::io::ErrorKind::{BrokenPipe, NotFound, UnexpectedEof};
    match e.kind() {
        NotFound | BrokenPipe | UnexpectedEof => Status::NoDiscord,
        _ => Status::Error(e.to_string()),
    }
}

fn set(status: &Mutex<Status>, value: Status) {
    *status.lock().unwrap_or_else(|e| e.into_inner()) = value;
}

/// Keeps the Discord app showing the latest status handed over, reconnecting when
/// Discord starts later. Closing the pipe clears the status.
fn work(rx: Receiver<Option<Presence>>, status: Arc<Mutex<Status>>) {
    let mut wanted: Option<Presence> = None;
    let mut shown: Option<Presence> = None;
    let mut conn: Option<Conn<std::fs::File>> = None;
    let mut last_sent: Option<Instant> = None;
    let mut retry_at: Option<Instant> = None;
    loop {
        let now = Instant::now();
        let mut wait = KEEP_ALIVE;
        if wanted.is_some() && wanted != shown {
            let gap_left = last_sent.map_or(Duration::ZERO, |at| MIN_GAP.saturating_sub(at.elapsed()));
            let retry_left = retry_at.map_or(Duration::ZERO, |at| at.saturating_duration_since(now));
            wait = gap_left.max(retry_left);
            if wait.is_zero() {
                if conn.is_none() {
                    match Conn::open() {
                        Ok(c) => conn = Some(c),
                        Err(e) => {
                            set(&status, failure(&e));
                            retry_at = Some(Instant::now() + RETRY);
                        }
                    }
                }
                if let (Some(c), Some(p)) = (conn.as_mut(), wanted.as_ref()) {
                    last_sent = Some(Instant::now());
                    match c.set_activity(&activity(p)) {
                        Ok(()) => {
                            shown = wanted.clone();
                            retry_at = None;
                            set(&status, Status::Shown);
                        }
                        Err(e) => {
                            conn = None;
                            shown = None;
                            retry_at = Some(Instant::now() + RETRY);
                            set(&status, failure(&e));
                        }
                    }
                }
                wait = if wanted == shown { KEEP_ALIVE } else { MIN_GAP };
            }
        }
        match rx.recv_timeout(wait) {
            Ok(next) => {
                wanted = next;
                if wanted.is_none() {
                    conn = None;
                    shown = None;
                }
            }
            // A Discord app restarted meanwhile lost the status: send it again now and then.
            Err(RecvTimeoutError::Timeout) => shown = None,
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// Discord's local IPC: little-endian opcode and length, then JSON.
struct Conn<T: Read + Write> {
    pipe: T,
    nonce: u64,
}

const OP_HANDSHAKE: u32 = 0;
const OP_FRAME: u32 = 1;
const OP_CLOSE: u32 = 2;
const OP_PING: u32 = 3;
const OP_PONG: u32 = 4;
/// Replies larger than this are not Discord's.
const MAX_FRAME: u32 = 1 << 20;

impl Conn<std::fs::File> {
    /// Connects to the first Discord app found: it listens on `discord-ipc-0` to `-9`.
    fn open() -> std::io::Result<Self> {
        let mut last = std::io::Error::from(std::io::ErrorKind::NotFound);
        for i in 0..10 {
            let path = format!(r"\\.\pipe\discord-ipc-{i}");
            match std::fs::OpenOptions::new().read(true).write(true).open(path) {
                Ok(pipe) => {
                    let mut conn = Conn { pipe, nonce: 0 };
                    conn.handshake()?;
                    return Ok(conn);
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => last = e,
            }
        }
        Err(last)
    }
}

impl<T: Read + Write> Conn<T> {
    fn handshake(&mut self) -> std::io::Result<()> {
        self.write(OP_HANDSHAKE, &json!({ "v": 1, "client_id": APP_ID }))?;
        let reply = self.reply()?;
        if reply["evt"] == "READY" {
            Ok(())
        } else {
            Err(refused(&reply))
        }
    }

    fn set_activity(&mut self, activity: &Value) -> std::io::Result<()> {
        self.nonce += 1;
        let nonce = self.nonce.to_string();
        let command = json!({
            "cmd": "SET_ACTIVITY",
            "args": { "pid": std::process::id(), "activity": activity },
            "nonce": nonce,
        });
        self.write(OP_FRAME, &command)?;
        loop {
            let reply = self.reply()?;
            if reply["nonce"] != nonce.as_str() {
                continue;
            }
            return if reply["evt"] == "ERROR" { Err(refused(&reply)) } else { Ok(()) };
        }
    }

    fn write(&mut self, op: u32, body: &Value) -> std::io::Result<()> {
        let body = body.to_string();
        let mut frame = Vec::with_capacity(8 + body.len());
        frame.extend_from_slice(&op.to_le_bytes());
        frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
        frame.extend_from_slice(body.as_bytes());
        self.pipe.write_all(&frame)?;
        self.pipe.flush()
    }

    /// The next frame Discord sends, answering its pings on the way.
    fn reply(&mut self) -> std::io::Result<Value> {
        loop {
            let mut header = [0u8; 8];
            self.pipe.read_exact(&mut header)?;
            let op = u32::from_le_bytes(header[..4].try_into().unwrap_or_default());
            let len = u32::from_le_bytes(header[4..].try_into().unwrap_or_default());
            if len > MAX_FRAME {
                return Err(std::io::Error::other("Discord sent an oversized frame"));
            }
            let mut body = vec![0u8; len as usize];
            self.pipe.read_exact(&mut body)?;
            let value: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            match op {
                OP_PING => self.write(OP_PONG, &value)?,
                OP_CLOSE => return Err(refused(&value)),
                _ => return Ok(value),
            }
        }
    }
}

fn refused(reply: &Value) -> std::io::Error {
    let message = reply["data"]["message"].as_str().or(reply["message"].as_str()).unwrap_or("refused");
    std::io::Error::other(format!("Discord: {message}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbers() -> Inputs {
        Inputs {
            total: 5027,
            today: 260,
            last_match: 49,
            treasures: 93,
            pirate_shop: 0.0744,
            in_match: false,
            match_start: None,
        }
    }

    #[test]
    fn shows_the_total_and_today_in_the_lobby() {
        let p = presence(&numbers(), &DiscordSettings::default());
        assert_eq!(p.details.as_deref(), Some("5,027 chests"));
        assert_eq!(p.state.as_deref(), Some("Today 260 · last match +49"));
        assert_eq!(p.start, None);
        assert!(p.count_in_list && p.button);
    }

    #[test]
    fn shows_a_timer_in_a_match() {
        let n = Inputs { match_start: Some(1_763_000_000_000), ..numbers() };
        let p = presence(&n, &DiscordSettings::default());
        assert_eq!(p.state.as_deref(), Some("In a match"));
        assert_eq!(p.start, Some(1_763_000_000_000));
        assert!(p.live);
        assert!(!presence(&numbers(), &DiscordSettings::default()).live, "no dot in the lobby");
        let off = presence(&n, &DiscordSettings { timer: false, ..Default::default() });
        assert_eq!((off.state, off.start), (None, None));
    }

    #[test]
    fn leaves_out_what_the_player_turned_off() {
        let s = DiscordSettings { total: false, today: false, button: false, odds: true, ..Default::default() };
        let p = presence(&numbers(), &s);
        assert_eq!((p.details.as_deref(), p.state.as_deref()), (None, None));
        assert!(!p.count_in_list, "nothing to put in the member list but the app's name");
        assert_eq!(p.picture_hint, "Treasures 93 · Pirate Shop chance 7.44%");
        let a = activity(&p);
        assert!(a.get("buttons").is_none() && a.get("details").is_none());
        assert_eq!(a["status_display_type"], 0);
    }

    #[test]
    fn builds_the_activity_discord_takes() {
        let n = Inputs { match_start: Some(1_763_000_000_000), ..numbers() };
        let a = activity(&presence(&n, &DiscordSettings::default()));
        assert_eq!(a["details"], "5,027 chests");
        assert_eq!(a["timestamps"]["start"], 1_763_000_000_000i64);
        assert_eq!(a["assets"]["small_image"], LIVE_DOT);
        assert_eq!(a["assets"]["small_text"], "In a match");
        assert_eq!(a["assets"]["large_image"], LOGO);
        assert_eq!(a["status_display_type"], 2);
        let button = &a["buttons"][0];
        assert_eq!(button["label"], "Get Chest Counter");
        assert_eq!(button["url"], format!("https://github.com/{}", crate::update::REPO));
    }

    /// A pipe with Discord's replies queued up, recording what the app writes.
    struct FakePipe {
        input: std::io::Cursor<Vec<u8>>,
        output: Vec<u8>,
    }

    impl Read for FakePipe {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.input.read(buf)
        }
    }

    impl Write for FakePipe {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.output.write(buf)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn frame(op: u32, body: Value) -> Vec<u8> {
        let body = body.to_string();
        let mut f = op.to_le_bytes().to_vec();
        f.extend_from_slice(&(body.len() as u32).to_le_bytes());
        f.extend_from_slice(body.as_bytes());
        f
    }

    fn frames(mut bytes: &[u8]) -> Vec<(u32, Value)> {
        let mut out = Vec::new();
        while bytes.len() >= 8 {
            let op = u32::from_le_bytes(bytes[..4].try_into().unwrap());
            let len = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
            out.push((op, serde_json::from_slice(&bytes[8..8 + len]).unwrap()));
            bytes = &bytes[8 + len..];
        }
        out
    }

    #[test]
    fn talks_discords_pipe_protocol() {
        let mut input = frame(OP_FRAME, json!({ "cmd": "DISPATCH", "evt": "READY" }));
        input.extend(frame(OP_PING, json!({ "n": 1 })));
        input.extend(frame(OP_FRAME, json!({ "cmd": "SET_ACTIVITY", "evt": null, "nonce": "1" })));
        let mut conn = Conn { pipe: FakePipe { input: std::io::Cursor::new(input), output: Vec::new() }, nonce: 0 };
        conn.handshake().unwrap();
        conn.set_activity(&json!({ "details": "5,027 chests" })).unwrap();
        let sent = frames(&conn.pipe.output);
        assert_eq!(sent[0], (OP_HANDSHAKE, json!({ "v": 1, "client_id": APP_ID })));
        assert_eq!(sent[1].0, OP_FRAME);
        assert_eq!(sent[1].1["cmd"], "SET_ACTIVITY");
        assert_eq!(sent[1].1["args"]["activity"]["details"], "5,027 chests");
        assert_eq!(sent[2], (OP_PONG, json!({ "n": 1 })), "a ping is answered with the same payload");
    }

    #[test]
    fn reports_what_discord_refuses() {
        let input = frame(OP_CLOSE, json!({ "code": 4000, "message": "Invalid Client ID" }));
        let mut conn = Conn { pipe: FakePipe { input: std::io::Cursor::new(input), output: Vec::new() }, nonce: 0 };
        let error = conn.handshake().unwrap_err();
        assert_eq!(error.to_string(), "Discord: Invalid Client ID");
    }
}
