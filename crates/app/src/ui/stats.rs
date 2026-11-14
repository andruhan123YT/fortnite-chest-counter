//! Aggregate stats, the running total chart and the drop odds table.

use chests::odds::{
    any_reward_rate, chance_at_least_one, chests_for_probability, expected_drops, PIRATE_SHOP_ACCESS, REWARDS,
};
use eframe::egui::{self, RichText};
use egui_plot::{Legend, Line, Plot, PlotPoints};

use crate::app::ChestApp;
use crate::i18n::{date_format, plural, tr, trf};
use crate::{stats, theme};

pub fn show(app: &mut ChestApp, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.label(theme::title(tr("Stats & odds")));
        ui.add_space(10.0);
        numbers(app, ui);
        ui.add_space(12.0);
        pirate_shop(app, ui);
        ui.add_space(12.0);
        rewards(app, ui);
        ui.add_space(12.0);
        chart(app, ui);
    });
}

fn numbers(app: &ChestApp, ui: &mut egui::Ui) {
    let t = &app.totals;
    let best = t.best_match.as_ref().map_or(0, |(_, n)| *n);
    theme::stat_row(
        ui,
        &[
            theme::Stat {
                label: tr("Counted matches"),
                value: t.counted_matches.to_string(),
                color: theme::TEXT,
                note: Some(trf("{0} minutes played", &[&format!("{:.0}", t.minutes)])),
            },
            theme::Stat {
                label: tr("Per match"),
                value: format!("{:.1}", t.per_match()),
                color: theme::GOLD,
                note: Some(trf("{0} per minute", &[&format!("{:.2}", t.per_minute())])),
            },
            theme::Stat { label: tr("Best match"), value: best.to_string(), color: theme::GOLD, note: None },
            theme::Stat {
                label: tr("Broken"),
                value: t.broken.to_string(),
                color: theme::BROKEN,
                note: Some(trf(
                    "{0} by vehicle · {1} with pickaxe",
                    &[&t.broken_by_vehicle, &(t.broken - t.broken_by_vehicle)],
                )),
            },
        ],
    );
    ui.add_space(8.0);
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(tr("By chest type")).strong());
        egui::Grid::new("by_kind").num_columns(4).spacing([24.0, 4.0]).show(ui, |ui| {
            for h in [tr("Type"), tr("Opened"), tr("Broken"), tr("Total")] {
                ui.label(theme::caption(h));
            }
            ui.end_row();
            for (key, (opened, broken)) in &t.by_kind {
                ui.label(theme::chest_kind(key.kind()));
                ui.label(RichText::new(opened.to_string()).color(theme::OPENED));
                ui.label(RichText::new(broken.to_string()).color(theme::BROKEN));
                ui.label(RichText::new((opened + broken).to_string()).strong());
                ui.end_row();
            }
        });
    });
}

fn pirate_shop(app: &ChestApp, ui: &mut egui::Ui) {
    let me = app.totals.mine();
    let squad = app.totals.with_squad();
    let one_in = PIRATE_SHOP_ACCESS.one_in;
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(tr("Pirate Shop Access")).size(17.0).strong().color(theme::GOLD));
        ui.label(theme::caption(trf(
            "1 in {0} per chest, opened or broken. The whole squad gets it when anyone in the squad does. \
             Squadmates who run the app count in full, from their own replays. For the rest, only the chests \
             your replay saw count, so the squad's real chance can be higher.",
            &[&theme::thousands(one_in as u64)],
        )));
        ui.add_space(6.0);
        egui::Grid::new("pirate").num_columns(3).spacing([28.0, 6.0]).show(ui, |ui| {
            ui.label("");
            ui.label(theme::caption(tr("Chests")));
            ui.label(theme::caption(tr("Chance of at least one")));
            ui.end_row();
            ui.label(RichText::new(tr("You + squad")).color(theme::SQUAD)).on_hover_text(theme::squad_nearby_note());
            ui.label(theme::squad_count(squad));
            ui.label(RichText::new(theme::percent(chance_at_least_one(squad, one_in))).strong().size(18.0));
            ui.end_row();
            ui.label(RichText::new(tr("Just you")).color(theme::GOLD));
            ui.label(theme::thousands(me));
            ui.label(RichText::new(theme::percent(chance_at_least_one(me, one_in))).strong().size(18.0));
            ui.end_row();
        });
        ui.add_space(6.0);
        for target in [0.25, 0.5, 0.9] {
            let needed = chests_for_probability(target, one_in);
            let left = needed.saturating_sub(squad);
            ui.label(theme::caption(trf(
                "{0}% chance at {1} chests · {2} more with your squad",
                &[&format!("{:.0}", target * 100.0), &theme::thousands(needed), &theme::thousands(left)],
            )));
        }
    });
}

