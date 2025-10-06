use std::collections::{HashMap, HashSet};
use std::fmt::{Debug, Formatter};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use crate::config::{Config, Direction, DefinedSequenceStep, Gesture};
use crate::Window;

#[derive(Debug, Clone, Copy)]
pub struct Position {
    pub x: u16,
    pub y: u16,
}

pub type State = HashMap<u8, Position>;

enum PerformedSequenceStep {
    Move { slots: HashSet<u8>, direction: Direction },
    TouchUp { slots: HashSet<u8> },
    TouchDown { slots: HashSet<u8> },
}

impl Debug for PerformedSequenceStep {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TouchDown { slots } => write!(f, "TouchDown({})", slots.len()),
            Self::TouchUp { slots } => write!(f, "TouchUp({})", slots.len()),
            Self::Move { slots, direction } => write!(f, "Move{:?}({})", direction, slots.len()),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MoveThresholdUnits {
    pub x: u16,
    pub y: u16,
}

#[derive(Debug)]
pub struct GesturesManager {
    pub config: Config,
    previous_state: State, // positions of fingers in the previous update
    start_state: State, // initial positions when fingers touch down
    last_known_state: State, // positions of fingers just before they were lifted
    performed_sequence: Vec<PerformedSequenceStep>,
    repeated_gesture: bool,
    move_threshold_units: MoveThresholdUnits,
    active_window: Arc<Mutex<Window>>,
}

impl GesturesManager {
    pub fn new(config: Config, active_window: Arc<Mutex<Window>>, move_threshold_units: MoveThresholdUnits) -> Self {
        Self {
            config,
            previous_state: HashMap::new(),
            start_state: HashMap::new(),
            last_known_state: HashMap::new(),
            performed_sequence: Vec::new(),
            repeated_gesture: false,
            move_threshold_units,
            active_window,
        }
    }

    fn update_last_step(&mut self, slot: u8, direction: &Direction) -> bool {
        if let Some(PerformedSequenceStep::Move { slots, direction: dir }) = self.performed_sequence.last_mut()
            && dir == direction
        {
            slots.insert(slot);
            return true;
        }
        false
    }

    pub fn update_state(&mut self, state: &State) {
        if state.is_empty() {
            if !self.repeated_gesture {
                self.match_gestures(false);
            } else {
                self.repeated_gesture = false;
            }

            self.previous_state.clear();
            self.start_state.clear();
            self.last_known_state.clear();
            self.performed_sequence.clear();
            return;
        }

        for (slot, pos) in state {
            if self.start_state.contains_key(slot) { continue; }
            self.start_state.insert(*slot, *pos);

            if self.match_gestures(true) {
                self.repeated_gesture = true;
            }
        }

        // Remove slots that are no longer active from `start_state` and store their last known positions in `last_known_state`
        let inactive_slots = self.start_state
            .extract_if(|slot, _| !state.contains_key(slot))
            .map(|(slot, _)| slot)
            .collect::<Vec<_>>();
        for slot in inactive_slots {
            self.last_known_state.insert(slot, *self.previous_state.get(&slot).unwrap());

            if let Some(PerformedSequenceStep::TouchUp { slots }) = self.performed_sequence.last_mut() {
                slots.insert(slot);
            } else {
                self.performed_sequence.push(PerformedSequenceStep::TouchUp { slots: HashSet::from([slot]) });
                // Set start positions for all slots to prevent issues with multi-finger gestures
                for (slot, pos) in state {
                    self.start_state.insert(*slot, *pos);
                }
            }
        }

        for (slot, pos) in state.clone().into_iter() {
            let start_pos = match self.start_state.get(&slot) {
                Some(pos) => *pos,
                _ => continue,
            };

            // TODO: ensure that y has not changed significantly to avoid diagonal moves
            // TODO: or maybe allow diagonal moves?

            let delta_x = pos.x as i32 - start_pos.x as i32;
            let delta_y = pos.y as i32 - start_pos.y as i32;

            let direction = if delta_x >= self.move_threshold_units.x as i32 {
                Direction::Right
            } else if delta_x <= -(self.move_threshold_units.x as i32) {
                Direction::Left
            } else if delta_y >= self.move_threshold_units.y as i32 {
                Direction::Down
            } else if delta_y <= -(self.move_threshold_units.y as i32) {
                Direction::Up
            } else {
                continue;
            };

            if !self.update_last_step(slot, &direction) {
                self.performed_sequence.push(PerformedSequenceStep::Move { slots: HashSet::from([slot]), direction });
            }

            match direction {
                Direction::Left | Direction::Right => self.start_state.get_mut(&slot).map(|p| p.x = pos.x),
                Direction::Up | Direction::Down => self.start_state.get_mut(&slot).map(|p| p.y = pos.y),
            };
        }

        self.previous_state = state.clone();
    }

    fn match_gestures(&mut self, repeating: bool) -> bool {
        // Remove all leading and trailing touch up and down steps
        while matches!(self.performed_sequence.first(), Some(PerformedSequenceStep::TouchDown { .. }) | Some(PerformedSequenceStep::TouchUp { .. })) {
            self.performed_sequence.remove(0);
        }
        while matches!(self.performed_sequence.last(), Some(PerformedSequenceStep::TouchUp { .. }) | Some(PerformedSequenceStep::TouchDown { .. })) {
            self.performed_sequence.pop();
        }

        if !self.performed_sequence.is_empty() {
            println!("Performed sequence: {:?}", self.performed_sequence);
        }

        let active_window_class = &self.active_window.lock().unwrap().class;
        let app_gestures = self.config.application_gestures.get(active_window_class)
            .map(|g| g.iter())
            .into_iter()
            .flatten();
        let matching_gestures = self.config.gestures
            .iter()
            .chain(app_gestures)
            .filter(|g| self.does_gesture_match(g, repeating))
            .cloned()
            .collect::<Vec<_>>();

        if !matching_gestures.is_empty() {
            let names = matching_gestures.iter().map(|g| &g.name).collect::<Vec<_>>();
            println!("Matched gestures: {:?}", names);

            for gesture in &matching_gestures {
                self.run_command(&gesture.command);
            }

            return true;
        }

        false
    }

    fn does_gesture_match(&self, gesture: &Gesture, repeating: bool) -> bool {
        if repeating && !gesture.repeatable
            || gesture.sequence.len() != self.performed_sequence.len() {
            return false;
        }

        for (i, defined_step) in gesture.sequence.iter().enumerate() {
            let performed_step = &self.performed_sequence[i];
            match (defined_step, performed_step) {
                (DefinedSequenceStep::Move { fingers, direction }, PerformedSequenceStep::Move { slots, direction: dir }) => {
                    if *fingers as usize != slots.len() || direction != dir {
                        return false;
                    }
                }
                (DefinedSequenceStep::TouchUp { fingers }, PerformedSequenceStep::TouchUp { slots }) => {
                    if *fingers as usize != slots.len() {
                        return false;
                    }
                }
                _ => return false,
            }
        }

        true
    }

    fn run_command(&self, command: &str) {
        if let Err(e) = std::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            eprintln!("Failed to execute command '{}': {}", command, e);
        }
    }
}
