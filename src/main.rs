mod gestures;
mod input;
mod config;
mod window_monitor;
mod args;
mod sequence_step;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use evdev::{AbsoluteAxisCode, EventType};
use clap::Parser;
use std::path::Path;
use crate::config::Config;
use crate::gestures::{GesturesManager, Position};
use crate::input::{calculate_move_threshold_units, get_touchpad_device, get_touchpad_size};
use crate::args::Args;

#[derive(Debug, Default)]
pub struct Window {
    class: String,
    title: String,
}

fn init_logger(args: &Args) {
    let level_filter = match args.verbose {
        1 => log::LevelFilter::Info,
        2 => log::LevelFilter::Debug,
        _ => log::LevelFilter::Error,
    };
    env_logger::Builder::new()
        .format_timestamp(None)
        .filter_level(level_filter)
        .init();
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    init_logger(&args);

    let active_window = Arc::new(Mutex::new(Window::default()));

    std::thread::spawn({
        let active_window = Arc::clone(&active_window);
        move || {
            let mut wlroots = window_monitor::WlrootsMonitor::new(Box::new(move |class_name: String, title: String| {
                let mut active_window_guard = active_window.lock().unwrap();
                active_window_guard.class = class_name;
                active_window_guard.title = title;
                log::debug!("Active window changed: {:?}", *active_window_guard);
            }));
            wlroots.run();
        }
    });

    let config_path = if let Some(config_file) = &args.config_file {
        Path::new(&config_file).to_path_buf()
    } else {
        Config::get_config_path()
            .ok_or("Could not determine config file path. Make sure that either XDG_CONFIG_PATH or HOME environment variables are set.")?
    };

    log::debug!("Using config file: {:?}", config_path);

    let config = Config::parse_from_file(config_path).map_err(|e| format!("Failed to parse config file: {}", e))?;

    log::debug!("Loaded config: {:#?}", config);

    let touchpad_device = get_touchpad_device().ok_or("No touchpad device found")?;
    let touchpad_size = get_touchpad_size(&touchpad_device)?;
    let move_threshold_units = calculate_move_threshold_units(&touchpad_size, config.options.move_threshold);

    let mut gestures_manager = GesturesManager::new(config, active_window, move_threshold_units, touchpad_size);

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
                let filtered_state = state.iter()
                    .filter_map(|(slot, pos)| {
                        Some((*slot, Position { x: pos.0?, y: pos.1? }))
                    })
                    .collect();
                gestures_manager.update_state(filtered_state);
            },
            _ => continue,
        }
    }

    Ok(())
}
