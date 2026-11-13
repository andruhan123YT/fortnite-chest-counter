//! Look and feel: a dark theme with treasure-gold accents.

use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Stroke};

pub const BG: Color32 = Color32::from_rgb(18, 20, 26);
pub const PANEL: Color32 = Color32::from_rgb(24, 27, 35);
pub const CARD: Color32 = Color32::from_rgb(32, 36, 47);
pub const CARD_HOVER: Color32 = Color32::from_rgb(40, 45, 58);
pub const GOLD: Color32 = Color32::from_rgb(245, 190, 60);
pub const TEXT: Color32 = Color32::from_rgb(228, 230, 235);
pub const MUTED: Color32 = Color32::from_rgb(150, 156, 170);
pub const OPENED: Color32 = Color32::from_rgb(94, 194, 122);
pub const BROKEN: Color32 = Color32::from_rgb(232, 145, 58);
pub const SQUAD: Color32 = Color32::from_rgb(90, 169, 230);
pub const WARN: Color32 = Color32::from_rgb(240, 110, 90);
pub const TREASURE: Color32 = Color32::from_rgb(196, 140, 255);

/// Why teammates' numbers are a lower bound.
pub const SQUAD_NEARBY_NOTE: &str =
    "Teammates' chests are counted only while they were near you, or while you spectated \
     them: a replay records nothing else. Chests they looted far away are missing, so the real squad total is higher.";

/// A squad count, marked as a lower bound.
pub fn squad_count(n: u64) -> String {
    format!("{}+", thousands(n))
}

/// Font families with Windows' own Segoe UI Semibold and Bold, read from the system font
/// folder (not shipped). Without them they fall back to the regular font.
pub const SEMIBOLD: &str = "semibold";
pub const BOLD: &str = "bold";
/// Windows' own icon font (Segoe MDL2 Assets), for the sidebar.
const ICONS: &str = "icons";
static ICONS_LOADED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Glyphs of Segoe MDL2 Assets / Segoe Fluent Icons.
pub mod icon {
    pub const HOME: char = '\u{E80F}';
    pub const LIST: char = '\u{E8FD}';
    pub const CHART: char = '\u{E9D2}';
    pub const MAP_PIN: char = '\u{E707}';
    pub const PEOPLE: char = '\u{E716}';
    pub const SETTINGS: char = '\u{E713}';
    pub const MENU: char = '\u{E700}';
    pub const DOWNLOAD: char = '\u{E896}';
    pub const LOCK: char = '\u{E72E}';
    pub const UNLOCK: char = '\u{E785}';
    pub const CLOSE: char = '\u{E711}';
    pub const STAR: char = '\u{E734}';
}

/// An icon at `size`, or `fallback` text when Windows has no icon font.
pub fn icon_text(glyph: char, fallback: &str, size: f32) -> RichText {
    if ICONS_LOADED.load(std::sync::atomic::Ordering::Relaxed) {
        RichText::new(glyph.to_string()).font(egui::FontId::new(size, egui::FontFamily::Name(ICONS.into())))
    } else {
        RichText::new(fallback).size(size * 0.8)
    }
}

pub fn font(family: &str, size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name(family.into()))
}

