//! Look and feel: color themes (Midnight by default) with treasure-gold accents.

use std::sync::atomic::{AtomicU8, Ordering};

use chests::ChestKind;
use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Stroke};
use serde::{Deserialize, Serialize};

use crate::i18n::{tr, Script};

/// The color theme picked in Settings > General.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorTheme {
    #[default]
    Midnight,
    Graphite,
    DeepSea,
    Amethyst,
    Oled,
    Parchment,
}

impl ColorTheme {
    pub const ALL: [ColorTheme; 6] = [
        ColorTheme::Midnight,
        ColorTheme::Graphite,
        ColorTheme::DeepSea,
        ColorTheme::Amethyst,
        ColorTheme::Oled,
        ColorTheme::Parchment,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ColorTheme::Midnight => "Midnight",
            ColorTheme::Graphite => "Graphite",
            ColorTheme::DeepSea => "Deep sea",
            ColorTheme::Amethyst => "Amethyst",
            ColorTheme::Oled => "OLED",
            ColorTheme::Parchment => "Parchment",
        }
    }

    pub fn note(self) -> &'static str {
        match self {
            ColorTheme::Midnight => tr("Dark blue, the default"),
            ColorTheme::Graphite => tr("Warm gray"),
            ColorTheme::DeepSea => tr("Dark teal, pirate gold"),
            ColorTheme::Amethyst => tr("Violet night"),
            ColorTheme::Oled => tr("True black for OLED screens"),
            ColorTheme::Parchment => tr("Light"),
        }
    }

    pub fn palette(self) -> &'static Palette {
        &PALETTES[self as usize]
    }
}

