//! Turns collected replay facts into per-player chest counts.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::breaks::{detect_breaks, marked_breaks, BreakConfig, Tracks};
use crate::collect::{ChestKind, Collected};
use crate::lost::lost_opens;
use crate::schema::fortnite_schema;
use fnreplay::Vec3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChestEvent {
    /// Seconds since the replay started.
    pub time: f32,
    pub kind: ChestKind,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub in_vehicle: bool,
    /// World position in centimeters, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
    /// Found from the chest's loot because the recording missed the open itself.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub from_loot: bool,
}

/// A treasure hunt prize the recorder received.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Treasure {
    pub time: f32,
    /// The squad chest that most likely rolled it, when one was in view just before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<TreasureSource>,
    /// Which prize, from its golden chest: weekly prizes and Pirate Shop Access have one,
    /// gizmos do not, and neither does a squadmate's prize rolled out of view.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prize: Option<String>,
    /// The squadmate whose chest rolled it. Prizes are shared with the whole squad.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub found_by: Option<String>,
}

/// A golden prize chest another player, outside the squad, opened in view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NearbyPrize {
    pub time: f32,
    pub prize: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TreasureSource {
    pub player: String,
    pub kind: ChestKind,
    pub broken: bool,
    /// Replay time of that chest's open or break, to find it among the player's chests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<f32>,
}

/// A prize is granted within a few seconds of the chest that rolled it.
const SOURCE_WINDOW_BEFORE: f32 = 3.5;
const SOURCE_WINDOW_AFTER: f32 = 0.5;
/// How far apart a prize's grant and its golden chest show up. Seen in real replays:
/// the chest 1.5 s before the grant, or 6.5 s after it.
const PRIZE_CHEST_WINDOW: f32 = 10.0;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlayerChests {
    pub name: String,
    #[serde(default)]
    pub team: Option<u32>,
    #[serde(default)]
    pub is_bot: bool,
    /// Epic account id as 32 hex digits; bots have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epic_id: Option<String>,
    #[serde(default)]
    pub opened: Vec<ChestEvent>,
    #[serde(default)]
    pub broken: Vec<ChestEvent>,
}

impl PlayerChests {
    pub fn total(&self) -> usize {
        self.opened.len() + self.broken.len()
    }

