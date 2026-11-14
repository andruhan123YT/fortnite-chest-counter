//! The interface language. Text stays in English in the code, wrapped in `tr` or `trf`;
//! `ru.rs` holds the Russian for each. A test reads the sources and fails on any wrapped
//! text without a translation.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::LazyLock;

use chrono::Datelike;
use serde::{Deserialize, Serialize};

mod ru;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    #[default]
    En,
    Ru,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::En, Lang::Ru];

    /// The language's own name, the same in either interface.
    pub fn name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Ru => "Русский",
        }
    }
}

static LANG: AtomicU8 = AtomicU8::new(0);

#[cfg(test)]
thread_local! {
    // Tests run in parallel: each keeps its own language.
    static TEST_LANG: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

pub fn set(lang: Lang) {
    #[cfg(test)]
    TEST_LANG.with(|l| l.set(lang as u8));
    LANG.store(lang as u8, Ordering::Relaxed);
}

pub fn lang() -> Lang {
    #[cfg(test)]
    let code = TEST_LANG.with(std::cell::Cell::get);
    #[cfg(not(test))]
    let code = LANG.load(Ordering::Relaxed);
    match code {
        1 => Lang::Ru,
        _ => Lang::En,
    }
}

static RU: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| ru::TEXT.iter().copied().collect());

/// `en` in the interface language.
pub fn tr(en: &'static str) -> &'static str {
    match lang() {
        Lang::En => en,
        Lang::Ru => RU.get(en).copied().unwrap_or(en),
    }
}

/// `en` in the interface language, with `{0}`, `{1}`... replaced by `args`. A translation
/// can put them in any order.
pub fn trf(en: &'static str, args: &[&dyn std::fmt::Display]) -> String {
    let mut text = tr(en).to_owned();
    for (i, arg) in args.iter().enumerate() {
        text = text.replace(&format!("{{{i}}}"), &arg.to_string());
    }
    text
}

/// The noun after `n`: English `one` or `many`, Russian by its three forms, as in
/// 1 матч, 2 матча, 5 матчей.
pub fn plural(n: u64, one: &'static str, many: &'static str, [ru1, ru2, ru5]: [&'static str; 3]) -> &'static str {
    match lang() {
        Lang::En if n == 1 => one,
        Lang::En => many,
        Lang::Ru => match (n % 10, n % 100) {
            (1, r) if r != 11 => ru1,
            (2..=4, r) if !(12..=14).contains(&r) => ru2,
            _ => ru5,
        },
    }
}

const MONTHS: [&str; 12] = ["янв", "фев", "мар", "апр", "мая", "июн", "июл", "авг", "сен", "окт", "ноя", "дек"];
const MONTHS_FULL: [&str; 12] = [
    "января",
    "февраля",
    "марта",
    "апреля",
    "мая",
    "июня",
    "июля",
    "августа",
    "сентября",
    "октября",
    "ноября",
    "декабря",
];
const DAYS: [&str; 7] = ["Пн", "Вт", "Ср", "Чт", "Пт", "Сб", "Вс"];
const DAYS_FULL: [&str; 7] = ["понедельник", "вторник", "среда", "четверг", "пятница", "суббота", "воскресенье"];

/// A chrono format string for `date` in the interface language: month and weekday names
/// (`%b`, `%B`, `%a`, `%A`) come out in Russian when it is on.
pub fn date_format(date: &impl Datelike, format: &str) -> String {
    if lang() == Lang::En {
        return format.to_owned();
    }
    let month = date.month0() as usize;
    let day = date.weekday().num_days_from_monday() as usize;
    format
        .replace("%b", MONTHS[month])
        .replace("%B", MONTHS_FULL[month])
        .replace("%a", DAYS[day])
        .replace("%A", DAYS_FULL[day])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads a Rust string literal starting at the opening quote; returns its text.
    fn literal(source: &str) -> Option<String> {
        let mut out = String::new();
        let mut chars = source.chars().skip(1).peekable();
        while let Some(c) = chars.next() {
            match c {
                '"' => return Some(out),
                '\\' => match chars.next()? {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    '\'' => out.push('\''),
                    // A line continuation drops the line break and the indent after it.
                    '\n' | '\r' => {
                        while chars.peek().is_some_and(|c| c.is_whitespace()) {
                            chars.next();
                        }
                    }
                    'u' => {
                        let code: String = chars.by_ref().skip(1).take_while(|&c| c != '}').collect();
                        out.push(char::from_u32(u32::from_str_radix(&code, 16).ok()?)?);
                    }
                    _ => return None,
                },
                c => out.push(c),
            }
        }
        None
    }

    /// Every text in `tr("...")` and `trf("...")` calls in the app's sources, with its file.
    fn wrapped_texts() -> Vec<(String, String)> {
        let mut files = vec![];
        let mut dirs = vec![std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") && !path.ends_with("i18n.rs") {
                    files.push(path);
                }
            }
        }
        let mut out = vec![];
        for file in files {
            let text = std::fs::read_to_string(&file).unwrap();
            let name = file.file_name().unwrap().to_string_lossy().into_owned();
            let mut push = |s: Option<String>| out.extend(s.map(|s| (name.clone(), s)));
            for (at, _) in text.match_indices("tr(") {
                // Skip `ptr(`, `attr(` and the like; take `trf(` too.
                let before = text[..at].chars().last();
                let call =
                    if before == Some('_') || before.is_some_and(char::is_alphanumeric) { None } else { Some(at + 3) };
                let Some(arg) = call else { continue };
                let arg = text[arg..].trim_start().trim_start_matches('&');
                if arg.starts_with('"') {
                    push(literal(arg));
                }
            }
            for (at, _) in text.match_indices("trf(") {
                if text[..at].chars().last().is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                let arg = text[at + 4..].trim_start();
                if arg.starts_with('"') {
                    push(literal(arg));
                }
            }
        }
        out
    }

    #[test]
    fn every_wrapped_text_has_a_russian_translation() {
        let texts = wrapped_texts();
        assert!(texts.len() > 100, "found only {} texts; is the scanner broken?", texts.len());
        let missing: Vec<String> =
            texts.iter().filter(|(_, t)| !RU.contains_key(t.as_str())).map(|(f, t)| format!("{f}: {t:?}")).collect();
        assert!(missing.is_empty(), "{} texts have no Russian:\n{}", missing.len(), missing.join("\n"));
    }

    #[test]
    fn translations_keep_their_placeholders() {
        for (en, ru) in ru::TEXT {
            for i in 0..6 {
                let p = format!("{{{i}}}");
                assert_eq!(en.contains(&p), ru.contains(&p), "{p} in {en:?} -> {ru:?}");
            }
        }
    }

    #[test]
    fn russian_plurals_and_dates() {
        set(Lang::Ru);
        let word = |n| plural(n, "match", "matches", ["матч", "матча", "матчей"]);
        assert_eq!([word(1), word(21), word(3), word(11), word(25)], ["матч", "матч", "матча", "матчей", "матчей"]);
        set(Lang::En);
        assert_eq!([word(1), word(3)], ["match", "matches"]);
        set(Lang::Ru);
        let day = chrono::NaiveDate::from_ymd_opt(2026, 11, 13).unwrap();
        assert_eq!(day.format(&date_format(&day, "%a %d %b")).to_string(), "Пт 13 ноя");
        set(Lang::En);
        assert_eq!(day.format(&date_format(&day, "%a %d %b")).to_string(), "Fri 13 Nov");
    }
}
