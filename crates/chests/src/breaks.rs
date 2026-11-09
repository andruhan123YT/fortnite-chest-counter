//! Chests destroyed by a pickaxe, gunfire or a vehicle.
//!
//! The server names whoever destroys a chest in its `SearchingPawn` without marking it
//! searched; [`marked_breaks`] reads that directly. Checked against the in-game quest:
//! opens plus marked breaks came to 499 where the quest counted 500.
//!
//! [`detect_breaks`] is the older heuristic, off by default. It looks for a burst of world loot
//! (pickups nobody dropped) at the chest, and then either the chest's health hitting zero
//! (pickaxe or gunfire, at the same moment) or the chest despawning a few seconds later
//! (vehicles destroy it outright). A burst that no open explains, paired with one of
//! those, is a broken chest, and the player standing (or driving) closest to the burst
//! gets the credit. A chest that was opened first is never counted again as broken.

use std::collections::{HashMap, HashSet};

use fnreplay::Vec3;
use serde::{Deserialize, Serialize};

use crate::collect::{ChestRef, Collected};

/// World units are centimeters.
const UNITS_PER_METER: f64 = 100.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BreakConfig {
    /// Pickups closer than this in time and space belong to the same burst.
    pub burst_window_s: f64,
    pub burst_radius_m: f64,
    /// A burst within this time and distance of an open came out of that open.
    pub open_window_s: f64,
    pub open_radius_m: f64,
    /// Three or more chests vanishing within this window is level streaming, not breaking.
    pub streaming_batch_window_s: f64,
    pub streaming_batch_size: usize,
    /// Accepted delay from the loot burst to the chest despawn.
    pub despawn_delay_min_s: f64,
    pub despawn_delay_max_s: f64,
    /// Typical delay; pairings closest to it win.
    pub despawn_delay_typical_s: f64,
    /// Accepted gap between a loot burst and the chest's health reaching zero.
    pub health_zero_window_s: f64,
    pub max_distance_on_foot_m: f64,
    pub max_distance_in_vehicle_m: f64,
    /// How far a vehicle may be from the pawn at the moment the pawn gets attached to it.
    pub vehicle_attach_radius_m: f64,
    /// Positions older than this are considered unknown.
    pub stale_position_s: f64,
    /// Also credit breaks found by the loot-burst heuristic, for replays without break
    /// marks. Over-counts on current replays.
    pub use_loot_heuristic: bool,
}

