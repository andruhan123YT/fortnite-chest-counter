//! Fortnite Chest Counter: counts the chests you open and break, from your replays.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod archive;
mod cli;
mod config;
mod icon;
mod online;
mod overlay;
mod platform;
mod quest;
mod record;
mod stats;
mod store;
mod theme;
mod tray;
mod ui;
mod update;
mod worker;

use std::process::ExitCode;

pub const APP_NAME: &str = "Fortnite Chest Counter";

fn main() -> ExitCode {
    log_panics();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = cli::run(&args) {
        return code;
    }
    if args.iter().any(|a| a == update::AFTER_UPDATE_ARG) {
        // Started by the version this one replaced, which is closing: wait for it.
        if !platform::claim_instance_waiting(std::time::Duration::from_secs(30)) {
            config::log_event("update: the old version did not quit, giving up");
            return ExitCode::FAILURE;
        }
        config::log_event(&format!("updated to v{}", env!("CARGO_PKG_VERSION")));
    } else if platform::activate_existing_instance() {
        return ExitCode::SUCCESS;
    }
    std::thread::spawn(update::remove_leftover);
    config::log_event(&format!("start v{} {}", env!("CARGO_PKG_VERSION"), args.join(" ")));
    let code = app::run(args.iter().any(|a| a == "--minimized"));
    config::log_event("event loop ended");
    code
}

/// The GUI build has no console, so a panic would vanish without a trace. Append it to
/// `crash.log` next to the settings.
fn log_panics() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        use std::io::Write;
        let dir = config::data_dir();
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("crash.log")) {
            let thread = std::thread::current();
            let _ = writeln!(
                file,
                "{} v{} thread '{}': {info}\n{}\n",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                env!("CARGO_PKG_VERSION"),
                thread.name().unwrap_or("?"),
                std::backtrace::Backtrace::force_capture()
            );
        }
        default_hook(info);
    }));
}
