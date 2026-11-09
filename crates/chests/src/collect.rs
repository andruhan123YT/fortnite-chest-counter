//! Gathers the raw facts the chest analysis needs while a replay streams by.

use std::collections::{HashMap, HashSet};

use fnreplay::{ActorDespawn, ActorSpawn, PropertyUpdate, Value, Vec3, Visitor, FAST_ARRAY_ELEMENT};

/// A chest opened in the same update that created it (a dug-up buried chest) is a real
/// open when the searching player stands this close to where it appeared.
const FRESH_OPEN_RADIUS_CM: f64 = 500.0;
use serde::{Deserialize, Serialize};

use crate::schema::Group;

/// `EAthenaGamePhase::Aircraft`: the battle bus is flying. Warmup (2) comes before it.
const AIRCRAFT_PHASE: u32 = 3;

/// The bus launches when the game phase moves on to the bus from an earlier one. A
/// recording that starts with the bus already flying never sees that.
fn bus_launched(previous: Option<u32>, phase: u32) -> bool {
    phase == AIRCRAFT_PHASE && previous.is_some_and(|p| p < AIRCRAFT_PHASE)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChestKind {
    Normal,
    Rare,
    Buried,
    LootCannon,
    Relic,
}

impl ChestKind {
    pub fn from_actor_name(name: &str) -> Self {
        if name.contains("RareChest") {
            Self::Rare
        } else if name.contains("BuriedChest") || name.contains("SpritePosters") || name.contains("Booty") {
            Self::Buried
        } else if name.contains("LootCannon") {
            Self::LootCannon
        } else if name.contains("Relics") {
            Self::Relic
        } else {
            Self::Normal
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Rare => "Rare",
            Self::Buried => "Buried / sprite",
            Self::LootCannon => "Loot cannon",
            Self::Relic => "Relic",
        }
    }

    pub const ALL: [ChestKind; 5] = [Self::Normal, Self::Rare, Self::Buried, Self::LootCannon, Self::Relic];
}

/// Identity of a chest actor: the level-placed name, or archetype plus guid when spawned.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChestRef {
    pub id: String,
    pub is_ammo: bool,
}

impl ChestRef {
    fn from_event(
        group: Option<Group>,
        guid: u32,
        static_actor_id: Option<&str>,
        archetype_path: Option<&str>,
    ) -> Self {
        let id = match static_actor_id {
            Some(id) => id.to_string(),
            None => format!("dyn:{}#{guid}", archetype_path.unwrap_or("?")),
        };
        Self { id, is_ammo: group == Some(Group::AmmoBox) }
    }

    pub fn kind(&self) -> ChestKind {
        ChestKind::from_actor_name(&self.id)
    }
}

#[derive(Debug, Clone)]
pub struct Open {
    pub time: f32,
    pub chest: ChestRef,
    pub pawn: u32,
}

#[derive(Debug, Clone)]
pub struct Despawn {
    pub time: f32,
    pub chest: ChestRef,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PickupSpawn {
    pub time: f32,
    pub location: Vec3,
    /// Pawn guid that dropped it, 0 for loot that came out of the world.
    pub dropped_by: u32,
    /// Thrown out of a chest, ammo box or the like, as opposed to lying on the floor.
    pub tossed: bool,
    pub item: LootKind,
}

/// What a pickup is, as far as telling a chest's loot apart goes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LootKind {
    #[default]
    Other,
    Weapon,
    Ammo,
}

impl LootKind {
    /// From the item definition's path: `.../WID_Shotgun_...` or `.../AthenaAmmoDataShells`.
    pub fn of(path: &str) -> Self {
        let name = path.rsplit(['/', '.']).next().unwrap_or(path);
        if name.starts_with("WID_") {
            Self::Weapon
        } else if name.contains("AmmoData") {
            Self::Ammo
        } else {
            Self::Other
        }
    }
}

#[derive(Debug, Default)]
pub struct Collected {
    pub pawn_positions: HashMap<u32, Vec<(f32, Vec3)>>,
    /// `AttachComponent` changes per pawn; non-zero while seated in a vehicle.
    pub pawn_attachments: HashMap<u32, Vec<(f32, u32)>>,
    pub pawn_state: HashMap<u32, u32>,
    pub state_name: HashMap<u32, String>,
    pub state_team: HashMap<u32, u32>,
    pub state_is_bot: HashMap<u32, bool>,
    pub vehicle_positions: HashMap<u32, Vec<(f32, Vec3)>>,
    pub opens: Vec<Open>,
    /// Chests that named a player in `SearchingPawn` without being searched: the server
    /// does that when a chest is destroyed, so these are breaks with the breaker known.
    pub break_marks: Vec<Open>,
    pub despawns: Vec<Despawn>,
    /// Chests whose health reached zero: hit until they broke.
    pub health_depleted: Vec<Despawn>,
    pub pickups: Vec<PickupSpawn>,
    /// Player state of the player who recorded the replay.
    pub recorder_state: Option<u32>,
    /// The server's id for the match, shared by every replay of it.
    pub session_id: Option<String>,
    /// Treasure hunt prizes granted: `(time, player state)`.
    pub treasure_grants: Vec<(f32, u32)>,
    /// When the battle bus launched. `None` when the recording started after that.
    pub bus_launch: Option<f32>,
    game_phase: Option<u32>,
    /// Index in `pickups` per pickup actor, so later updates can fill in what it is.
    pickup_index: HashMap<(u32, u32), usize>,
    seen_grants: HashSet<(u32, i32)>,
    seen_depleted: HashSet<String>,
    seen_marks: HashSet<String>,
    /// Where and when spawned chests appeared, by actor guid.
    chest_spawns: HashMap<u32, (f32, Vec3)>,
}

