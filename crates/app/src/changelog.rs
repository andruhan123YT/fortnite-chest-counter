//! What changed in each version, from `CHANGELOG.md`, shown once after an update.

use eframe::egui::{self, text::LayoutJob, RichText, TextFormat};

use crate::theme;
use crate::update::{parse_version, version_text, Version};

const CHANGELOG: &str = include_str!("../../../CHANGELOG.md");
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
    unseen_in(parse(CHANGELOG), seen, crate::update::current())
}

/// The latest notes up to this version, for Settings > Updates.
pub fn recent() -> Vec<Entry> {
    let current = crate::update::current();
    parse(CHANGELOG).into_iter().filter(|e| e.version <= current).take(MAX_SHOWN).collect()
}

/// An item with its `**bold**` parts in bold.
fn item_text(item: &str, ui: &egui::Ui) -> LayoutJob {
    let regular = TextFormat::simple(egui::FontId::proportional(14.0), theme::TEXT);
    let bold = TextFormat::simple(theme::font(theme::SEMIBOLD, 14.0), theme::GOLD);
    let mut job = LayoutJob::default();
    for (i, part) in item.split("**").enumerate() {
        job.append(part, 0.0, if i % 2 == 1 { bold.clone() } else { regular.clone() });
    }
    job.wrap.max_width = ui.available_width();
    job
}

/// A dialog with `entries`. Returns true once the player closes it.
pub fn show(ctx: &egui::Context, entries: &[Entry]) -> bool {
    let Some(newest) = entries.first() else {
        return true;
    };
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::unique("whats_new")).show(ctx, |ui| {
        ui.set_width(460.0_f32.min(ctx.content_rect().width() - 60.0));
        ui.label(theme::title(&format!("What's new in v{}", version_text(newest.version))));
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
                        ui.label(RichText::new("•").color(theme::GOLD));
                        // A horizontal row would stretch the label to one line instead.
                        ui.add(egui::Label::new(item_text(item, ui)).wrap());
                    });
                }
            }
        });
        ui.add_space(12.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let button = egui::Button::new(RichText::new("Got it").strong().color(theme::BG)).fill(theme::GOLD);
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
        for _ in 0..3 {
            ctx.run_ui(input.clone(), |ui| {
                show(ui.ctx(), &entries);
            })
            .textures_delta
            .clear();
        }
        let dialog = ctx.memory(|m| m.area_rect(egui::Id::unique("whats_new"))).expect("dialog shown");
        assert!(window.contains_rect(dialog), "dialog {dialog:?} spills out of {window:?}");
    }

    #[test]
    fn the_changelog_covers_this_version() {
        let entries = parse(CHANGELOG);
        assert!(
            entries.iter().any(|e| e.version == crate::update::current()),
            "CHANGELOG.md needs a section for v{}",
            env!("CARGO_PKG_VERSION")
        );
    }
}
