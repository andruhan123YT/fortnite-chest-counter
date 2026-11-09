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

pub fn counted<'a>(matches: &'a [MatchRecord], settings: &'a Settings) -> impl Iterator<Item = &'a MatchRecord> {
    matches.iter().filter(|m| settings.counts(m.started_at))
}

pub fn totals(matches: &[MatchRecord], settings: &Settings) -> Totals {
    let mut t = Totals { base: settings.base_count, adjustment: settings.adjustment, ..Default::default() };
    for m in counted(matches, settings) {
        t.counted_matches += 1;
        t.minutes += m.length_minutes();
        let summary = m.summary(&settings.nick);
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
        if let Some(me) = m.me(&settings.nick) {
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

/// Running totals over time: `(unix seconds of the local wall clock, mine, teammates)`.
pub fn running_totals(matches: &[MatchRecord], settings: &Settings) -> Vec<(f64, f64, f64)> {
    let mut points: Vec<_> =
        counted(matches, settings).filter_map(|m| Some((m.started_at?, m.summary(&settings.nick)))).collect();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse_since;
    use crate::record::ANALYZER_VERSION;
    use chests::{ChestEvent, PlayerChests};

    fn ev(kind: ChestKind) -> ChestEvent {
        ChestEvent { time: 1.0, kind, in_vehicle: false, x: None, y: None }
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
                    opened: vec![ev(ChestKind::Rare); mine],
                    broken: vec![],
                },
                PlayerChests {
                    name: "Mate".into(),
                    team: Some(1),
                    is_bot: false,
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
        }
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
}
