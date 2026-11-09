//! Where chests were opened and broken, on world coordinates, over a picture of the
//! island when the player has added one.

use std::path::{Path, PathBuf};

use chests::{same_name, ChestEvent, PlayerChests};
use eframe::egui::{self, Color32, RichText, TextureHandle, TextureOptions};
use egui_plot::{
    ColorConflictHandling, ItemId, Legend, MarkerShape, Plot, PlotImage, PlotMemory, PlotPoint, PlotPoints, Points,
};

use crate::app::ChestApp;
use crate::config::{data_dir, Settings};
use crate::record::MatchRecord;
use crate::theme;

/// Marker colors picked to stand out on every part of the island: none of them occurs in
/// the map's water, grass, sand or desert.
const OPENED: Color32 = Color32::from_rgb(255, 222, 0);
const BROKEN: Color32 = Color32::from_rgb(255, 40, 130);
const TEAMMATE: Color32 = Color32::from_rgb(80, 200, 255);
const MARKER_RADIUS: f32 = 6.0;
/// Radius of the white ring around the picked marker.
const PICKED_RING: f32 = 13.0;
/// Width of the dark ring around every marker.
const OUTLINE: f32 = 3.0;
/// How much of the picture's brightness "Dim map" keeps.
const DIMMED: u8 = 150;

/// A picture of the island the player chose instead of the built-in one.
pub fn picture_path() -> PathBuf {
    data_dir().join("map.png")
}

/// The v43 island with location names, the picture the calibration below was fitted to.
const BUILT_IN_PICTURE: &[u8] = include_bytes!("../../assets/map.png");

/// Calibration of fortnite.gg's v43 map picture (2048 px, with or without names):
/// `pixel = world * MAP_PX_PER_UNIT + MAP_ORIGIN_PX`. Fitted to 3,854 chest positions
/// from real matches; 96% of them land on land. Pictures of the same framing at another
/// resolution scale with their width. A new season's island needs a new fit.
const MAP_REFERENCE_PX: f64 = 2048.0;
const MAP_PX_PER_UNIT: f64 = 0.00675;
const MAP_ORIGIN_PX: [f64; 2] = [969.0, 1026.0];

/// The picture's center and size in plot coordinates, for a picture of `size` pixels.
fn picture_rect(size: [usize; 2]) -> (PlotPoint, egui::Vec2) {
    let units_per_px = MAP_REFERENCE_PX / size[0] as f64 / MAP_PX_PER_UNIT;
    let left = -MAP_ORIGIN_PX[0] / MAP_PX_PER_UNIT;
    let top = -MAP_ORIGIN_PX[1] / MAP_PX_PER_UNIT;
    let (w, h) = (size[0] as f64 * units_per_px, size[1] as f64 * units_per_px);
    let [cx, cy] = plot_point(left + w / 2.0, top + h / 2.0);
    (PlotPoint::new(cx, cy), egui::vec2((w / 100.0) as f32, (h / 100.0) as f32))
}

fn load_picture(ctx: &egui::Context, path: Option<&Path>) -> Result<TextureHandle, String> {
    let image = match path {
        Some(path) => image::open(path),
        None => image::load_from_memory(BUILT_IN_PICTURE),
    };
    let image = image.map_err(|e| e.to_string())?.to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
    Ok(ctx.load_texture("map_picture", color, TextureOptions::LINEAR))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    #[default]
    Counted,
    Selected,
    Everything,
}

#[derive(Clone)]
pub struct MapUi {
    pub scope: Scope,
    pub show_squad: bool,
    pub show_picture: bool,
    picture: Option<TextureHandle>,
    /// Set once loading was attempted, so a missing file is not retried every frame.
    picture_tried: bool,
    picture_error: Option<String>,
    picked: Option<PickedChest>,
}

impl Default for MapUi {
    fn default() -> Self {
        Self {
            scope: Scope::Counted,
            show_squad: true,
            show_picture: true,
            picture: None,
            picture_tried: false,
            picture_error: None,
            picked: None,
        }
    }
}

