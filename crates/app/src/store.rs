//! The match database: one JSON file with every analyzed match.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use chests::same_name;

use crate::config::{data_dir, write_json_atomically};
use crate::record::MatchRecord;

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoreFile {
    version: u32,
    matches: Vec<MatchRecord>,
    #[serde(default)]
    squad_replays: Vec<MatchRecord>,
}

#[derive(Debug, Default)]
pub struct Store {
    path: PathBuf,
    /// Newest first.
    pub matches: Vec<MatchRecord>,
    /// Matches as recorded by teammates, added by hand. Never counted on their own: they
    /// only fill in players in the user's match with the same match id.
    pub squad_replays: Vec<MatchRecord>,
    /// Squadmates' own counts from the sync server, for every match. Kept by the online
    /// mode in its own file; empty while it is off.
    online: Vec<crate::online::Found>,
}

/// What a teammate's replay is linked to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SquadLink {
    /// The replay predates match ids; re-adding it after an update fixes that.
    NoMatchId,
    /// None of the user's replays is of this match.
    Unmatched,
    /// It is one of the user's own replays.
    Own,
    /// Linked to the user's match with this record id; `added` chests beyond what the
    /// user's replay saw.
    Linked { match_record: String, added: usize },
}

impl Store {
    pub fn default_path() -> PathBuf {
        data_dir().join("matches.json")
    }

    pub fn load(path: PathBuf) -> Self {
        let file = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<StoreFile>(&text).ok())
            .unwrap_or_default();
        let mut store = Self { path, matches: file.matches, squad_replays: file.squad_replays, online: Vec::new() };
        store.sort();
        store.link();
        store
    }

    pub fn save(&self) -> std::io::Result<()> {
        write_json_atomically(
            &self.path,
            &StoreFile { version: 1, matches: self.matches.clone(), squad_replays: self.squad_replays.clone() },
        )
    }

    /// Adds or replaces a teammate's replay and links it.
    pub fn add_squad_replay(&mut self, record: MatchRecord) {
        self.squad_replays.retain(|m| m.id != record.id);
        self.squad_replays.push(record);
        self.squad_replays.sort_by_key(|m| std::cmp::Reverse(m.started_at));
        self.link();
    }

    pub fn remove_squad_replay(&mut self, id: &str) {
        self.squad_replays.retain(|m| m.id != id);
        self.link();
    }

    /// Replaces the squadmates' counts from the sync server.
    pub fn set_online(&mut self, found: Vec<crate::online::Found>) {
        self.online = found;
        self.link();
    }

    /// Fills each match's `fuller` players from teammates' replays of the same match:
    /// a player's whole view is taken from whichever replay saw the most of their chests.
    /// Also hands each match the squadmates' counts from the sync server.
    pub fn link(&mut self) {
        for m in &mut self.matches {
            m.fuller.clear();
            m.linked_from.clear();
            m.online.clear();
            let Some(id) = m.match_id.clone() else { continue };
            m.online.extend(self.online.iter().filter(|f| f.match_id == id).cloned());
            for other in self.squad_replays.iter().filter(|o| o.match_id.as_deref() == Some(id.as_str())) {
                if other.id == m.id || same_owner(m, other) {
                    continue;
                }
                let mut used = false;
                for p in &other.players {
                    let seen = |list: &[chests::PlayerChests]| {
                        list.iter().find(|q| same_name(&q.name, &p.name)).map_or(0, |q| q.total())
                    };
                    let best_so_far = seen(&m.players).max(seen(&m.fuller));
                    if p.total() > best_so_far {
                        m.fuller.retain(|q| !same_name(&q.name, &p.name));
                        m.fuller.push(on_clock_of(p, other.bus_launch, m.bus_launch));
                        used = true;
                    }
                }
                if let Some(owner) = other.owner.clone().filter(|_| used) {
                    m.linked_from.push(owner);
                }
            }
        }
    }

    /// What a teammate's replay was linked to, for the Squad replays page.
    pub fn squad_link(&self, replay: &MatchRecord) -> SquadLink {
        let Some(id) = replay.match_id.as_deref() else { return SquadLink::NoMatchId };
        let Some(m) = self.matches.iter().find(|m| m.match_id.as_deref() == Some(id)) else {
            return SquadLink::Unmatched;
        };
        if m.id == replay.id || same_owner(m, replay) {
            return SquadLink::Own;
        }
        let added = replay
            .players
            .iter()
            // Only the players `link` took from this very replay: the same shift applied
            // again gives an identical copy, while another replay's events differ.
            .filter(|p| m.fuller.iter().any(|f| *f == on_clock_of(p, replay.bus_launch, m.bus_launch)))
            .map(|p| p.total() - m.players.iter().find(|q| same_name(&q.name, &p.name)).map_or(0, |q| q.total()))
            .sum();
        SquadLink::Linked { match_record: m.id.clone(), added }
    }

    fn sort(&mut self) {
        self.matches.sort_by(|a, b| b.started_at.cmp(&a.started_at).then_with(|| b.file_name.cmp(&a.file_name)));
    }

    pub fn upsert(&mut self, record: MatchRecord) {
        self.matches.retain(|m| m.id != record.id);
        self.matches.push(record);
        self.sort();
        self.link();
    }

    pub fn remove(&mut self, id: &str) {
        self.matches.retain(|m| m.id != id);
    }

    /// `(file name, size)` of every replay already analyzed with the current analyzer.
    pub fn known_files(&self) -> HashMap<(String, u64), u32> {
        self.matches.iter().map(|m| ((m.file_name.clone(), m.file_size), m.analyzer_version)).collect()
    }
}

