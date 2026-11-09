//! Event dump as JSON lines (`dump-events`), for debugging the parser or diffing it
//! against another one on the same replay.

use std::collections::HashSet;
use std::io::Write;

use fnreplay::{ActorDespawn, PropertyUpdate, Visitor};
use serde_json::{json, Value as Json};

use crate::collect::decode_player_name;
use crate::schema::Group;

pub struct EventDump<W: Write> {
    out: W,
    seen_pickups: HashSet<(u32, u32)>,
    bots: HashSet<u32>,
}

impl<W: Write> EventDump<W> {
    pub fn new(out: W) -> Self {
        Self { out, seen_pickups: HashSet::new(), bots: HashSet::new() }
    }

    fn emit(&mut self, value: Json) {
        let _ = writeln!(self.out, "{value}");
    }
}

/// `Math.round(v * 10) / 10` from JavaScript, which rounds halves towards +infinity.
fn round1(v: f64) -> f64 {
    (v * 10.0 + 0.5).floor() / 10.0
}

fn time(t: f32) -> f64 {
    (f64::from(t) * 1000.0 + 0.5).floor() / 1000.0
}

fn chest_id(guid: u32, static_actor_id: Option<&str>, archetype_path: Option<&str>) -> String {
    match static_actor_id {
        Some(id) => id.to_string(),
        None => format!("dyn:{}#{guid}", archetype_path.unwrap_or("?")),
    }
}

impl<W: Write> Visitor for EventDump<W> {
    fn on_update(&mut self, u: &PropertyUpdate<'_>) {
        let t = time(u.time);
        let g = u.actor_guid;
        match Group::from_index(u.group) {
            Some(Group::PlayerState) => {
                if let Some(bot) = u.get("bIsABot").and_then(|v| v.as_bool()) {
                    if bot {
                        self.bots.insert(g);
                    }
                }
                if let Some(raw) = u.get("PlayerNamePrivate").and_then(|v| v.as_str()).filter(|n| !n.is_empty()) {
                    let name = if self.bots.contains(&g) { raw.to_string() } else { decode_player_name(raw) };
                    self.emit(json!({"k": "name", "t": t, "guid": g, "name": name}));
                }
            }
            Some(Group::Pawn) => {
                if let Some(state) = u.get("PlayerState").and_then(|v| v.as_u32()).filter(|&s| s != 0) {
                    self.emit(json!({"k": "pawn", "t": t, "guid": g, "state": state}));
                }
                if let Some(m) = u.get("ReplicatedMovement").and_then(|v| v.as_movement()) {
                    let p = m.location;
                    self.emit(
                        json!({"k": "pos", "t": t, "guid": g, "x": round1(p.x), "y": round1(p.y), "z": round1(p.z)}),
                    );
                }
                if u.changed("AttachComponent") {
                    let v = u.get("AttachComponent").and_then(|v| v.as_u32()).unwrap_or(0);
                    self.emit(json!({"k": "attach", "t": t, "guid": g, "v": v}));
                }
            }
            Some(Group::Vehicle) => {
                if let Some(m) = u.get("ReplicatedMovement").and_then(|v| v.as_movement()) {
                    let p = m.location;
                    self.emit(
                        json!({"k": "valet", "t": t, "guid": g, "x": round1(p.x), "y": round1(p.y), "z": round1(p.z)}),
                    );
                }
            }
            Some(Group::Chest | Group::AmmoBox) => {
                let mut props: Vec<&str> = u.props.iter().map(|(n, _)| *n).collect();
                props.sort_unstable();
                let searched = if u.changed("bAlreadySearched") {
                    Json::Bool(u.get("bAlreadySearched").and_then(|v| v.as_bool()).unwrap_or(false))
                } else {
                    Json::Null
                };
                let pawn = u.get("SearchingPawn").and_then(|v| v.as_u32()).map_or(Json::Null, Json::from);
                let id = chest_id(g, u.static_actor_id, u.archetype_path);
                self.emit(json!({"k": "chest", "t": t, "id": id, "props": props.join(","), "searched": searched, "pawn": pawn}));
            }
            Some(Group::Pickup) => {
                let Some(m) = u.get("ReplicatedMovement").and_then(|v| v.as_movement()) else { return };
                if !self.seen_pickups.insert((u.channel, g)) {
                    return;
                }
                let p = m.location;
                let dropper = u.get("PawnWhoDroppedPickup").and_then(|v| v.as_u32()).unwrap_or(0);
                let tossed = u.get("bTossedFromContainer").and_then(|v| v.as_bool()).unwrap_or(false);
                let item = u.get("ItemDefinition").and_then(|v| v.as_u32()).and_then(|i| u.object_path(i));
                let item = item.map(|p| p.rsplit(['/', '.']).next().unwrap_or(p));
                self.emit(json!({"k": "pickup", "t": t, "guid": g, "x": round1(p.x), "y": round1(p.y), "z": round1(p.z), "dropper": dropper, "tossed": tossed, "item": item}));
            }
            Some(Group::GameState) => {
                if let Some(state) = u.get("RecorderPlayerState").and_then(|v| v.as_u32()).filter(|&s| s != 0) {
                    self.emit(json!({"k": "recorder", "t": t, "state": state}));
                }
            }
            Some(Group::Abilities) => {
                if let Some(path) = u.get("ability").and_then(|v| v.as_u32()).and_then(|a| u.object_path(a)) {
                    self.emit(json!({"k": "ability", "t": t, "state": g, "ability": path}));
                }
            }
            Some(Group::GamePhase) => {
                if let Some(phase) = u.get("GamePhase").and_then(|v| v.as_u32()) {
                    self.emit(json!({"k": "phase", "t": t, "phase": phase}));
                }
            }
            Some(Group::AbilitiesCache) => {}
            None => {}
        }
    }

    fn on_despawn(&mut self, d: &ActorDespawn<'_>) {
        if matches!(Group::from_index(d.group), Some(Group::Chest | Group::AmmoBox)) {
            let id = chest_id(d.actor_guid, d.static_actor_id, d.archetype_path);
            self.emit(json!({"k": "despawn", "t": time(d.time), "id": id}));
        }
    }
}