impl MapUi {
    /// The player's own picture when there is one, otherwise the built-in island.
    fn ensure_picture(&mut self, ctx: &egui::Context) {
        if self.picture_tried {
            return;
        }
        self.picture_tried = true;
        let path = picture_path();
        if path.exists() {
            match load_picture(ctx, Some(&path)) {
                Ok(texture) => {
                    self.picture = Some(texture);
                    return;
                }
                Err(e) => self.picture_error = Some(format!("Could not read {}: {e}", path.display())),
            }
        }
        match load_picture(ctx, None) {
            Ok(texture) => self.picture = Some(texture),
            Err(e) => self.picture_error = Some(format!("Could not load the built-in map: {e}")),
        }
    }

    /// Drops the player's own picture and goes back to the built-in island.
    fn use_built_in(&mut self, ctx: &egui::Context) {
        let _ = std::fs::remove_file(picture_path());
        self.picture = None;
        self.picture_error = None;
        self.picture_tried = false;
        self.ensure_picture(ctx);
    }

    /// Copies a picture the player chose into the data folder and shows it.
    fn replace_picture(&mut self, ctx: &egui::Context, source: &Path) {
        let target = picture_path();
        let copied = std::fs::create_dir_all(data_dir()).and_then(|_| std::fs::copy(source, &target));
        self.picture = None;
        self.picture_error = None;
        self.picture_tried = false;
        if let Err(e) = copied {
            self.picture_tried = true;
            self.picture_error = Some(format!("Could not copy the picture: {e}"));
            return;
        }
        self.ensure_picture(ctx);
        self.show_picture = true;
    }
}

/// Matches the in-game map: world +X points east and
/// world +Y points south, so the plot's up axis is -Y.
fn plot_point(x: f64, y: f64) -> [f64; 2] {
    [x / 100.0, -y / 100.0]
}

fn scope_help(settings: &Settings) -> String {
    let since = match settings.count_since {
        Some(at) => format!(
            "Counted matches: matches that add to your chest count, the ones that started at or after \
             \"Count matches from\" in Settings ({}). Overview and Stats use the same matches.",
            at.format("%Y-%m-%d %H:%M")
        ),
        None => "Counted matches: matches that add to your chest count. \"Count matches from\" in Settings is empty, \
                 so right now this is every match."
            .to_owned(),
    };
    format!(
        "{since}\n\nAll matches: every match in the Matches list, including the ones marked \"not counted\" \
         that started earlier. Their chests are shown on the map but not added to your count.\n\n\
         Selected match: only the match picked in the Matches list."
    )
}

