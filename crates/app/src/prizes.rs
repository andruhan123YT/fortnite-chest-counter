//! What a treasure hunt prize is, from the id its golden chest carries, and when the
//! Treasure Hours with their boosted odds run. A copy of the list ships in the exe; a
//! newer one comes from the public repository with the update check, so a new week's
//! names and a moved Treasure Hour show up without a release.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::config::data_dir;
use crate::i18n::{tr, trf};

const BUILT_IN: &str = include_str!("../../../data/prizes.json");
pub const URL: &str = "https://raw.githubusercontent.com/andruhan123YT/fortnite-chest-counter/main/data/prizes.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Outfit,
    /// A pickaxe or a back bling, depending on the week.
    Pickaxe,
    Car,
    Jam,
    Loading,
    PirateShop,
    /// A kind a newer list knows and this version does not.
    #[serde(other)]
    Other,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Outfit => tr("Gold outfit"),
            Self::Pickaxe => tr("Pickaxe or back bling"),
            Self::Car => tr("Car decal"),
            Self::Jam => tr("Jam track"),
            Self::Loading => tr("Loading screen"),
            Self::PirateShop => tr("Pirate Shop Access"),
            Self::Other => tr("Prize"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Prize {
    #[serde(default)]
    pub week: Option<u32>,
    pub kind: Kind,
    /// Left out until the week's prizes are known.
    #[serde(default)]
    pub name: Option<String>,
}

impl Prize {
    /// The prize's own name, or what kind of prize it is when the name is not known yet.
    pub fn title(&self) -> String {
        match (&self.name, self.week) {
            (Some(name), _) => name.clone(),
            (None, Some(week)) => trf("{0}, week {1}", &[&self.kind.label(), &week]),
            (None, None) => self.kind.label().to_string(),
        }
    }

    /// What kind of prize, and its week: "Car decal · week 2".
    pub fn detail(&self) -> String {
        match (self.week, self.name.is_some()) {
            (Some(week), true) => trf("{0} · week {1}", &[&self.kind.label(), &week]),
            (_, true) | (None, false) => self.kind.label().to_string(),
            (Some(_), false) => String::new(),
        }
    }
}

/// How much likelier a prize is during a Treasure Hour.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct Boosts {
    #[serde(default = "no_boost")]
    pub pirate_shop: f64,
    #[serde(default = "no_boost")]
    pub outfit: f64,
    #[serde(default = "no_boost")]
    pub car: f64,
}

fn no_boost() -> f64 {
    1.0
}

impl Default for Boosts {
    fn default() -> Self {
        Self { pirate_shop: 1.0, outfit: 1.0, car: 1.0 }
    }
}