fn load_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let dir = std::env::var_os("WINDIR").map_or_else(|| "C:\\Windows".into(), std::path::PathBuf::from).join("Fonts");
    let regular = fonts.families.get(&egui::FontFamily::Proportional).cloned().unwrap_or_default();
    for (family, file) in [(SEMIBOLD, "seguisb.ttf"), (BOLD, "segoeuib.ttf")] {
        let mut list = regular.clone();
        if let Ok(bytes) = std::fs::read(dir.join(file)) {
            fonts.font_data.insert(file.into(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
            list.insert(0, file.into());
        }
        fonts.families.insert(egui::FontFamily::Name(family.into()), list);
    }
    let mut icons = regular;
    if let Ok(bytes) = std::fs::read(dir.join("segmdl2.ttf")) {
        fonts.font_data.insert(ICONS.into(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
        icons.insert(0, ICONS.into());
        ICONS_LOADED.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    fonts.families.insert(egui::FontFamily::Name(ICONS.into()), icons);
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context) {
    load_fonts(ctx);
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = Color32::from_rgb(14, 16, 21);
    visuals.faint_bg_color = Color32::from_rgb(28, 31, 40);
    visuals.hyperlink_color = GOLD;
    visuals.selection.bg_fill = Color32::from_rgb(92, 70, 22);
    visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.window_corner_radius = CornerRadius::same(10);
    visuals.override_text_color = Some(TEXT);
    for w in [&mut visuals.widgets.inactive, &mut visuals.widgets.hovered, &mut visuals.widgets.active] {
        w.corner_radius = CornerRadius::same(6);
    }
    visuals.widgets.inactive.weak_bg_fill = CARD;
    visuals.widgets.hovered.weak_bg_fill = CARD_HOVER;
    // The app is dark only. Left to follow the system, egui swaps in its stock light
    // style on a light Windows theme, under panels that stay dark.
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.set_visuals_of(egui::Theme::Dark, visuals);
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
    });
}

pub fn card() -> egui::Frame {
    egui::Frame::new().fill(CARD).corner_radius(CornerRadius::same(10)).inner_margin(Margin::same(14))
}

pub fn title(text: &str) -> RichText {
    RichText::new(text).size(22.0).strong().color(TEXT)
}

pub fn caption(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).size(12.5).color(MUTED)
}

/// A small circled "i" that shows `text` on hover.
pub fn info_icon(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
    let color = if response.hovered() { TEXT } else { MUTED };
    let painter = ui.painter();
    painter.circle_stroke(rect.center(), 7.0, Stroke::new(1.2, color));
    painter.text(rect.center(), egui::Align2::CENTER_CENTER, "i", egui::FontId::proportional(11.0), color);
    response.on_hover_text(text)
}

/// A small button with a triangle pointing left or right, drawn rather than typed: the
/// bundled fonts have no arrow glyphs.
pub fn arrow_button(ui: &mut egui::Ui, left: bool, enabled: bool) -> egui::Response {
    let response = ui.add_enabled(enabled, egui::Button::new("").min_size(egui::vec2(30.0, 22.0)));
    let color = ui.style().interact(&response).fg_stroke.color;
    let color = if enabled { color } else { color.gamma_multiply(0.35) };
    let c = response.rect.center();
    let (tip, back) = if left { (-4.5, 3.5) } else { (4.5, -3.5) };
    let points = vec![c + egui::vec2(tip, 0.0), c + egui::vec2(back, -5.0), c + egui::vec2(back, 5.0)];
    ui.painter().add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
    response
}

/// A small card: a caption over a bold number, with an optional note under it.
pub fn tile(ui: &mut egui::Ui, label: &str, value: &str, color: Color32, note: Option<&str>) -> egui::Response {
    egui::Frame::new()
        .fill(CARD)
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(caption(label));
            ui.label(RichText::new(value).font(font(BOLD, 22.0)).color(color));
            if let Some(note) = note {
                ui.add(egui::Label::new(caption(note)).truncate());
            }
        })
        .response
}

pub struct Stat {
    pub label: &'static str,
    pub value: String,
    pub color: Color32,
    pub note: Option<String>,
}

const STAT_MIN_WIDTH: f32 = 200.0;

/// Big-number cards laid out in as many columns as the width allows.
pub fn stat_row(ui: &mut egui::Ui, stats: &[Stat]) {
    let per_row = ((ui.available_width() / STAT_MIN_WIDTH).floor() as usize).clamp(1, stats.len().max(1));
    for chunk in stats.chunks(per_row) {
        ui.columns(per_row, |columns| {
            for (column, stat) in columns.iter_mut().zip(chunk) {
                card().show(column, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(caption(stat.label));
                    ui.label(RichText::new(&stat.value).size(34.0).strong().color(stat.color));
                    if let Some(note) = &stat.note {
                        ui.add(egui::Label::new(caption(note.clone())).wrap());
                    }
                });
            }
        });
        ui.add_space(4.0);
    }
}

pub fn percent(p: f64) -> String {
    if p >= 0.0995 {
        format!("{:.1}%", p * 100.0)
    } else if p >= 0.000_995 {
        format!("{:.2}%", p * 100.0)
    } else {
        format!("{:.4}%", p * 100.0)
    }
}

pub fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stays_dark_on_a_light_windows_theme() {
        let ctx = egui::Context::default();
        apply(&ctx);
        let input = egui::RawInput { system_theme: Some(egui::Theme::Light), ..Default::default() };
        // A frame that reports a light system theme, as eframe passes it on.
        ctx.run_ui(input, |_| {}).textures_delta.clear();
        assert_eq!(ctx.theme(), egui::Theme::Dark);
        let style = ctx.global_style();
        assert_eq!(style.visuals.panel_fill, BG);
        assert_eq!(style.visuals.override_text_color, Some(TEXT));
        assert_eq!(style.spacing.button_padding, egui::vec2(10.0, 5.0));
    }

    #[test]
    fn formats_numbers() {
        assert_eq!(thousands(1_562_500), "1,562,500");
        assert_eq!(thousands(999), "999");
        assert_eq!(percent(0.00746), "0.75%");
        assert_eq!(percent(0.5), "50.0%");
        assert_eq!(percent(0.000_012), "0.0012%");
    }
}
