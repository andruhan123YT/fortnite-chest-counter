//! Aggregate stats, the running total chart and the drop odds table.

use chests::odds::{
    any_reward_rate, chance_at_least_one, chests_for_probability, expected_drops, PIRATE_SHOP_ACCESS, REWARDS,
};
use eframe::egui::{self, RichText};
use egui_plot::{Legend, Line, Plot, PlotPoints};

use crate::app::ChestApp;
use crate::{stats, theme};

pub fn show(app: &mut ChestApp, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.label(theme::title("Stats & odds"));
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
                label: "Counted matches",
                value: t.counted_matches.to_string(),
                color: theme::TEXT,
                note: Some(format!("{:.0} minutes played", t.minutes)),
            },
            theme::Stat {
                label: "Per match",
                value: format!("{:.1}", t.per_match()),
                color: theme::GOLD,
                note: Some(format!("{:.2} per minute", t.per_minute())),
            },
            theme::Stat { label: "Best match", value: best.to_string(), color: theme::GOLD, note: None },
            theme::Stat {
                label: "Broken",
                value: t.broken.to_string(),
                color: theme::BROKEN,
                note: Some(format!(
                    "{} by vehicle · {} with pickaxe",
                    t.broken_by_vehicle,
                    t.broken - t.broken_by_vehicle
                )),
            },
        ],
    );
    ui.add_space(8.0);
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("By chest type").strong());
        egui::Grid::new("by_kind").num_columns(4).spacing([24.0, 4.0]).show(ui, |ui| {
            for h in ["Type", "Opened", "Broken", "Total"] {
                ui.label(theme::caption(h));
            }
            ui.end_row();
            for (key, (opened, broken)) in &t.by_kind {
                ui.label(key.kind().label());
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
        ui.label(RichText::new("Pirate Shop Access").size(17.0).strong().color(theme::GOLD));
        ui.label(theme::caption(format!(
            "1 in {} per chest, opened or broken. The whole squad gets it when anyone in the squad does. \
             Squad chests here are only those seen near you, so the squad's real chance is higher.",
            theme::thousands(one_in as u64)
        )));
        ui.add_space(6.0);
        egui::Grid::new("pirate").num_columns(3).spacing([28.0, 6.0]).show(ui, |ui| {
            ui.label("");
            ui.label(theme::caption("Chests"));
            ui.label(theme::caption("Chance of at least one"));
            ui.end_row();
            ui.label(RichText::new("You + squad").color(theme::SQUAD)).on_hover_text(theme::SQUAD_NEARBY_NOTE);
            ui.label(theme::squad_count(squad));
            ui.label(RichText::new(theme::percent(chance_at_least_one(squad, one_in))).strong().size(18.0));
            ui.end_row();
            ui.label(RichText::new("Just you").color(theme::GOLD));
            ui.label(theme::thousands(me));
            ui.label(RichText::new(theme::percent(chance_at_least_one(me, one_in))).strong().size(18.0));
            ui.end_row();
        });
        ui.add_space(6.0);
        for target in [0.25, 0.5, 0.9] {
            let needed = chests_for_probability(target, one_in);
            let left = needed.saturating_sub(squad);
            ui.label(theme::caption(format!(
                "{:.0}% chance at {} chests · {} more with your squad",
                target * 100.0,
                theme::thousands(needed),
                theme::thousands(left)
            )));
        }
    });
}

fn rewards(app: &mut ChestApp, ui: &mut egui::Ui) {
    let squad = app.totals.with_squad();
    let mut changed = false;
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Expected drops").strong());
        ui.label(theme::caption(format!(
            "Based on the {} chests you and your squad looted in your replays: every chest anyone in your squad opens or breaks rolls every reward \
             for the whole squad, and chests teammates looted far from you are not recorded, so expect a little more than shown. \
             Replays show that a prize arrived but not which one, so enter what you got to see how lucky you are.",
            theme::thousands(squad)
        )));
        ui.add_space(4.0);
        egui::Grid::new("rewards").num_columns(6).striped(true).spacing([22.0, 6.0]).show(ui, |ui| {
            for h in ["Reward", "Odds", "Expected", "At least one", "You got", ""] {
                ui.label(theme::caption(h));
            }
            ui.end_row();
            let counted = app.totals.counted_squad();
            let expected_any = counted as f64 * any_reward_rate();
            ui.label(RichText::new("Any prize, counted matches").color(theme::TREASURE));
            ui.label(format!("1 in {:.0}", 1.0 / any_reward_rate()));
            ui.label(format!("{expected_any:.2}"));
            ui.label(theme::caption(format!("{} chests", theme::thousands(counted))));
            ui.label(RichText::new(app.totals.treasures.to_string()).strong().color(theme::TREASURE));
            ui.label(luck(app.totals.treasures as u32, expected_any));
            ui.end_row();
            for reward in REWARDS {
                let expected = expected_drops(squad, reward.one_in);
                ui.label(reward.name);
                ui.label(format!("1 in {}", theme::thousands(reward.one_in as u64)));
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

fn luck(got: u32, expected: f64) -> RichText {
    if expected < 0.05 {
        return RichText::new(if got > 0 { "very lucky" } else { "" }).color(theme::OPENED);
    }
    let ratio = f64::from(got) / expected.max(1e-9);
    if ratio >= 1.25 {
        RichText::new(format!("lucky, {ratio:.1}x expected")).color(theme::OPENED)
    } else if ratio <= 0.75 {
        RichText::new(format!("unlucky, {ratio:.1}x expected")).color(theme::BROKEN)
    } else {
        RichText::new("about as expected").color(theme::MUTED)
    }
}

fn chart(app: &ChestApp, ui: &mut egui::Ui) {
    let series = stats::running_totals(&app.store.matches, &app.settings);
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Running total").strong());
        if series.len() < 2 {
            ui.label(theme::caption("The chart appears after the first counted match."));
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
                    .map(|d| d.format("%d %b %H:%M").to_string())
                    .unwrap_or_default()
            })
            .show(ui, |plot| {
                plot.line(Line::new("Teammates", mates).color(theme::SQUAD).width(2.0));
                plot.line(Line::new("Mine", mine).color(theme::GOLD).width(2.5));
            });
    });
}
