//! Online mode: the app shares its player's own chest count per match with the sync
//! server, and gets back the counts of squadmates who run the app too. A squadmate's own
//! replay sees every chest they loot, while ours only sees the ones near us.
//!
//! What is sent: the recorder's Epic account id and name, and per match its id, start
//! time, opened and broken chests, and each of those chests: when, what kind and where.
//! To get a squadmate's chests the app needs the match id and their Epic account id, both
//! read from the user's own replay of it.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use chests::{same_name, ChestEvent, ChestKind, PlayerChests};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use crate::config::{data_dir, load_json, write_json_atomically};
use crate::record::MatchRecord;

pub const SERVER: &str = "https://chests.l4ve.dev";
/// Replaces the server address, to try the app against a local server.
#[cfg(debug_assertions)]
const SERVER_ENV: &str = "FCC_SYNC_SERVER";
/// How often the app looks for something to send or ask.
pub const TICK: Duration = Duration::from_secs(60);
/// Matches per upload. The server takes up to 500, but with every chest listed that is a
/// couple of megabytes, past what it reads in one request.
const MAX_UPLOAD: usize = 50;
/// The server's limit per lookup.
const MAX_LOOKUP: usize = 300;

/// One chest as it goes over the wire: short names, positions in whole centimeters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chest {
    /// Replay seconds.
    pub t: f32,
    pub k: ChestKind,
    /// Opened; false when broken.
    pub o: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub v: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub l: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<i32>,
}

impl Chest {
    fn of(e: &ChestEvent, opened: bool) -> Self {
        let cm = |v: Option<f64>| v.filter(|v| v.is_finite()).map(|v| v.round() as i32);
        Self { t: e.time, k: e.kind, o: opened, v: e.in_vehicle, l: e.from_loot, x: cm(e.x), y: cm(e.y) }
    }

    pub fn event(&self) -> ChestEvent {
        ChestEvent {
            time: self.t,
            kind: self.k,
            in_vehicle: self.v,
            x: self.x.map(f64::from),
            y: self.y.map(f64::from),
            from_loot: self.l,
        }
    }
}

/// A player's chests in a match, in the order they happened.
fn chests_of(p: &PlayerChests) -> Vec<Chest> {
    p.in_order().into_iter().map(|(e, opened)| Chest::of(e, opened)).collect()
}

/// A squadmate's own count of their chests in a match.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Found {
    #[serde(rename = "match")]
    pub match_id: String,
    /// Epic account id.
    pub player: String,
    pub name: String,
    pub opened: u32,
    pub broken: u32,
    /// Replay seconds of the bus launch in the squadmate's replay. Their app sends it and
    /// `chests` since 1.1.6.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bus: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chests: Option<Vec<Chest>>,
}

impl Found {
    pub fn total(&self) -> usize {
        (self.opened + self.broken) as usize
    }

