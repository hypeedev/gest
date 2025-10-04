use evdev::{AbsoluteAxisCode, Device, EventType};
use crate::gestures::MoveThresholdUnits;

pub fn get_touchpad_device() -> Result<Device, String> {
    for (_, device) in evdev::enumerate() {
        let is_touchpad = device.supported_events().contains(EventType::KEY)
            && device.supported_events().contains(EventType::ABSOLUTE)
            && device.supported_keys().is_some_and(|keys| keys.contains(evdev::KeyCode::BTN_TOUCH));

        if is_touchpad {
            return Ok(device);
        }
    }

    Err("Error: No touchpad device found.".to_string())
}

pub fn calculate_move_threshold_units(touchpad_device: &Device, threshold: f32) -> Result<MoveThresholdUnits, Box<dyn std::error::Error>> {
    let mut x = 0;
    let mut y = 0;

    for (code, abs) in touchpad_device.get_absinfo()? {
        match code {
            AbsoluteAxisCode::ABS_MT_POSITION_X => x = (abs.maximum() as f32 * threshold) as u16,
            AbsoluteAxisCode::ABS_MT_POSITION_Y => y = (abs.maximum() as f32 * threshold) as u16,
            _ => {}
        }
    }

    Ok(MoveThresholdUnits { x, y })
}
