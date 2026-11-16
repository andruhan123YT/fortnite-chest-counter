//! What changed in each version, from `changelogs/<language>.md`, shown once after an
//! update.

use eframe::egui::{self, text::LayoutJob, RichText, TextFormat};

use crate::i18n::{tr, trf, Lang};
use crate::theme;
use crate::update::{parse_version, version_text, Version};

const CHANGELOG: &str = include_str!("../../../changelogs/en.md");

/// The notes written in `lang`, from `changelogs/<its code>.md`.
fn text(lang: Lang) -> &'static str {
    match lang {
        Lang::En => CHANGELOG,
        Lang::Ru => include_str!("../../../changelogs/ru.md"),
        Lang::Ar => include_str!("../../../changelogs/ar.md"),
        Lang::De => include_str!("../../../changelogs/de.md"),
        Lang::Es => include_str!("../../../changelogs/es.md"),
        Lang::Es419 => include_str!("../../../changelogs/es-419.md"),
        Lang::Fr => include_str!("../../../changelogs/fr.md"),
        Lang::Id => include_str!("../../../changelogs/id.md"),
        Lang::It => include_str!("../../../changelogs/it.md"),
        Lang::Ja => include_str!("../../../changelogs/ja.md"),
        Lang::Ko => include_str!("../../../changelogs/ko.md"),
        Lang::Pl => include_str!("../../../changelogs/pl.md"),
        Lang::PtBr => include_str!("../../../changelogs/pt-BR.md"),
        Lang::Th => include_str!("../../../changelogs/th.md"),
        Lang::Tr => include_str!("../../../changelogs/tr.md"),
        Lang::Vi => include_str!("../../../changelogs/vi.md"),
        Lang::ZhHans => include_str!("../../../changelogs/zh-Hans.md"),
        Lang::ZhHant => include_str!("../../../changelogs/zh-Hant.md"),
    }
}

/// The notes in the interface language.
fn notes() -> Vec<Entry> {
    parse(text(crate::i18n::lang()))
}
/// Versions shown at most, newest first, after skipping several updates.
const MAX_SHOWN: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub version: Version,
    pub items: Vec<String>,
}

/// `## 1.2.3` starts a version, `- ` starts an item; anything else is ignored.
fn parse(text: &str) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    for line in text.lines() {
        if let Some(version) = line.strip_prefix("## ").and_then(parse_version) {
            entries.push(Entry { version, items: Vec::new() });
        } else if let (Some(item), Some(entry)) = (line.strip_prefix("- "), entries.last_mut()) {
            entry.items.push(item.trim().to_owned());
        }
    }
    entries.retain(|e| !e.items.is_empty());
    entries.sort_by_key(|e| std::cmp::Reverse(e.version));
    entries
}

/// Notes the player has not seen, newest first. `seen` is the version whose notes they
/// last closed; an install from before notes existed has none and gets only this
/// version's.
fn unseen_in(entries: Vec<Entry>, seen: &str, current: Version) -> Vec<Entry> {
    let after = parse_version(seen);
    entries
        .into_iter()
        .filter(|e| e.version <= current && after.map_or(e.version == current, |seen| e.version > seen))
        .take(MAX_SHOWN)
        .collect()
}

pub fn unseen(seen: &str) -> Vec<Entry> {
    unseen_in(notes(), seen, crate::update::current())
}

/// The latest notes up to this version, for Settings > About.
pub fn recent() -> Vec<Entry> {
    let current = crate::update::current();
    notes().into_iter().filter(|e| e.version <= current).take(MAX_SHOWN).collect()
}

/// An item with its `**bold**` parts in bold.
fn item_text(item: &str, ui: &egui::Ui) -> LayoutJob {
    let regular = TextFormat::simple(egui::FontId::proportional(14.0), theme::text());
    let bold = TextFormat::simple(theme::font(theme::SEMIBOLD, 14.0), theme::gold());
    let mut job = LayoutJob::default();
    if crate::i18n::lang() == Lang::Ar {
        // Right-to-left text is laid out line by line ahead of time, so a bold title gets a
        // line of its own above the rest; bold inside a sentence stays plain.
        let title = item.strip_prefix("**").and_then(|rest| rest.split_once("**"));
        match title {
            Some((title, rest)) if !rest.trim().is_empty() => {
                job.append(&(crate::i18n::for_screen(title.trim()) + "\n"), 0.0, bold.clone());
                job.append(&crate::i18n::for_screen(rest.trim()), 0.0, regular.clone());
            }
            _ => job.append(&crate::i18n::for_screen(&item.replace("**", "")), 0.0, regular.clone()),
        }
    } else {
        for (i, part) in item.split("**").enumerate() {
            job.append(part, 0.0, if i % 2 == 1 { bold.clone() } else { regular.clone() });
        }
    }
    job.wrap.max_width = ui.available_width();
    job
}

