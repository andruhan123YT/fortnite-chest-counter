//! What gets stored per match, and how it is summarized for the configured player.

use chests::{same_name, MatchAnalysis, PlayerChests, Treasure};
use chrono::{DateTime, NaiveDateTime};
use serde::{Deserialize, Serialize};

use crate::cli::mmss;

/// Bump when the analysis changes in a way that makes stored results stale.
pub const ANALYZER_VERSION: u32 = 11;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MatchRecord {
    /// The replay's recording guid, stable across renames and archive copies.
    pub id: String,
    pub file_name: String,
    pub file_size: u64,
    #[serde(default)]
    pub started_at: Option<NaiveDateTime>,
    #[serde(default)]
    pub length_ms: u32,
    #[serde(default)]
    pub season: Option<String>,
    #[serde(default)]
    pub placement: Option<u32>,
    #[serde(default)]
    pub total_players: Option<u32>,
    #[serde(default)]
    pub eliminations: Option<u32>,
    #[serde(default)]
    pub players: Vec<PlayerChests>,
    /// Display name of the player who recorded the replay.
    #[serde(default)]
    pub owner: Option<String>,
    /// Treasure hunt prizes the recorder received.
    #[serde(default)]
    pub treasures: Vec<Treasure>,
    #[serde(default)]
    pub analyzer_version: u32,
    #[serde(default = "full_health")]
    pub packet_health: f32,
    /// The server's id for the match (`GameSessionId`), the same in every player's replay
    /// of it. Teammates' replays are matched on this, never on dates.
    #[serde(default)]
    pub match_id: Option<String>,
    /// Replay time of the battle bus launch. Chest times are shown from it.
    #[serde(default)]
    pub bus_launch: Option<f32>,
    /// Players whose chests a teammate's replay of this match saw more of than this one
    /// did. Filled in by the store; never saved.
    #[serde(skip)]
    pub fuller: Vec<PlayerChests>,
    /// Who recorded the teammates' replays linked to this match. Never saved.
    #[serde(skip)]
    pub linked_from: Vec<String>,
    /// Squadmates who run the app too, with their own replay's count of their chests.
    /// Filled in by the store from the sync server; never saved here.
    #[serde(skip)]
    pub online: Vec<crate::online::Found>,
}

fn full_health() -> f32 {
    1.0
}

#[derive(Debug, Clone, Default)]
pub struct MatchSummary {
    pub found: bool,
    pub opened: usize,
    pub broken: usize,
    /// Chests of teammates, without the counted player's own: exact for those who run the
    /// app too, what the replay saw near the player for the rest.
    pub mates: usize,
    pub teammates: Vec<Teammate>,
    pub treasures: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Teammate {
    pub name: String,
    pub epic_id: Option<String>,
    pub opened: usize,
    pub broken: usize,
    /// They run the app too, so the counts are theirs, from their own replay.
    pub with_app: bool,
}

impl Teammate {
    pub fn total(&self) -> usize {
        self.opened + self.broken
    }
}

impl MatchSummary {
    pub fn mine(&self) -> usize {
        self.opened + self.broken
    }
}

impl MatchRecord {
    pub fn from_analysis(
        file_name: &str,
        file_size: u64,
        replay: &fnreplay::Replay,
        analysis: MatchAnalysis,
        nick: &str,
    ) -> Self {
        // Keep the configured player, the recorder and both their squads, so changing the
        // name in Settings later does not need a re-analysis.
        let owner = analysis.owner.clone();
        let treasures = analysis.treasures.clone();
        let interesting: Vec<&str> =
            [Some(nick), owner.as_deref()].into_iter().flatten().filter(|n| !n.trim().is_empty()).collect();
        let teams: Vec<u32> = interesting.iter().filter_map(|n| analysis.player(n).and_then(|p| p.team)).collect();
        let players = analysis
            .players
            .into_iter()
            .filter(|p| {
                p.total() > 0
                    || interesting.iter().any(|n| same_name(&p.name, n))
                    || p.team.is_some_and(|t| teams.contains(&t))
            })
            .collect();
        let started_at = replay.info.timestamp_ms.and_then(DateTime::from_timestamp_millis).map(|dt| dt.naive_utc());
        let season = match (replay.header.major, replay.header.minor) {
            (Some(major), Some(minor)) => Some(format!("{major}.{minor:02}")),
            _ => None,
        };
        Self {
            id: replay.header.guid_hex().unwrap_or_else(|| format!("{file_name}|{file_size}")),
            file_name: file_name.to_string(),
            file_size,
            started_at,
            length_ms: replay.info.length_ms,
            season,
            placement: replay.team_stats.map(|s| s.placement).filter(|&p| p > 0),
            total_players: replay.team_stats.map(|s| s.total_players).filter(|&p| p > 0),
            eliminations: replay.match_stats.map(|s| s.eliminations),
            players,
            owner,
            treasures,
            analyzer_version: ANALYZER_VERSION,
            packet_health: replay.packet_health() as f32,
            match_id: analysis.match_id,
            bus_launch: analysis.bus_launch,
            fuller: Vec::new(),
            linked_from: Vec::new(),
            online: Vec::new(),
        }
    }

