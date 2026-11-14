//! Aggregates over all counted matches.

use std::collections::BTreeMap;

use chests::ChestKind;

use crate::config::Settings;
use crate::record::MatchRecord;

#[derive(Debug, Clone, Default)]
pub struct Totals {
    pub counted_matches: usize,
    /// Counted matches in which the configured player does not appear.
    pub matches_without_player: usize,
    pub opened: usize,
    pub broken: usize,
    pub broken_by_vehicle: usize,
    /// Chests of teammates in counted matches.
    pub teammates: usize,
    pub minutes: f64,
    pub by_kind: BTreeMap<KindKey, (usize, usize)>,
    pub best_match: Option<(String, usize)>,
    pub base: u64,
    /// Chests added or taken off by hand.
    pub adjustment: i64,
    /// Treasure hunt prizes received in counted matches.
    pub treasures: usize,
}

/// `ChestKind` ordered for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct KindKey(pub u8);

impl KindKey {
    pub fn of(kind: ChestKind) -> Self {
        Self(ChestKind::ALL.iter().position(|&k| k == kind).unwrap_or(0) as u8)
    }

    pub fn kind(self) -> ChestKind {
        ChestKind::ALL[self.0 as usize]
    }
}

impl Totals {
    pub fn mine(&self) -> u64 {
        let counted = self.base as i64 + (self.opened + self.broken) as i64 + self.adjustment;
        counted.max(0) as u64
    }

    /// Mine plus teammates': what every squad-wide prize rolls on.
    pub fn with_squad(&self) -> u64 {
        self.mine() + self.teammates as u64
    }

    /// Squad chests in counted matches, without the base.
    pub fn counted_squad(&self) -> u64 {
        (self.opened + self.broken + self.teammates) as u64
    }

    pub fn per_match(&self) -> f64 {
        if self.counted_matches == 0 {
            0.0
        } else {
            (self.opened + self.broken) as f64 / self.counted_matches as f64
        }
    }

    pub fn per_minute(&self) -> f64 {
        if self.minutes <= 0.0 {
            0.0
        } else {
            (self.opened + self.broken) as f64 / self.minutes
        }
    }
}

/// The active account's matches, counted or not.
pub fn own<'a>(matches: &'a [MatchRecord], settings: &'a Settings) -> impl Iterator<Item = &'a MatchRecord> {
    matches.iter().filter(|m| settings.owns(m))
}

pub fn counted<'a>(matches: &'a [MatchRecord], settings: &'a Settings) -> impl Iterator<Item = &'a MatchRecord> {
    own(matches, settings).filter(|m| settings.counts(m.started_at))
}

/// An Epic account that recorded replays on this PC.
#[derive(Debug, Clone, PartialEq)]
pub struct Account {
    pub id: String,
    /// Its name in the newest of those replays: players rename themselves.
    pub name: String,
    pub matches: usize,
    pub last: Option<chrono::NaiveDateTime>,
}

/// Accounts that recorded replays here, by Epic id, the one with the most matches first.
pub fn accounts(matches: &[MatchRecord]) -> Vec<Account> {
    let mut seen: Vec<Account> = Vec::new();
    for m in matches {
        let (Some(id), Some(owner)) = (m.recorder_id(), m.owner.as_deref()) else { continue };
        match seen.iter_mut().find(|a| a.id == id) {
            Some(a) => {
                a.matches += 1;
                if m.started_at >= a.last {
                    a.last = m.started_at;
                    a.name = owner.to_owned();
                }
            }
            None => seen.push(Account { id: id.to_owned(), name: owner.to_owned(), matches: 1, last: m.started_at }),
        }
    }
    seen.sort_by(|a, b| b.matches.cmp(&a.matches).then(b.last.cmp(&a.last)));
    seen
}

/// The first account seen in replays that the player has not said is theirs or not.
/// Nothing is asked before the player's own account is known.
pub fn unknown_account<'a>(accounts: &'a [Account], settings: &Settings) -> Option<&'a Account> {
    if settings.account_id.is_empty() {
        return None;
    }
    accounts.iter().find(|a| !settings.knows_account(&a.id))
}

/// The Epic id of the newest replay recorded under the name `name`.
pub fn id_of(matches: &[MatchRecord], name: &str) -> Option<String> {
    matches
        .iter()
        .filter(|m| m.owner.as_deref().is_some_and(|o| chests::same_name(o, name)))
        .filter_map(|m| Some((m.started_at, m.recorder_id()?)))
        .max_by_key(|(at, _)| *at)
        .map(|(_, id)| id.to_owned())
}

