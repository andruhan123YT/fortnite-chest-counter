//! Online mode: the app shares its player's own chest count per match with the sync
//! server, and gets back the counts of squadmates who run the app too. A squadmate's own
//! replay sees every chest they loot, while ours only sees the ones near us.
//!
//! What is sent: the recorder's Epic account id and name, and per match its id, start
//! time and opened and broken chests. To get a squadmate's numbers the app needs the
//! match id and their Epic account id, both read from the user's own replay of it.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use chests::same_name;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use crate::config::{data_dir, load_json, write_json_atomically};
use crate::record::MatchRecord;

pub const SERVER: &str = "https://chests.l4ve.dev";
/// Replaces the server address, to try the app against a local server.
const SERVER_ENV: &str = "FCC_SYNC_SERVER";
/// How often the app looks for something to send or ask.
pub const TICK: Duration = Duration::from_secs(60);
/// The server's limits per request.
const MAX_UPLOAD: usize = 500;
const MAX_LOOKUP: usize = 300;

/// A squadmate's own count of their chests in a match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Found {
    #[serde(rename = "match")]
    pub match_id: String,
    /// Epic account id.
    pub player: String,
    pub name: String,
    pub opened: u32,
    pub broken: u32,
}

impl Found {
    pub fn total(&self) -> usize {
        (self.opened + self.broken) as usize
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
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct Upload {
    player: String,
    name: String,
    install: String,
    matches: Vec<UploadMatch>,
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
    /// What the server holds for each of the user's matches: opened and broken.
    uploaded: HashMap<String, (u32, u32)>,
    found: Vec<Found>,
    /// Unix seconds each match was last asked about.
    asked: HashMap<String, i64>,
}

/// One round of work for the network thread.
#[derive(Debug, Default, PartialEq)]
struct Job {
    uploads: Vec<Upload>,
    lookup: Vec<LookupMatch>,
}

impl Job {
    fn is_empty(&self) -> bool {
        self.uploads.is_empty() && self.lookup.is_empty()
    }
}

#[derive(Debug, Default)]
struct Outcome {
    uploaded: Vec<(String, (u32, u32))>,
    asked: Vec<String>,
    found: Vec<Found>,
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

/// What to send and ask this round, newest matches first.
fn plan(matches: &[MatchRecord], saved: &Saved, install: &str, now: i64) -> Job {
    let mut uploads: BTreeMap<(String, String), Vec<UploadMatch>> = BTreeMap::new();
    let mut lookup = Vec::new();
    let mut newest_first: Vec<&MatchRecord> = matches.iter().collect();
    newest_first.sort_by_key(|m| std::cmp::Reverse(m.started_at));
    for m in newest_first {
        let Some(match_id) = m.match_id.as_deref().filter(|id| is_id(id)) else { continue };
        let Some(me) = m.recorder() else { continue };
        let Some(my_id) = me.epic_id.as_deref().filter(|id| is_id(id)) else { continue };
        let counts = (me.opened.len() as u32, me.broken.len() as u32);
        if saved.uploaded.get(match_id) != Some(&counts) {
            uploads.entry((my_id.to_owned(), me.name.clone())).or_default().push(UploadMatch {
                match_id: match_id.to_owned(),
                started: m.started_at.map_or(0, unix),
                opened: counts.0,
                broken: counts.1,
            });
        }

        let Some(team) = me.team else { continue };
        let mates: Vec<String> = m
            .players
            .iter()
            .filter(|p| p.team == Some(team) && !p.is_bot && !same_name(&p.name, &me.name))
            .filter_map(|p| p.epic_id.clone().filter(|id| is_id(id)))
            .collect();
        let known = |id: &String| saved.found.iter().any(|f| f.match_id == match_id && &f.player == id);
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
    Job { uploads, lookup }
}

fn server() -> String {
    std::env::var(SERVER_ENV).unwrap_or_else(|_| SERVER.to_owned()).trim_end_matches('/').to_owned()
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .user_agent(format!("fortnite-chest-counter/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn post(agent: &ureq::Agent, path: &str, body: &impl Serialize) -> Result<String, String> {
    let body = serde_json::to_vec(body).map_err(|e| e.to_string())?;
    let mut response =
        agent.post(format!("{}{path}", server())).header("Content-Type", "application/json").send(&body[..]).map_err(
            |e| match e {
                ureq::Error::StatusCode(429) => "the server is busy, will try again".to_string(),
                ureq::Error::StatusCode(code) => format!("the server answered {code}"),
                e => e.to_string(),
            },
        )?;
    response.body_mut().read_to_string().map_err(|e| e.to_string())
}

fn work(agent: &ureq::Agent, job: Job) -> Outcome {
    let mut out = Outcome::default();
    for upload in &job.uploads {
        if let Err(e) = post(agent, "/v1/matches", upload) {
            out.error = Some(e);
            return out;
        }
        out.uploaded.extend(upload.matches.iter().map(|m| (m.match_id.clone(), (m.opened, m.broken))));
    }
    if job.lookup.is_empty() {
        return out;
    }
    let reply = post(agent, "/v1/lookup", &Lookup { matches: &job.lookup })
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
    pub fn tick(&mut self, enabled: bool, matches: &[MatchRecord], install: &str) -> bool {
        let mut changed = false;
        while let Ok(out) = self.outcomes.try_recv() {
            self.busy = false;
            changed |= self.merge(out);
        }
        if enabled && !self.busy && Instant::now() >= self.next {
            self.next = Instant::now() + TICK;
            let now = chrono::Utc::now().timestamp();
            let job = plan(matches, &self.saved, install, now);
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
        self.saved.uploaded.extend(out.uploaded);
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
    use chests::PlayerChests;

    const ME: &str = "0123456789abcdef0123456789abcdef";
    const MATE: &str = "fedcba9876543210fedcba9876543210";
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

    fn at(text: &str) -> i64 {
        unix(NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M").unwrap())
    }

    #[test]
    fn sends_own_counts_and_asks_about_squadmates_only() {
        let m = record("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "2026-11-09 18:00");
        let job = plan(&[m], &Saved::default(), "inst", at("2026-11-09 18:30"));
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
        let mut saved = Saved::default();
        saved.uploaded.insert(id.into(), (5, 0));
        saved.asked.insert(id.into(), at("2026-11-09 18:29"));
        // Uploaded with the same numbers and asked a minute ago: nothing to do yet.
        assert!(plan(std::slice::from_ref(&m), &saved, "i", at("2026-11-09 18:30")).is_empty());
        // Two minutes on, a fresh match is asked about again.
        assert_eq!(plan(std::slice::from_ref(&m), &saved, "i", at("2026-11-09 18:31")).lookup.len(), 1);
        // A day old, it waits half an hour between asks.
        saved.asked.insert(id.into(), at("2026-11-10 18:30"));
        assert!(plan(std::slice::from_ref(&m), &saved, "i", at("2026-11-10 18:45")).lookup.is_empty());
        assert_eq!(plan(std::slice::from_ref(&m), &saved, "i", at("2026-11-10 19:00")).lookup.len(), 1);
        // Once every squadmate is found, twice a day for three days, then never again.
        saved.found.push(Found { match_id: id.into(), player: MATE.into(), name: "Mate".into(), opened: 9, broken: 1 });
        assert!(plan(std::slice::from_ref(&m), &saved, "i", at("2026-11-11 06:00")).is_empty());
        assert_eq!(plan(std::slice::from_ref(&m), &saved, "i", at("2026-11-11 06:30")).lookup.len(), 1);
        saved.asked.insert(id.into(), at("2026-11-12 17:00"));
        assert!(plan(&[m], &saved, "i", at("2026-11-13 06:00")).is_empty());
    }

    #[test]
    fn match_times_are_read_as_local_time() {
        let now = chrono::Local::now();
        assert!((unix(now.naive_local()) - now.timestamp()).abs() <= 1);
    }

    #[test]
    fn a_changed_count_is_sent_again() {
        let id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let mut saved = Saved::default();
        saved.uploaded.insert(id.into(), (4, 0));
        let job = plan(&[record(id, "2026-11-09 18:00")], &saved, "i", at("2026-11-09 18:30"));
        assert_eq!(job.uploads[0].matches[0].opened, 5);
    }

    #[test]
    fn merging_keeps_one_entry_per_squadmate() {
        let path = std::env::temp_dir().join(format!("fcc-online-{}.json", std::process::id()));
        let mut sync = Sync::start(path.clone(), || {});
        let f = |opened| Found { match_id: "m".into(), player: MATE.into(), name: "Mate".into(), opened, broken: 0 };
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