/// Every color the interface draws with.
pub struct Palette {
    pub bg: Color32,
    pub panel: Color32,
    pub card: Color32,
    pub card_hover: Color32,
    pub gold: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub opened: Color32,
    pub broken: Color32,
    pub squad: Color32,
    pub warn: Color32,
    pub treasure: Color32,
    /// Text fields and other sunken areas.
    pub sunken: Color32,
    /// Striped table rows.
    pub faint: Color32,
    pub selection: Color32,
    pub light: bool,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// `c` with `by` added to each channel.
fn lighten(c: Color32, by: u8) -> Color32 {
    Color32::from_rgb(c.r().saturating_add(by), c.g().saturating_add(by), c.b().saturating_add(by))
}

static PALETTES: [Palette; 6] = [
    Palette {
        bg: rgb(18, 20, 26),
        panel: rgb(24, 27, 35),
        card: rgb(32, 36, 47),
        card_hover: rgb(40, 45, 58),
        gold: rgb(245, 190, 60),
        text: rgb(228, 230, 235),
        muted: rgb(150, 156, 170),
        opened: rgb(94, 194, 122),
        broken: rgb(232, 145, 58),
        squad: rgb(90, 169, 230),
        warn: rgb(240, 110, 90),
        treasure: rgb(196, 140, 255),
        sunken: rgb(14, 16, 21),
        faint: rgb(28, 31, 40),
        selection: rgb(92, 70, 22),
        light: false,
    },
    Palette {
        bg: rgb(31, 30, 29),
        panel: rgb(38, 38, 36),
        card: rgb(48, 48, 46),
        card_hover: rgb(58, 57, 54),
        gold: rgb(232, 163, 61),
        text: rgb(236, 234, 228),
        muted: rgb(163, 161, 153),
        opened: rgb(109, 191, 138),
        broken: rgb(224, 137, 74),
        squad: rgb(106, 174, 224),
        warn: rgb(236, 112, 92),
        treasure: rgb(196, 155, 255),
        sunken: rgb(24, 23, 22),
        faint: rgb(40, 39, 37),
        selection: rgb(92, 66, 28),
        light: false,
    },
    Palette {
        bg: rgb(11, 26, 30),
        panel: rgb(15, 36, 42),
        card: rgb(22, 50, 57),
        card_hover: rgb(29, 62, 70),
        gold: rgb(242, 193, 78),
        text: rgb(227, 240, 238),
        muted: rgb(134, 166, 164),
        opened: rgb(95, 208, 164),
        broken: rgb(240, 150, 90),
        squad: rgb(98, 184, 232),
        warn: rgb(240, 112, 96),
        treasure: rgb(201, 160, 255),
        sunken: rgb(7, 20, 23),
        faint: rgb(18, 42, 48),
        selection: rgb(88, 76, 30),
        light: false,
    },
    Palette {
        bg: rgb(21, 18, 30),
        panel: rgb(28, 24, 40),
        card: rgb(39, 33, 55),
        card_hover: rgb(49, 41, 67),
        gold: rgb(201, 155, 255),
        text: rgb(236, 232, 245),
        muted: rgb(160, 151, 181),
        opened: rgb(111, 211, 154),
        broken: rgb(240, 154, 106),
        squad: rgb(111, 180, 240),
        warn: rgb(240, 110, 110),
        treasure: rgb(240, 184, 90),
        sunken: rgb(16, 13, 23),
        faint: rgb(33, 28, 47),
        selection: rgb(78, 56, 110),
        light: false,
    },
    Palette {
        bg: rgb(0, 0, 0),
        panel: rgb(0, 0, 0),
        card: rgb(21, 21, 21),
        card_hover: rgb(31, 31, 31),
        gold: rgb(255, 201, 64),
        text: rgb(242, 242, 242),
        muted: rgb(140, 140, 140),
        opened: rgb(79, 208, 122),
        broken: rgb(255, 154, 60),
        squad: rgb(77, 168, 240),
        warn: rgb(255, 105, 90),
        treasure: rgb(200, 140, 255),
        sunken: rgb(0, 0, 0),
        faint: rgb(12, 12, 12),
        selection: rgb(96, 74, 20),
        light: false,
    },
    Palette {
        bg: rgb(243, 239, 230),
        panel: rgb(232, 226, 213),
        card: rgb(255, 253, 248),
        card_hover: rgb(236, 230, 218),
        gold: rgb(181, 119, 26),
        text: rgb(42, 38, 32),
        muted: rgb(122, 115, 102),
        opened: rgb(47, 143, 85),
        broken: rgb(200, 100, 30),
        squad: rgb(45, 119, 184),
        warn: rgb(196, 60, 40),
        treasure: rgb(138, 79, 208),
        sunken: rgb(255, 255, 255),
        faint: rgb(246, 242, 234),
        selection: rgb(240, 214, 160),
        light: true,
    },
];

static THEME: AtomicU8 = AtomicU8::new(0);

#[cfg(test)]
thread_local! {
    // Tests run in parallel: each keeps its own theme.
    static TEST_THEME: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

/// Switches the colors everything draws with. `apply_colors` restyles the widgets.
pub fn set(theme: ColorTheme) {
    #[cfg(test)]
    TEST_THEME.with(|t| t.set(theme as u8));
    THEME.store(theme as u8, Ordering::Relaxed);
}

pub fn current() -> ColorTheme {
    #[cfg(test)]
    let code = TEST_THEME.with(std::cell::Cell::get);
    #[cfg(not(test))]
    let code = THEME.load(Ordering::Relaxed);
    ColorTheme::ALL.get(code as usize).copied().unwrap_or_default()
}

pub fn palette() -> &'static Palette {
    current().palette()
}

macro_rules! colors {
    ($($name:ident => $field:ident),*) => {
        $(pub fn $name() -> Color32 { palette().$field })*
    };
}
colors!(
    bg => bg, panel => panel, card_color => card, card_hover => card_hover, gold => gold, text => text,
    muted => muted, opened => opened, broken => broken, squad => squad, warn => warn, treasure => treasure
);

/// Why teammates' numbers are a lower bound.
pub fn squad_nearby_note() -> &'static str {
    tr("Teammates' chests are counted only while they were near you, or while you spectated \
        them: a replay records nothing else. Chests they looted far away are missing, so the real squad total is higher.")
}

/// The chest type as a table heading: "Rare".
pub fn chest_kind(kind: ChestKind) -> &'static str {
    match kind {
        ChestKind::Normal => tr("Normal"),
        ChestKind::Rare => tr("Rare"),
        ChestKind::Buried => tr("Buried / sprite"),
        ChestKind::LootCannon => tr("Loot cannon"),
        ChestKind::Relic => tr("Relic"),
    }
}

/// The chest type inside a sentence: "rare chest".
pub fn chest_phrase(kind: ChestKind) -> &'static str {
    match kind {
        ChestKind::Normal => tr("normal chest"),
        ChestKind::Rare => tr("rare chest"),
        ChestKind::Buried => tr("buried / sprite chest"),
        ChestKind::LootCannon => tr("loot cannon"),
        ChestKind::Relic => tr("relic chest"),
    }
}

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
    pub const STAR_FILLED: char = '\u{E735}';
    pub const ACCOUNT: char = '\u{E77B}';
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

/// Set once the language picker has been opened: from then on every script's font stays
/// loaded, so every language's name shows in it.
static EVERY_SCRIPT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// How far down to move a script's glyphs, as a share of the font size, so they sit on
/// the same baseline as the Latin text and digits beside them.
fn script_offset(script: Script) -> f32 {
    match script {
        // Yu Gothic draws high; Microsoft YaHei, JhengHei and Malgun Gothic already line up.
        Script::Japanese => 0.2,
        _ => 0.0,
    }
}