/// Lines of squadmates who run the app, apart from the gold, blue and purple of the
/// app's own series.
const SQUADMATE_COLORS: [egui::Color32; 5] = [
    egui::Color32::from_rgb(0x4f, 0xd1, 0xc5),
    egui::Color32::from_rgb(0xf2, 0x7e, 0x9c),
    egui::Color32::from_rgb(0x9b, 0xd8, 0x5a),
    egui::Color32::from_rgb(0xf0, 0x9a, 0x4a),
    egui::Color32::from_rgb(0xe0, 0xe0, 0xe0),
];

fn rewards(app: &mut ChestApp, ui: &mut egui::Ui) {
    let squad = app.totals.with_squad();
    let mut changed = false;
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(tr("Expected drops")).strong());
        ui.label(theme::caption(trf(
            "Based on the {0} chests you and your squad looted in your replays: every chest anyone in your squad opens or breaks rolls every reward \
             for the whole squad, and chests teammates looted far from you are not recorded, so expect a little more than shown. \
             Replays show that a prize arrived but not which one, so enter what you got to see how lucky you are.",
            &[&theme::thousands(squad)],
        )));
        ui.add_space(4.0);
        egui::Grid::new("rewards").num_columns(6).striped(true).spacing([22.0, 6.0]).show(ui, |ui| {
            for h in [tr("Reward"), tr("Odds"), tr("Expected"), tr("At least one"), tr("You got"), ""] {
                ui.label(theme::caption(h));
            }
            ui.end_row();
            let counted = app.totals.counted_squad();
            let expected_any = counted as f64 * any_reward_rate();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(RichText::new(tr("Any prize, counted matches")).color(theme::TREASURE));
                theme::info_icon(ui, any_prize_help(counted, expected_any, app.totals.treasures as u64));
            });
            ui.label(trf("1 in {0}", &[&format!("{:.0}", 1.0 / any_reward_rate())]));
            ui.label(format!("{expected_any:.2}"));
            ui.label(theme::caption(trf("{0} chests", &[&theme::thousands(counted)])));
            ui.label(RichText::new(app.totals.treasures.to_string()).strong().color(theme::TREASURE));
            ui.label(luck(app.totals.treasures as u32, expected_any));
            ui.end_row();
            for reward in REWARDS {
                let expected = expected_drops(squad, reward.one_in);
                ui.label(theme::reward_name(reward.name));
                ui.label(trf("1 in {0}", &[&theme::thousands(reward.one_in as u64)]));
                ui.label(format!("{expected:.2}"));
                ui.label(theme::percent(chance_at_least_one(squad, reward.one_in)));
                let entered = app.settings.received.get(reward.name).copied();
                let mut got = entered.unwrap_or(0);
                if ui.add(egui::DragValue::new(&mut got).range(0..=100_000)).changed() {
                    app.settings.received.insert(reward.name.to_string(), got);
                    changed = true;
                }
                ui.label(entered.map_or_else(|| RichText::new(""), |got| luck(got, expected)));
                ui.end_row();
            }
        });
    });
    if changed {
        let _ = app.settings.save();
    }
}