impl Default for BreakConfig {
    fn default() -> Self {
        Self {
            burst_window_s: 0.4,
            burst_radius_m: 3.0,
            open_window_s: 1.2,
            open_radius_m: 6.0,
            streaming_batch_window_s: 0.15,
            streaming_batch_size: 3,
            despawn_delay_min_s: 2.9,
            despawn_delay_max_s: 5.7,
            despawn_delay_typical_s: 4.6,
            health_zero_window_s: 1.0,
            max_distance_on_foot_m: 5.0,
            max_distance_in_vehicle_m: 8.0,
            vehicle_attach_radius_m: 15.0,
            stale_position_s: 3.0,
            use_loot_heuristic: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Break {
    pub time: f32,
    pub chest: ChestRef,
    pub player: String,
    pub distance_m: f64,
    pub in_vehicle: bool,
    pub location: Option<Vec3>,
}

#[derive(Debug, Clone, Copy)]
struct Seat {
    from: f32,
    until: f32,
    vehicle: u32,
}

#[derive(Debug, Default)]
struct Track {
    positions: Vec<(f32, Vec3)>,
    seats: Vec<Seat>,
}

/// Where every player was over time, following them into vehicles.
pub struct Tracks<'a> {
    by_player: Vec<(String, Track)>,
    vehicles: &'a HashMap<u32, Vec<(f32, Vec3)>>,
    stale_s: f64,
}

fn distance_2d(a: Vec3, b: Vec3) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

/// Latest sample at or before `t`.
fn sample_at<T: Copy>(series: &[(f32, T)], t: f64) -> Option<(f32, T)> {
    let idx = series.partition_point(|&(st, _)| f64::from(st) <= t);
    idx.checked_sub(1).map(|i| series[i])
}

impl<'a> Tracks<'a> {
    pub fn build(c: &'a Collected, cfg: &BreakConfig) -> Self {
        let mut pawns_by_player: HashMap<&str, Vec<u32>> = HashMap::new();
        let mut pawns: Vec<u32> = c.pawn_positions.keys().copied().collect();
        pawns.sort_unstable();
        for pawn in pawns {
            if let Some(name) = c.player_of_pawn(pawn) {
                pawns_by_player.entry(name).or_default().push(pawn);
            }
        }

        let mut by_player: Vec<(String, Track)> = pawns_by_player
            .into_iter()
            .map(|(name, pawns)| {
                let mut positions: Vec<(f32, Vec3)> =
                    pawns.iter().flat_map(|p| c.pawn_positions[p].iter().copied()).collect();
                positions.sort_by(|a, b| a.0.total_cmp(&b.0));
                let mut attachments: Vec<(f32, u32)> =
                    pawns.iter().filter_map(|p| c.pawn_attachments.get(p)).flatten().copied().collect();
                attachments.sort_by(|a, b| a.0.total_cmp(&b.0));

                let mut seats = Vec::new();
                for (i, &(from, component)) in attachments.iter().enumerate() {
                    if component == 0 {
                        continue;
                    }
                    let until = attachments.get(i + 1).map_or(f32::MAX, |a| a.0);
                    let Some((_, pawn_at)) = sample_at(&positions, f64::from(from) + 0.05) else { continue };
                    if let Some((vehicle, distance)) =
                        nearest_vehicle(&c.vehicle_positions, pawn_at, f64::from(from) + 0.5)
                    {
                        if distance < cfg.vehicle_attach_radius_m * UNITS_PER_METER {
                            seats.push(Seat { from, until, vehicle });
                        }
                    }
                }
                (name.to_string(), Track { positions, seats })
            })
            .collect();
        by_player.sort_by(|a, b| a.0.cmp(&b.0));

        Self { by_player, vehicles: &c.vehicle_positions, stale_s: cfg.stale_position_s }
    }

    pub fn players(&self) -> impl Iterator<Item = &str> {
        self.by_player.iter().map(|(n, _)| n.as_str())
    }

    /// Position of a player at `t` and whether they were in a vehicle. While seated the
    /// pawn stops replicating movement, so the vehicle's position stands in for it.
    pub fn position(&self, player: &str, t: f64) -> Option<(Vec3, bool)> {
        let track = &self.by_player.iter().find(|(n, _)| n == player)?.1;
        if let Some(seat) = track.seats.iter().find(|s| t >= f64::from(s.from) && t < f64::from(s.until)) {
            let (st, p) = sample_at(self.vehicles.get(&seat.vehicle)?, t)?;
            return (t - f64::from(st) < self.stale_s).then_some((p, true));
        }
        let (st, p) = sample_at(&track.positions, t)?;
        (t - f64::from(st) < self.stale_s).then_some((p, false))
    }
}

fn nearest_vehicle(vehicles: &HashMap<u32, Vec<(f32, Vec3)>>, at: Vec3, t: f64) -> Option<(u32, f64)> {
    let mut ids: Vec<u32> = vehicles.keys().copied().collect();
    ids.sort_unstable();
    ids.into_iter()
        .filter_map(|id| {
            let series = &vehicles[&id];
            let (_, p) = sample_at(series, t).or_else(|| series.first().copied())?;
            Some((id, distance_2d(p, at)))
        })
        .fold(None, |best: Option<(u32, f64)>, cand| match best {
            Some(b) if b.1 <= cand.1 => Some(b),
            _ => Some(cand),
        })
}

#[derive(Debug)]
struct Burst {
    time: f32,
    location: Vec3,
    from_open: bool,
}

/// Breaks the server attributed by name. A chest someone opened first is skipped: the
/// open took its roll.
pub fn marked_breaks(c: &Collected, tracks: &Tracks<'_>) -> Vec<Break> {
    let opened: HashSet<&str> = c.opens.iter().map(|o| o.chest.id.as_str()).collect();
    let mut breaks: Vec<Break> = c
        .break_marks
        .iter()
        .filter(|m| !m.chest.is_ammo && !opened.contains(m.chest.id.as_str()))
        .filter_map(|m| {
            let player = c.player_of_pawn(m.pawn)?;
            let position = tracks.position(player, f64::from(m.time));
            Some(Break {
                time: m.time,
                chest: m.chest.clone(),
                player: player.to_string(),
                distance_m: 0.0,
                in_vehicle: position.is_some_and(|(_, seated)| seated),
                location: position.map(|(p, _)| p),
            })
        })
        .collect();
    breaks.sort_by(|a, b| a.time.total_cmp(&b.time));
    breaks
}

pub fn detect_breaks(c: &Collected, tracks: &Tracks<'_>, cfg: &BreakConfig) -> Vec<Break> {
    let mut world_loot: Vec<_> = c.pickups.iter().filter(|p| p.dropped_by == 0).collect();
    world_loot.sort_by(|a, b| a.time.total_cmp(&b.time));

    let mut bursts: Vec<Burst> = Vec::new();
    for p in world_loot {
        let joins = bursts.iter().any(|b| {
            f64::from((b.time - p.time).abs()) < cfg.burst_window_s
                && distance_2d(b.location, p.location) < cfg.burst_radius_m * UNITS_PER_METER
        });
        if !joins {
            bursts.push(Burst { time: p.time, location: p.location, from_open: false });
        }
    }

    for burst in &mut bursts {
        burst.from_open = c.opens.iter().any(|o| {
            if f64::from((o.time - burst.time).abs()) > cfg.open_window_s {
                return false;
            }
            let Some(player) = c.player_of_pawn(o.pawn) else { return false };
            tracks
                .position(player, f64::from(o.time))
                .is_some_and(|(p, _)| distance_2d(p, burst.location) < cfg.open_radius_m * UNITS_PER_METER)
        });
    }

    let mut despawns: Vec<_> = c.despawns.iter().collect();
    despawns.sort_by(|a, b| a.time.total_cmp(&b.time));
    let streamed: Vec<bool> = despawns
        .iter()
        .map(|d| {
            despawns.iter().filter(|o| f64::from((o.time - d.time).abs()) < cfg.streaming_batch_window_s).count()
                >= cfg.streaming_batch_size
        })
        .collect();

    // Chest events that confirm a burst: health hitting zero right at the burst, or a
    // despawn a few seconds after it. Health is the stronger signal and pairs first.
    let mut pairs: Vec<(&ChestRef, usize, f64)> = Vec::new();
    for d in &c.health_depleted {
        for (bi, b) in bursts.iter().enumerate() {
            let gap = f64::from((d.time - b.time).abs());
            if !b.from_open && gap <= cfg.health_zero_window_s {
                pairs.push((&d.chest, bi, gap - 1000.0));
            }
        }
    }
    for (di, d) in despawns.iter().enumerate() {
        if streamed[di] {
            continue;
        }
        for (bi, b) in bursts.iter().enumerate() {
            if b.from_open {
                continue;
            }
            let delay = f64::from(d.time - b.time);
            if (cfg.despawn_delay_min_s..=cfg.despawn_delay_max_s).contains(&delay) {
                pairs.push((&d.chest, bi, (delay - cfg.despawn_delay_typical_s).abs()));
            }
        }
    }
    pairs.sort_by(|a, b| a.2.total_cmp(&b.2));

    // A chest counts once: opening it took its roll, breaking it afterwards does not add one.
    let mut used_chests: HashSet<&str> = c.opens.iter().map(|o| o.chest.id.as_str()).collect();
    let mut used_bursts = vec![false; bursts.len()];
    let mut breaks = Vec::new();
    for (chest, bi, _) in pairs {
        if used_bursts[bi] || used_chests.contains(chest.id.as_str()) {
            continue;
        }
        used_chests.insert(chest.id.as_str());
        used_bursts[bi] = true;
        if chest.is_ammo || chest.id.to_ascii_lowercase().contains("ammo") {
            continue;
        }
        let burst = &bursts[bi];
        let t = f64::from(burst.time);
        let nearest = tracks
            .players()
            .filter_map(|name| {
                let (p, in_vehicle) = tracks.position(name, t)?;
                Some((name, distance_2d(p, burst.location) / UNITS_PER_METER, in_vehicle))
            })
            .fold(None, |best: Option<(&str, f64, bool)>, cand| match best {
                Some(b) if b.1 <= cand.1 => Some(b),
                _ => Some(cand),
            });
        let Some((player, distance_m, in_vehicle)) = nearest else { continue };
        let limit = if in_vehicle { cfg.max_distance_in_vehicle_m } else { cfg.max_distance_on_foot_m };
        if distance_m <= limit {
            breaks.push(Break {
                time: burst.time,
                chest: chest.clone(),
                player: player.to_string(),
                distance_m,
                in_vehicle,
                location: Some(burst.location),
            });
        }
    }
    breaks.sort_by(|a, b| a.time.total_cmp(&b.time));
    breaks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collect::{Despawn, Open, PickupSpawn};

    fn v(x: f64, y: f64) -> Vec3 {
        Vec3::new(x, y, 0.0)
    }

    /// One player walking along the x axis, one chest at x = 1000 (10 m).
    fn scene() -> Collected {
        let mut c = Collected::default();
        c.pawn_state.insert(10, 20);
        c.state_name.insert(20, "Hero".into());
        c.pawn_positions.insert(10, (0..200).map(|i| (i as f32 * 0.1, v(i as f64 * 10.0, 0.0))).collect());
        c
    }

    fn chest(id: &str) -> ChestRef {
        ChestRef { id: id.into(), is_ammo: false }
    }

    #[test]
    fn credits_a_break_to_the_nearby_player() {
        let mut c = scene();
        for dx in [0.0, 30.0, 60.0] {
            c.pickups.push(PickupSpawn { time: 10.0, location: v(1000.0 + dx, 50.0), dropped_by: 0 });
        }
        c.despawns.push(Despawn { time: 14.4, chest: chest("Tiered_Chest_Athena_C_UAID_1") });

        let cfg = BreakConfig::default();
        let tracks = Tracks::build(&c, &cfg);
        let breaks = detect_breaks(&c, &tracks, &cfg);
        assert_eq!(breaks.len(), 1);
        assert_eq!(breaks[0].player, "Hero");
        assert!(!breaks[0].in_vehicle);
    }

    #[test]
    fn loot_from_an_open_is_not_a_break() {
        let mut c = scene();
        c.pickups.push(PickupSpawn { time: 10.0, location: v(1000.0, 50.0), dropped_by: 0 });
        c.opens.push(Open { time: 9.8, chest: chest("Tiered_Chest_Athena_C_UAID_1"), pawn: 10 });
        c.despawns.push(Despawn { time: 14.4, chest: chest("Tiered_Chest_Athena_C_UAID_2") });

        let cfg = BreakConfig::default();
        let tracks = Tracks::build(&c, &cfg);
        assert!(detect_breaks(&c, &tracks, &cfg).is_empty());
    }

    #[test]
    fn health_hitting_zero_confirms_a_break_without_a_despawn() {
        let mut c = scene();
        c.pickups.push(PickupSpawn { time: 10.0, location: v(1000.0, 50.0), dropped_by: 0 });
        c.health_depleted.push(Despawn { time: 10.2, chest: chest("Tiered_Chest_Athena_C_UAID_1") });
        let cfg = BreakConfig::default();
        let tracks = Tracks::build(&c, &cfg);
        let breaks = detect_breaks(&c, &tracks, &cfg);
        assert_eq!(breaks.len(), 1);
        assert_eq!(breaks[0].player, "Hero");
    }

    #[test]
    fn a_chest_opened_before_it_broke_counts_once() {
        let mut c = scene();
        // Opened at 3 s far from the later burst, then broken at 10 s: the burst is not
        // explained by the open, but the chest already had its roll.
        c.opens.push(Open { time: 3.0, chest: chest("Tiered_Chest_Athena_C_UAID_1"), pawn: 10 });
        c.pickups.push(PickupSpawn { time: 10.0, location: v(1000.0, 50.0), dropped_by: 0 });
        c.health_depleted.push(Despawn { time: 10.1, chest: chest("Tiered_Chest_Athena_C_UAID_1") });
        c.despawns.push(Despawn { time: 14.4, chest: chest("Tiered_Chest_Athena_C_UAID_1") });
        let cfg = BreakConfig::default();
        let tracks = Tracks::build(&c, &cfg);
        assert!(detect_breaks(&c, &tracks, &cfg).is_empty());
    }

    #[test]
    fn marks_name_the_breaker_and_skip_opened_chests() {
        let mut c = scene();
        c.break_marks.push(Open { time: 5.0, chest: chest("Tiered_Chest_Athena_C_UAID_1"), pawn: 10 });
        c.break_marks.push(Open { time: 9.0, chest: chest("Tiered_Chest_Athena_C_UAID_2"), pawn: 10 });
        c.opens.push(Open { time: 8.0, chest: chest("Tiered_Chest_Athena_C_UAID_2"), pawn: 10 });
        // A pawn nobody owns cannot be credited.
        c.break_marks.push(Open { time: 12.0, chest: chest("Tiered_Chest_Athena_C_UAID_3"), pawn: 99 });
        let tracks = Tracks::build(&c, &BreakConfig::default());
        let breaks = marked_breaks(&c, &tracks);
        assert_eq!(breaks.len(), 1);
        assert_eq!(breaks[0].player, "Hero");
        assert_eq!(breaks[0].chest.id, "Tiered_Chest_Athena_C_UAID_1");
        assert!(breaks[0].location.is_some());
    }

    #[test]
    fn streaming_batches_are_ignored() {
        let mut c = scene();
        c.pickups.push(PickupSpawn { time: 10.0, location: v(1000.0, 0.0), dropped_by: 0 });
        for i in 0..3 {
            c.despawns
                .push(Despawn { time: 14.4 + i as f32 * 0.01, chest: chest(&format!("Tiered_Chest_Athena_C_{i}")) });
        }
        let cfg = BreakConfig::default();
        let tracks = Tracks::build(&c, &cfg);
        assert!(detect_breaks(&c, &tracks, &cfg).is_empty());
    }

    #[test]
    fn far_players_get_no_credit() {
        let mut c = scene();
        c.pickups.push(PickupSpawn { time: 1.0, location: v(5000.0, 0.0), dropped_by: 0 });
        c.despawns.push(Despawn { time: 5.6, chest: chest("Tiered_Chest_Athena_C_UAID_1") });
        let cfg = BreakConfig::default();
        let tracks = Tracks::build(&c, &cfg);
        assert!(detect_breaks(&c, &tracks, &cfg).is_empty());
    }

    #[test]
    fn follows_the_player_into_a_vehicle() {
        let mut c = scene();
        c.pawn_attachments.insert(10, vec![(5.0, 77), (15.0, 0)]);
        // The car waits at x = 500 until the player gets in, then drives off at 10 m/s.
        let car_x = |t: f64| if t < 5.5 { 500.0 } else { 500.0 + (t - 5.5) * 1000.0 };
        c.vehicle_positions.insert(500, (0..200).map(|i| (i as f32 * 0.1, v(car_x(i as f64 * 0.1), 0.0))).collect());
        // At t = 8 s the car is at x = 3000; the pawn track would say 800.
        c.pickups.push(PickupSpawn { time: 8.0, location: v(3000.0, 300.0), dropped_by: 0 });
        c.despawns.push(Despawn { time: 12.6, chest: chest("Tiered_Chest_Athena_C_UAID_1") });
        let cfg = BreakConfig::default();
        let tracks = Tracks::build(&c, &cfg);
        let breaks = detect_breaks(&c, &tracks, &cfg);
        assert_eq!(breaks.len(), 1);
        assert!(breaks[0].in_vehicle);
    }
}