    /// Opened and broken chests in the order they happened; `true` marks an opened one.
    pub fn in_order(&self) -> Vec<(&ChestEvent, bool)> {
        let mut events: Vec<_> =
            self.opened.iter().map(|e| (e, true)).chain(self.broken.iter().map(|e| (e, false))).collect();
        events.sort_by(|a, b| a.0.time.total_cmp(&b.0.time));
        events
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MatchAnalysis {
    pub players: Vec<PlayerChests>,
    /// Display name of the player who recorded the replay.
    #[serde(default)]
    pub owner: Option<String>,
    /// Prizes the recorder received. Other players' prizes are not in the replay.
    #[serde(default)]
    pub treasures: Vec<Treasure>,
    /// The server's id for the match (`GameSessionId`), the same in every player's replay.
    #[serde(default)]
    pub match_id: Option<String>,
    /// Replay time of the battle bus launch, when the recording caught it.
    #[serde(default)]
    pub bus_launch: Option<f32>,
    /// Prizes players outside the squad got in view. Not the recorder's.
    #[serde(default)]
    pub nearby_prizes: Vec<NearbyPrize>,
}

/// Fortnite display names are unique regardless of case.
pub fn same_name(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

impl MatchAnalysis {
    pub fn player(&self, name: &str) -> Option<&PlayerChests> {
        self.players.iter().find(|p| same_name(&p.name, name))
    }

    /// The player and everyone on their team. A player without a known team is a squad
    /// of one.
    pub fn squad(&self, name: &str) -> Vec<&PlayerChests> {
        let Some(me) = self.player(name) else { return Vec::new() };
        let mut squad = vec![me];
        if let Some(team) = me.team {
            squad.extend(self.players.iter().filter(|p| p.team == Some(team) && !same_name(&p.name, &me.name)));
        }
        squad
    }
}

pub fn analyze(c: &Collected, cfg: &BreakConfig) -> MatchAnalysis {
    let tracks = Tracks::build(c, cfg);
    let mut players: BTreeMap<String, PlayerChests> = BTreeMap::new();

    for (state, name) in &c.state_name {
        let entry =
            players.entry(name.clone()).or_insert_with(|| PlayerChests { name: name.clone(), ..Default::default() });
        entry.team = entry.team.or_else(|| c.state_team.get(state).copied());
        entry.is_bot |= c.state_is_bot.get(state).copied().unwrap_or(false);
        if entry.epic_id.is_none() && !entry.is_bot {
            entry.epic_id = c.state_epic_id.get(state).cloned();
        }
    }

    let mut seen_chests = HashSet::new();
    for open in &c.opens {
        if open.chest.is_ammo || !seen_chests.insert(open.chest.id.as_str()) {
            continue;
        }
        let Some(name) = c.player_of_pawn(open.pawn) else { continue };
        let position = tracks.position(name, f64::from(open.time));
        let event = ChestEvent {
            time: open.time,
            kind: open.chest.kind(),
            in_vehicle: false,
            x: position.map(|(p, _)| p.x),
            y: position.map(|(p, _)| p.y),
            from_loot: false,
        };
        if let Some(p) = players.get_mut(name) {
            p.opened.push(event);
        }
    }

    let mut breaks = marked_breaks(c, &tracks);
    if cfg.use_loot_heuristic {
        let marked: HashSet<String> = breaks.iter().map(|b| b.chest.id.clone()).collect();
        breaks.extend(detect_breaks(c, &tracks, cfg).into_iter().filter(|b| !marked.contains(&b.chest.id)));
    }
    // Loot the optional break heuristic already turned into a break is not a lost open.
    let guessed: Vec<(f32, Option<Vec3>)> = breaks.iter().map(|b| (b.time, b.location)).collect();
    for b in breaks {
        let event = ChestEvent {
            time: b.time,
            kind: b.chest.kind(),
            in_vehicle: b.in_vehicle,
            x: b.location.map(|l| l.x),
            y: b.location.map(|l| l.y),
            from_loot: false,
        };
        if let Some(p) = players.get_mut(&b.player) {
            p.broken.push(event);
        }
    }

    let lost = lost_opens(c, &tracks).into_iter().filter(|l| {
        !guessed.iter().any(|&(t, at)| {
            (t - l.time).abs() <= 3.0 && at.is_none_or(|p| (p.x - l.location.x).hypot(p.y - l.location.y) <= 800.0)
        })
    });
    for lost in lost {
        let event = ChestEvent {
            time: lost.time,
            kind: ChestKind::Normal,
            in_vehicle: lost.in_vehicle,
            x: Some(lost.location.x),
            y: Some(lost.location.y),
            from_loot: true,
        };
        if let Some(p) = players.get_mut(&lost.player) {
            p.opened.push(event);
        }
    }

    let owner = c.recorder_state.and_then(|s| c.state_name.get(&s)).cloned();
    let players: Vec<PlayerChests> = players.into_values().collect();
    let (treasures, nearby_prizes) = match c.recorder_state {
        Some(state) => treasures(c, state, owner.as_deref(), &players),
        None => (Vec::new(), Vec::new()),
    };
    MatchAnalysis { players, owner, treasures, match_id: c.session_id.clone(), bus_launch: c.bus_launch, nearby_prizes }
}

fn treasures(
    c: &Collected,
    recorder: u32,
    owner: Option<&str>,
    players: &[PlayerChests],
) -> (Vec<Treasure>, Vec<NearbyPrize>) {
    let team = owner.and_then(|o| players.iter().find(|p| same_name(&p.name, o))).and_then(|p| p.team);
    let squad: Vec<&PlayerChests> = players
        .iter()
        .filter(|p| owner.is_some_and(|o| same_name(&p.name, o)) || (team.is_some() && p.team == team))
        .collect();
    let mut granted: Vec<Treasure> = c
        .treasure_grants
        .iter()
        .filter(|&&(_, state)| state == recorder)
        .map(|&(time, _)| {
            let source = squad
                .iter()
                .flat_map(|p| {
                    let opened = p.opened.iter().map(move |e| (e, p, false));
                    opened.chain(p.broken.iter().map(move |e| (e, p, true)))
                })
                .filter(|(e, _, _)| e.time >= time - SOURCE_WINDOW_BEFORE && e.time <= time + SOURCE_WINDOW_AFTER)
                .max_by(|a, b| a.0.time.total_cmp(&b.0.time))
                .map(|(e, p, broken)| TreasureSource {
                    player: p.name.clone(),
                    kind: e.kind,
                    broken,
                    time: Some(e.time),
                });
            Treasure { time, source, prize: None, found_by: None }
        })
        .collect();

    // Golden chests name the prize. One opened in the squad goes with the recorder's
    // grant closest in time; one opened by anybody else is theirs alone.
    let mut chests: Vec<_> = c.prize_chests.values().filter(|p| p.prize.is_some()).collect();
    chests.sort_by(|a, b| a.time.total_cmp(&b.time));
    let mut nearby = Vec::new();
    for chest in chests {
        let prize = chest.prize.clone().unwrap_or_default();
        let opener = c.player_of_pawn(chest.pawn);
        let in_squad = opener.is_some_and(|o| squad.iter().any(|p| same_name(&p.name, o)));
        if !in_squad {
            nearby.push(NearbyPrize { time: chest.time, prize });
            continue;
        }
        let grant = granted
            .iter_mut()
            .filter(|t| t.prize.is_none() && (t.time - chest.time).abs() <= PRIZE_CHEST_WINDOW)
            .min_by(|a, b| (a.time - chest.time).abs().total_cmp(&(b.time - chest.time).abs()));
        if let Some(t) = grant {
            t.prize = Some(prize);
            t.found_by = opener.filter(|o| owner.is_none_or(|me| !same_name(o, me))).map(str::to_owned);
        }
    }
    (granted, nearby)
}

/// Parses a replay and analyzes it in one go.
pub fn analyze_replay<V: fnreplay::Visitor>(
    bytes: &[u8],
    cfg: &BreakConfig,
    progress: &mut V,
) -> fnreplay::Result<(fnreplay::Replay, MatchAnalysis)> {
    struct Both<'a, V> {
        collected: Collected,
        outer: &'a mut V,
    }
    impl<V: fnreplay::Visitor> fnreplay::Visitor for Both<'_, V> {
        fn on_update(&mut self, u: &fnreplay::PropertyUpdate<'_>) {
            self.collected.on_update(u);
        }
        fn on_despawn(&mut self, d: &fnreplay::ActorDespawn<'_>) {
            self.collected.on_despawn(d);
        }
        fn on_actor_spawn(&mut self, s: &fnreplay::ActorSpawn<'_>) {
            self.collected.on_actor_spawn(s);
        }
        fn on_progress(&mut self, fraction: f32) {
            self.outer.on_progress(fraction);
        }
        fn should_stop(&self) -> bool {
            self.outer.should_stop()
        }
    }

    let schema = fortnite_schema();
    let mut both = Both { collected: Collected::default(), outer: progress };
    let replay = fnreplay::parse(bytes, &schema, &mut both)?;
    let analysis = analyze(&both.collected, cfg);
    Ok((replay, analysis))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(name: &str, team: Option<u32>) -> PlayerChests {
        PlayerChests { name: name.into(), team, ..Default::default() }
    }

    #[test]
    fn squad_includes_teammates_only() {
        let m = MatchAnalysis {
            players: vec![
                player("Me", Some(3)),
                player("Mate", Some(3)),
                player("Enemy", Some(4)),
                player("Solo", None),
            ],
            owner: None,
            treasures: Vec::new(),
            match_id: None,
            bus_launch: None,
            nearby_prizes: Vec::new(),
        };
        let names: Vec<_> = m.squad("me").iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Me", "Mate"]);
        assert_eq!(m.squad("Solo").len(), 1);
        assert!(m.squad("Nobody").is_empty());
    }

    #[test]
    fn treasures_point_at_the_squad_chest_just_before() {
        let ev =
            |time| ChestEvent { time, kind: ChestKind::Normal, in_vehicle: false, x: None, y: None, from_loot: false };
        let mut me = player("Me", Some(3));
        me.opened.push(ev(100.0));
        let mut mate = player("Mate", Some(3));
        mate.broken.push(ev(200.0));
        let mut enemy = player("Enemy", Some(4));
        enemy.opened.push(ev(299.0));
        let players = vec![me, mate, enemy];
        let mut c = Collected::default();
        c.recorder_state = Some(7);
        // Mine right away, the mate's broken chest 2 s later, an enemy's chest (never
        // ours), a grant to another player, and one with nothing in view.
        c.treasure_grants = vec![(100.0, 7), (202.0, 7), (300.0, 7), (300.0, 8), (500.0, 7)];
        let (got, _) = treasures(&c, 7, Some("Me"), &players);
        assert_eq!(got.len(), 4);
        let source = |i: usize| got[i].source.as_ref().map(|s| (s.player.as_str(), s.broken));
        assert_eq!(source(0), Some(("Me", false)));
        assert_eq!(source(1), Some(("Mate", true)));
        assert_eq!(source(2), None);
        assert_eq!(source(3), None);
    }

    #[test]
    fn golden_chests_name_the_prize_and_who_found_it() {
        use crate::collect::PrizeChest;
        let players = vec![player("Me", Some(3)), player("Mate", Some(3)), player("Enemy", Some(4))];
        let mut c = Collected::default();
        c.recorder_state = Some(7);
        for (pawn, state, name) in [(70, 7, "Me"), (80, 8, "Mate"), (90, 9, "Enemy")] {
            c.pawn_state.insert(pawn, state);
            c.state_name.insert(state, name.to_string());
        }
        // Mine: the chest 1.5 s before the grant (as in a real replay). The mate's: the
        // chest 6.5 s after. A gizmo: no chest at all. The enemy's: no grant of ours.
        c.treasure_grants = vec![(237.8 + 1.5, 7), (431.9, 7), (600.0, 7)];
        let chest = |time, prize: &str, pawn| PrizeChest { time, prize: Some(prize.into()), pawn };
        c.prize_chests.insert(1, chest(237.8, "DelilahPrize:dpid_item18", 70));
        c.prize_chests.insert(2, chest(438.4, "DelilahPrize:dpid_item17", 80));
        c.prize_chests.insert(3, chest(300.0, "9f73f94d-2e61-4523-97d7-d61235d3365e", 90));
        let (got, nearby) = treasures(&c, 7, Some("Me"), &players);
        let named: Vec<_> = got.iter().map(|t| (t.prize.as_deref(), t.found_by.as_deref())).collect();
        assert_eq!(
            named,
            [(Some("DelilahPrize:dpid_item18"), None), (Some("DelilahPrize:dpid_item17"), Some("Mate")), (None, None)]
        );
        assert_eq!(nearby, [NearbyPrize { time: 300.0, prize: "9f73f94d-2e61-4523-97d7-d61235d3365e".into() }]);
    }
}