/// The hover text of the "Any prize" row, in plain words and with the player's numbers.
fn any_prize_help(chests: u64, expected: f64, got: u64) -> String {
    let chests = format!(
        "{} {}",
        theme::thousands(chests),
        plural(chests, "chest", "chests", ["сундук", "сундука", "сундуков"])
    );
    let got = format!("{} {}", theme::thousands(got), plural(got, "prize", "prizes", ["приз", "приза", "призов"]));
    trf(
        "Shows whether you have been lucky with prizes.\n\n\
         About 1 in {0} chests gives some prize (any of the rewards below), and every chest you or your squad \
         loot counts.\n\n\
         Your squad looted {1} in counted matches, so about {2} prizes were expected on average. \
         You got {3}.\n\n\
         The base and chests added by hand are left out: their prizes are not in your replays.",
        &[&format!("{:.0}", 1.0 / any_reward_rate()), &chests, &format!("{expected:.1}"), &got],
    )
}

fn luck(got: u32, expected: f64) -> RichText {
    if expected < 0.05 {
        return RichText::new(if got > 0 { tr("very lucky") } else { "" }).color(theme::OPENED);
    }
    let ratio = f64::from(got) / expected.max(1e-9);
    if ratio >= 1.25 {
        RichText::new(trf("lucky, {0}x expected", &[&format!("{ratio:.1}")])).color(theme::OPENED)
    } else if ratio <= 0.75 {
        RichText::new(trf("unlucky, {0}x expected", &[&format!("{ratio:.1}")])).color(theme::BROKEN)
    } else {
        RichText::new(tr("about as expected")).color(theme::MUTED)
    }
}

fn chart(app: &ChestApp, ui: &mut egui::Ui) {
    let series = stats::running_totals(&app.store.matches, &app.settings);
    let squadmates = stats::squadmate_totals(&app.store.matches, &app.settings);
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(tr("Running total")).strong());
        if series.len() < 2 {
            ui.label(theme::caption(tr("The chart appears after the first counted match.")));
            return;
        }
        let mine: PlotPoints = series.iter().map(|&(x, m, _)| [x, m]).collect();
        let mates: PlotPoints = series.iter().map(|&(x, _, s)| [x, s]).collect();
        Plot::new("running_total")
            .height(260.0)
            .legend(Legend::default())
            .allow_scroll(false)
            .x_axis_formatter(|mark, _| {
                chrono::DateTime::from_timestamp(mark.value as i64, 0)
                    .map(|d| d.format(&date_format(&d, "%d %b %H:%M")).to_string())
                    .unwrap_or_default()
            })
            .show(ui, |plot| {
                plot.line(Line::new(tr("Teammates"), mates).color(theme::SQUAD).width(2.0));
                for (i, (name, points)) in squadmates.iter().enumerate() {
                    let points: PlotPoints = points.iter().map(|&(x, y)| [x, y]).collect();
                    let color = SQUADMATE_COLORS[i % SQUADMATE_COLORS.len()];
                    plot.line(Line::new(name.as_str(), points).color(color).width(1.6));
                }
                plot.line(Line::new(tr("Mine"), mine).color(theme::GOLD).width(2.5));
            });
        if !squadmates.is_empty() {
            ui.label(theme::caption(tr(
                "Teammates is everyone you played with. Named lines are squadmates who run the app too, counted \
                 in full over the matches you played together. Click a name to hide its line.",
            )));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn any_prize_help_shows_the_players_numbers() {
        let text = any_prize_help(1234, 19.93, 1);
        assert!(text.contains("looted 1,234 chests in counted matches"), "{text}");
        assert!(text.contains("about 19.9 prizes were expected"), "{text}");
        assert!(text.contains("You got 1 prize."), "{text}");
        assert!(text.contains(&format!("About 1 in {:.0} chests", 1.0 / any_reward_rate())), "{text}");
    }
}
