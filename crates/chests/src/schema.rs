//! The replicated Fortnite classes the counter needs, as of Chapter 7 (v43).

use fnreplay::{Decoder, GroupSpec, Schema};

/// Group indices in [`fortnite_schema`], in declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Pawn = 0,
    PlayerState = 1,
    Chest = 2,
    AmmoBox = 3,
    Pickup = 4,
    Vehicle = 5,
    GameState = 6,
    Abilities = 7,
    AbilitiesCache = 8,
    GamePhase = 9,
}

impl Group {
    pub fn from_index(index: usize) -> Option<Self> {
        [
            Self::Pawn,
            Self::PlayerState,
            Self::Chest,
            Self::AmmoBox,
            Self::Pickup,
            Self::Vehicle,
            Self::GameState,
            Self::Abilities,
            Self::AbilitiesCache,
            Self::GamePhase,
        ]
        .get(index)
        .copied()
    }
}

const CHEST_PATHS: &[&str] = &[
    "/Game/Building/ActorBlueprints/Containers/Tiered_Chest_Athena.Tiered_Chest_Athena_C",
    "/Game/Building/ActorBlueprints/Containers/AlwaysSpawn_NormalChest.AlwaysSpawn_NormalChest_C",
    "/Game/Building/ActorBlueprints/Containers/AlwaysSpawn_RareChest.AlwaysSpawn_RareChest_C",
    "/Game/Building/ActorBlueprints/Containers/Tiered_Chest_5_Child.Tiered_Chest_5_Child_C",
    "/DelilahDynamicLevels/Gameplay/BGA_LootCannon_StandardChest_Pirate.BGA_LootCannon_StandardChest_Pirate_C",
    "/RelicChest/Gameplay/B_TieredChest_Relics.B_TieredChest_Relics_C",
    "/SpritePosters/Blueprints/Items/Booty/B_SpritePosters_Booty_Chest.B_SpritePosters_Booty_Chest_C",
    "/Game/Athena/Items/ForagedItems/BuriedChest/B_Athena_BuriedChest_Chest.B_Athena_BuriedChest_Chest_C",
    "/ExtractionPointGameplay/Gameplay/BGA_ExtractionPointGameplay_Chest.BGA_ExtractionPointGameplay_Chest_C",
];

const CHEST_STATIC_IDS: &[&str] = &[
    "Tiered_Chest_Athena",
    "AlwaysSpawn_NormalChest",
    "AlwaysSpawn_RareChest",
    "Tiered_Chest_5_Child",
    "BGA_LootCannon_StandardChest_Pirate",
    "B_TieredChest_Relics",
    "B_SpritePosters_Booty_Chest",
    "B_Athena_BuriedChest_Chest",
    "BGA_ExtractionPointGameplay_Chest",
    "Extraction_Chest",
];

const VEHICLE_PATH_FRAGMENTS: &[&str] = &[
    "/Valet/BasicCar",
    "/Valet/BasicTruck",
    "/Valet/BigRig/",
    "/Valet/SportsCar",
    "/Valet/TaxiCab",
    "/BasicSUV",
    "/ImmunityCarriage/Vehicle",
    "/Motorboat/Vehicle",
];

/// Movement of players and vehicles is quantized to two decimals.
const MOVEMENT: Decoder = Decoder::Movement { location_level: 2, rotation_short: false, velocity_level: 0 };
/// Pickups replicate their location with one decimal, ten times coarser than pawns.
const PICKUP_MOVEMENT: Decoder = Decoder::Movement { location_level: 1, rotation_short: false, velocity_level: 0 };

fn container(name: &str, with_health: bool) -> GroupSpec {
    let spec = GroupSpec::new(name)
        .prop("bHidden", Decoder::Bit)
        .prop("bAlreadySearched", Decoder::Bit)
        .prop("ReplicatedLootTier", Decoder::I32)
        .prop("SearchingPawn", Decoder::IntPacked)
        .prop("bReplicateMovement", Decoder::Bit)
        .prop("RemoteRole", Decoder::UInt)
        .prop("Role", Decoder::UInt)
        .prop("ReplicatedMovement", Decoder::Skip)
        .prop("ForceMetadataRelevant", Decoder::Skip)
        .prop("StaticMesh", Decoder::IntPacked)
        .prop("SearchedMesh", Decoder::IntPacked)
        .prop("AltMeshIdx", Decoder::Skip)
        .prop("ChosenRandomUpgrade", Decoder::Skip);
    if !with_health {
        return spec;
    }
    spec.prop("bDestroyed", Decoder::Bit)
        .prop("Health", Decoder::U16)
        .prop("MaxHealth", Decoder::U16)
        .prop("SearchAnimationCount", Decoder::IntPacked)
}

