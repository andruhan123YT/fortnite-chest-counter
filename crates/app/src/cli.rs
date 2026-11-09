//! Command-line mode, for scripting and for checking the parser against real replays.

use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use crate::config::Settings;
use crate::record::MatchRecord;
use crate::{platform, worker, APP_NAME};

const HELP: &str = "\
Usage:
  fortnite-chest-counter                     Start the app
  fortnite-chest-counter --minimized         Start in the system tray
  fortnite-chest-counter analyze <replays or folders>... [--nick NAME] [--json]
  fortnite-chest-counter info <file.replay>
  fortnite-chest-counter dump-events <file.replay>
  fortnite-chest-counter list-exports <file.replay> [filter]
  fortnite-chest-counter update [--check]    Install the latest release from GitHub
";

struct Silent;
impl fnreplay::Visitor for Silent {}

/// Runs a subcommand, or returns `None` to start the GUI.
pub fn run(args: &[String]) -> Option<ExitCode> {
    let command = args.first()?.as_str();
    if !matches!(command, "analyze" | "info" | "dump-events" | "list-exports" | "update" | "--help" | "-h" | "help") {
        return None;
    }
    platform::attach_parent_console();
    let rest = &args[1..];
    let result = match command {
        "analyze" => analyze(rest),
        "info" => info(rest),
        "dump-events" => dump_events(rest),
        "list-exports" => list_exports(rest),
        "update" => self_update(rest),
        _ => {
            print!("{APP_NAME}\n\n{HELP}");
            Ok(())
        }
    };
    Some(match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    })
}

fn flag_value(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned()
}

fn positional(args: &[String]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
        } else if a == "--nick" {
            skip = true;
        } else if !a.starts_with("--") {
            out.push(PathBuf::from(a));
        }
    }
    out
}