pub fn show(app: &mut ChestApp, ui: &mut egui::Ui) {
    app.map_ui.ensure_picture(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(theme::title("Map"));
        ui.add_space(16.0);
        let ui_state = &mut app.map_ui;
        ui.selectable_value(&mut ui_state.scope, Scope::Counted, "Counted matches");
        ui.selectable_value(&mut ui_state.scope, Scope::Selected, "Selected match");
        ui.selectable_value(&mut ui_state.scope, Scope::Everything, "All matches");
        theme::info_icon(ui, scope_help(&app.settings));
        let ui_state = &mut app.map_ui;
        ui.separator();
        ui.checkbox(&mut ui_state.show_squad, "Teammates");
        if ui_state.picture.is_some() {
            ui.checkbox(&mut ui_state.show_picture, "Map picture");
            if ui.checkbox(&mut app.settings.dim_map, "Dim map").changed() {
                let _ = app.settings.save();
            }
        }
        if cfg!(windows)
            && ui
                .button("Change picture...")
                .on_hover_text("Use your own PNG of the island, framed like the built-in one")
                .clicked()
        {
            if let Some(path) = super::settings::pick_file("PNG picture", "png") {
                ui_state.replace_picture(ui.ctx(), &path);
            }
        }
        if picture_path().exists() && ui.button("Use built-in map").clicked() {
            ui_state.use_built_in(ui.ctx());
        }
    });
    ui.label(theme::caption(
        "Positions are where the chest was opened from or where its loot burst out. Drag to pan, scroll to zoom.",
    ));
    if let Some(error) = &app.map_ui.picture_error {
        ui.label(RichText::new(error).color(theme::BROKEN));
    }

    let nick = app.settings.nick.clone();
    let scope = app.map_ui.scope;
    let matches: Vec<&MatchRecord> = app
        .store
        .matches
        .iter()
        .filter(|m| match scope {
            Scope::Counted => app.settings.counts(m.started_at),
            Scope::Selected => app.selected_match.as_deref() == Some(m.id.as_str()),
            Scope::Everything => true,
        })
        .collect();

    let mut markers = Vec::new();
    for m in &matches {
        let who = m.player_name(&nick);
        let team = m.me(&nick).and_then(|p| p.team);
        for p in m.roster() {
            let mine = same_name(&p.name, who);
            let mate = !mine && team.is_some() && p.team == team;
            if !mine && !(mate && app.map_ui.show_squad) {
                continue;
            }
            for (e, opened) in p.in_order() {
                let (Some(x), Some(y)) = (e.x, e.y) else { continue };
                let chest = ChestRef { match_id: &m.id, player: &p.name, time: e.time, opened };
                markers.push(Marker { point: plot_point(x, y), series: Series::of(mine, opened), chest });
            }
        }
    }

    if markers.is_empty() {
        ui.add_space(20.0);
        ui.label(RichText::new("Nothing to show yet.").color(theme::MUTED));
        if scope == Scope::Selected && app.selected_match.is_none() {
            ui.label(theme::caption("Pick a match on the Matches page first."));
        }
        return;
    }

    // Series switched off in the legend are not drawn, so they cannot be clicked or stay
    // picked either. Read from the previous frame, which is what is on screen.
    let plot_id = ui.make_persistent_id("chest_map");
    let hidden = PlotMemory::load(ui.ctx(), plot_id).map(|m| m.hidden_items).unwrap_or_default();
    let shown_series = |series: Series| {
        !hidden.contains(&ItemId::new(series.style().0)) && (series != Series::Teammate || app.map_ui.show_squad)
    };

    // A picked chest that is no longer shown (another scope, a hidden series, a match
    // that changed) is dropped.
    let resolve_picked = |picked: &Option<PickedChest>| {
        picked.as_ref().and_then(|c| resolve(&matches, c, &nick)).filter(|p| shown_series(p.series))
    };
    let mut picked = resolve_picked(&app.map_ui.picked);
    if picked.is_none() {
        app.map_ui.picked = None;
    }
    let step = picked_panel(ui, picked.as_ref(), scope);
    if let (Some(index), Some(p)) = (step, &picked) {
        if let Some(&(e, opened)) = p.player.in_order().get(index) {
            app.map_ui.picked = Some(PickedChest {
                match_id: p.record.id.clone(),
                player: p.player.name.clone(),
                time: e.time,
                opened,
            });
        }
        picked = resolve_picked(&app.map_ui.picked);
    }
    // An arrow can step to a chest outside the part of the map in view.
    let follow = step.is_some();

    let highlight = picked.as_ref().and_then(|p| Some(plot_point(p.event.x?, p.event.y?)));
    let picture = app.map_ui.picture.as_ref().filter(|_| app.map_ui.show_picture).map(|t| (t.id(), t.size()));
    let tint = if app.settings.dim_map { Color32::from_gray(DIMMED) } else { Color32::WHITE };
    // Every marker is a dark disc with its color on top, drawn one marker at a time so
    // each ring separates it from the ones it overlaps. Items of a series share a name,
    // so the legend shows the series once, in its color, and toggles all of it.
    let legend = Legend::default().color_conflict_handling(ColorConflictHandling::PickLast);
    let plot =
        Plot::new("chest_map").id(plot_id).data_aspect(1.0).legend(legend).show_axes([false, false]).show_grid(false);
    let shown = plot.show(ui, |plot| {
        if let Some((texture, size)) = picture {
            let (center, extent) = picture_rect(size);
            // Empty name: the picture stays out of the legend.
            plot.image(PlotImage::new("", texture, center, extent).tint(tint).allow_hover(false));
        }
        for series in Series::DRAW_ORDER {
            let (name, color, shape) = series.style();
            for marker in markers.iter().filter(|m| m.series == series) {
                plot.points(
                    Points::new(name, PlotPoints::from(vec![marker.point]))
                        .color(Color32::BLACK)
                        .radius(MARKER_RADIUS + OUTLINE)
                        .shape(shape),
                );
                plot.points(
                    Points::new(name, PlotPoints::from(vec![marker.point]))
                        .color(color)
                        .radius(MARKER_RADIUS)
                        .shape(shape),
                );
            }
        }
        if let (true, Some(point)) = (follow, highlight) {
            let bounds = plot.plot_bounds();
            let (min, max) = (bounds.min(), bounds.max());
            let (mx, my) = (bounds.width() * 0.1, bounds.height() * 0.1);
            let inside =
                (min[0] + mx..=max[0] - mx).contains(&point[0]) && (min[1] + my..=max[1] - my).contains(&point[1]);
            if !inside {
                let center = bounds.center();
                let mut moved = bounds;
                moved.translate((point[0] - center.x, point[1] - center.y));
                plot.set_plot_bounds(moved);
            }
        }
        if let Some(point) = highlight {
            for (color, radius) in [(Color32::BLACK, PICKED_RING + 1.5), (Color32::WHITE, PICKED_RING)] {
                plot.points(
                    Points::new("", PlotPoints::from(vec![point]))
                        .color(color)
                        .radius(radius)
                        .filled(false)
                        .shape(MarkerShape::Circle),
                );
            }
        }
    });

    // The marker under the pointer. Where markers overlap, the one drawn last is on top,
    // so on equal distance the later one wins.
    let near = |pos: egui::Pos2| {
        let mut best: Option<(&Marker<'_>, f32)> = None;
        for series in Series::DRAW_ORDER.into_iter().filter(|&s| shown_series(s)) {
            for m in markers.iter().filter(|m| m.series == series) {
                let d = shown.transform.position_from_point(&PlotPoint::new(m.point[0], m.point[1])).distance(pos);
                if d <= MARKER_RADIUS + OUTLINE + 2.0 && best.is_none_or(|(_, b)| d <= b) {
                    best = Some((m, d));
                }
            }
        }
        best.map(|(m, _)| m)
    };
    if shown.response.hover_pos().is_some_and(|pos| near(pos).is_some()) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if shown.response.clicked() {
        if let Some(pos) = shown.response.interact_pointer_pos() {
            app.map_ui.picked = near(pos).map(|m| PickedChest {
                match_id: m.chest.match_id.to_owned(),
                player: m.chest.player.to_owned(),
                time: m.chest.time,
                opened: m.chest.opened,
            });
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Series {
    Opened,
    Broken,
    Teammate,
}

impl Series {
    /// Bottom to top.
    const DRAW_ORDER: [Series; 3] = [Self::Teammate, Self::Broken, Self::Opened];

    fn of(mine: bool, opened: bool) -> Self {
        match (mine, opened) {
            (false, _) => Self::Teammate,
            (true, true) => Self::Opened,
            (true, false) => Self::Broken,
        }
    }

    fn style(self) -> (&'static str, Color32, MarkerShape) {
        match self {
            Self::Opened => ("Opened", OPENED, MarkerShape::Circle),
            Self::Broken => ("Broken", BROKEN, MarkerShape::Square),
            Self::Teammate => ("Teammates", TEAMMATE, MarkerShape::Diamond),
        }
    }
}

struct ChestRef<'a> {
    match_id: &'a str,
    player: &'a str,
    time: f32,
    opened: bool,
}

struct Marker<'a> {
    point: [f64; 2],
    series: Series,
    chest: ChestRef<'a>,
}

/// A chest clicked on the map. Kept by the moment it was opened or broken, not by its
/// place in a list, so it stays the same chest when the match's chests change.
#[derive(Debug, Clone, PartialEq)]
pub struct PickedChest {
    match_id: String,
    player: String,
    time: f32,
    opened: bool,
}

struct Resolved<'a> {
    record: &'a MatchRecord,
    player: &'a PlayerChests,
    event: &'a ChestEvent,
    opened: bool,
    series: Series,
    /// Place among the player's chests of the match, opened and broken, in the order
    /// they happened: chest number `index + 1`.
    index: usize,
    total: usize,
}