/// Undoes the light obfuscation Fortnite applies to human player names in replays:
/// every UTF-16 unit is shifted down by a small amount derived from its position and the
/// name length.
pub fn decode_player_name(raw: &str) -> String {
    let units: Vec<u16> = raw.encode_utf16().collect();
    let len = units.len();
    let decoded: Vec<u16> = units
        .iter()
        .enumerate()
        .map(|(i, &unit)| {
            let shift = ((len % 4 * 3 % 8 + 1 + i) * 3 % 8) as u16;
            unit.wrapping_add(shift)
        })
        .collect();
    String::from_utf16_lossy(&decoded)
}

impl Collected {
    /// Whether this update created the chest already searched, next to its searcher.
    fn opened_on_spawn(&self, u: &PropertyUpdate<'_>, pawn: u32) -> bool {
        let Some(&(spawned, at)) = self.chest_spawns.get(&u.actor_guid) else { return false };
        if (spawned - u.time).abs() > 0.05 {
            return false;
        }
        let Some(&(_, pos)) = self.pawn_positions.get(&pawn).and_then(|track| track.last()) else { return false };
        ((pos.x - at.x).powi(2) + (pos.y - at.y).powi(2)).sqrt() < FRESH_OPEN_RADIUS_CM
    }

    pub fn player_of_pawn(&self, pawn: u32) -> Option<&str> {
        self.pawn_state.get(&pawn).and_then(|s| self.state_name.get(s)).map(String::as_str)
    }
}

