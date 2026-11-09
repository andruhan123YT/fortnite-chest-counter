//! Reads the chest quests from an Epic profile file saved by `get-epic-profile.ps1`, to
//! tell exactly when the in-game count stopped. The app never talks to Epic itself.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Local, NaiveDateTime};
use serde_json::Value;

use crate::config::data_dir;
use crate::record::MatchRecord;

pub const PROFILE_FILE: &str = "epic-profile.json";
const SCRIPT_PS1: &str = include_str!("../scripts/get-epic-profile.ps1");
const SCRIPT_BAT: &str = include_str!("../scripts/get-epic-profile.bat");
const QUEST_MARKER: &str = "treasurehunt_count_";

/// Where counting should start, according to the profile.
#[derive(Debug, Clone, PartialEq)]
pub struct QuestStart {
    pub quest: String,
    /// Chests the quest had counted.
    pub progress: u64,
    pub finished: bool,
    /// Local time the quest was completed, or when the profile was downloaded if it is
    /// still running.
    pub at: NaiveDateTime,
}

pub fn read_profile(path: &Path) -> Result<QuestStart, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let json: Value = serde_json::from_str(&text).map_err(|_| "This is not a profile file.".to_string())?;
    from_profile(&json)
}

fn from_profile(json: &Value) -> Result<QuestStart, String> {
    if let Some(message) = json.get("errorMessage").and_then(Value::as_str) {
        return Err(format!("Epic returned an error instead of a profile: {message}"));
    }
    let mut quests = Vec::new();
    collect_quests(json, &mut quests);
    // The chest quests are a series (count_01 .. count_05); the last one present is the
    // furthest the player got.
    let (name, attributes) = quests
        .into_iter()
        .max_by_key(|(name, _)| quest_number(name))
        .ok_or("No chest quests in this profile. Is it the Battle Royale (athena) profile?")?;
    let progress = attributes
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(key, _)| key.starts_with("completion_"))
        .filter_map(|(_, v)| v.as_u64())
        .max()
        .unwrap_or(0);
    let state = attributes.get("quest_state").and_then(Value::as_str).unwrap_or("");
    let finished = matches!(state, "Claimed" | "Completed");
    let stamp = if finished {
        attributes.get("last_state_change_time")
    } else {
        json.get("serverTime").or_else(|| attributes.get("last_state_change_time"))
    };
    let at = stamp
        .and_then(Value::as_str)
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.with_timezone(&Local).naive_local())
        .ok_or("The profile has no time for this quest.")?;
    Ok(QuestStart { quest: name, progress, finished, at })
}

fn collect_quests<'a>(value: &'a Value, out: &mut Vec<(String, &'a Value)>) {
    match value {
        Value::Object(map) => {
            if let (Some(template), Some(attributes)) =
                (map.get("templateId").and_then(Value::as_str), map.get("attributes"))
            {
                let lower = template.to_ascii_lowercase();
                if lower.starts_with("quest:") && lower.contains(QUEST_MARKER) {
                    out.push((template["Quest:".len()..].to_string(), attributes));
                }
            }
            map.values().for_each(|v| collect_quests(v, out));
        }
        Value::Array(items) => items.iter().for_each(|v| collect_quests(v, out)),
        _ => {}
    }
}

fn quest_number(name: &str) -> u32 {
    let lower = name.to_ascii_lowercase();
    lower
        .split(QUEST_MARKER)
        .nth(1)
        .map(|rest| rest.chars().take_while(char::is_ascii_digit).collect::<String>())
        .and_then(|digits| digits.parse().ok())
        .unwrap_or(0)
}

/// The match during which the quest completed: the server records completion when the
/// player leaves the match, so it is the one that ended at that moment.
pub fn match_at(matches: &[MatchRecord], at: NaiveDateTime) -> Option<&MatchRecord> {
    matches
        .iter()
        .filter_map(|m| {
            let start = m.started_at?;
            let end = start + chrono::Duration::milliseconds(i64::from(m.length_ms));
            let inside = start <= at && at <= end + chrono::Duration::seconds(90);
            inside.then(|| (m, (end - at).num_seconds().abs()))
        })
        .min_by_key(|&(_, gap)| gap)
        .map(|(m, _)| m)
}