/// A dialog with `entries`, and the switch for installing updates by themselves when
/// `auto_update` is given. Returns true once the player closes it.
pub fn show(ctx: &egui::Context, entries: &[Entry], auto_update: Option<&mut bool>) -> bool {
    let Some(newest) = entries.first() else {
        return true;
    };
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::unique("whats_new")).show(ctx, |ui| {
        ui.set_width(460.0_f32.min(ctx.content_rect().width() - 60.0));
        ui.label(theme::title(&trf("What's new in v{0}", &[&version_text(newest.version)])));
        ui.add_space(8.0);
        egui::ScrollArea::vertical().max_height(ctx.content_rect().height() * 0.6).show(ui, |ui| {
            for (i, entry) in entries.iter().enumerate() {
                if entries.len() > 1 {
                    if i > 0 {
                        ui.add_space(8.0);
                    }
                    ui.label(RichText::new(format!("v{}", version_text(entry.version))).strong());
                }
                for item in &entry.items {
                    ui.horizontal_top(|ui| {
                        ui.label(RichText::new("•").color(theme::gold()));
                        // A horizontal row would stretch the label to one line instead.
                        ui.add(egui::Label::new(item_text(item, ui)).wrap());
                    });
                }
            }
        });
        if let Some(on) = auto_update {
            ui.add_space(6.0);
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(tr("Install updates automatically"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| theme::toggle(ui, on));
            });
        }
        ui.add_space(12.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let button = egui::Button::new(RichText::new(tr("Got it")).strong().color(theme::bg())).fill(theme::gold());
            close = ui.add(button).clicked();
        });
    });
    close || modal.should_close()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# Changelog\n\nIntro line.\n\n## 1.2.0\n\n- **New.** One\n- Two\n\n## 1.1.0\n\n- Old\n\n## 1.0.0\n\n- Older\n\n## 2.0.0\n\n- Future\n";

    fn versions(entries: &[Entry]) -> Vec<Version> {
        entries.iter().map(|e| e.version).collect()
    }

    #[test]
    fn reads_versions_and_items() {
        let entries = parse(SAMPLE);
        assert_eq!(versions(&entries), [(2, 0, 0), (1, 2, 0), (1, 1, 0), (1, 0, 0)]);
        assert_eq!(entries[1].items, ["**New.** One", "Two"]);
    }

    #[test]
    fn shows_what_came_since_the_last_notes_seen() {
        let current = (1, 2, 0);
        assert_eq!(versions(&unseen_in(parse(SAMPLE), "1.0.0", current)), [(1, 2, 0), (1, 1, 0)]);
        assert!(unseen_in(parse(SAMPLE), "1.2.0", current).is_empty());
        // An install from before the notes existed sees only this version's.
        assert_eq!(versions(&unseen_in(parse(SAMPLE), "", current)), [(1, 2, 0)]);
    }

    #[test]
    fn the_dialog_fits_a_small_window() {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let window = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0));
        let input = egui::RawInput { screen_rect: Some(window), ..Default::default() };
        let entries = parse(CHANGELOG);
        let mut auto_update = true;
        for _ in 0..3 {
            ctx.run_ui(input.clone(), |ui| {
                show(ui.ctx(), &entries, Some(&mut auto_update));
            })
            .textures_delta
            .clear();
        }
        let dialog = ctx.memory(|m| m.area_rect(egui::Id::unique("whats_new"))).expect("dialog shown");
        assert!(window.contains_rect(dialog), "dialog {dialog:?} spills out of {window:?}");
    }

    #[test]
    fn every_language_has_the_same_notes_as_english() {
        let shape = |text| parse(text).into_iter().map(|e| (e.version, e.items.len())).collect::<Vec<_>>();
        for lang in Lang::ALL {
            assert_eq!(
                shape(text(lang)),
                shape(CHANGELOG),
                "changelogs for {lang:?} must have the same versions and items as en.md"
            );
        }
    }

    #[test]
    fn arabic_notes_put_the_title_on_its_own_line() {
        crate::i18n::set(Lang::Ar);
        let ui_ctx = egui::Context::default();
        let mut job = None;
        ui_ctx
            .run_ui(Default::default(), |ui| {
                job = Some(item_text("**عنوان.** نص البند", ui));
            })
            .textures_delta
            .clear();
        let job = job.unwrap();
        crate::i18n::set(Lang::En);
        assert_eq!(job.sections.len(), 2);
        assert!(job.text.contains('\n') && !job.text.contains("**"));
    }

    #[test]
    fn the_changelog_covers_this_version() {
        let entries = parse(CHANGELOG);
        assert!(
            entries.iter().any(|e| e.version == crate::update::current()),
            "changelogs/en.md needs a section for v{}",
            env!("CARGO_PKG_VERSION")
        );
    }
}