fn resolve<'a>(matches: &[&'a MatchRecord], chest: &PickedChest, nick: &str) -> Option<Resolved<'a>> {
    let record = *matches.iter().find(|m| m.id == chest.match_id)?;
    let player = record.roster().find(|p| same_name(&p.name, &chest.player))?;
    let events = player.in_order();
    let index = events.iter().position(|&(e, opened)| e.time == chest.time && opened == chest.opened)?;
    let (event, opened) = events[index];
    let series = Series::of(same_name(&player.name, record.player_name(nick)), opened);
    Some(Resolved { record, player, event, opened, series, index, total: events.len() })
}

/// The line above the map about the picked chest, with arrows to step through the other
/// chests of the same player in the same match. Returns the index an arrow stepped to.
fn picked_panel(ui: &mut egui::Ui, picked: Option<&Resolved<'_>>, scope: Scope) -> Option<usize> {
    let mut step = None;
    ui.horizontal_wrapped(|ui| {
        ui.set_min_height(26.0);
        let Some(p) = picked else {
            ui.label(theme::caption("Click a marker to see when that chest was opened or broken."));
            return;
        };
        let back = theme::arrow_button(ui, true, p.index > 0).on_hover_text("Earlier chest in this match");
        if back.clicked() {
            step = Some(p.index - 1);
        }
        let next = theme::arrow_button(ui, false, p.index + 1 < p.total).on_hover_text("Later chest in this match");
        if next.clicked() {
            step = Some(p.index + 1);
        }
        let (verb, color) = match (p.opened, p.event.in_vehicle) {
            (true, _) => ("Opened", OPENED),
            (false, true) => ("Rammed", BROKEN),
            (false, false) => ("Broke", BROKEN),
        };
        let chest = format!("{} chest", p.event.kind.label().to_lowercase());
        if p.series == Series::Teammate {
            ui.label(RichText::new(format!("{} ({verb} a {chest})", p.player.name)).strong().color(TEAMMATE));
            ui.label(format!("their chest {} of {} in this match", p.index + 1, p.total));
        } else {
            ui.label(RichText::new(format!("{verb} a {chest}")).strong().color(color));
            ui.label(format!("chest {} of {} in this match", p.index + 1, p.total));
        }
        ui.label(RichText::new(format!("{} {}", p.record.clock(p.event.time), p.record.clock_origin())).strong());
        if scope != Scope::Selected {
            if let Some(at) = p.record.started_at {
                ui.label(theme::caption(format!("match of {}", at.format("%d %b %H:%M"))));
            }
        }
        if p.event.x.is_none() {
            ui.label(theme::caption("(no position recorded, so not on the map)"));
        }
    });
    step
}

