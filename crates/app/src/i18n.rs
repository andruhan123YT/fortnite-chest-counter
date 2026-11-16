//! The interface language. Text stays in English in the code, wrapped in `tr` or `trf`;
//! `i18n/<code>.rs` holds each language's table. A test reads the sources and fails on any
//! wrapped text that a language has no translation for.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::LazyLock;

use chrono::Datelike;
use serde::{Deserialize, Serialize};

mod ar;
mod de;
mod es;
mod es_419;
mod fr;
mod id;
mod it;
mod ja;
mod ko;
mod pl;
mod pt_br;
mod rtl;
mod ru;
mod th;
mod tr_tr;
mod vi;
mod zh_hans;
mod zh_hant;

/// The languages Fortnite itself offers. Arabic goes through `rtl`: egui draws left to right only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Lang {
    #[default]
    #[serde(rename = "en")]
    En,
    #[serde(rename = "ru")]
    Ru,
    #[serde(rename = "ar")]
    Ar,
    #[serde(rename = "de")]
    De,
    #[serde(rename = "es")]
    Es,
    #[serde(rename = "es-419")]
    Es419,
    #[serde(rename = "fr")]
    Fr,
    #[serde(rename = "id")]
    Id,
    #[serde(rename = "it")]
    It,
    #[serde(rename = "ja")]
    Ja,
    #[serde(rename = "ko")]
    Ko,
    #[serde(rename = "pl")]
    Pl,
    #[serde(rename = "pt-BR")]
    PtBr,
    #[serde(rename = "th")]
    Th,
    #[serde(rename = "tr")]
    Tr,
    #[serde(rename = "vi")]
    Vi,
    #[serde(rename = "zh-Hans")]
    ZhHans,
    #[serde(rename = "zh-Hant")]
    ZhHant,
}

/// How a language picks the noun after a number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluralRule {
    /// One form for every number: Japanese, Turkish...
    Single,
    /// 1 and the rest: German, Spanish, Italian.
    One,
    /// 0 and 1, and the rest: French, Portuguese.
    ZeroOne,
    /// 1 матч, 2 матча, 5 матчей.
    Russian,
    /// 1 mecz, 2 mecze, 5 meczów.
    Polish,
    /// One, two, 3 to 10, 11 to 99, and the rest (0, 100...).
    Arabic,
}

impl PluralRule {
    /// How many forms a translation lists, split by `|`.
    #[cfg(test)]
    pub fn forms(self) -> usize {
        match self {
            PluralRule::Single => 1,
            PluralRule::One | PluralRule::ZeroOne => 2,
            PluralRule::Russian | PluralRule::Polish => 3,
            PluralRule::Arabic => 5,
        }
    }

    fn index(self, n: u64) -> usize {
        let (last, last_two) = (n % 10, n % 100);
        match self {
            PluralRule::Single => 0,
            PluralRule::One => usize::from(n != 1),
            PluralRule::ZeroOne => usize::from(n > 1),
            PluralRule::Russian => match last {
                1 if last_two != 11 => 0,
                2..=4 if !(12..=14).contains(&last_two) => 1,
                _ => 2,
            },
            PluralRule::Polish => match last {
                _ if n == 1 => 0,
                2..=4 if !(12..=14).contains(&last_two) => 1,
                _ => 2,
            },
            PluralRule::Arabic => match last_two {
                _ if n == 1 => 0,
                _ if n == 2 => 1,
                3..=10 => 2,
                11..=99 => 3,
                _ => 4,
            },
        }
    }
}

/// Which extra Windows font a language needs beyond the Latin and Cyrillic ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    Latin,
    Japanese,
    Korean,
    ChineseSimplified,
    ChineseTraditional,
    Thai,
    Arabic,
    /// Latin, with letters egui's Ubuntu lacks (ạ, ậ, ắ, ế, ộ, ủ...).
    Vietnamese,
}