/// Windows fonts for the scripts Segoe UI and egui's own fonts lack, the first one found used.
fn script_fonts(script: Script) -> &'static [&'static str] {
    match script {
        Script::Latin => &[],
        Script::Japanese => &["YuGothM.ttc", "meiryo.ttc", "msgothic.ttc"],
        Script::Korean => &["malgun.ttf", "gulim.ttc"],
        Script::ChineseSimplified => &["msyh.ttc", "simsun.ttc"],
        Script::ChineseTraditional => &["msjh.ttc", "mingliub.ttc"],
        Script::Thai => &["LeelawUI.ttf", "leelawad.ttf", "tahoma.ttf"],
        Script::Arabic | Script::Vietnamese => &["segoeui.ttf", "tahoma.ttf"],
    }
}

/// Loads Windows' Segoe UI weights and icon font, and the font the interface language's
/// script needs (every script's once the language picker was opened).
pub fn load_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let dir = std::env::var_os("WINDIR").map_or_else(|| "C:\\Windows".into(), std::path::PathBuf::from).join("Fonts");
    let current = crate::i18n::lang().script();
    let mut scripts = vec![current];
    if EVERY_SCRIPT.load(Ordering::Relaxed) {
        // The current language's script first: Japanese and Chinese draw the same
        // characters differently.
        scripts.extend(
            [
                Script::Japanese,
                Script::Korean,
                Script::ChineseSimplified,
                Script::ChineseTraditional,
                Script::Thai,
                Script::Arabic,
                Script::Vietnamese,
            ]
            .into_iter()
            .filter(|&s| s != current),
        );
    }
    let mut fallbacks = vec![];
    for script in scripts {
        if let Some((file, bytes)) =
            script_fonts(script).iter().find_map(|f| std::fs::read(dir.join(f)).ok().map(|b| (*f, b)))
        {
            let tweak = egui::FontTweak { y_offset_factor: script_offset(script), ..Default::default() };
            fonts.font_data.insert(file.into(), std::sync::Arc::new(egui::FontData::from_owned(bytes).tweak(tweak)));
            fallbacks.push(file.to_string());
        }
    }
    for list in fonts.families.values_mut() {
        list.extend(fallbacks.iter().cloned());
    }
    if current == Script::Vietnamese {
        // Segoe UI draws all of it, so a word doesn't mix two fonts.
        if let (Some(list), Some(file)) = (fonts.families.get_mut(&egui::FontFamily::Proportional), fallbacks.first()) {
            list.retain(|f| f != file);
            list.insert(0, file.clone());
        }
    }
    let regular = fonts.families.get(&egui::FontFamily::Proportional).cloned().unwrap_or_default();
    // Segoe UI Semibold and Bold have Arabic too: they would draw whole Arabic phrases as one
    // run, which egui turns around with its numbers. Arabic stays in the regular weight.
    let arabic = current == Script::Arabic;
    for (family, file) in [(SEMIBOLD, "seguisb.ttf"), (BOLD, "segoeuib.ttf")] {
        let mut list = regular.clone();
        if let Some(bytes) = (!arabic).then(|| std::fs::read(dir.join(file)).ok()).flatten() {
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

/// Loads every script's font, once, for the language picker.
pub fn load_every_script(ctx: &egui::Context) {
    if !EVERY_SCRIPT.swap(true, Ordering::Relaxed) {
        load_fonts(ctx);
    }
}

pub fn apply(ctx: &egui::Context) {
    load_fonts(ctx);
    apply_colors(ctx);
}

/// Styles the widgets in the current theme's colors.
pub fn apply_colors(ctx: &egui::Context) {
    let p = palette();
    let mut visuals = if p.light { egui::Visuals::light() } else { egui::Visuals::dark() };
    visuals.panel_fill = p.bg;
    visuals.window_fill = p.panel;
    visuals.extreme_bg_color = p.sunken;
    visuals.faint_bg_color = p.faint;
    visuals.hyperlink_color = p.gold;
    visuals.selection.bg_fill = p.selection;
    visuals.selection.stroke = Stroke::new(1.0, if p.light { p.text } else { Color32::WHITE });
    visuals.window_corner_radius = CornerRadius::same(10);
    visuals.override_text_color = Some(p.text);
    for w in [&mut visuals.widgets.inactive, &mut visuals.widgets.hovered, &mut visuals.widgets.active] {
        w.corner_radius = CornerRadius::same(6);
    }
    if p.light {
        // Buttons sit on white cards: give them the panel's tint so they stand out.
        visuals.widgets.inactive.weak_bg_fill = p.panel;
        visuals.widgets.inactive.bg_fill = p.panel;
        visuals.widgets.hovered.weak_bg_fill = p.card_hover;
        visuals.widgets.hovered.bg_fill = p.card_hover;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.card_hover);
    } else {
        visuals.widgets.inactive.weak_bg_fill = p.card_hover;
        visuals.widgets.hovered.weak_bg_fill = lighten(p.card_hover, 14);
    }
    // egui keeps a style per system theme. Left to follow the system, it swaps in its stock
    // style on a light or dark Windows theme, so the app pins one slot and fills it.
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.set_visuals_of(egui::Theme::Dark, visuals);
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
    });
}