    /// The squadmate's chests from their own replay, when their app sent them.
    pub fn player(&self) -> Option<PlayerChests> {
        let chests = self.chests.as_ref()?;
        let (opened, broken) = chests.iter().partition::<Vec<&Chest>, _>(|c| c.o);
        Some(PlayerChests {
            name: self.name.clone(),
            epic_id: Some(self.player.clone()),
            opened: opened.into_iter().map(Chest::event).collect(),
            broken: broken.into_iter().map(Chest::event).collect(),
            ..Default::default()
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct UploadMatch {
    #[serde(rename = "match")]
    match_id: String,
    /// Unix seconds.
    started: i64,
    opened: u32,
    broken: u32,
    bus: Option<f32>,
    chests: Vec<Chest>,
}

impl UploadMatch {
    /// What the server holds once this went out, to tell when the match changed.
    fn fingerprint(&self) -> u64 {
        fnv(&serde_json::to_vec(&(self.opened, self.broken, self.bus, &self.chests)).unwrap_or_default())
    }
}

/// FNV-1a: the same on every run and every build, unlike the standard library's hasher.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct Upload {
    player: String,
    name: String,
    install: String,
    matches: Vec<UploadMatch>,
}

/// The count the app shows, so the server's admin page shows the same number.
#[derive(Debug, Clone, PartialEq, Serialize)]
struct TotalUpload {
    player: String,
    install: String,
    total: u64,
    /// Hide the player's name in the leaderboard.
    anonymous: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct LookupMatch {
    #[serde(rename = "match")]
    match_id: String,
    players: Vec<String>,
}

#[derive(Serialize)]
struct Lookup<'a> {
    matches: &'a [LookupMatch],
}

#[derive(Deserialize)]
struct LookupReply {
    found: Vec<Found>,
}

/// Kept between runs in `online.json`.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    /// `UploadMatch::fingerprint` of what went out last for each match. A match missing
    /// here goes out again, chests and all, so matches sent by 1.1.5 and earlier get their
    /// chests too.
    sent: HashMap<String, u64>,
    found: Vec<Found>,
    /// Unix seconds each match was last asked about.
    asked: HashMap<String, i64>,
    /// The total the server has from this app.
    total_sent: Option<u64>,
    /// The leaderboard setting the server has from this app.
    anonymous_sent: Option<bool>,
}

/// One round of work for the network thread.
#[derive(Debug, Default, PartialEq)]
struct Job {
    uploads: Vec<Upload>,
    lookup: Vec<LookupMatch>,
    total: Option<TotalUpload>,
}

impl Job {
    fn is_empty(&self) -> bool {
        self.uploads.is_empty() && self.lookup.is_empty() && self.total.is_none()
    }
}

#[derive(Debug, Default)]
struct Outcome {
    /// Match id and `UploadMatch::fingerprint`.
    uploaded: Vec<(String, u64)>,
    asked: Vec<String>,
    found: Vec<Found>,
    total_sent: Option<(u64, bool)>,
    error: Option<String>,
}

fn is_id(text: &str) -> bool {
    text.len() == 32 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Match start times are the recording PC's clock, without a time zone.
fn unix(at: NaiveDateTime) -> i64 {
    at.and_local_timezone(chrono::Local).earliest().map_or_else(|| at.and_utc().timestamp(), |t| t.timestamp())
}

/// Matches whose squadmates were all found are asked about again this often, for as
/// long as `RECHECK_FOUND_FOR` after the match.
const RECHECK_FOUND_EVERY: i64 = 12 * 3600;
const RECHECK_FOUND_FOR: i64 = 3 * 86_400;

/// How long to wait before asking about a match again. Squadmates' apps send their
/// numbers once their replay is done, usually minutes after the match, sometimes when
/// they next start the game.
fn ask_every(age_secs: i64) -> i64 {
    if age_secs < 3 * 3600 {
        120
    } else if age_secs < 2 * 86_400 {
        1800
    } else {
        12 * 3600
    }
}

/// What to send and ask this round, newest matches first. Only matches of the player's
/// own `accounts` go out; `total` and `anonymous` belong to the first, the active one.
fn plan(
    matches: &[MatchRecord],
    saved: &Saved,
    install: &str,
    accounts: &[&str],
    total: u64,
    anonymous: bool,
    now: i64,
) -> Job {
    let mut uploads: BTreeMap<(String, String), Vec<UploadMatch>> = BTreeMap::new();
    let mut lookup = Vec::new();
    let mut newest_first: Vec<&MatchRecord> = matches.iter().collect();
    newest_first.sort_by_key(|m| std::cmp::Reverse(m.started_at));
    for m in newest_first {
        let Some(match_id) = m.match_id.as_deref().filter(|id| is_id(id)) else { continue };
        let Some(me) = m.recorder() else { continue };
        let Some(my_id) = me.epic_id.as_deref().filter(|id| is_id(id) && accounts.contains(id)) else { continue };
        let upload = UploadMatch {
            match_id: match_id.to_owned(),
            started: m.started_at.map_or(0, unix),
            opened: me.opened.len() as u32,
            broken: me.broken.len() as u32,
            bus: m.bus_launch,
            chests: chests_of(me),
        };
        if saved.sent.get(match_id) != Some(&upload.fingerprint()) {
            uploads.entry((my_id.to_owned(), me.name.clone())).or_default().push(upload);
        }

        let Some(team) = me.team else { continue };
        let mates: Vec<String> = m
            .players
            .iter()
            .filter(|p| p.team == Some(team) && !p.is_bot && !same_name(&p.name, &me.name))
            .filter_map(|p| p.epic_id.clone().filter(|id| is_id(id)))
            .collect();
        // Found without chests: their app may send them yet, after an update.
        let known =
            |id: &String| saved.found.iter().any(|f| f.match_id == match_id && &f.player == id && f.chests.is_some());
        if mates.is_empty() || lookup.len() >= MAX_LOOKUP {
            continue;
        }
        let age = m.started_at.map_or(i64::MAX, |at| now - unix(at));
        let every = if !mates.iter().all(known) {
            ask_every(age)
        } else if age < RECHECK_FOUND_FOR {
            // Everyone is found, but a squadmate's app may still count their replay again,
            // say after an update to the analysis.
            RECHECK_FOUND_EVERY
        } else {
            continue;
        };
        let last = saved.asked.get(match_id).copied().unwrap_or(i64::MIN / 2);
        if now - last >= every {
            lookup.push(LookupMatch { match_id: match_id.to_owned(), players: mates });
        }
    }
    let uploads = uploads
        .into_iter()
        .flat_map(|((player, name), matches)| {
            matches
                .chunks(MAX_UPLOAD)
                .map(|chunk| Upload {
                    player: player.clone(),
                    name: name.clone(),
                    install: install.to_owned(),
                    matches: chunk.to_vec(),
                })
                .collect::<Vec<_>>()
        })
        .collect();
    let changed = saved.total_sent != Some(total) || saved.anonymous_sent != Some(anonymous);
    let total = accounts.first().filter(|id| changed && is_id(id)).map(|player| TotalUpload {
        player: (*player).to_owned(),
        install: install.to_owned(),
        total,
        anonymous,
    });
    Job { uploads, lookup, total }
}

fn server() -> String {
    #[cfg(debug_assertions)]
    if let Ok(url) = std::env::var(SERVER_ENV) {
        return url.trim_end_matches('/').to_owned();
    }
    SERVER.to_owned()
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .user_agent(format!("fortnite-chest-counter/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// Why a request failed, and whether sending the same thing again can help.
#[derive(Debug)]
struct Failure {
    message: String,
    /// The server refused this very request (4xx other than "busy"): it would refuse it
    /// again.
    refused: bool,
}

fn post(agent: &ureq::Agent, path: &str, body: &impl Serialize) -> Result<String, Failure> {
    let failed = |message: String| Failure { message, refused: false };
    let body = serde_json::to_vec(body).map_err(|e| failed(e.to_string()))?;
    let mut response =
        agent.post(format!("{}{path}", server())).header("Content-Type", "application/json").send(&body[..]).map_err(
            |e| match e {
                ureq::Error::StatusCode(429) => failed("the server is busy, will try again".to_string()),
                ureq::Error::StatusCode(code) => {
                    Failure { message: format!("the server answered {code}"), refused: (400..500).contains(&code) }
                }
                e => failed(e.to_string()),
            },
        )?;
    response.body_mut().read_to_string().map_err(|e| failed(e.to_string()))
}

fn work(agent: &ureq::Agent, job: Job) -> Outcome {
    let mut out = Outcome::default();
    for upload in &job.uploads {
        match post(agent, "/v1/matches", upload) {
            Ok(_) => {}
            // A batch the server will never take is skipped until its counts change, so
            // the rest still goes out.
            Err(e) if e.refused => {
                crate::config::log_event(&format!(
                    "the server refused {} matches: {}",
                    upload.matches.len(),
                    e.message
                ));
                out.error = Some(e.message);
            }
            Err(e) => {
                out.error = Some(e.message);
                return out;
            }
        }
        out.uploaded.extend(upload.matches.iter().map(|m| (m.match_id.clone(), m.fingerprint())));
    }
    if let Some(total) = &job.total {
        match post(agent, "/v1/total", total) {
            Ok(_) => out.total_sent = Some((total.total, total.anonymous)),
            Err(e) if e.refused => {
                crate::config::log_event(&format!("the server refused the total: {}", e.message));
                out.total_sent = Some((total.total, total.anonymous));
                out.error = Some(e.message);
            }
            Err(e) => {
                out.error = Some(e.message);
                return out;
            }
        }
    }
    if job.lookup.is_empty() {
        return out;
    }
    let reply = post(agent, "/v1/lookup", &Lookup { matches: &job.lookup })
        .map_err(|e| e.message)
        .and_then(|text| serde_json::from_str::<LookupReply>(&text).map_err(|e| e.to_string()));
    match reply {
        Ok(reply) => {
            out.found = reply.found;
            out.asked = job.lookup.into_iter().map(|l| l.match_id).collect();
        }
        Err(e) => out.error = Some(e),
    }
    out
}

/// One player in the leaderboard.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LeaderEntry {
    pub name: String,
    pub matches: u32,
    /// Opened and broken.
    pub chests: u64,
    /// The most chests in one match.
    pub best: u32,
    /// The player running this app.
    pub me: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Leaderboard {
    /// Unix seconds this week and today began, by the server's clock (00:00 UTC).
    pub week_start: i64,
    pub day_start: i64,
    pub all: Vec<LeaderEntry>,
    pub week: Vec<LeaderEntry>,
    pub today: Vec<LeaderEntry>,
}

#[derive(Serialize)]
struct LeaderboardAsk<'a> {
    me: &'a str,
    /// The server marks `me` only when this installation uploads that player's matches.
    install: &'a str,
}

/// Asks the server for the leaderboard; blocks, so call it off the UI thread. `me` marks
/// the player's own row.
pub fn fetch_leaderboard(me: Option<&str>, install: &str) -> Result<Leaderboard, String> {
    let text =
        post(&agent(), "/v1/leaderboard", &LeaderboardAsk { me: me.unwrap_or(""), install }).map_err(|e| e.message)?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Default)]
pub struct Status {
    pub last_ok: Option<chrono::DateTime<chrono::Local>>,
    pub error: Option<String>,
}

/// The online mode's state on the UI thread; the network calls run on a thread of their own.
pub struct Sync {
    jobs: Sender<Job>,
    outcomes: Receiver<Outcome>,
    path: PathBuf,
    saved: Saved,
    busy: bool,
    next: Instant,
    pub status: Status,
}

impl Sync {
    pub fn default_path() -> PathBuf {
        data_dir().join("online.json")
    }

    pub fn start(path: PathBuf, repaint: impl Fn() + Send + 'static) -> Self {
        let mut saved: Saved = load_json(&path).value;
        // Squadmates may have started the app while this one was closed: the first round
        // asks about every match still missing someone, whenever it was last asked.
        saved.asked.clear();
        let (jobs, job_rx) = mpsc::channel::<Job>();
        let (outcome_tx, outcomes) = mpsc::channel();
        std::thread::Builder::new()
            .name("online".into())
            .spawn(move || {
                let agent = agent();
                for job in job_rx {
                    let _ = outcome_tx.send(work(&agent, job));
                    repaint();
                }
            })
            .expect("spawn the online thread");
        // The first round waits a little so it does not compete with the startup scan.
        let next = Instant::now() + Duration::from_secs(5);
        Self { jobs, outcomes, path, saved, busy: false, next, status: Status::default() }
    }

    /// Squadmates' counts known so far.
    pub fn found(&self) -> Vec<Found> {
        self.saved.found.clone()
    }

    /// Different squadmates with the app found so far.
    pub fn squadmates(&self) -> usize {
        self.saved.found.iter().map(|f| f.player.as_str()).collect::<std::collections::HashSet<_>>().len()
    }

    /// Run a round soon, e.g. after a new match came in.
    pub fn nudge(&mut self) {
        self.next = self.next.min(Instant::now() + Duration::from_secs(2));
    }

    /// Called every frame: takes back what the network thread did and hands it the next
    /// round when one is due. True when squadmates' counts changed.
    pub fn tick(
        &mut self,
        enabled: bool,
        matches: &[MatchRecord],
        install: &str,
        accounts: &[&str],
        (total, anonymous): (u64, bool),
    ) -> bool {
        let mut changed = false;
        while let Ok(out) = self.outcomes.try_recv() {
            self.busy = false;
            changed |= self.merge(out);
        }
        if enabled && !self.busy && Instant::now() >= self.next {
            self.next = Instant::now() + TICK;
            let now = chrono::Utc::now().timestamp();
            let job = plan(matches, &self.saved, install, accounts, total, anonymous, now);
            if !job.is_empty() && self.jobs.send(job).is_ok() {
                self.busy = true;
            }
        }
        changed
    }

    fn merge(&mut self, out: Outcome) -> bool {
        let now = chrono::Utc::now().timestamp();
        match out.error {
            Some(e) => self.status.error = Some(e),
            None => {
                self.status.error = None;
                self.status.last_ok = Some(chrono::Local::now());
            }
        }
        self.saved.sent.extend(out.uploaded);
        if let Some((total, anonymous)) = out.total_sent {
            self.saved.total_sent = Some(total);
            self.saved.anonymous_sent = Some(anonymous);
        }
        for id in out.asked {
            self.saved.asked.insert(id, now);
        }
        let mut changed = false;
        for f in out.found {
            match self.saved.found.iter_mut().find(|g| g.match_id == f.match_id && g.player == f.player) {
                Some(g) if *g == f => {}
                Some(g) => {
                    *g = f;
                    changed = true;
                }
                None => {
                    self.saved.found.push(f);
                    changed = true;
                }
            }
        }
        if let Err(e) = write_json_atomically(&self.path, &self.saved) {
            crate::config::log_event(&format!("could not save online.json: {e}"));
        }
        changed
    }
}

/// A random id for this installation, sent with uploads.
pub fn new_install_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    (0..2u64)
        .map(|i| {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u64(i);
            h.write_u128(nanos);
            h.write_u32(std::process::id());
            format!("{:016x}", h.finish())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: &str = "0123456789abcdef0123456789abcdef";
    const MATE: &str = "fedcba9876543210fedcba9876543210";
    const TOTAL: u64 = 1005;
    const STRANGER: &str = "00112233445566778899aabbccddeeff";

    fn ev() -> chests::ChestEvent {
        chests::ChestEvent {
            time: 1.0,
            kind: chests::ChestKind::Normal,
            in_vehicle: false,
            x: None,
            y: None,
            from_loot: false,
        }
    }

    fn player(name: &str, id: &str, team: u32, opened: usize) -> PlayerChests {
        PlayerChests {
            name: name.into(),
            team: Some(team),
            epic_id: Some(id.into()),
            opened: vec![ev(); opened],
            ..Default::default()
        }
    }

    fn record(match_id: &str, started: &str) -> MatchRecord {
        MatchRecord {
            id: match_id.into(),
            match_id: Some(match_id.into()),
            owner: Some("Me".into()),
            started_at: NaiveDateTime::parse_from_str(started, "%Y-%m-%d %H:%M").ok(),
            players: vec![player("Me", ME, 3, 5), player("Mate", MATE, 3, 1), player("Other", STRANGER, 4, 2)],
            ..Default::default()
        }
    }

    /// Notes `m` as uploaded the way `merge` does after the server took it.
    fn mark_sent(saved: &mut Saved, m: &MatchRecord) {
        let job = plan(std::slice::from_ref(m), &Saved::default(), "i", &[ME], TOTAL, false, 0);
        let sent = &job.uploads[0].matches[0];
        saved.sent.insert(sent.match_id.clone(), sent.fingerprint());
    }

    fn found(match_id: &str, opened: u32, chests: Option<Vec<Chest>>) -> Found {
        Found {
            match_id: match_id.into(),
            player: MATE.into(),
            name: "Mate".into(),
            opened,
            broken: 1,
            bus: None,
            chests,
        }
    }

    fn at(text: &str) -> i64 {
        unix(NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M").unwrap())
    }

    #[test]
    fn sends_own_counts_and_asks_about_squadmates_only() {
        let m = record("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "2026-11-09 18:00");
        let job = plan(&[m], &Saved::default(), "inst", &[ME], TOTAL, false, at("2026-11-09 18:30"));
        assert_eq!(job.uploads.len(), 1);
        assert_eq!((job.uploads[0].player.as_str(), job.uploads[0].name.as_str()), (ME, "Me"));
        assert_eq!(job.uploads[0].matches[0].opened, 5);
        assert_eq!(
            job.lookup,
            vec![LookupMatch { match_id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(), players: vec![MATE.into()] }]
        );
    }

    #[test]
    fn does_not_repeat_itself() {
        let id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let m = record(id, "2026-11-09 18:00");
        let mut saved = Saved { total_sent: Some(TOTAL), anonymous_sent: Some(false), ..Default::default() };
        mark_sent(&mut saved, &m);
        saved.asked.insert(id.into(), at("2026-11-09 18:29"));
        // Uploaded with the same numbers and asked a minute ago: nothing to do yet.
        assert!(plan(std::slice::from_ref(&m), &saved, "i", &[ME], TOTAL, false, at("2026-11-09 18:30")).is_empty());
        // Two minutes on, a fresh match is asked about again.
        assert_eq!(
            plan(std::slice::from_ref(&m), &saved, "i", &[ME], TOTAL, false, at("2026-11-09 18:31")).lookup.len(),
            1
        );
        // A day old, it waits half an hour between asks.
        saved.asked.insert(id.into(), at("2026-11-10 18:30"));
        assert!(plan(std::slice::from_ref(&m), &saved, "i", &[ME], TOTAL, false, at("2026-11-10 18:45"))
            .lookup
            .is_empty());
        assert_eq!(
            plan(std::slice::from_ref(&m), &saved, "i", &[ME], TOTAL, false, at("2026-11-10 19:00")).lookup.len(),
            1
        );
        // Once every squadmate is found, twice a day for three days, then never again.
        saved.found.push(found(id, 9, Some(Vec::new())));
        assert!(plan(std::slice::from_ref(&m), &saved, "i", &[ME], TOTAL, false, at("2026-11-11 06:00")).is_empty());
        assert_eq!(
            plan(std::slice::from_ref(&m), &saved, "i", &[ME], TOTAL, false, at("2026-11-11 06:30")).lookup.len(),
            1
        );
        saved.asked.insert(id.into(), at("2026-11-12 17:00"));
        assert!(plan(&[m], &saved, "i", &[ME], TOTAL, false, at("2026-11-13 06:00")).is_empty());
    }

    #[test]
    fn the_total_goes_once_and_again_when_it_changes() {
        let id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let m = record(id, "2026-11-09 18:00");
        let job = plan(std::slice::from_ref(&m), &Saved::default(), "i", &[ME], 3631, false, at("2026-11-09 18:30"));
        assert_eq!(
            job.total,
            Some(TotalUpload { player: ME.into(), install: "i".into(), total: 3631, anonymous: false })
        );
        let saved = Saved { total_sent: Some(3631), anonymous_sent: Some(false), ..Default::default() };
        assert!(plan(std::slice::from_ref(&m), &saved, "i", &[ME], 3631, false, at("2026-11-09 18:30"))
            .total
            .is_none());
        assert_eq!(
            plan(std::slice::from_ref(&m), &saved, "i", &[ME], 3632, false, at("2026-11-09 18:30"))
                .total
                .unwrap()
                .total,
            3632
        );
        // Hiding the name in the leaderboard sends it again.
        assert!(
            plan(std::slice::from_ref(&m), &saved, "i", &[ME], 3631, true, at("2026-11-09 18:30"))
                .total
                .unwrap()
                .anonymous
        );
        // Until a replay shows the player's account there is nobody to send it for.
        assert!(plan(&[], &Saved::default(), "i", &[], 3631, false, at("2026-11-09 18:30")).total.is_none());
    }

    #[test]
    fn matches_of_other_accounts_stay_here() {
        let m = record("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "2026-11-09 18:00");
        let job = plan(&[m], &Saved::default(), "i", &[STRANGER], TOTAL, false, at("2026-11-09 18:30"));
        assert!(job.uploads.is_empty() && job.lookup.is_empty());
    }

    #[test]
    fn match_times_are_read_as_local_time() {
        let now = chrono::Local::now();
        assert!((unix(now.naive_local()) - now.timestamp()).abs() <= 1);
    }

    #[test]
    fn a_changed_count_is_sent_again() {
        let id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let mut m = record(id, "2026-11-09 18:00");
        let mut saved = Saved::default();
        mark_sent(&mut saved, &m);
        m.players[0].opened.pop();
        let job = plan(&[m], &saved, "i", &[ME], TOTAL, false, at("2026-11-09 18:30"));
        assert_eq!(job.uploads[0].matches[0].opened, 4);
    }

    #[test]
    fn every_chest_goes_out_with_its_place_and_time() {
        let id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let mut m = record(id, "2026-11-09 18:00");
        m.bus_launch = Some(40.0);
        let at_place = |time, x: f64, y: f64| ChestEvent {
            time,
            kind: ChestKind::Rare,
            in_vehicle: false,
            x: Some(x),
            y: Some(y),
            from_loot: false,
        };
        m.players[0].opened = vec![at_place(90.0, 4251.4, -54656.6)];
        m.players[0].broken = vec![ChestEvent { in_vehicle: true, ..at_place(60.0, 1.0, 2.0) }];
        let job = plan(&[m.clone()], &Saved::default(), "i", &[ME], TOTAL, false, at("2026-11-09 18:30"));
        let sent = &job.uploads[0].matches[0];
        assert_eq!(sent.bus, Some(40.0));
        // In the order they happened, positions rounded to whole centimeters.
        assert_eq!(
            sent.chests,
            vec![
                Chest { t: 60.0, k: ChestKind::Rare, o: false, v: true, l: false, x: Some(1), y: Some(2) },
                Chest { t: 90.0, k: ChestKind::Rare, o: true, v: false, l: false, x: Some(4251), y: Some(-54657) },
            ]
        );
        let wire = serde_json::to_string(&sent.chests[1]).unwrap();
        assert_eq!(wire, r#"{"t":90.0,"k":"Rare","o":true,"x":4251,"y":-54657}"#);

        // Same counts, a chest moved (say, a fix to the analysis): it goes out again.
        let mut saved = Saved::default();
        mark_sent(&mut saved, &m);
        m.players[0].opened[0].x = Some(10.0);
        assert_eq!(plan(&[m], &saved, "i", &[ME], TOTAL, false, at("2026-11-09 18:30")).uploads.len(), 1);
    }

    #[test]
    fn matches_sent_before_chests_were_sent_go_out_again_in_small_batches() {
        // online.json of 1.1.5: counts only, nothing in `sent`.
        let old: Saved = serde_json::from_str(r#"{"uploaded":{"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa":[5,0]}}"#).unwrap();
        let matches: Vec<MatchRecord> = (0..120).map(|i| record(&format!("{i:032x}"), "2026-11-09 18:00")).collect();
        let job = plan(&matches, &old, "i", &[ME], TOTAL, false, at("2026-11-09 18:30"));
        let sizes: Vec<usize> = job.uploads.iter().map(|u| u.matches.len()).collect();
        assert_eq!(sizes, [50, 50, 20]);
    }

    #[test]
    fn a_squadmate_found_without_chests_is_asked_about_again() {
        let id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let m = record(id, "2026-11-01 18:00");
        let mut saved = Saved { total_sent: Some(TOTAL), anonymous_sent: Some(false), ..Default::default() };
        mark_sent(&mut saved, &m);
        saved.asked.insert(id.into(), at("2026-11-09 06:00"));
        // A week old and their app sent counts only: asked twice a day, not dropped.
        saved.found.push(found(id, 9, None));
        let job = plan(std::slice::from_ref(&m), &saved, "i", &[ME], TOTAL, false, at("2026-11-09 18:30"));
        assert_eq!(job.lookup.len(), 1);
        // With their chests in, a week-old match is done.
        saved.found[0].chests = Some(Vec::new());
        assert!(plan(&[m], &saved, "i", &[ME], TOTAL, false, at("2026-11-09 18:30")).is_empty());
    }

    #[test]
    fn a_squadmates_chests_turn_back_into_their_player() {
        let chest = |t, o| Chest { t, k: ChestKind::Normal, o, v: false, l: false, x: Some(5), y: None };
        let p = found("m", 1, Some(vec![chest(30.0, false), chest(20.0, true)])).player().unwrap();
        assert_eq!((p.name.as_str(), p.epic_id.as_deref(), p.opened.len(), p.broken.len()), ("Mate", Some(MATE), 1, 1));
        assert_eq!((p.opened[0].time, p.opened[0].x, p.opened[0].y), (20.0, Some(5.0), None));
        assert!(found("m", 1, None).player().is_none());
    }

    #[test]
    fn merging_keeps_one_entry_per_squadmate() {
        let path = std::env::temp_dir().join(format!("fcc-online-{}.json", std::process::id()));
        let mut sync = Sync::start(path.clone(), || {});
        let f = |opened| found("m", opened, None);
        assert!(sync.merge(Outcome { found: vec![f(3)], ..Default::default() }));
        assert!(!sync.merge(Outcome { found: vec![f(3)], ..Default::default() }));
        assert!(sync.merge(Outcome { found: vec![f(4)], ..Default::default() }));
        assert_eq!(sync.found(), vec![f(4)]);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn install_ids_differ() {
        let (a, b) = (new_install_id(), new_install_id());
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
    }
}
