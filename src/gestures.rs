use std::collections::HashMap;
use std::process::Stdio;
use crate::config::{Config, Direction, Step};

#[derive(Debug, Clone, Copy)]
pub struct Position {
    pub x: u16,
    pub y: u16,
}

pub type State = HashMap<u8, Position>;

#[derive(Debug)]
enum SequenceStep {
    Move { slots: Vec<u8>, direction: Direction },
    TouchUp { slots: Vec<u8> },
    TouchDown { slots: Vec<u8> },
}

#[derive(Debug, Clone, Copy)]
pub struct MoveThresholdUnits {
    pub x: u16,
    pub y: u16,
}

#[derive(Debug)]
pub struct GesturesManager {
    config: Config,
    previous_state: State, // positions of fingers in the previous update
    start_state: State, // initial positions when fingers touch down
    last_known_state: State, // positions of fingers just before they were lifted
    current_sequence: Vec<SequenceStep>,
    repeated_gesture: bool,
    move_threshold_units: MoveThresholdUnits,
}

impl GesturesManager {
    pub fn new(config: Config, move_threshold_units: MoveThresholdUnits) -> Self {
        Self {
            config,
            previous_state: HashMap::new(),
            start_state: HashMap::new(),
            last_known_state: HashMap::new(),
            current_sequence: Vec::new(),
            repeated_gesture: false,
            move_threshold_units,
        }
    }

    fn update_last_step(&mut self, slot: u8, direction: &Direction) -> bool {
        if let Some(SequenceStep::Move { slots, direction: dir }) = self.current_sequence.last_mut()
            && dir == direction
        {
                if slots.contains(&slot) {
                    return true;
                }
                slots.push(slot);
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
            self.current_sequence.clear();
            return;
        }

        for (slot, pos) in state {
            if self.start_state.contains_key(slot) { continue; }

            self.start_state.insert(*slot, *pos);

            self.current_sequence.push(SequenceStep::TouchDown { slots: vec![*slot] });

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

            if let Some(SequenceStep::TouchUp { slots }) = self.current_sequence.last_mut() {
                if slots.contains(&slot) { continue; }
                slots.push(slot);
            } else {
                self.current_sequence.push(SequenceStep::TouchUp { slots: vec![slot] });
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
                self.current_sequence.push(SequenceStep::Move { slots: vec![slot], direction });
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
        while matches!(self.current_sequence.first(), Some(SequenceStep::TouchDown { .. }) | Some(SequenceStep::TouchUp { .. })) {
            self.current_sequence.remove(0);
        }
        while matches!(self.current_sequence.last(), Some(SequenceStep::TouchUp { .. }) | Some(SequenceStep::TouchDown { .. })) {
            self.current_sequence.pop();
        }

        if !self.current_sequence.is_empty() {
            self.pretty_print_sequence();
        }

        let mut matching_gestures = Vec::new();

        'gesture: for gesture in &self.config.gestures {
            if repeating && !gesture.repeatable
                || gesture.sequence.len() != self.current_sequence.len() {
                continue;
            }

            for (i, step) in gesture.sequence.iter().enumerate() {
                match (step, &self.current_sequence[i]) {
                    (Step::Move { fingers, direction }, SequenceStep::Move { slots, direction: dir }) => {
                        if *fingers as usize != slots.len() || direction != dir {
                            continue 'gesture;
                        }
                    }
                    (Step::TouchUp { fingers }, SequenceStep::TouchUp { slots }) => {
                        if *fingers as usize != slots.len() {
                            continue 'gesture;
                        }
                    }
                    _ => continue 'gesture,
                }
            }

            matching_gestures.push(gesture.clone());
        }

        if !matching_gestures.is_empty() {
            let matched_gestures = matching_gestures.iter().map(|g| &g.name).collect::<Vec<_>>();
            println!("Matched gestures: {:?}", matched_gestures);

            for gesture in &matching_gestures {
                if let Err(e) = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(&gesture.command)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                {
                    eprintln!("Failed to execute command '{}': {}", gesture.command, e);
                }
            }

            return true;
        }

        false
    }

    fn pretty_print_sequence(&self) {
        let steps = self.current_sequence.iter().map(|step| {
            match step {
                SequenceStep::TouchDown { slots } => format!("TouchDown({})", slots.len()),
                SequenceStep::TouchUp { slots } => format!("TouchUp({})", slots.len()),
                SequenceStep::Move { slots, direction } => {
                    let dir_str = match direction {
                        Direction::Up => "Up",
                        Direction::Down => "Down",
                        Direction::Left => "Left",
                        Direction::Right => "Right",
                    };
                    format!("Move{}({})", dir_str, slots.len())
                }
            }
        }).collect::<Vec<_>>().join(" -> ");
        println!("Current sequence: {}", steps);
    }
}