pub fn fortnite_schema() -> Schema {
    Schema::new(vec![
        GroupSpec::new("PlayerPawn")
            .paths(["/Game/Athena/PlayerPawn_Athena.PlayerPawn_Athena_C"])
            .prop("PlayerState", Decoder::IntPacked)
            .prop("ReplicatedMovement", MOVEMENT)
            .prop("AttachComponent", Decoder::IntPacked)
            .prop("bIsDying", Decoder::Bit)
            .prop("Vehicle", Decoder::IntPacked),
        GroupSpec::new("PlayerState")
            .paths(["/Script/FortniteGame.FortPlayerStateAthena"])
            .prop("PlayerID", Decoder::U32)
            .prop("UniqueId", Decoder::Skip)
            .prop("PlayerNamePrivate", Decoder::String)
            .prop("bOnlySpectator", Decoder::Bit)
            .prop("DeathCause", Decoder::UInt)
            .prop("bIsDisconnected", Decoder::Bit)
            .prop("bIsABot", Decoder::Bit)
            .prop("DeathTags", Decoder::Skip)
            .prop("VictimTags", Decoder::Skip)
            .prop("FinisherOrDownerTags", Decoder::Skip)
            .prop("KillScore", Decoder::I32)
            .prop("TeamIndex", Decoder::UInt)
            .prop("SquadId", Decoder::UInt),
        container("Chest", true).paths(CHEST_PATHS.iter().copied()).static_actors(CHEST_STATIC_IDS.iter().copied()),
        container("AmmoBox", false)
            .paths(["/Game/Building/ActorBlueprints/Containers/Tiered_Ammo_Athena.Tiered_Ammo_Athena_C"])
            .static_actors(["Tiered_Ammo_Athena"]),
        GroupSpec::new("Pickup")
            .paths(["/Script/FortniteGame.FortPickupAthena"])
            .prop("ReplicatedMovement", PICKUP_MOVEMENT)
            .prop("PawnWhoDroppedPickup", Decoder::IntPacked)
            .prop("bTossedFromContainer", Decoder::Bit)
            .prop("Count", Decoder::I32),
        GroupSpec::new("Vehicle")
            .paths(VEHICLE_PATH_FRAGMENTS.iter().copied())
            .partial_path()
            .prop("Health", Decoder::F32)
            .prop("ReplicatedMovement", MOVEMENT),
        GroupSpec::new("GameState")
            .paths(["/Game/Athena/Athena_GameState.Athena_GameState_C"])
            .prop("RecorderPlayerState", Decoder::IntPacked)
            // The server's id for the match: the same in every player's replay of it.
            .prop("GameSessionId", Decoder::String),
        // Abilities granted to a player. Only the recorder's replicate in full; a treasure
        // hunt prize shows up as a GA_TreasureHandler_* grant.
        GroupSpec::new("Abilities")
            .paths(["/Script/FortniteGame.FortAbilitySystemComponentAthena"])
            .subobjects(["AbilitySystemComponent"])
            .prop("ability", Decoder::IntPacked),
        GroupSpec::new("AbilitiesCache")
            .paths(["/Script/FortniteGame.FortAbilitySystemComponentAthena_ClassNetCache"])
            .prop("ActivatableAbilities", Decoder::FastArray),
        GroupSpec::new("GamePhase")
            .paths(["/Game/Athena/Athena_GamePhaseLogic.Athena_GamePhaseLogic_C"])
            .prop("GamePhase", Decoder::UInt),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_order_matches_enum() {
        let schema = fortnite_schema();
        let names: Vec<_> = schema.groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "PlayerPawn",
                "PlayerState",
                "Chest",
                "AmmoBox",
                "Pickup",
                "Vehicle",
                "GameState",
                "Abilities",
                "AbilitiesCache",
                "GamePhase"
            ]
        );
        assert_eq!(Group::from_index(6), Some(Group::GameState));
        assert_eq!(Group::from_index(8), Some(Group::AbilitiesCache));
        assert_eq!(Group::from_index(4), Some(Group::Pickup));
        assert_eq!(Group::from_index(9), Some(Group::GamePhase));
    }
}
