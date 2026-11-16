//! Arabic for egui, which shapes text with HarfBuzz but has no bidirectional layout: it
//! joins Arabic letters and turns each run of them right to left, yet lays the runs out
//! left to right in the order they come. So the app puts words, numbers and Latin names
//! into display order itself (the Unicode bidi algorithm), then turns each Arabic word
//! back to reading order for egui to turn once more. Long text is broken into lines first,
//! so a wrapped paragraph keeps its lines in reading order.
//!
//! Each Arabic word becomes its own run because the interface font, egui's Ubuntu, has no
//! Arabic: the letters come from the Segoe UI fallback, spaces, digits and Latin from
//! Ubuntu. In Arabic the bold families leave out Segoe UI Semibold and Bold for the same
//! reason (see `theme::load_fonts`).
//!
//! Windows menus, notifications, Discord and the OBS page lay out Arabic themselves, so
//! text bound for them goes back to its logical form through `native`.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use unicode_bidi::{BidiInfo, Level};

/// Lines longer than this many characters are broken at a space before reordering, so egui
/// does not have to wrap them (it would wrap the reordered text and swap the lines).
const LINE: usize = 52;

/// Display form of logical Arabic text.
pub fn visual(logical: &str) -> String {
    logical
        .split('\n')
        .flat_map(|paragraph| wrap(paragraph, LINE))
        .map(|line| reorder(&line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_arabic(c: char) -> bool {
    matches!(c, '\u{0600}'..='\u{06FF}' | '\u{0750}'..='\u{077F}' | '\u{08A0}'..='\u{08FF}' | '\u{FB50}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}')
}

/// The glyph a paired character takes in right-to-left text, where "(" opens on the right
/// and so has to look like ")". Renderers do this mirroring; egui does not.
fn mirror(c: char) -> char {
    match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '«' => '»',
        '»' => '«',
        '‹' => '›',
        '›' => '‹',
        other => other,
    }
}

/// One line in display order, with paired characters mirrored in right-to-left runs and
/// every Arabic word back in reading order.
fn reorder(line: &str) -> String {
    let info = BidiInfo::new(line, Some(Level::rtl()));
    let shown = match info.paragraphs.first() {
        Some(paragraph) => {
            let (levels, runs) = info.visual_runs(paragraph, paragraph.range.clone());
            let mut shown = String::with_capacity(line.len());
            for run in runs {
                let text = &line[run.clone()];
                if levels[run.start].is_rtl() {
                    shown.extend(text.chars().rev().map(mirror));
                } else {
                    shown.push_str(text);
                }
            }
            shown
        }
        None => line.to_owned(),
    };
    let mut out = String::with_capacity(shown.len());
    let mut word: Vec<char> = vec![];
    for c in shown.chars() {
        if is_arabic(c) {
            word.push(c);
        } else {
            out.extend(word.drain(..).rev());
            out.push(c);
        }
    }
    out.extend(word.drain(..).rev());
    out
}

/// Greedy word wrap by character count.
fn wrap(text: &str, width: usize) -> Vec<String> {
    if text.chars().count() <= width {
        return vec![text.to_owned()];
    }
    let mut lines = vec![];
    let mut line = String::new();
    for word in text.split(' ') {
        let len = line.chars().count();
        if len > 0 && len + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut line));
        } else if len > 0 {
            line.push(' ');
        }
        line.push_str(word);
    }
    lines.push(line);
    lines
}

/// Display forms of static texts, made once.
static STATIC: LazyLock<Mutex<HashMap<&'static str, &'static str>>> = LazyLock::new(Default::default);

/// Logical text by display form, for `native` and for display forms passed into `trf`.
static LOGICAL: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Default::default);

/// Display forms remembered at most; past it the ones made at run time are dropped.
const REMEMBERED: usize = 4096;

fn remember(shown: &str, logical: &str) {
    if shown == logical {
        return;
    }
    let mut map = LOGICAL.lock().unwrap_or_else(|e| e.into_inner());
    if map.len() > REMEMBERED {
        map.clear();
        // Static texts stay findable.
        for (&logical, &shown) in STATIC.lock().unwrap_or_else(|e| e.into_inner()).iter() {
            map.insert(shown.to_owned(), logical.to_owned());
        }
    }
    map.insert(shown.to_owned(), logical.to_owned());
}

/// The display form of a static text, made once and kept.
pub fn visual_static(logical: &'static str) -> &'static str {
    if let Some(shown) = STATIC.lock().unwrap_or_else(|e| e.into_inner()).get(logical) {
        return shown;
    }
    let shown: &'static str = Box::leak(visual(logical).into_boxed_str());
    STATIC.lock().unwrap_or_else(|e| e.into_inner()).insert(logical, shown);
    remember(shown, logical);
    shown
}

/// The display form of a text built at run time.
pub fn visual_owned(logical: &str) -> String {
    let shown = visual(logical);
    remember(&shown, logical);
    shown
}

/// The logical text behind a display form, or the text itself when it is not one.
pub fn logical(text: &str) -> String {
    LOGICAL.lock().unwrap_or_else(|e| e.into_inner()).get(text).cloned().unwrap_or_else(|| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_swap_places_and_keep_their_letters() {
        assert_eq!(visual("صندوق"), "صندوق");
        assert_eq!(visual("نظرة عامة"), "عامة نظرة");
        assert_eq!(logical(&visual_owned("نظرة عامة شاملة")), "نظرة عامة شاملة");
    }

    #[test]
    fn numbers_and_latin_keep_their_order() {
        // "opened 2,953 in Fortnite": the number and the name read left to right, placed
        // where right-to-left reading expects them.
        assert_eq!(visual("فتح 2,953 في Fortnite"), "Fortnite في 2,953 فتح");
    }

    #[test]
    fn paired_characters_are_mirrored() {
        // Logical "(text)" shows with its opening bracket on the right, drawn as ")".
        assert_eq!(visual("(نص)"), "(نص)");
        assert_eq!(visual("«في مباراة»"), "«مباراة في»");
        // "Settings > General": the arrow points from Settings on the right to General.
        assert_eq!(visual("الإعدادات > عام"), "عام < الإعدادات");
        // Brackets around Latin text keep their glyphs facing the Latin.
        assert_eq!(visual("تسجيل (Record Replays)"), "(Record Replays) تسجيل");
    }

    #[test]
    fn long_text_is_broken_into_lines_in_reading_order() {
        let words: Vec<String> = (1..=30).map(|i| format!("كلمة{i}")).collect();
        let shown = visual(&words.join(" "));
        let lines: Vec<&str> = shown.lines().collect();
        assert!(lines.len() > 1);
        assert!(lines.iter().all(|l| l.chars().count() <= LINE), "{lines:?}");
        // The first line holds the first words, its first word rightmost.
        assert_eq!(lines[0].rsplit(' ').next(), Some(visual("كلمة1").as_str()), "{:?}", lines[0]);
    }
}