/// A language's text and date words.
pub struct Table {
    pub months: [&'static str; 12],
    pub months_full: [&'static str; 12],
    pub days: [&'static str; 7],
    pub days_full: [&'static str; 7],
    pub text: &'static [(&'static str, &'static str)],
}

impl Lang {
    pub const ALL: [Lang; 18] = [
        Lang::En,
        Lang::De,
        Lang::Es,
        Lang::Es419,
        Lang::Fr,
        Lang::Id,
        Lang::It,
        Lang::Pl,
        Lang::PtBr,
        Lang::Ru,
        Lang::Tr,
        Lang::Vi,
        Lang::Ar,
        Lang::Th,
        Lang::Ja,
        Lang::Ko,
        Lang::ZhHans,
        Lang::ZhHant,
    ];

    /// The language's own name, the same in every interface.
    pub fn name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Ar => *ARABIC_NAME,
            Lang::Ru => "Русский",
            Lang::De => "Deutsch",
            Lang::Es => "Español (España)",
            Lang::Es419 => "Español (Latinoamérica)",
            Lang::Fr => "Français",
            Lang::Id => "Bahasa Indonesia",
            Lang::It => "Italiano",
            Lang::Ja => "日本語",
            Lang::Ko => "한국어",
            Lang::Pl => "Polski",
            Lang::PtBr => "Português (Brasil)",
            Lang::Th => "ไทย",
            Lang::Tr => "Türkçe",
            Lang::Vi => "Tiếng Việt",
            Lang::ZhHans => "简体中文",
            Lang::ZhHant => "繁體中文",
        }
    }

    pub fn table(self) -> Option<&'static Table> {
        Some(match self {
            Lang::En => return None,
            Lang::Ru => &ru::TABLE,
            Lang::Ar => &ar::TABLE,
            Lang::De => &de::TABLE,
            Lang::Es => &es::TABLE,
            Lang::Es419 => &es_419::TABLE,
            Lang::Fr => &fr::TABLE,
            Lang::Id => &id::TABLE,
            Lang::It => &it::TABLE,
            Lang::Ja => &ja::TABLE,
            Lang::Ko => &ko::TABLE,
            Lang::Pl => &pl::TABLE,
            Lang::PtBr => &pt_br::TABLE,
            Lang::Th => &th::TABLE,
            Lang::Tr => &tr_tr::TABLE,
            Lang::Vi => &vi::TABLE,
            Lang::ZhHans => &zh_hans::TABLE,
            Lang::ZhHant => &zh_hant::TABLE,
        })
    }

    pub fn plural_rule(self) -> PluralRule {
        match self {
            Lang::En | Lang::De | Lang::Es | Lang::Es419 | Lang::It => PluralRule::One,
            Lang::Fr | Lang::PtBr => PluralRule::ZeroOne,
            Lang::Ru => PluralRule::Russian,
            Lang::Pl => PluralRule::Polish,
            Lang::Ar => PluralRule::Arabic,
            Lang::Id | Lang::Ja | Lang::Ko | Lang::Th | Lang::Tr | Lang::Vi | Lang::ZhHans | Lang::ZhHant => {
                PluralRule::Single
            }
        }
    }

    pub fn script(self) -> Script {
        match self {
            Lang::Ja => Script::Japanese,
            Lang::Ko => Script::Korean,
            Lang::ZhHans => Script::ChineseSimplified,
            Lang::ZhHant => Script::ChineseTraditional,
            Lang::Th => Script::Thai,
            Lang::Ar => Script::Arabic,
            Lang::Vi => Script::Vietnamese,
            _ => Script::Latin,
        }
    }

    /// Dates as numbers rather than with month names, the way these languages write them.
    fn numeric_dates(self) -> bool {
        matches!(self, Lang::Ja | Lang::Ko | Lang::ZhHans | Lang::ZhHant | Lang::Ar)
    }

    /// The language for a Windows language id (`GetUserDefaultUILanguage`), English when the
    /// app does not have it.
    pub fn from_windows(langid: u16) -> Lang {
        let primary = langid & 0x3ff;
        match primary {
            0x19 => Lang::Ru,
            0x01 => Lang::Ar,
            0x07 => Lang::De,
            // Spain (traditional and modern sort); every other Spanish is Latin American.
            0x0a if matches!(langid, 0x040a | 0x0c0a) => Lang::Es,
            0x0a => Lang::Es419,
            0x0c => Lang::Fr,
            0x21 => Lang::Id,
            0x10 => Lang::It,
            0x11 => Lang::Ja,
            0x12 => Lang::Ko,
            0x15 => Lang::Pl,
            0x16 => Lang::PtBr,
            0x1e => Lang::Th,
            0x1f => Lang::Tr,
            0x2a => Lang::Vi,
            // Taiwan, Hong Kong and Macau write Traditional Chinese.
            0x04 if matches!(langid, 0x0404 | 0x0c04 | 0x1404) => Lang::ZhHant,
            0x04 => Lang::ZhHans,
            _ => Lang::En,
        }
    }
}

/// "العربية" as the screen shows it, for the language list in every language.
static ARABIC_NAME: LazyLock<&'static str> = LazyLock::new(|| rtl::visual_static("العربية"));

static LANG: AtomicU8 = AtomicU8::new(0);

#[cfg(test)]
thread_local! {
    // Tests run in parallel: each keeps its own language.
    static TEST_LANG: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

fn code(lang: Lang) -> u8 {
    Lang::ALL.iter().position(|&l| l == lang).unwrap_or(0) as u8
}

pub fn set(lang: Lang) {
    #[cfg(test)]
    TEST_LANG.with(|l| l.set(code(lang)));
    LANG.store(code(lang), Ordering::Relaxed);
}

pub fn lang() -> Lang {
    #[cfg(test)]
    let code = TEST_LANG.with(std::cell::Cell::get);
    #[cfg(not(test))]
    let code = LANG.load(Ordering::Relaxed);
    Lang::ALL.get(code as usize).copied().unwrap_or_default()
}

static TABLES: LazyLock<Vec<HashMap<&'static str, &'static str>>> = LazyLock::new(|| {
    Lang::ALL.iter().map(|l| l.table().map(|t| t.text.iter().copied().collect()).unwrap_or_default()).collect()
});

fn lookup(en: &str) -> Option<&'static str> {
    TABLES.get(code(lang()) as usize)?.get(en).copied()
}

/// `en` in the interface language.
pub fn tr(en: &'static str) -> &'static str {
    let text = lookup(en).unwrap_or(en);
    if lang() == Lang::Ar {
        rtl::visual_static(text)
    } else {
        text
    }
}

/// `text` as Windows, Discord or a browser should get it: they lay out Arabic themselves, so
/// the display form the app draws goes back to plain Arabic. Other languages pass through.
pub fn native(text: &str) -> String {
    if lang() == Lang::Ar {
        rtl::logical(text)
    } else {
        text.to_owned()
    }
}

/// Text that does not come through `tr`, such as the release notes, as the screen should
/// show it: Arabic in display order, other languages as they are.
pub fn for_screen(text: &str) -> String {
    if lang() == Lang::Ar {
        rtl::visual_owned(text)
    } else {
        text.to_owned()
    }
}

/// `en` in the interface language, with `{0}`, `{1}`... replaced by `args`. A translation
/// can put them in any order.
pub fn trf(en: &'static str, args: &[&dyn std::fmt::Display]) -> String {
    let arabic = lang() == Lang::Ar;
    let mut text = lookup(en).unwrap_or(en).to_owned();
    for (i, arg) in args.iter().enumerate() {
        // Translated pieces arrive in display form; the whole is laid out again below.
        let arg = if arabic { rtl::logical(&arg.to_string()) } else { arg.to_string() };
        text = text.replace(&format!("{{{i}}}"), &arg);
    }
    if arabic {
        rtl::visual_owned(&text)
    } else {
        text
    }
}

/// The noun after `n`. English uses `one` or `many`; other languages look up `plural:one`, whose
/// translation lists the forms their rule needs, split by `|`, as in "матч|матча|матчей".
pub fn plural(n: u64, one: &'static str, many: &'static str) -> &'static str {
    let lang = lang();
    if lang == Lang::En {
        return if n == 1 { one } else { many };
    }
    let Some(forms) = lookup(&format!("plural:{one}")) else {
        return if n == 1 { one } else { many };
    };
    let index = lang.plural_rule().index(n);
    let form = forms.split('|').nth(index).or_else(|| forms.rsplit('|').next()).unwrap_or(many);
    if lang == Lang::Ar {
        rtl::visual_static(form)
    } else {
        form
    }
}

/// A chrono format string for `date` in the interface language: month and weekday names
/// (`%b`, `%B`, `%a`, `%A`) come out in that language. Japanese, Korean and Chinese write
/// the month as a number instead.
pub fn date_format(date: &impl Datelike, format: &str) -> String {
    let lang = lang();
    let Some(table) = lang.table() else {
        return format.to_owned();
    };
    let month = date.month0() as usize;
    let day = date.weekday().num_days_from_monday() as usize;
    let format = if lang.numeric_dates() {
        format
            .replace("%A, %d %B %Y", "%Y/%m/%d (%a)")
            .replace("%d %b %Y", "%Y/%m/%d")
            .replace("%a %d %b", "%m/%d (%a)")
            .replace("%d %b", "%m/%d")
    } else {
        format.to_owned()
    };
    let word = |w: &'static str| if lang == Lang::Ar { rtl::visual_static(w) } else { w };
    format
        .replace("%b", word(table.months[month]))
        .replace("%B", word(table.months_full[month]))
        .replace("%a", word(table.days[day]))
        .replace("%A", word(table.days_full[day]))
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
            for (at, _) in text.match_indices("plural(") {
                if text[..at].chars().last().is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                // plural(n, "match", "matches"): the singular is the key, after `plural:`.
                let Some(quote) = text[at..].find('"') else { continue };
                push(literal(&text[at + quote..]).map(|s| format!("plural:{s}")));
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

    /// Every language but English, with its table.
    fn tables() -> Vec<(Lang, &'static Table)> {
        Lang::ALL.iter().filter_map(|&l| l.table().map(|t| (l, t))).collect()
    }

    #[test]
    fn every_wrapped_text_has_a_translation_in_every_language() {
        let texts = wrapped_texts();
        assert!(texts.len() > 100, "found only {} texts; is the scanner broken?", texts.len());
        let mut problems = vec![];
        for (lang, table) in tables() {
            let keys: HashMap<&str, &str> = table.text.iter().copied().collect();
            for (file, text) in &texts {
                if !keys.contains_key(text.as_str()) {
                    problems.push(format!("{lang:?} {file}: {text:?}"));
                }
            }
        }
        assert!(problems.is_empty(), "{} missing:\n{}", problems.len(), problems.join("\n"));
    }

    #[test]
    fn no_language_keeps_texts_the_app_no_longer_has() {
        let texts: std::collections::HashSet<String> = wrapped_texts().into_iter().map(|(_, t)| t).collect();
        for (lang, table) in tables() {
            let stale: Vec<&str> = table.text.iter().map(|&(en, _)| en).filter(|en| !texts.contains(*en)).collect();
            assert!(stale.is_empty(), "{lang:?} has texts no code uses:\n{}", stale.join("\n"));
        }
    }

    #[test]
    fn every_language_lists_each_text_once() {
        for (lang, table) in tables() {
            let mut seen = std::collections::HashSet::new();
            for (en, _) in table.text {
                assert!(seen.insert(*en), "{lang:?} lists {en:?} twice");
            }
        }
    }

    #[test]
    fn translations_keep_their_placeholders() {
        for (lang, table) in tables() {
            for (en, text) in table.text {
                for i in 0..8 {
                    let p = format!("{{{i}}}");
                    assert_eq!(en.contains(&p), text.contains(&p), "{lang:?}: {p} in {en:?} -> {text:?}");
                }
                assert_eq!(en.matches('\n').count(), text.matches('\n').count(), "{lang:?}: line breaks in {en:?}");
            }
        }
    }

    #[test]
    fn plurals_have_the_forms_their_language_needs() {
        for (lang, table) in tables() {
            for (en, text) in table.text.iter().filter(|(en, _)| en.starts_with("plural:")) {
                let forms = text.split('|').count();
                assert_eq!(forms, lang.plural_rule().forms(), "{lang:?}: {en:?} -> {text:?}");
                assert!(text.split('|').all(|f| !f.trim().is_empty()), "{lang:?}: {en:?} -> {text:?}");
            }
        }
    }

    #[test]
    fn plurals_and_dates() {
        let word = |n| plural(n, "match", "matches");
        set(Lang::Ru);
        assert_eq!([word(1), word(21), word(3), word(11), word(25)], ["матч", "матч", "матча", "матчей", "матчей"]);
        set(Lang::Pl);
        assert_eq!([word(1), word(3), word(5), word(12), word(22)], ["mecz", "mecze", "meczów", "meczów", "mecze"]);
        set(Lang::Fr);
        assert_eq!([word(0), word(1), word(2)], ["partie", "partie", "parties"]);
        set(Lang::En);
        assert_eq!([word(1), word(3)], ["match", "matches"]);
        let day = chrono::NaiveDate::from_ymd_opt(2026, 11, 13).unwrap();
        let show = |lang| {
            set(lang);
            day.format(&date_format(&day, "%a %d %b")).to_string()
        };
        assert_eq!(show(Lang::En), "Fri 13 Nov");
        assert_eq!(show(Lang::Ru), "Пт 13 ноя");
        assert_eq!(show(Lang::De), "Fr 13 Nov.");
        assert_eq!(show(Lang::Ja), "11/13 (金)");
        set(Lang::Ar);
        assert_eq!(
            [word(1), word(2), word(5), word(11), word(100)].map(rtl::logical),
            ["مباراة", "مباراتان", "مباريات", "مباراة", "مباراة"]
        );
        assert_eq!(native(tr("Settings")), "الإعدادات");
    }

    #[test]
    fn windows_languages() {
        assert_eq!(Lang::from_windows(0x0419), Lang::Ru);
        assert_eq!(Lang::from_windows(0x0c0a), Lang::Es);
        assert_eq!(Lang::from_windows(0x080a), Lang::Es419);
        assert_eq!(Lang::from_windows(0x0416), Lang::PtBr);
        assert_eq!(Lang::from_windows(0x0804), Lang::ZhHans);
        assert_eq!(Lang::from_windows(0x0404), Lang::ZhHant);
        assert_eq!(Lang::from_windows(0x0409), Lang::En);
        assert_eq!(Lang::from_windows(0x0422), Lang::En);
        assert_eq!(Lang::from_windows(0x0401), Lang::Ar);
    }

    #[test]
    fn settings_keep_their_language_codes() {
        for lang in Lang::ALL {
            let json = serde_json::to_string(&lang).unwrap();
            assert_eq!(serde_json::from_str::<Lang>(&json).unwrap(), lang);
        }
        assert_eq!(serde_json::from_str::<Lang>("\"ru\"").unwrap(), Lang::Ru);
        assert_eq!(serde_json::to_string(&Lang::Es419).unwrap(), "\"es-419\"");
    }
}
