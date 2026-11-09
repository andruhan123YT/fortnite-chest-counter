//! Keeps copies of analyzed replays, because Fortnite deletes all but the newest 100.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub fn store_copy(dir: &Path, file_name: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let target = dir.join(file_name);
    let already_there = fs::metadata(&target).is_ok_and(|m| m.len() == bytes.len() as u64);
    if !already_there {
        let tmp = target.with_extension("replay.part");
        fs::write(&tmp, bytes)?;
        fs::rename(&tmp, &target)?;
    }
    Ok(target)
}

fn replays(dir: &Path) -> Vec<(PathBuf, u64, std::time::SystemTime)> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("replay")))
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            Some((e.path(), meta.len(), meta.modified().unwrap_or(std::time::UNIX_EPOCH)))
        })
        .collect()
}

pub fn usage_bytes(dir: &Path) -> u64 {
    replays(dir).iter().map(|(_, size, _)| size).sum()
}

/// Deletes the oldest copies until the archive fits in `max_bytes`. Returns bytes freed.
pub fn enforce_cap(dir: &Path, max_bytes: u64) -> io::Result<u64> {
    let mut files = replays(dir);
    files.sort_by_key(|(_, _, modified)| *modified);
    let mut total: u64 = files.iter().map(|(_, size, _)| size).sum();
    let mut freed = 0;
    for (path, size, _) in files {
        if total <= max_bytes {
            break;
        }
        fs::remove_file(&path)?;
        total -= size;
        freed += size;
    }
    Ok(freed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_once_and_trims_oldest() {
        let dir = std::env::temp_dir().join(format!("fcc-archive-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        store_copy(&dir, "a.replay", &[0; 100]).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        store_copy(&dir, "b.replay", &[0; 100]).unwrap();
        store_copy(&dir, "b.replay", &[0; 100]).unwrap();
        assert_eq!(usage_bytes(&dir), 200);
        assert_eq!(enforce_cap(&dir, 150).unwrap(), 100);
        assert!(!dir.join("a.replay").exists());
        assert!(dir.join("b.replay").exists());
        fs::remove_dir_all(dir).ok();
    }
}
