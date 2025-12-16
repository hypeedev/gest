// TODO: Create a lock file to prevent multiple instances running simultaneously
// TODO: Fix 4 up -> 4 left gesture switching to left tab

mod gestures;
mod input;
mod config;
mod window_monitor;
mod args;
mod sequence_step;

use std::collections::HashMap;
use std::sync::Arc;
use arc_swap::ArcSwap;
use evdev::{AbsoluteAxisCode, EventType};
use clap::Parser;
use notify::Watcher;
use std::path::Path;
use crate::config::Config;
use crate::gestures::{GesturesEngine, Position, State};
use crate::input::{calculate_move_threshold_units, get_touchpad_device, get_touchpad_size};
use crate::args::Args;

#[derive(Debug, Default)]
pub struct Window {
    class: String,
    title: String,
}

fn init_logger(args: &Args) {
    let level_filter = match args.verbose {
        0 => log::LevelFilter::Error,
        1 => log::LevelFilter::Info,
        2.. => log::LevelFilter::Debug,
    };

    let target = if let Some(log_file) = &args.log_file {
        let file = match std::fs::File::create(log_file) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Failed to create log file {}: {}", log_file, e);
                std::process::exit(1);
            }
        };
        env_logger::Target::Pipe(Box::new(file))
    } else {
        env_logger::Target::Stdout
    };

    env_logger::Builder::new()
        .format_timestamp(None)
        .filter_level(level_filter)
        .target(target)
        .init();
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    init_logger(&args);

    let active_window = Arc::new(ArcSwap::new(Window::default().into()));

    std::thread::spawn({
        let active_window = active_window.clone();
        move || {
            let mut wlroots = window_monitor::WlrootsMonitor::new(Box::new(move |class: String, title: String| {
                let new_window = Window { class, title };
                log::debug!("Active window changed: {:?}", new_window);
                active_window.swap(new_window.into());
            }));
            wlroots.run();
        }
    });

    let config_path = if let Some(config_file) = &args.config_file {
        Path::new(&config_file).to_path_buf()
    } else {
        match Config::get_config_path() {
            Some(path) => path,
            None => {
                log::error!("Could not determine config file path. Make sure that either XDG_CONFIG_PATH or HOME environment variables are set.");
                std::process::exit(1);
            }
        }
    };

    log::debug!("Using config file: {:?}", config_path);

    let config = Arc::new(ArcSwap::new(match Config::parse_from_file(&config_path) {
        Ok(cfg) => cfg.into(),
        Err(e) => {
            log::error!("Failed to parse config file: {}", e);
            std::process::exit(1);
        }
    }));

    log::debug!("Loaded config: {:#?}", config);

    // Watch config file for changes
    std::thread::spawn({
        let config = config.clone();
        move || {
            let (tx, rx) = std::sync::mpsc::channel();
            let mut watcher = notify::recommended_watcher(tx).unwrap();
            let parent = config_path.parent().unwrap();
            watcher.watch(Path::new(parent), notify::RecursiveMode::Recursive).unwrap();
            for res in rx {
                match res {
                    Ok(event) => {
                        if let notify::EventKind::Modify(notify::event::ModifyKind::Data(_)) = event.kind {
                            let config_guard = config.load();
                            if event.paths.iter().any(|path| *path == config_path || config_guard.import.contains(path)) {
                                log::info!("Config file changed, reloading...");
                                match Config::parse_from_file(&config_path) {
                                    Ok(new_config) => {
                                        config.swap(new_config.into());
                                        log::info!("Config reloaded successfully.");
                                    }
                                    Err(e) => {
                                        log::error!("Failed to reload config file: {}", e);
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("Watch error: {:?}", e);
                    }
                }
            }
        }
    });

    let touchpad_device = match get_touchpad_device() {
        Some(device) => device,
        None => {
            log::error!("No touchpad device found.");
            std::process::exit(1);
        }
    };
    let touchpad_size = match get_touchpad_size(&touchpad_device) {
        Ok(size) => size,
        Err(e) => {
            log::error!("Could not determine touchpad size: {}", e);
            std::process::exit(1);
        }
    };

    let move_threshold_units = calculate_move_threshold_units(&touchpad_size, config.load().options.move_threshold);

    let mut gestures_manager = GesturesEngine::new(config, active_window, move_threshold_units, touchpad_size);

    let mut state: HashMap<u8, (Option<u16>, Option<u16>)> = HashMap::new();
    let mut current_slot = 0u8;

    let mut event_stream = touchpad_device.into_event_stream().unwrap();
    while let Ok(event) = event_stream.next_event().await {
        match event.event_type() {
            EventType::ABSOLUTE => {
                match AbsoluteAxisCode(event.code()) {
                    AbsoluteAxisCode::ABS_MT_SLOT => {
                        current_slot = event.value() as u8;
                    }
                    AbsoluteAxisCode::ABS_MT_TRACKING_ID => {
                        if event.value() == -1 {
                            state.remove(&current_slot);
                        } else {
                            state.insert(current_slot, (None, None));
                        }
                    }
                    AbsoluteAxisCode::ABS_MT_POSITION_X => {
                        if let Some(position) = state.get_mut(&current_slot) {
                            position.0 = Some(event.value() as u16);
                        }
                    }
                    AbsoluteAxisCode::ABS_MT_POSITION_Y => {
                        if let Some(position) = state.get_mut(&current_slot) {
                            position.1 = Some(event.value() as u16);
                        }
                    }
                    _ => {}
                }
            },
            EventType::SYNCHRONIZATION => {
                let mut filtered_state = State::default();
                for (u8, (pos_x, pos_y)) in &state {
                    if let (Some(x), Some(y)) = (pos_x, pos_y) {
                        filtered_state.positions.insert(*u8, Position { x: *x, y: *y });
                    }
                }
                gestures_manager.update_state(filtered_state);
            },
            _ => continue,
        }
    }

    Ok(())
}