impl Visitor for Collected {
    fn on_update(&mut self, u: &PropertyUpdate<'_>) {
        match Group::from_index(u.group) {
            Some(Group::PlayerState) => {
                if let Some(bot) = u.get("bIsABot").and_then(|v| v.as_bool()) {
                    self.state_is_bot.insert(u.actor_guid, bot);
                }
                if let Some(team) = u.get("TeamIndex").and_then(|v| v.as_u32()) {
                    self.state_team.insert(u.actor_guid, team);
                }
                if let Some(raw) = u.get("PlayerNamePrivate").and_then(|v| v.as_str()).filter(|n| !n.is_empty()) {
                    let is_bot = self.state_is_bot.get(&u.actor_guid).copied().unwrap_or(false);
                    let name = if is_bot { raw.to_string() } else { decode_player_name(raw) };
                    self.state_name.insert(u.actor_guid, name);
                }
            }
            Some(Group::Pawn) => {
                if let Some(state) = u.get("PlayerState").and_then(|v| v.as_u32()).filter(|&s| s != 0) {
                    self.pawn_state.insert(u.actor_guid, state);
                }
                if let Some(m) = u.get("ReplicatedMovement").and_then(|v| v.as_movement()) {
                    self.pawn_positions.entry(u.actor_guid).or_default().push((u.time, m.location));
                }
                if u.changed("AttachComponent") {
                    let value = u.get("AttachComponent").and_then(|v| v.as_u32()).unwrap_or(0);
                    self.pawn_attachments.entry(u.actor_guid).or_default().push((u.time, value));
                }
            }
            Some(group @ (Group::Chest | Group::AmmoBox)) => {
                // Pickaxe hits and gunfire show up as falling health. A full state resend
                // (with Role) of an already broken chest is not a new break.
                let depleted = u.get("Health").and_then(|v| v.as_u32()) == Some(0) && !u.changed("Role");
                if depleted && group == Group::Chest {
                    let chest = ChestRef::from_event(Some(group), u.actor_guid, u.static_actor_id, u.archetype_path);
                    if self.seen_depleted.insert(chest.id.clone()) {
                        self.health_depleted.push(Despawn { time: u.time, chest });
                    }
                }
                let searched =
                    u.changed("bAlreadySearched") && u.get("bAlreadySearched").and_then(|v| v.as_bool()) == Some(true);
                let Some(pawn) = u.get("SearchingPawn").and_then(|v| v.as_u32()).filter(|&p| p != 0) else { return };
                let chest = ChestRef::from_event(Some(group), u.actor_guid, u.static_actor_id, u.archetype_path);
                // A chest re-entering relevancy replays its whole state, including an old
                // SearchingPawn; the accompanying Role tells those apart from fresh changes.
                // The exception is a chest that was created already opened.
                let resend = u.changed("Role") && !self.opened_on_spawn(u, pawn);
                if resend {
                    return;
                }
                if searched {
                    self.opens.push(Open { time: u.time, chest, pawn });
                } else if group == Group::Chest && self.seen_marks.insert(chest.id.clone()) {
                    self.break_marks.push(Open { time: u.time, chest, pawn });
                }
            }
            Some(Group::Pickup) => {
                let key = (u.channel, u.actor_guid);
                let tossed = u.get("bTossedFromContainer").and_then(|v| v.as_bool());
                let item =
                    u.get("ItemDefinition").and_then(|v| v.as_u32()).and_then(|g| u.object_path(g)).map(LootKind::of);
                if let Some(&i) = self.pickup_index.get(&key) {
                    // Fields that arrive after the pickup first showed up.
                    let p = &mut self.pickups[i];
                    p.tossed |= tossed.unwrap_or(false);
                    if let Some(item) = item {
                        p.item = item;
                    }
                    return;
                }
                let Some(m) = u.get("ReplicatedMovement").and_then(|v| v.as_movement()) else { return };
                let dropped_by = u.get("PawnWhoDroppedPickup").and_then(|v| v.as_u32()).unwrap_or(0);
                self.pickup_index.insert(key, self.pickups.len());
                self.pickups.push(PickupSpawn {
                    time: u.time,
                    location: m.location,
                    dropped_by,
                    tossed: tossed.unwrap_or(false),
                    item: item.unwrap_or_default(),
                });
            }
            Some(Group::Vehicle) => {
                if let Some(m) = u.get("ReplicatedMovement").and_then(|v| v.as_movement()) {
                    self.vehicle_positions.entry(u.actor_guid).or_default().push((u.time, m.location));
                }
            }
            Some(Group::Abilities) => {
                let Some(&Value::Int(element)) = u.get(FAST_ARRAY_ELEMENT) else { return };
                let Some(ability) = u.get("ability").and_then(|v| v.as_u32()) else { return };
                let is_prize = u.object_path(ability).is_some_and(|p| p.contains("TreasureHandler"));
                // An element is resent when it changes; a prize counts once.
                if is_prize && self.seen_grants.insert((u.actor_guid, element)) {
                    self.treasure_grants.push((u.time, u.actor_guid));
                }
            }
            Some(Group::AbilitiesCache) => {}
            Some(Group::GamePhase) => {
                let Some(phase) = u.get("GamePhase").and_then(|v| v.as_u32()) else { return };
                if self.bus_launch.is_none() && bus_launched(self.game_phase, phase) {
                    self.bus_launch = Some(u.time);
                }
                self.game_phase = Some(phase);
            }
            Some(Group::GameState) => {
                if let Some(state) = u.get("RecorderPlayerState").and_then(|v| v.as_u32()).filter(|&s| s != 0) {
                    self.recorder_state = Some(state);
                }
                if let Some(id) = u.get("GameSessionId").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                    self.session_id = Some(id.to_string());
                }
            }
            None => {}
        }
    }

    fn on_actor_spawn(&mut self, s: &ActorSpawn<'_>) {
        if let (Some(path), Some(location)) = (s.archetype_path, s.location) {
            if path.contains("Chest") {
                self.chest_spawns.insert(s.actor_guid, (s.time, location));
            }
        }
    }

    fn on_despawn(&mut self, d: &ActorDespawn<'_>) {
        if Group::from_index(d.group) == Some(Group::Chest) {
            self.despawns.push(Despawn {
                time: d.time,
                chest: ChestRef::from_event(Some(Group::Chest), d.actor_guid, d.static_actor_id, d.archetype_path),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_obfuscated_names() {
        // Raw values from a Chapter 5 replay and the names they decode to.
        assert_eq!(decode_player_name("NLuNmP"), "SLxTnT");
        assert_eq!(decode_player_name("Iet<qeZlBo"), "NewBrianGo");
        assert_eq!(decode_player_name("^i[[cqeog`j"), "dj_beverman");
    }

    #[test]
    fn bus_launch_needs_the_switch_from_warmup() {
        assert!(bus_launched(Some(2), 3));
        // Recording started with the bus in the air: no launch seen.
        assert!(!bus_launched(None, 3));
        assert!(!bus_launched(Some(3), 3));
        assert!(!bus_launched(Some(3), 4));
    }

    #[test]
    fn classifies_chest_kinds() {
        assert_eq!(ChestKind::from_actor_name("AlwaysSpawn_RareChest_C_UAID_1"), ChestKind::Rare);
        assert_eq!(ChestKind::from_actor_name("dyn:B_Athena_BuriedChest_Chest_C#12"), ChestKind::Buried);
        assert_eq!(ChestKind::from_actor_name("Tiered_Chest_Athena_C_UAID_1"), ChestKind::Normal);
    }
}