fn analyze(args: &[String]) -> Result<(), String> {
    let settings = Settings::load();
    let nick = flag_value(args, "--nick").unwrap_or(settings.nick.clone());
    let json = args.iter().any(|a| a == "--json");
    let files = worker::expand(&positional(args));
    if files.is_empty() {
        return Err(format!("no replays given\n\n{HELP}"));
    }
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let (mut total_mine, mut total_squad) = (0, 0);
    for path in files {
        let Some((name, size)) = worker::file_key(&path) else { continue };
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let started = std::time::Instant::now();
        let (replay, analysis) = match chests::analyze_replay(&bytes, &settings.breaks, &mut Silent) {
            Ok(result) => result,
            Err(e) => {
                let _ = writeln!(out, "{name}: {e}");
                continue;
            }
        };
        let record = MatchRecord::from_analysis(&name, size, &replay, analysis, &nick);
        if json {
            let _ = writeln!(out, "{}", serde_json::to_string(&record).unwrap_or_default());
            continue;
        }
        let s = record.summary(&nick);
        total_mine += s.mine();
        total_squad += s.mates;
        let _ = writeln!(
            out,
            "{name}  {}  opened {}  broken {}  squad {}  treasures {}  packets ok {:.2}%  ({:.1}s){}",
            record.started_at.map(|t| t.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default(),
            s.opened,
            s.broken,
            s.mates,
            s.treasures,
            record.packet_health * 100.0,
            started.elapsed().as_secs_f32(),
            if s.found { String::new() } else { format!("  ({nick} not in this match)") },
        );
        let _ = writeln!(
            out,
            "    recorded by {}, match id {}, times {}",
            record.owner.as_deref().unwrap_or("?"),
            record.match_id.as_deref().unwrap_or("-"),
            record.clock_origin()
        );
        if let Some(me) = record.me(&nick) {
            for e in &me.opened {
                let how = if e.from_loot { "  (from loot)" } else { "" };
                let _ = writeln!(out, "    {:>5}  opened  {}{how}", record.clock(e.time), e.kind.label());
            }
            for e in &me.broken {
                let _ = writeln!(
                    out,
                    "    {:>5}  broke   {}{}",
                    record.clock(e.time),
                    e.kind.label(),
                    if e.in_vehicle { " (vehicle)" } else { "" }
                );
            }
        }
        for t in record.treasures(&nick) {
            let from = t.source.as_ref().map_or_else(
                || "squad chest out of view".to_string(),
                |src| format!("{} {} {}", src.player, if src.broken { "broke" } else { "opened" }, src.kind.label()),
            );
            let _ = writeln!(out, "    {:>5}  treasure  {from}", record.clock(t.time));
        }
        for (mate, n) in &s.teammates {
            let _ = writeln!(out, "    teammate {mate}: {n}");
        }
    }
    if !json {
        let _ = writeln!(out, "\ntotal: mine {total_mine}, squad {total_squad}");
    }
    Ok(())
}

fn info(args: &[String]) -> Result<(), String> {
    let path = positional(args).into_iter().next().ok_or("no replay given")?;
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let info = fnreplay::read_info(&bytes).map_err(|e| e.to_string())?;
    println!("file version      {}", info.file_version);
    println!("length            {:.1} min", f64::from(info.length_ms) / 60_000.0);
    println!("live flag         {}", info.is_live);
    println!("complete          {}", info.is_complete());
    match fnreplay::parse(&bytes, &fnreplay::Schema::default(), &mut Silent) {
        Ok(replay) => {
            let h = &replay.header;
            println!("network version   {}", h.network_version);
            println!("engine net ver.   {}", h.engine_network_version);
            println!("branch            {}", h.branch);
            println!("packets           {} ({} failed)", replay.stream.packets, replay.stream.failed_packets);
            let seconds = (f64::from(info.length_ms) / 1000.0).max(1.0);
            println!(
                "frames            {} ({:.1} per second recorded)",
                replay.stream.frames,
                replay.stream.frames as f64 / seconds
            );
            if let Some(t) = replay.team_stats {
                println!("placement         {} / {}", t.placement, t.total_players);
            }
        }
        Err(e) => println!("stream            {e}"),
    }
    Ok(())
}

fn dump_events(args: &[String]) -> Result<(), String> {
    let path = positional(args).into_iter().next().ok_or("no replay given")?;
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let stdout = std::io::stdout();
    let mut dump = chests::dump::EventDump::new(BufWriter::new(stdout.lock()));
    fnreplay::parse(&bytes, &chests::fortnite_schema(), &mut dump).map_err(|e| e.to_string())?;
    Ok(())
}

/// Every replicated class and field in a replay, for finding what a game update changed.
fn list_exports(args: &[String]) -> Result<(), String> {
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Exports(BTreeMap<String, BTreeMap<u32, String>>);
    impl fnreplay::Visitor for Exports {
        fn on_net_field_export(&mut self, group_path: &str, handle: u32, name: &str) {
            self.0.entry(group_path.to_string()).or_default().insert(handle, name.to_string());
        }
    }

    let files = positional(args);
    let path = files.first().ok_or("no replay given")?;
    let filter = files.get(1).map(|f| f.to_string_lossy().to_lowercase());
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut exports = Exports::default();
    fnreplay::parse(&bytes, &fnreplay::Schema::default(), &mut exports).map_err(|e| e.to_string())?;
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    for (group, fields) in &exports.0 {
        let matches = |text: &str| filter.as_deref().is_none_or(|f| text.to_lowercase().contains(f));
        if !matches(group) && !fields.values().any(|n| matches(n)) {
            continue;
        }
        let _ = writeln!(out, "{group}");
        for (handle, name) in fields {
            let _ = writeln!(out, "  {handle:>4} {name}");
        }
    }
    Ok(())
}

fn self_update(args: &[String]) -> Result<(), String> {
    use crate::update::{check, current, install, version_text};
    println!("installed: v{}", version_text(current()));
    let Some(release) = check()? else {
        println!("no newer release");
        return Ok(());
    };
    println!("available: v{}", version_text(release.version));
    if args.iter().any(|a| a == "--check") {
        return Ok(());
    }
    let mut shown = 0;
    let exe = install(&release, |p| {
        let percent = (p * 100.0) as u32;
        if percent >= shown + 10 {
            shown = percent - percent % 10;
            println!("  {shown}%");
        }
    })?;
    println!("installed v{} at {}; restart the app to use it", version_text(release.version), exe.display());
    Ok(())
}

pub fn mmss(seconds: f32) -> String {
    let sign = if seconds <= -1.0 { "-" } else { "" };
    let s = seconds.abs() as u32;
    format!("{sign}{}:{:02}", s / 60, s % 60)
}