pub fn totals(matches: &[MatchRecord], settings: &Settings) -> Totals {
    let mut t = Totals { base: settings.base_count, adjustment: settings.adjustment, ..Default::default() };
    for m in counted(matches, settings) {
        t.counted_matches += 1;
        t.minutes += m.length_minutes();
        let summary = m.summary(settings.name_in(m));
        if !summary.found {
            t.matches_without_player += 1;
            continue;
        }
        t.opened += summary.opened;
        t.broken += summary.broken;
        t.teammates += summary.mates;
        t.treasures += summary.treasures;
        if t.best_match.as_ref().is_none_or(|(_, best)| summary.mine() > *best) {
            t.best_match = Some((m.id.clone(), summary.mine()));
        }
        if let Some(me) = m.me(settings.name_in(m)) {
            for e in &me.opened {
                t.by_kind.entry(KindKey::of(e.kind)).or_default().0 += 1;
            }
            for e in &me.broken {
                t.by_kind.entry(KindKey::of(e.kind)).or_default().1 += 1;
                t.broken_by_vehicle += usize::from(e.in_vehicle);
            }
        }
    }
    t
}

/// The counted player's chests per day (the replay's local date), in counted matches.
pub fn by_day(matches: &[MatchRecord], settings: &Settings) -> BTreeMap<chrono::NaiveDate, u64> {
    let mut days = BTreeMap::new();
    for m in counted(matches, settings) {
        if let Some(at) = m.started_at {
            *days.entry(at.date()).or_insert(0) += m.summary(settings.name_in(m)).mine() as u64;
        }
    }
    days
}

/// Running totals over time: `(unix seconds of the local wall clock, mine, teammates)`.
pub fn running_totals(matches: &[MatchRecord], settings: &Settings) -> Vec<(f64, f64, f64)> {
    let mut points: Vec<_> =
        counted(matches, settings).filter_map(|m| Some((m.started_at?, m.summary(settings.name_in(m))))).collect();
    points.sort_by_key(|(at, _)| *at);
    // Chests added by hand have no date; they count from the start, like the base.
    let mut mine = (settings.base_count as i64 + settings.adjustment).max(0) as f64;
    let mut mates = 0.0;
    let mut out = Vec::with_capacity(points.len() + 1);
    if let Some((first, _)) = points.first() {
        out.push((first.and_utc().timestamp() as f64 - 1.0, mine, mates));
    }
    for (at, s) in points {
        mine += s.mine() as f64;
        mates += s.mates as f64;
        out.push((at.and_utc().timestamp() as f64, mine, mates));
    }
    out
}

