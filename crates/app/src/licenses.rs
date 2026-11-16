//! The open-source licenses of the crates built into the app, shown from Settings > About.
//! `assets/licenses.json` is written by `tools/package.py`; each distinct text is stored once.

use std::sync::LazyLock;

use eframe::egui::{self, RichText};
use serde::Deserialize;

use crate::i18n::{tr, trf};
use crate::theme;

#[derive(Debug, Default, Deserialize)]
struct List {
    crates: Vec<Crate>,
    texts: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Crate {
    name: String,
    version: String,
    license: String,
    repository: String,
    /// Indexes into `List::texts`.
    texts: Vec<usize>,
}

static LIST: LazyLock<List> =
    LazyLock::new(|| serde_json::from_str(include_str!("../assets/licenses.json")).unwrap_or_default());

/// How many third-party crates are built in.
pub fn count() -> usize {
    LIST.crates.len()
}

#[derive(Debug, Default)]
pub struct LicensesUi {
    pub open: bool,
    filter: String,
    expanded: Option<usize>,
}

/// The licenses dialog over a dimmed window, the way What's new opens, while it is open.
pub fn window(ctx: &egui::Context, state: &mut LicensesUi) {
    if !state.open {
        return;
    }
    let mut close = false;
    let screen = ctx.content_rect();
    let modal = egui::Modal::new(egui::Id::unique("licenses")).show(ctx, |ui| {
        ui.set_width(560.0_f32.min(screen.width() - 60.0));
        ui.horizontal(|ui| {
            ui.label(theme::title(tr("Open-source licenses")));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let x = theme::icon_text(theme::icon::CLOSE, "x", 13.0).color(theme::muted());
                close = ui.add(egui::Button::new(x).frame(false)).on_hover_text(tr("Close")).clicked();
            });
        });
        ui.add_space(4.0);
        {
            ui.label(theme::caption(trf(
                "Fortnite Chest Counter is built with {0} open-source Rust crates. Their licenses ask for these texts to come with the app.",
                &[&count()],
            )));
            ui.add_space(4.0);
            ui.add(egui::TextEdit::singleline(&mut state.filter).hint_text(tr("Search")).desired_width(f32::INFINITY));
            ui.add_space(4.0);
            let needle = state.filter.trim().to_lowercase();
            let height = (screen.height() * 0.6).max(160.0);
            egui::ScrollArea::vertical().auto_shrink([false, true]).max_height(height).min_scrolled_height(height).show(ui, |ui| {
                for (i, c) in LIST.crates.iter().enumerate() {
                    if !needle.is_empty()
                        && !c.name.to_lowercase().contains(&needle)
                        && !c.license.to_lowercase().contains(&needle)
                    {
                        continue;
                    }
                    let open = state.expanded == Some(i);
                    if row(ui, c, open).clicked() {
                        state.expanded = if open { None } else { Some(i) };
                    }
                    if open {
                        details(ui, c);
                    }
                }
            });
        }
        ui.add_space(12.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let button = egui::Button::new(RichText::new(tr("Close")).strong().color(theme::bg())).fill(theme::gold());
            close |= ui.add(button).clicked();
        });
    });
    if close || modal.should_close() {
        state.open = false;
    }
}

/// One crate: name and version on the left, license on the right; the whole line clicks.
fn row(ui: &mut egui::Ui, c: &Crate, open: bool) -> egui::Response {
    let width = ui.available_width() - 12.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 24.0), egui::Sense::click());
    let painter = ui.painter();
    if response.hovered() || open {
        painter.rect_filled(rect, 4.0, theme::card_hover());
    }
    let name_color = if open { theme::gold() } else { theme::text() };
    let name = painter.layout_no_wrap(c.name.clone(), theme::font(theme::SEMIBOLD, 13.5), name_color);
    let name_width = name.size().x;
    let left = rect.left_center() + egui::vec2(6.0, 0.0);
    painter.galley(left - egui::vec2(0.0, name.size().y / 2.0), name, name_color);
    painter.text(
        left + egui::vec2(name_width + 8.0, 0.0),
        egui::Align2::LEFT_CENTER,
        &c.version,
        egui::FontId::proportional(12.0),
        theme::muted(),
    );
    painter.text(
        rect.right_center() - egui::vec2(6.0, 0.0),
        egui::Align2::RIGHT_CENTER,
        &c.license,
        egui::FontId::proportional(12.0),
        theme::muted(),
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn details(ui: &mut egui::Ui, c: &Crate) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        if !c.repository.is_empty() {
            ui.hyperlink(&c.repository);
        }
        if c.texts.is_empty() {
            ui.label(theme::caption(trf("The package has no license file; it is licensed under {0}.", &[&c.license])));
        }
        for &t in &c.texts {
            if let Some(text) = LIST.texts.get(t) {
                ui.add_space(6.0);
                ui.label(RichText::new(text).monospace().size(11.5).color(theme::muted()));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_list_reads() {
        assert!(LIST.crates.len() > 100, "{} crates", LIST.crates.len());
        assert!(LIST.crates.iter().any(|c| c.name == "eframe"));
        for c in &LIST.crates {
            assert!(c.texts.iter().all(|&t| t < LIST.texts.len()), "{}", c.name);
        }
    }
}
