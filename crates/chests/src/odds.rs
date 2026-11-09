//! Drop odds for the Treasure Hunters chest rewards.

/// A reward that can drop from any chest, opened or destroyed, with a flat per-chest chance.
///
/// Every reward is squad-wide: a chest any squad member opens or breaks rolls for the
/// whole squad. Replays show it: prize grants arrive in matches where the recorder
/// touched no chest, and there are far more of them than the recorder's own chests
/// could explain (39 against about 14 expected over 901 chests).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reward {
    pub name: &'static str,
    pub one_in: f64,
}

pub const PIRATE_SHOP_ACCESS: Reward = Reward { name: "Pirate Shop Access", one_in: 137_500.0 };

pub const REWARDS: [Reward; 7] = [
    Reward { name: "Weekly Gold Outfit", one_in: 1_562_500.0 },
    PIRATE_SHOP_ACCESS,
    Reward { name: "Weekly Vehicle & Decal", one_in: 7_813.0 },
    Reward { name: "Weekly Pickaxe or Back Bling", one_in: 568.0 },
    Reward { name: "Weekly Jam Track", one_in: 391.0 },
    Reward { name: "Weekly Loading Screen", one_in: 272.0 },
    Reward { name: "Gizmo", one_in: 127.0 },
];

/// Expected prizes of any kind per chest.
pub fn any_reward_rate() -> f64 {
    REWARDS.iter().map(|r| 1.0 / r.one_in).sum()
}

/// Probability of at least one drop in `chests` independent rolls.
pub fn chance_at_least_one(chests: u64, one_in: f64) -> f64 {
    if chests == 0 || one_in <= 1.0 {
        return if one_in <= 1.0 && chests > 0 { 1.0 } else { 0.0 };
    }
    // 1 - (1 - p)^n, computed without losing precision for tiny p.
    -((chests as f64) * (-1.0 / one_in).ln_1p()).exp_m1()
}

pub fn expected_drops(chests: u64, one_in: f64) -> f64 {
    chests as f64 / one_in
}

/// Chests needed to reach `probability` of at least one drop.
pub fn chests_for_probability(probability: f64, one_in: f64) -> u64 {
    let probability = probability.clamp(0.0, 0.999_999);
    ((-probability).ln_1p() / (-1.0 / one_in).ln_1p()).ceil() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_closed_form() {
        let p = chance_at_least_one(1030, 137_500.0);
        assert!((p - 0.007463).abs() < 1e-5, "{p}");
        assert!((chance_at_least_one(10_000, 137_500.0) - 0.0701).abs() < 1e-3);
        assert_eq!(chance_at_least_one(0, 127.0), 0.0);
    }

    #[test]
    fn median_and_ninetieth_percentile() {
        assert_eq!(chests_for_probability(0.5, 137_500.0), 95_308);
        assert!((chests_for_probability(0.9, 137_500.0) as i64 - 316_605).abs() <= 1);
    }

    #[test]
    fn expected_gizmos() {
        assert!((expected_drops(1030, 127.0) - 8.11).abs() < 0.01);
    }
}
