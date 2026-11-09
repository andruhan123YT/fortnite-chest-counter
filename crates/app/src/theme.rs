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

pub fn apply(ctx: &egui::Context) {
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
    ctx.set_visuals(visuals);
    ctx.global_style_mut(|style| {
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
    fn formats_numbers() {
        assert_eq!(thousands(1_562_500), "1,562,500");
        assert_eq!(thousands(999), "999");
        assert_eq!(percent(0.00746), "0.75%");
        assert_eq!(percent(0.5), "50.0%");
        assert_eq!(percent(0.000_012), "0.0012%");
    }
}