impl Boosts {
    /// "Pirate Shop Access ×1.5 · Gold outfit ×7.5 · Car decal ×10", leaving out what is
    /// not boosted.
    pub fn text(&self) -> String {
        [(Kind::PirateShop, self.pirate_shop), (Kind::Outfit, self.outfit), (Kind::Car, self.car)]
            .into_iter()
            .filter(|&(_, x)| x > 1.0)
            .map(|(kind, x)| format!("{} ×{}", kind.label(), times(x)))
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

/// `1.5` as "1.5" and `10.0` as "10".
pub fn times(x: f64) -> String {
    let text = format!("{x:.1}");
    text.strip_suffix(".0").map_or(text.clone(), str::to_owned)
}

/// A window of boosted odds, as Epic announced it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TreasureHour {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    #[serde(default)]
    pub boosts: Boosts,
    /// False for windows expected from the pattern but not announced yet.
    #[serde(default = "confirmed")]
    pub confirmed: bool,
}

fn confirmed() -> bool {
    true
}

impl TreasureHour {
    pub fn contains(&self, at: DateTime<Utc>) -> bool {
        self.start <= at && at < self.end
    }
}

#[derive(Debug, Clone, Deserialize)]
struct Catalog {
    /// `YYYY-MM-DD`; a list replaces the one in use only when it is at least as new.
    updated: String,
    prizes: HashMap<String, Prize>,
    #[serde(default)]
    treasure_hours: Vec<TreasureHour>,
}

static CATALOG: RwLock<Option<Catalog>> = RwLock::new(None);

fn parse(text: &str) -> Option<Catalog> {
    serde_json::from_str(text).ok()
}

fn cache_path() -> PathBuf {
    data_dir().join("prizes.json")
}

/// The built-in list, or the one saved from GitHub when that is newer.
pub fn load() {
    let built_in = parse(BUILT_IN).expect("the built-in prize list parses");
    let saved = std::fs::read_to_string(cache_path()).ok().and_then(|t| parse(&t));
    let best = match saved {
        Some(saved) if saved.updated >= built_in.updated => saved,
        _ => built_in,
    };
    *CATALOG.write().unwrap_or_else(|e| e.into_inner()) = Some(best);
}

/// Takes a list fetched from GitHub when it parses and is not older than the one in use,
/// and saves it for the next start. True when it was taken.
pub fn accept(text: &str) -> bool {
    let Some(fresh) = parse(text) else { return false };
    let mut catalog = CATALOG.write().unwrap_or_else(|e| e.into_inner());
    if catalog.as_ref().is_some_and(|c| c.updated > fresh.updated) {
        return false;
    }
    let _ = std::fs::create_dir_all(data_dir()).and_then(|_| std::fs::write(cache_path(), text));
    *catalog = Some(fresh);
    true
}

/// The prize behind a golden chest's id.
pub fn lookup(id: &str) -> Option<Prize> {
    with_catalog(|c| c.prizes.get(id).cloned())
}

fn with_catalog<T>(f: impl FnOnce(&Catalog) -> T) -> T {
    let catalog = CATALOG.read().unwrap_or_else(|e| e.into_inner());
    match catalog.as_ref() {
        Some(c) => f(c),
        None => f(&parse(BUILT_IN).expect("the built-in prize list parses")),
    }
}

/// Every Treasure Hour the list knows, in order.
pub fn treasure_hours() -> Vec<TreasureHour> {
    let mut hours = with_catalog(|c| c.treasure_hours.clone());
    hours.sort_by_key(|h| h.start);
    hours
}

/// The Treasure Hour running at `at`.
pub fn hour_at(hours: &[TreasureHour], at: DateTime<Utc>) -> Option<&TreasureHour> {
    hours.iter().find(|h| h.contains(at))
}

/// The next Treasure Hour to start after `at`.
pub fn next_hour(hours: &[TreasureHour], at: DateTime<Utc>) -> Option<&TreasureHour> {
    hours.iter().filter(|h| h.start > at).min_by_key(|h| h.start)
}

/// How much likelier Pirate Shop Access was from a chest looted at `at`.
pub fn pirate_shop_boost(hours: &[TreasureHour], at: DateTime<Utc>) -> f64 {
    hour_at(hours, at).map_or(1.0, |h| h.boosts.pirate_shop)
}

/// How long until something, roughly: "2 d 3 h", "3 h 12 min", "14 min".
pub fn countdown(left: chrono::Duration) -> String {
    let minutes = left.num_minutes().max(1);
    let (days, hours, mins) = (minutes / 1440, minutes / 60 % 24, minutes % 60);
    if days > 0 {
        trf("{0} d {1} h", &[&days, &hours])
    } else if hours > 0 {
        trf("{0} h {1} min", &[&hours, &mins])
    } else {
        trf("{0} min", &[&mins])
    }
}

/// The name to show for a prize id, even one the list does not know.
pub fn title(id: &str) -> String {
    lookup(id).map_or_else(|| tr("Prize").to_string(), |p| p.title())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_list_names_the_prizes_seen_in_real_replays() {
        let catalog = parse(BUILT_IN).unwrap();
        let get = |id: &str| catalog.prizes.get(id).cloned().unwrap();
        let decal = get("DelilahPrize:dpid_item18");
        assert_eq!((decal.title(), decal.kind, decal.week), ("Gold Cadillac CT5-V Decal".into(), Kind::Car, Some(2)));
        assert_eq!(get("DelilahPrize:dpid_item17").kind, Kind::Loading);
        assert_eq!(get("9f73f94d-2e61-4523-97d7-d61235d3365e").kind, Kind::PirateShop);
        // Five prizes a week for five weeks, and Pirate Shop Access.
        assert_eq!(catalog.prizes.len(), 26);
    }

    #[test]
    fn treasure_hours_say_what_runs_when_and_what_is_boosted() {
        let hours = parse(BUILT_IN).unwrap().treasure_hours;
        let at = |text: &str| DateTime::parse_from_rfc3339(text).unwrap().with_timezone(&Utc);
        // 14 Nov, 19:00 to 23:00 UTC.
        assert!(hour_at(&hours, at("2026-11-14T22:59:00Z")).is_some());
        assert!(hour_at(&hours, at("2026-11-14T23:00:00Z")).is_none());
        assert_eq!(pirate_shop_boost(&hours, at("2026-11-08T03:00:00Z")), 2.0);
        assert_eq!(pirate_shop_boost(&hours, at("2026-11-10T03:00:00Z")), 1.0);
        let next = next_hour(&hours, at("2026-11-19T12:00:00Z")).unwrap();
        assert_eq!(next.start, at("2026-11-21T19:00:00Z"));
        assert!(!next.confirmed);
        assert_eq!(next.boosts.text(), "Pirate Shop Access ×1.5 · Gold outfit ×7.5 · Car decal ×10");
        assert!(next_hour(&hours, at("2026-12-31T00:00:00Z")).is_none());
    }

    #[test]
    fn countdowns_round_to_what_matters() {
        let d = chrono::Duration::minutes;
        assert_eq!(countdown(d(3 * 1440 + 125)), "3 d 2 h");
        assert_eq!(countdown(d(192)), "3 h 12 min");
        assert_eq!(countdown(d(14)), "14 min");
        assert_eq!(countdown(chrono::Duration::seconds(20)), "1 min");
        assert_eq!(times(1.5), "1.5");
        assert_eq!(times(10.0), "10");
    }

    #[test]
    fn a_prize_without_a_name_yet_says_what_it_is() {
        let p = Prize { week: Some(4), kind: Kind::Outfit, name: None };
        assert_eq!(p.title(), "Gold outfit, week 4");
        assert_eq!(p.detail(), "");
        let named = Prize { name: Some("Solid Gold Miku".into()), ..p };
        assert_eq!(named.detail(), "Gold outfit · week 4");
    }

    #[test]
    fn a_newer_list_may_bring_kinds_this_version_does_not_know() {
        let text = r#"{"updated":"2026-12-01","prizes":{"x":{"kind":"emote","name":"Wave"}}}"#;
        assert_eq!(parse(text).unwrap().prizes["x"].kind, Kind::Other);
        assert!(parse("not json").is_none());
    }
}