/// A running total per squadmate who runs the app, over the counted matches played with
/// them: `(name, [(unix seconds of the local wall clock, their chests so far)])`. Which
/// squadmates get one follows the online settings.
pub fn squadmate_totals(matches: &[MatchRecord], settings: &Settings) -> Vec<(String, Vec<(f64, f64)>)> {
    if !settings.online {
        return Vec::new();
    }
    let mut points: Vec<_> =
        counted(matches, settings).filter_map(|m| Some((m.started_at?, m.summary(settings.name_in(m))))).collect();
    points.sort_by_key(|(at, _)| *at);
    // Keyed by Epic account id: names can change, the id does not.
    struct Line {
        id: String,
        name: String,
        total: f64,
        points: Vec<(f64, f64)>,
    }
    let mut lines: Vec<Line> = Vec::new();
    for (at, s) in points {
        let x = at.and_utc().timestamp() as f64;
        for t in s.teammates.iter().filter(|t| t.with_app) {
            let Some(id) = &t.epic_id else { continue };
            let i = match lines.iter().position(|l| &l.id == id) {
                Some(i) => i,
                None => {
                    // Starts from nothing just before the first match together.
                    lines.push(Line { id: id.clone(), name: String::new(), total: 0.0, points: vec![(x - 1.0, 0.0)] });
                    lines.len() - 1
                }
            };
            let line = &mut lines[i];
            line.name = t.name.clone();
            line.total += t.total() as f64;
            line.points.push((x, line.total));
        }
    }
    lines
        .into_iter()
        .filter(|l| settings.online_lines_all || settings.online_lines.iter().any(|n| chests::same_name(n, &l.name)))
        .map(|l| (l.name, l.points))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{parse_since, Profile};
    use crate::record::ANALYZER_VERSION;
    use chests::{ChestEvent, PlayerChests};

    fn ev(kind: ChestKind) -> ChestEvent {
        ChestEvent { time: 1.0, kind, in_vehicle: false, x: None, y: None, from_loot: false }
    }

    fn record(at: &str, mine: usize, mate: usize) -> MatchRecord {
        MatchRecord {
            id: at.into(),
            file_name: format!("{at}.replay"),
            file_size: 1,
            started_at: parse_since(at),
            length_ms: 600_000,
            season: None,
            placement: None,
            total_players: None,
            eliminations: None,
            players: vec![
                PlayerChests {
                    name: "Me".into(),
                    team: Some(1),
                    is_bot: false,
                    epic_id: None,
                    opened: vec![ev(ChestKind::Rare); mine],
                    broken: vec![],
                },
                PlayerChests {
                    name: "Mate".into(),
                    team: Some(1),
                    is_bot: false,
                    epic_id: None,
                    opened: vec![ev(ChestKind::Normal); mate],
                    broken: vec![],
                },
            ],
            owner: None,
            treasures: Vec::new(),
            analyzer_version: ANALYZER_VERSION,
            packet_health: 1.0,
            match_id: None,
            bus_launch: None,
            fuller: Vec::new(),
            linked_from: Vec::new(),
            online: Vec::new(),
        }
    }

    fn recorded(at: &str, mine: usize, name: &str, id: &str) -> MatchRecord {
        let mut m = record(at, mine, 0);
        m.owner = Some(name.into());
        m.players[0].name = name.into();
        m.players[0].epic_id = Some(id.into());
        m
    }

    #[test]
    fn each_account_counts_only_its_own_replays() {
        let mut settings =
            Settings { nick: "Me".into(), account_id: "me".into(), base_count: 1000, ..Default::default() };
        let mine = recorded("2026-11-08 10:00", 5, "Me", "me");
        let second = recorded("2026-11-08 11:00", 7, "Alt", "alt");
        let unknown = record("2026-11-08 12:00", 3, 0);
        let matches = vec![mine, second.clone(), second.clone(), unknown];
        let names: Vec<_> = accounts(&matches).into_iter().map(|a| a.name).collect();
        assert_eq!(names, ["Alt", "Me"]);
        let t = totals(&matches, &settings);
        // The replay without a recorder still counts for whoever is active.
        assert_eq!((t.counted_matches, t.mine()), (2, 1008));

        settings.switch_account("alt", "Alt");
        let t = totals(&matches, &settings);
        assert_eq!((t.counted_matches, t.mine(), t.base), (3, 14, 0));
        settings.base_count = 50;
        settings.switch_account("me", "Me");
        assert_eq!((settings.nick.as_str(), settings.base_count), ("Me", 1000));
        settings.switch_account("alt", "Alt");
        assert_eq!(settings.base_count, 50);
        assert_eq!(settings.profiles.keys().collect::<Vec<_>>(), ["me"]);
    }

    #[test]
    fn a_new_account_is_asked_about_once_and_counts_only_once_claimed() {
        let mut settings = Settings { nick: "Me".into(), account_id: "me".into(), ..Default::default() };
        let matches = vec![
            recorded("2026-11-08 10:00", 5, "Me", "me"),
            recorded("2026-11-08 11:00", 7, "Friend", "friend"),
            recorded("2026-11-08 12:00", 3, "Alt", "alt"),
        ];
        let all = accounts(&matches);
        // Not answered yet: asked about, and its matches are not the player's.
        // The one that played last comes first.
        assert_eq!(unknown_account(&all, &settings).map(|a| a.name.as_str()), Some("Alt"));
        assert_eq!(totals(&matches, &settings).mine(), 5);
        settings.profiles.insert("alt".into(), Profile { name: "Alt".into(), ..Default::default() });
        assert_eq!(unknown_account(&all, &settings).map(|a| a.name.as_str()), Some("Friend"));
        settings.not_mine.push("friend".into());
        assert!(unknown_account(&all, &settings).is_none());
        assert_eq!(settings.my_accounts().collect::<Vec<_>>(), ["me", "alt"]);
        // Still the active account's total until the player switches.
        assert_eq!(totals(&matches, &settings).mine(), 5);
        settings.switch_account("alt", "Alt");
        assert_eq!(totals(&matches, &settings).mine(), 3);
        // Before the player's own account is known nothing is asked.
        let fresh = Settings::default();
        assert!(unknown_account(&all, &fresh).is_none());
    }

    #[test]
    fn a_renamed_account_keeps_its_matches() {
        let settings = Settings { nick: "New".into(), account_id: "me".into(), ..Default::default() };
        let matches = vec![recorded("2026-11-08 10:00", 5, "Old", "me"), recorded("2026-11-09 10:00", 2, "New", "me")];
        let t = totals(&matches, &settings);
        assert_eq!((t.counted_matches, t.mine()), (2, 7));
        let all = accounts(&matches);
        assert_eq!((all.len(), all[0].name.as_str()), (1, "New"));
        assert_eq!(id_of(&matches, "old").as_deref(), Some("me"));
    }

    #[test]
    fn totals_respect_since_and_base() {
        let settings = Settings {
            nick: "me".into(),
            base_count: 1000,
            count_since: parse_since("2026-11-07 22:39"),
            ..Default::default()
        };
        let matches =
            vec![record("2026-11-08 10:00", 5, 2), record("2026-11-07 20:00", 50, 0), record("2026-11-08 12:00", 3, 4)];
        let t = totals(&matches, &settings);
        assert_eq!(t.counted_matches, 2);
        assert_eq!(t.mine(), 1008);
        assert_eq!(t.with_squad(), 1014);
        assert_eq!(t.teammates, 6);
        assert_eq!(t.by_kind[&KindKey::of(ChestKind::Rare)].0, 8);
        assert!((t.per_minute() - 0.4).abs() < 1e-9);

        let series = running_totals(&matches, &settings);
        assert_eq!(series.len(), 3);
        assert_eq!(series.last().unwrap().1, 1008.0);
        assert_eq!(series.last().unwrap().2, 6.0);
    }

    #[test]
    fn sums_chests_per_day() {
        let settings = Settings { nick: "me".into(), ..Default::default() };
        let matches =
            vec![record("2026-11-08 10:00", 5, 0), record("2026-11-08 23:59", 3, 0), record("2026-11-09 00:10", 2, 0)];
        let days = by_day(&matches, &settings);
        let day = |s: &str| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        assert_eq!(days[&day("2026-11-08")], 8);
        assert_eq!(days[&day("2026-11-09")], 2);
    }

    #[test]
    fn chests_added_by_hand_count_and_never_go_below_zero() {
        let matches = vec![record("2026-11-08 10:00", 5, 2)];
        let settings = Settings { nick: "me".into(), base_count: 1000, adjustment: 25, ..Default::default() };
        let t = totals(&matches, &settings);
        assert_eq!(t.mine(), 1030);
        assert_eq!(running_totals(&matches, &settings).last().unwrap().1, 1030.0);
        // Prize odds for the replays' own chests leave hand-added ones out.
        assert_eq!(t.counted_squad(), 7);

        let settings = Settings { nick: "me".into(), adjustment: -3, ..settings };
        assert_eq!(totals(&matches, &settings).mine(), 1002);
        let settings = Settings { base_count: 0, adjustment: -100, ..settings };
        assert_eq!(totals(&matches, &settings).mine(), 0);
    }

    #[test]
    fn squadmates_with_the_app_get_lines_over_shared_matches() {
        let with_app = |at: &str, mate: usize, theirs: u32| {
            let mut m = record(at, 1, mate);
            m.players[1].epic_id = Some("b".repeat(32));
            m.online = vec![crate::online::Found {
                match_id: "m".into(),
                player: "b".repeat(32),
                name: "Mate".into(),
                opened: theirs,
                broken: 0,
            }];
            m
        };
        let matches = vec![
            with_app("2026-11-08 10:00", 1, 7),
            record("2026-11-08 11:00", 1, 3),
            with_app("2026-11-08 12:00", 0, 5),
        ];
        let settings = Settings { nick: "me".into(), ..Default::default() };
        let lines = squadmate_totals(&matches, &settings);
        assert_eq!(lines.len(), 1);
        let (name, points) = &lines[0];
        assert_eq!(name, "Mate");
        // From zero, then 7, then 12: the match without them in the app adds nothing.
        let ys: Vec<f64> = points.iter().map(|p| p.1).collect();
        assert_eq!(ys, vec![0.0, 7.0, 12.0]);
        // Squad total counts them in full too.
        assert_eq!(totals(&matches, &settings).teammates, 15);

        let only_others =
            Settings { online_lines_all: false, online_lines: vec!["Someone".into()], ..settings.clone() };
        assert!(squadmate_totals(&matches, &only_others).is_empty());
        let only_mate = Settings { online_lines_all: false, online_lines: vec!["mate".into()], ..settings.clone() };
        assert_eq!(squadmate_totals(&matches, &only_mate).len(), 1);
        let off = Settings { online: false, ..settings };
        assert!(squadmate_totals(&matches, &off).is_empty());
    }
}
