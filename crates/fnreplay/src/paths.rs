//! Object path normalization used to match exported classes and level-placed actors.

/// Keeps only the object name: `/Game/Foo.Bar_C` -> `Bar_C`, `Default__Bar_C` -> `Bar_C`.
/// Paths that end in a package (`/Game/Foo`) are returned unchanged.
pub fn remove_path_prefix(path: &str) -> &str {
    for (i, c) in path.char_indices().rev() {
        match c {
            '.' => return &path[i + 1..],
            '/' => return path,
            _ => {}
        }
    }
    path.strip_prefix("Default__").unwrap_or(path)
}

/// Reduces a level-placed actor name to its class stem so all instances share one key.
///
/// UE5 world partition names actors `<Class>_C_UAID_<hex>_<hex>_<n>`, older levels use
/// `<Class>_C_<n>`; both become `<Class>`.
pub fn clean_static_id_suffix(id: &str) -> &str {
    let id = id.find("_UAID_").map_or(id, |i| &id[..i]);
    let id = id.trim_end_matches(|c: char| c.is_ascii_digit() || c == '_');
    id.strip_suffix("_C").unwrap_or(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_package_and_default_prefix() {
        assert_eq!(remove_path_prefix("/Game/Athena/PlayerPawn_Athena.PlayerPawn_Athena_C"), "PlayerPawn_Athena_C");
        assert_eq!(remove_path_prefix("Default__PlayerPawn_Athena_C"), "PlayerPawn_Athena_C");
        assert_eq!(remove_path_prefix("/Game/Athena/Maps/Athena_Terrain"), "/Game/Athena/Maps/Athena_Terrain");
        assert_eq!(remove_path_prefix("Tiered_Chest_Athena_C_12"), "Tiered_Chest_Athena_C_12");
    }

    #[test]
    fn cleans_static_ids() {
        assert_eq!(
            clean_static_id_suffix("Tiered_Chest_Athena_C_UAID_387C1A2B_68CF00_1881408023"),
            "Tiered_Chest_Athena"
        );
        assert_eq!(clean_static_id_suffix("Tiered_Chest_Athena_C_42"), "Tiered_Chest_Athena");
        assert_eq!(clean_static_id_suffix("Tiered_Chest_5_Child_C"), "Tiered_Chest_5_Child");
        assert_eq!(clean_static_id_suffix("AlwaysSpawn_RareChest_C_UAID_00_11_2"), "AlwaysSpawn_RareChest");
    }
}