#[cfg(test)]
mod tests {
    use chests::{ChestEvent, ChestKind, PlayerChests};

    use super::{
        picture_rect, plot_point, resolve, PickedChest, Series, MAP_ORIGIN_PX, MAP_PX_PER_UNIT, MAP_REFERENCE_PX,
    };
    use crate::record::MatchRecord;

    fn chest(time: f32) -> ChestEvent {
        ChestEvent { time, kind: ChestKind::Normal, in_vehicle: false, x: Some(1.0), y: Some(1.0) }
    }

    #[test]
    fn built_in_picture_matches_the_calibration() {
        let image = image::load_from_memory(super::BUILT_IN_PICTURE).unwrap();
        assert_eq!((image.width(), image.height()), (MAP_REFERENCE_PX as u32, MAP_REFERENCE_PX as u32));
    }

    #[test]
    fn picked_chest_is_numbered_by_when_it_was_looted_and_survives_changes() {
        let me = PlayerChests {
            name: "Me".into(),
            opened: vec![chest(10.0)],
            broken: vec![chest(5.0)],
            ..Default::default()
        };
        let mut record =
            MatchRecord { id: "m".into(), owner: Some("Me".into()), players: vec![me], ..Default::default() };
        let pick = PickedChest { match_id: "m".into(), player: "me".into(), time: 10.0, opened: true };
        let got = resolve(&[&record], &pick, "").unwrap();
        // Broken at 5 s, then opened at 10 s: the second chest of two.
        assert_eq!((got.index, got.total, got.series), (1, 2, Series::Opened));

        // An earlier chest shows up (say, after a re-analysis): the pick follows its chest.
        record.players[0].opened.push(chest(1.0));
        let got = resolve(&[&record], &pick, "").unwrap();
        assert_eq!((got.index, got.total), (2, 3));

        // The same moment as a break is a different chest.
        assert!(resolve(&[&record], &PickedChest { opened: false, ..pick.clone() }, "").is_none());
        // Picked chest gone: nothing is picked.
        record.players[0].opened.clear();
        assert!(resolve(&[&record], &pick, "").is_none());
    }

