//! Opens the recording lost.
//!
//! Some replays go without chest updates for minutes while everything else keeps
//! recording, the loot included: a player opens chest after chest, and all the replay
//! shows is their loot landing at the player's feet. A chest throws out a weapon and a
//! couple of other items, so such a burst with nobody's recorded open, break or ammo box
//! near it in time is a chest the recording missed, credited to the player standing at it.
//!
//! Loot a player drops (dying, or by hand) names who dropped it and is not tossed from a
//! container, and floor loot is not tossed either, so neither can pass for a chest.

use fnreplay::Vec3;

use crate::breaks::Tracks;
use crate::collect::{Collected, LootKind};

/// World units are centimeters.
const UNITS_PER_METER: f64 = 100.0;
/// Items landing within this time and distance of the first come from the same container.
const BURST_WINDOW_S: f32 = 1.5;
const BURST_RADIUS_M: f64 = 3.0;
/// A chest's worth of loot. Ammo boxes throw fewer items and no weapon; llamas and
/// supply drops throw more.
const CHEST_ITEMS: std::ops::RangeInclusive<usize> = 3..=6;
/// A recorded open, break or ammo box this close in time and space explains a burst.
const EXPLAIN_WINDOW_S: f32 = 3.0;
const EXPLAIN_RADIUS_M: f64 = 8.0;
/// How close the player must be to the loot to be the one who opened the chest.
const ON_FOOT_M: f64 = 4.0;
const IN_VEHICLE_M: f64 = 8.0;

#[derive(Debug, Clone)]
pub struct LostOpen {
    pub time: f32,
    pub player: String,
    pub location: Vec3,
    pub in_vehicle: bool,
}

#[derive(Debug)]
struct Burst {
    time: f32,
    location: Vec3,
    items: usize,
    weapons: usize,
}

fn distance_m(a: Vec3, b: Vec3) -> f64 {
    (a.x - b.x).hypot(a.y - b.y) / UNITS_PER_METER
}

/// Container loot grouped into what each container threw out.
fn bursts(c: &Collected) -> Vec<Burst> {
    let after_bus = c.bus_launch.unwrap_or(0.0);
    let mut loot: Vec<_> = c.pickups.iter().filter(|p| p.tossed && p.dropped_by == 0 && p.time >= after_bus).collect();
    loot.sort_by(|a, b| a.time.total_cmp(&b.time));
    let mut bursts: Vec<Burst> = Vec::new();
    for p in loot {
        let weapon = usize::from(p.item == LootKind::Weapon);
        let joined = bursts
            .iter_mut()
            .rev()
            .take_while(|b| p.time - b.time <= BURST_WINDOW_S)
            .find(|b| distance_m(b.location, p.location) < BURST_RADIUS_M);
        match joined {
            Some(b) => {
                b.items += 1;
                b.weapons += weapon;
            }
            None => bursts.push(Burst { time: p.time, location: p.location, items: 1, weapons: weapon }),
        }
    }
    bursts
}

/// Whether a recorded chest or ammo box event accounts for the burst.
fn explained(c: &Collected, tracks: &Tracks<'_>, burst: &Burst) -> bool {
    let recorded = c.opens.iter().chain(&c.break_marks);
    let by_player = recorded.filter(|e| (e.time - burst.time).abs() <= EXPLAIN_WINDOW_S).any(|e| {
        let at = c.player_of_pawn(e.pawn).and_then(|name| tracks.position(name, f64::from(e.time)));
        // Without a position the time alone has to do.
        at.is_none_or(|(p, _)| distance_m(p, burst.location) <= EXPLAIN_RADIUS_M)
    });
    // A chest hit until it broke, with nobody named.
    let depleted = c.health_depleted.iter().any(|d| (d.time - burst.time).abs() <= EXPLAIN_WINDOW_S / 2.0);
    by_player || depleted
}

