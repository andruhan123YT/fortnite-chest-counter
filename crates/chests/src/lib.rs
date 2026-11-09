//! Counts the chests a player opened or destroyed in a Fortnite replay.

pub mod analysis;
pub mod breaks;
pub mod collect;
pub mod dump;
pub mod odds;
pub mod schema;

pub use analysis::{
    analyze, analyze_replay, same_name, ChestEvent, MatchAnalysis, PlayerChests, Treasure, TreasureSource,
};
pub use breaks::BreakConfig;
pub use collect::{ChestKind, Collected};
pub use schema::{fortnite_schema, Group};
