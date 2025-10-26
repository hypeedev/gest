use evdev::{AbsoluteAxisCode, Device, EventType, KeyCode};
use crate::gestures::MoveThresholdUnits;

pub fn get_touchpad_device() -> Option<Device> {
    for (_, device) in evdev::enumerate() {
        let is_touchpad = device.supported_events().contains(EventType::KEY)
            && device.supported_events().contains(EventType::ABSOLUTE)
            && device.supported_keys().is_some_and(|keys| keys.contains(KeyCode::BTN_TOUCH));

        if is_touchpad {
            return Some(device);
        }
    }
    None
}

pub fn calculate_move_threshold_units(touchpad_size: &MoveThresholdUnits, threshold: f32) -> MoveThresholdUnits {
    let x = (touchpad_size.x as f32 * threshold) as u16;
    let y = (touchpad_size.y as f32 * threshold) as u16;
    MoveThresholdUnits { x, y }
}

pub fn get_touchpad_size(touchpad_device: &Device) -> Result<MoveThresholdUnits, Box<dyn std::error::Error>> {
    let mut width = 0;
    let mut height = 0;

    for (code, abs) in touchpad_device.get_absinfo()? {
        match code {
            AbsoluteAxisCode::ABS_MT_POSITION_X => width = abs.maximum() as u16,
            AbsoluteAxisCode::ABS_MT_POSITION_Y => height = abs.maximum() as u16,
            _ => {}
        }
    }

    Ok(MoveThresholdUnits { x: width, y: height })
}