pub fn card() -> egui::Frame {
    egui::Frame::new().fill(card_color()).corner_radius(CornerRadius::same(10)).inner_margin(Margin::same(14))
}

pub fn title(words: &str) -> RichText {
    RichText::new(words).size(22.0).strong().color(text())
}

pub fn caption(words: impl Into<String>) -> RichText {
    RichText::new(words.into()).size(12.5).color(muted())
}

/// A small circled "i" that shows `hint` on hover.
pub fn info_icon(ui: &mut egui::Ui, hint: impl Into<egui::WidgetText>) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
    let color = if response.hovered() { text() } else { muted() };
    let painter = ui.painter();
    let c = rect.center();
    painter.circle_stroke(c, 7.0, Stroke::new(1.2, color));
    // Drawn rather than typed: a font's "i" sits off center in the circle.
    painter.circle_filled(c + egui::vec2(0.0, -3.4), 1.0, color);
    painter.line_segment([c + egui::vec2(0.0, -1.2), c + egui::vec2(0.0, 3.8)], Stroke::new(1.4, color));
    response.on_hover_text(hint)
}

/// An on/off switch. Returns a response that is `changed()` when it was flipped.
/// A row of buttons centered in the available width. Returns the index of the one clicked.
pub fn centered_buttons(ui: &mut egui::Ui, labels: Vec<RichText>) -> Option<usize> {
    let pad = ui.spacing().button_padding.x * 2.0;
    let gap = ui.spacing().item_spacing.x;
    let texts: Vec<egui::WidgetText> = labels.into_iter().map(egui::WidgetText::from).collect();
    let width: f32 = texts
        .iter()
        .map(|t| {
            t.clone().into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Button).size().x
                + pad
        })
        .sum::<f32>()
        + gap * texts.len().saturating_sub(1) as f32;
    let mut clicked = None;
    ui.horizontal(|ui| {
        ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
        for (i, text) in texts.into_iter().enumerate() {
            if ui.button(text).clicked() {
                clicked = Some(i);
            }
        }
    });
    clicked
}

pub fn toggle(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let (rect, mut response) = ui.allocate_exact_size(egui::vec2(38.0, 20.0), egui::Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let enabled = ui.is_enabled();
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, enabled, *on, ""));
    let t = ui.ctx().animate_bool_responsive(response.id, *on);
    let p = palette();
    let mix = |a: Color32, b: Color32| {
        let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
        Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
    };
    let off_track = if p.light { p.panel } else { p.card_hover };
    let mut track = mix(off_track, p.gold);
    let mut knob = mix(p.muted, if p.light { Color32::WHITE } else { p.bg });
    if !enabled {
        track = track.gamma_multiply(0.45);
        knob = knob.gamma_multiply(0.45);
    }
    let outline = if response.hovered() && enabled { p.muted } else { mix(p.muted.gamma_multiply(0.5), p.gold) };
    let radius = rect.height() / 2.0;
    let painter = ui.painter();
    painter.rect(rect, radius, track, Stroke::new(1.0, outline), egui::StrokeKind::Inside);
    let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
    painter.circle_filled(egui::pos2(x, rect.center().y), radius - 4.0, knob);
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
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
        .fill(card_color())
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

/// A prize from the odds table, in the interface language.
pub fn reward_name(name: &'static str) -> &'static str {
    match name {
        "Weekly Gold Outfit" => tr("Weekly Gold Outfit"),
        "Pirate Shop Access" => tr("Pirate Shop Access"),
        "Weekly Vehicle & Decal" => tr("Weekly Vehicle & Decal"),
        "Weekly Pickaxe or Back Bling" => tr("Weekly Pickaxe or Back Bling"),
        "Weekly Jam Track" => tr("Weekly Jam Track"),
        "Weekly Loading Screen" => tr("Weekly Loading Screen"),
        "Gizmo" => tr("Gizmo"),
        other => other,
    }
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
        assert_eq!(style.visuals.panel_fill, bg());
        assert_eq!(style.visuals.override_text_color, Some(text()));
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