    // World coordinates of v43.00 locations.
    const RUBBER_RESORT: (f64, f64) = (-24028.0, -45216.0);
    const FORTUNES_FORT: (f64, f64) = (48013.0, -27312.0);
    const PLUNDER_PORT: (f64, f64) = (26307.0, 89235.0);
    const ARLONG_PARK: (f64, f64) = (-51707.0, 16134.0);

    fn at((x, y): (f64, f64)) -> [f64; 2] {
        plot_point(x, y)
    }

    #[test]
    fn layout_matches_the_in_game_map() {
        // Rubber Resort top left, Fortune's Fort top right, Plunder Port at the bottom,
        // Arlong Park on the west coast.
        assert!(at(RUBBER_RESORT)[0] < at(FORTUNES_FORT)[0]);
        assert!(at(RUBBER_RESORT)[1] > at(PLUNDER_PORT)[1]);
        assert!(at(FORTUNES_FORT)[1] > at(PLUNDER_PORT)[1]);
        assert!(at(ARLONG_PARK)[0] < at(PLUNDER_PORT)[0]);
    }

    /// Where a world point falls on a picture of `size` pixels, through `picture_rect`.
    fn pixel_of(world: (f64, f64), size: [usize; 2]) -> (f64, f64) {
        let (center, extent) = picture_rect(size);
        let [x, y] = at(world);
        let left = center.x - f64::from(extent.x) / 2.0;
        let top = center.y + f64::from(extent.y) / 2.0;
        ((x - left) / f64::from(extent.x) * size[0] as f64, (top - y) / f64::from(extent.y) * size[1] as f64)
    }

    #[test]
    fn picture_follows_the_calibration() {
        for world in [RUBBER_RESORT, PLUNDER_PORT, ARLONG_PARK] {
            let want = (world.0 * MAP_PX_PER_UNIT + MAP_ORIGIN_PX[0], world.1 * MAP_PX_PER_UNIT + MAP_ORIGIN_PX[1]);
            let got = pixel_of(world, [2048, 2048]);
            assert!((got.0 - want.0).abs() < 0.5 && (got.1 - want.1).abs() < 0.5, "{got:?} vs {want:?}");
            // Half the resolution, same place on the island.
            let half = pixel_of(world, [1024, 1024]);
            assert!((half.0 * 2.0 - want.0).abs() < 0.5 && (half.1 * 2.0 - want.1).abs() < 0.5);
        }
        // The S.S. Rocky sits on the ship east of the island (checked by eye).
        let rocky = pixel_of((66471.0, 30308.0), [2048, 2048]);
        assert!((1390.0..1430.0).contains(&rocky.0) && (1210.0..1250.0).contains(&rocky.1), "{rocky:?}");
    }
}