    /// Every player, each in the fullest view available: this replay's, or a linked
    /// teammate's replay that saw more of that player's chests.
    pub fn roster(&self) -> impl Iterator<Item = &PlayerChests> {
        let replaced = |p: &PlayerChests| self.fuller.iter().any(|f| same_name(&f.name, &p.name));
        self.players.iter().filter(move |p| !replaced(p)).chain(self.fuller.iter())
    }

    /// The name to count for: the configured one, or the replay's recorder when it is empty.
    pub fn player_name<'a>(&'a self, nick: &'a str) -> &'a str {
        if nick.trim().is_empty() {
            self.owner.as_deref().unwrap_or("")
        } else {
            nick
        }
    }

    pub fn summary(&self, nick: &str) -> MatchSummary {
        let nick = self.player_name(nick);
        let Some(me) = self.me(nick) else {
            return MatchSummary::default();
        };
        let teammates: Vec<Teammate> = match me.team {
            Some(team) => self
                .roster()
                .filter(|p| p.team == Some(team) && !same_name(&p.name, nick))
                .map(|p| self.teammate(p))
                .collect(),
            None => Vec::new(),
        };
        let mates = teammates.iter().map(Teammate::total).sum::<usize>();
        let treasures = self.treasures(nick).len();
        MatchSummary { found: true, opened: me.opened.len(), broken: me.broken.len(), mates, teammates, treasures }
    }

    /// A squadmate's chests: their own count when they run the app too, unless this
    /// replay somehow saw more.
    fn teammate(&self, p: &PlayerChests) -> Teammate {
        let online = p.epic_id.as_deref().and_then(|id| self.online.iter().find(|f| f.player == id));
        match online {
            Some(f) if f.total() >= p.total() => Teammate {
                name: p.name.clone(),
                epic_id: p.epic_id.clone(),
                opened: f.opened as usize,
                broken: f.broken as usize,
                with_app: true,
            },
            _ => Teammate {
                name: p.name.clone(),
                epic_id: p.epic_id.clone(),
                opened: p.opened.len(),
                broken: p.broken.len(),
                with_app: online.is_some(),
            },
        }
    }

    /// The recorder's own entry, the one the app shares with the sync server.
    pub fn recorder(&self) -> Option<&PlayerChests> {
        let owner = self.owner.as_deref()?;
        self.players.iter().find(|p| same_name(&p.name, owner))
    }

    /// Prizes of the counted player. Only the recorder's are in a replay.
    pub fn treasures(&self, nick: &str) -> &[Treasure] {
        let nick = self.player_name(nick);
        match &self.owner {
            Some(owner) if same_name(owner, nick) => &self.treasures,
            _ => &[],
        }
    }