/// Folders where the script and its profile file can be: next to the executable (release
/// zip) or the app's data folder (written by the app).
fn script_dirs() -> Vec<PathBuf> {
    let beside_exe = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf));
    beside_exe.into_iter().chain([data_dir().join("Epic profile")]).collect()
}

/// The folder holding the script, writing it to the data folder when it is not next to
/// the executable.
pub fn script_folder() -> std::io::Result<PathBuf> {
    let dirs = script_dirs();
    if let Some(dir) = dirs.iter().find(|d| d.join("get-epic-profile.bat").is_file()) {
        return Ok(dir.clone());
    }
    let dir = dirs.last().expect("data folder").clone();
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("get-epic-profile.ps1"), SCRIPT_PS1.replace("\r\n", "\n").replace('\n', "\r\n"))?;
    std::fs::write(dir.join("get-epic-profile.bat"), SCRIPT_BAT.replace("\r\n", "\n").replace('\n', "\r\n"))?;
    Ok(dir)
}

/// The newest profile file the script saved, if any.
pub fn saved_profile() -> Option<PathBuf> {
    script_dirs()
        .into_iter()
        .map(|d| d.join(PROFILE_FILE))
        .filter_map(|p| Some((std::fs::metadata(&p).ok()?.modified().ok()?, p)))
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, p)| p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn profile(state: &str, progress: u64) -> Value {
        json!({
            "profileRevision": 54469,
            "profileChanges": [{
                "changeType": "fullProfileUpdate",
                "profile": {
                    "items": {
                        "a1": {
                            "templateId": "Quest:quest_s43_treasurehunt_count_04",
                            "attributes": {
                                "quest_state": "Claimed",
                                "last_state_change_time": "2026-11-05T20:20:53.120Z",
                                "completion_quest_s43_treasurehunt_count_04_obj": 500
                            }
                        },
                        "a2": {
                            "templateId": "Quest:quest_s43_treasurehunt_count_05",
                            "attributes": {
                                "quest_state": state,
                                "last_state_change_time": "2026-11-07T19:39:05.000Z",
                                "completion_quest_s43_treasurehunt_count_05_obj": progress
                            }
                        },
                        "a3": { "templateId": "AthenaCharacter:cid_001", "attributes": {} }
                    }
                }
            }],
            "serverTime": "2026-11-08T18:00:00.000Z"
        })
    }

    fn local(utc: &str) -> NaiveDateTime {
        DateTime::parse_from_rfc3339(utc).unwrap().with_timezone(&Local).naive_local()
    }

    #[test]
    fn finished_quest_gives_its_completion_time() {
        let start = from_profile(&profile("Claimed", 1000)).unwrap();
        assert_eq!(start.quest, "quest_s43_treasurehunt_count_05");
        assert!(start.finished);
        assert_eq!(start.progress, 1000);
        assert_eq!(start.at, local("2026-11-07T19:39:05Z"));
    }

    #[test]
    fn running_quest_gives_progress_at_download_time() {
        let start = from_profile(&profile("Active", 870)).unwrap();
        assert!(!start.finished);
        assert_eq!(start.progress, 870);
        assert_eq!(start.at, local("2026-11-08T18:00:00Z"));
    }

    #[test]
    fn epic_errors_and_other_files_are_explained() {
        let err = from_profile(&json!({"errorCode": "x", "errorMessage": "Sorry the authorization code is invalid"}));
        assert!(err.unwrap_err().contains("authorization code is invalid"));
        assert!(from_profile(&json!({"items": []})).is_err());
    }

    #[test]
    fn finds_the_match_that_ended_with_the_quest() {
        let mk = |id: &str, start: &str, minutes: u32| {
            let mut m: MatchRecord = serde_json::from_value(json!({
                "id": id, "file_name": format!("{id}.replay"), "file_size": 1
            }))
            .unwrap();
            m.started_at = NaiveDateTime::parse_from_str(start, "%Y-%m-%d %H:%M:%S").ok();
            m.length_ms = minutes * 60_000;
            m
        };
        let matches = vec![mk("next", "2026-11-07 22:41:00", 15), mk("quest", "2026-11-07 22:22:42", 16)];
        let at = NaiveDateTime::parse_from_str("2026-11-07 22:39:05", "%Y-%m-%d %H:%M:%S").unwrap();
        assert_eq!(match_at(&matches, at).map(|m| m.id.as_str()), Some("quest"));
    }
}