pub fn lost_opens(c: &Collected, tracks: &Tracks<'_>) -> Vec<LostOpen> {
    bursts(c)
        .into_iter()
        .filter(|b| CHEST_ITEMS.contains(&b.items) && b.weapons > 0)
        .filter(|b| !explained(c, tracks, b))
        .filter_map(|b| {
            let t = f64::from(b.time);
            let (player, distance, in_vehicle) = tracks
                .players()
                .filter_map(|name| {
                    let (p, seated) = tracks.position(name, t)?;
                    Some((name, distance_m(p, b.location), seated))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))?;
            let reach = if in_vehicle { IN_VEHICLE_M } else { ON_FOOT_M };
            (distance <= reach).then(|| LostOpen {
                time: b.time,
                player: player.to_string(),
                location: b.location,
                in_vehicle,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::breaks::BreakConfig;
    use crate::collect::{ChestRef, Open, PickupSpawn};

    fn v(x: f64, y: f64) -> Vec3 {
        Vec3 { x, y, z: 0.0 }
    }

    /// A player standing at (0, 0) all match.
    fn match_with_player() -> Collected {
        let mut c = Collected::default();
        c.state_name.insert(1, "Me".into());
        c.pawn_state.insert(10, 1);
        c.pawn_positions.insert(10, (0..200).map(|t| (t as f32, v(0.0, 0.0))).collect());
        c
    }

    fn loot(c: &mut Collected, time: f32, at: Vec3, items: &[LootKind], tossed: bool, dropped_by: u32) {
        for (i, &item) in items.iter().enumerate() {
            let location = v(at.x + i as f64 * 20.0, at.y);
            c.pickups.push(PickupSpawn { time: time + i as f32 * 0.1, location, dropped_by, tossed, item });
        }
    }

    const CHEST: [LootKind; 3] = [LootKind::Weapon, LootKind::Other, LootKind::Ammo];

    fn lost(c: &Collected) -> Vec<LostOpen> {
        let cfg = BreakConfig::default();
        lost_opens(c, &Tracks::build(c, &cfg))
    }

    #[test]
    fn unrecorded_chest_loot_at_the_player_counts() {
        let mut c = match_with_player();
        loot(&mut c, 50.0, v(150.0, 0.0), &CHEST, true, 0);
        let got = lost(&c);
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].player.as_str(), got[0].time), ("Me", 50.0));
    }

    #[test]
    fn recorded_opens_ammo_boxes_and_drops_do_not() {
        let mut c = match_with_player();
        // A recorded open right there.
        loot(&mut c, 50.0, v(150.0, 0.0), &CHEST, true, 0);
        c.opens.push(Open { time: 49.8, chest: ChestRef { id: "chest".into(), is_ammo: false }, pawn: 10 });
        // An ammo box: no weapon.
        loot(&mut c, 80.0, v(150.0, 0.0), &[LootKind::Ammo, LootKind::Ammo, LootKind::Ammo], true, 0);
        // A player's inventory dropped on death: named dropper, not from a container.
        loot(&mut c, 110.0, v(150.0, 0.0), &CHEST, false, 77);
        // Floor loot.
        loot(&mut c, 140.0, v(150.0, 0.0), &CHEST, false, 0);
        // A supply drop or llama: too much at once.
        loot(&mut c, 170.0, v(150.0, 0.0), &[LootKind::Weapon; 9], true, 0);
        assert!(lost(&c).is_empty());
    }

    #[test]
    fn loot_far_from_everyone_is_nobody_s() {
        let mut c = match_with_player();
        loot(&mut c, 50.0, v(2000.0, 0.0), &CHEST, true, 0);
        assert!(lost(&c).is_empty());
    }

    #[test]
    fn reads_item_kinds() {
        assert_eq!(LootKind::of("/Game/Athena/Items/Weapons/WID_Shotgun_Standard_Athena_C_Ore_T03"), LootKind::Weapon);
        assert_eq!(LootKind::of("/Game/Athena/Items/Ammo/AthenaAmmoDataShells.AthenaAmmoDataShells"), LootKind::Ammo);
        assert_eq!(
            LootKind::of("/Game/Athena/Items/Consumables/Shields/Athena_Shields.Athena_Shields"),
            LootKind::Other
        );
    }
}