    pub fn me<'a>(&'a self, nick: &str) -> Option<&'a PlayerChests> {
        let nick = self.player_name(nick);
        if nick.is_empty() {
            return None;
        }
        self.roster().find(|p| same_name(&p.name, nick))
    }

    /// A replay time as minutes and seconds from the bus launch, or from the start of the
    /// recording when the replay missed the launch.
    pub fn clock(&self, replay_time: f32) -> String {
        mmss(replay_time - self.bus_launch.unwrap_or(0.0))
    }

    /// What [`Self::clock`] counts from.
    pub fn clock_origin(&self) -> &'static str {
        if self.bus_launch.is_some() {
            "from the bus launch"
        } else {
            "from the start of the recording"
        }
    }

    pub fn length_minutes(&self) -> f64 {
        f64::from(self.length_ms) / 60_000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chests::{ChestEvent, ChestKind};

    #[test]
    fn summary_counts_teammates() {
        let ev =
            ChestEvent { time: 1.0, kind: ChestKind::Normal, in_vehicle: false, x: None, y: None, from_loot: false };
        let p = |name: &str, team, n| PlayerChests {
            name: name.into(),
            team,
            is_bot: false,
            epic_id: None,
            opened: vec![ev.clone(); n],
            broken: vec![],
        };
        let record = MatchRecord {
            id: "x".into(),
            file_name: "a.replay".into(),
            file_size: 1,
            started_at: None,
            length_ms: 0,
            season: None,
            placement: None,
            total_players: None,
            eliminations: None,
            players: vec![p("Me", Some(5), 3), p("Mate", Some(5), 2), p("Enemy", Some(6), 9)],
            owner: Some("Me".into()),
            treasures: vec![Treasure { time: 5.0, source: None }],
            analyzer_version: ANALYZER_VERSION,
            packet_health: 1.0,
            match_id: None,
            bus_launch: None,
            fuller: Vec::new(),
            linked_from: Vec::new(),
            online: Vec::new(),
        };
        let s = record.summary("ME");
        assert_eq!((s.mine(), s.mates, s.teammates.len()), (3, 2, 1));
        assert!(!record.summary("Ghost").found);
        // An empty name means the replay's recorder.
        assert_eq!(record.summary("").mine(), 3);
        assert_eq!(record.summary("  ").mates, 2);
        assert_eq!(record.summary("").treasures, 1);
        // Prizes belong to the recorder only.
        assert_eq!(record.summary("Mate").treasures, 0);
    }

    #[test]
    fn a_squadmate_with_the_app_counts_in_full() {
        let ev =
            ChestEvent { time: 1.0, kind: ChestKind::Normal, in_vehicle: false, x: None, y: None, from_loot: false };
        let p = |name: &str, id: &str, n| PlayerChests {
            name: name.into(),
            team: Some(5),
            epic_id: Some(id.into()),
            opened: vec![ev.clone(); n],
            ..Default::default()
        };
        let found = |player: &str, opened, broken| crate::online::Found {
            match_id: "m".into(),
            player: player.into(),
            name: "x".into(),
            opened,
            broken,
        };
        let mut record = MatchRecord {
            owner: Some("Me".into()),
            players: vec![p("Me", "a", 3), p("Mate", "b", 2), p("Random", "c", 4)],
            ..Default::default()
        };
        assert_eq!(record.summary("").mates, 6);
        // Mate's app says 9 opened and 3 broken; this replay only saw 2 of them.
        record.online = vec![found("b", 9, 3)];
        let s = record.summary("");
        assert_eq!(s.mates, 16);
        let mate = s.teammates.iter().find(|t| t.name == "Mate").unwrap();
        assert_eq!((mate.opened, mate.broken, mate.with_app), (9, 3, true));
        assert!(!s.teammates.iter().find(|t| t.name == "Random").unwrap().with_app);
        // A lower number than this replay saw is not taken, but they still run the app.
        record.online = vec![found("c", 1, 0)];
        let random = record.summary("").teammates.into_iter().find(|t| t.name == "Random").unwrap();
        assert_eq!((random.total(), random.with_app), (4, true));
    }

    #[test]
    fn clock_counts_from_the_bus() {
        let mut record = MatchRecord {
            id: "x".into(),
            file_name: "a.replay".into(),
            file_size: 1,
            started_at: None,
            length_ms: 0,
            season: None,
            placement: None,
            total_players: None,
            eliminations: None,
            players: Vec::new(),
            owner: None,
            treasures: Vec::new(),
            analyzer_version: ANALYZER_VERSION,
            packet_health: 1.0,
            match_id: None,
            bus_launch: None,
            fuller: Vec::new(),
            linked_from: Vec::new(),
            online: Vec::new(),
        };
        assert_eq!(record.clock(125.0), "2:05");
        record.bus_launch = Some(55.0);
        assert_eq!(record.clock(125.0), "1:10");
        assert_eq!(record.clock(50.0), "-0:05");
    }
}