/// A teammate's chests with their replay times moved onto this replay's timeline, lined
/// up on the bus launch both replays saw.
fn on_clock_of(p: &chests::PlayerChests, theirs: Option<f32>, ours: Option<f32>) -> chests::PlayerChests {
    let mut p = p.clone();
    if let (Some(theirs), Some(ours)) = (theirs, ours) {
        for e in p.opened.iter_mut().chain(p.broken.iter_mut()) {
            e.time += ours - theirs;
        }
    }
    p
}

/// Both replays were recorded by the same player.
fn same_owner(a: &MatchRecord, b: &MatchRecord) -> bool {
    matches!((&a.owner, &b.owner), (Some(x), Some(y)) if same_name(x, y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::ANALYZER_VERSION;

    fn record(id: &str, file: &str) -> MatchRecord {
        MatchRecord {
            id: id.into(),
            file_name: file.into(),
            file_size: 10,
            started_at: None,
            length_ms: 0,
            season: None,
            placement: None,
            total_players: None,
            eliminations: None,
            players: vec![],
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

    #[test]
    fn matches_are_keyed_by_recording_not_file_name() {
        let mut store = Store::default();
        store.upsert(record("guid-1", "a.replay"));
        store.upsert(record("guid-1", "renamed.replay"));
        assert_eq!(store.matches.len(), 1);
        assert_eq!(store.matches[0].file_name, "renamed.replay");
    }

    fn player(name: &str, team: u32, opened: usize) -> chests::PlayerChests {
        let ev = chests::ChestEvent {
            time: 1.0,
            kind: chests::ChestKind::Normal,
            in_vehicle: false,
            x: None,
            y: None,
            from_loot: false,
        };
        chests::PlayerChests {
            name: name.into(),
            team: Some(team),
            is_bot: false,
            epic_id: None,
            opened: vec![ev; opened],
            broken: vec![],
        }
    }

    #[test]
    fn teammate_replays_link_by_match_id_only() {
        let mut mine = record("mine", "a.replay");
        mine.owner = Some("Me".into());
        mine.match_id = Some("session-1".into());
        mine.players = vec![player("Me", 2, 5), player("Mate", 2, 1)];
        let mut store = Store::default();
        store.upsert(mine);

        // Mate's replay of the same match saw all of Mate's chests and fewer of mine.
        let mut theirs = record("theirs", "b.replay");
        theirs.owner = Some("Mate".into());
        theirs.match_id = Some("session-1".into());
        theirs.players = vec![player("Me", 2, 3), player("Mate", 2, 9)];
        // Another match on the same day must not link.
        let mut other = record("other", "c.replay");
        other.owner = Some("Mate".into());
        other.match_id = Some("session-2".into());
        other.players = vec![player("Mate", 2, 40)];
        store.add_squad_replay(theirs.clone());
        store.add_squad_replay(other.clone());

        let s = store.matches[0].summary("");
        assert_eq!((s.mine(), s.mates), (5, 9));
        assert_eq!(store.matches[0].linked_from, vec!["Mate".to_string()]);
        assert_eq!(store.squad_link(&theirs), SquadLink::Linked { match_record: "mine".into(), added: 8 });
        assert_eq!(store.squad_link(&other), SquadLink::Unmatched);

        // A re-analysis of the user's replay keeps the link.
        let mut again = store.matches[0].clone();
        again.fuller.clear();
        store.upsert(again);
        assert_eq!(store.matches[0].summary("").mates, 9);

        store.remove_squad_replay("theirs");
        assert_eq!(store.matches[0].summary("").mates, 1);
    }

    #[test]
    fn teammate_chests_line_up_on_the_bus_launch() {
        let mut mine = record("mine", "a.replay");
        mine.owner = Some("Me".into());
        mine.match_id = Some("s".into());
        mine.bus_launch = Some(50.0);
        mine.players = vec![player("Me", 2, 1)];
        let mut store = Store::default();
        store.upsert(mine);
        // Mate started recording 20 s later, so their bus launched at 30 s.
        let mut theirs = record("theirs", "b.replay");
        theirs.owner = Some("Mate".into());
        theirs.match_id = Some("s".into());
        theirs.bus_launch = Some(30.0);
        theirs.players = vec![player("Mate", 2, 1)];
        store.add_squad_replay(theirs.clone());
        let mate = store.matches[0].roster().find(|p| p.name == "Mate").unwrap();
        assert_eq!(mate.opened[0].time, 21.0);
        assert!(matches!(store.squad_link(&theirs), SquadLink::Linked { added: 1, .. }));
    }

    #[test]
    fn only_the_replay_that_filled_in_a_player_gets_credit() {
        let mut mine = record("mine", "a.replay");
        mine.owner = Some("Me".into());
        mine.match_id = Some("s".into());
        mine.bus_launch = Some(50.0);
        mine.players = vec![player("Me", 2, 1), player("Mate", 2, 3)];
        let mut store = Store::default();
        store.upsert(mine);
        // Two teammates' replays both saw Mate's 5 chests, recorded on different clocks.
        let replay = |id: &str, owner: &str, bus: f32| {
            let mut r = record(id, &format!("{id}.replay"));
            r.owner = Some(owner.into());
            r.match_id = Some("s".into());
            r.bus_launch = Some(bus);
            r.players = vec![player("Mate", 2, 5)];
            r
        };
        let (a, b) = (replay("a", "Mate", 30.0), replay("b", "Other", 40.0));
        store.add_squad_replay(a.clone());
        store.add_squad_replay(b.clone());
        let added = |r: &MatchRecord| match store.squad_link(r) {
            SquadLink::Linked { added, .. } => added,
            other => panic!("{other:?}"),
        };
        assert_eq!(added(&a) + added(&b), 2);
    }

    #[test]
    fn own_replay_added_as_squad_is_ignored() {
        let mut mine = record("mine", "a.replay");
        mine.owner = Some("Me".into());
        mine.match_id = Some("s".into());
        mine.players = vec![player("Me", 1, 2)];
        let mut store = Store::default();
        store.upsert(mine.clone());
        let mut copy = mine.clone();
        copy.id = "copy".into();
        copy.players = vec![player("Me", 1, 9)];
        store.add_squad_replay(copy.clone());
        assert_eq!(store.matches[0].summary("").mine(), 2);
        assert_eq!(store.squad_link(&copy), SquadLink::Own);
    }
}
