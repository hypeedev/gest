use std::collections::{HashMap, HashSet};
use std::fmt::{Debug, Formatter};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use crate::config::{Config, Direction, DefinedSequenceStep, Gesture};
use crate::Window;
use crate::args::Args;

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
pub struct GesturesManager<'a> {
    pub config: Config,
    previous_state: State, // positions of fingers in the previous update
    start_state: State, // initial positions when fingers touch down
    performed_sequence: Vec<PerformedSequenceStep>,
    repeated_gesture: bool,
    move_threshold_units: MoveThresholdUnits,
    active_window: Arc<Mutex<Window>>,
    args: &'a Args,
}

impl<'a> GesturesManager<'a> {
    pub fn new(config: Config, active_window: Arc<Mutex<Window>>, move_threshold_units: MoveThresholdUnits, args: &'a Args) -> Self {
        Self {
            config,
            previous_state: HashMap::new(),
            start_state: HashMap::new(),
            performed_sequence: Vec::new(),
            repeated_gesture: false,
            move_threshold_units,
            active_window,
            args,
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

    pub fn update_state(&mut self, state: State) {
        // All fingers lifted
        if state.is_empty() {
            if !self.repeated_gesture {
                self.match_gestures();
            } else {
                self.repeated_gesture = false;
            }

            self.previous_state.clear();
            self.start_state.clear();
            self.performed_sequence.clear();
            return;
        }

        for (slot, pos) in &state {
            if self.start_state.contains_key(slot) { continue; }
            self.start_state.insert(*slot, *pos);
        }

        // Remove slots that are no longer active from `start_state`
        let inactive_slots = self.start_state
            .extract_if(|slot, _| !state.contains_key(slot))
            .map(|(slot, _)| slot)
            .collect::<Vec<_>>();
        for slot in inactive_slots {
            if let Some(PerformedSequenceStep::TouchUp { slots }) = self.performed_sequence.last_mut() {
                slots.insert(slot);
            } else {
                self.performed_sequence.push(PerformedSequenceStep::TouchUp { slots: HashSet::from([slot]) });
                // Reset start positions for all slots
                for (slot, pos) in &state {
                    self.start_state.insert(*slot, *pos);
                }
            }
        }

        for (slot, pos) in state.clone().into_iter() {
            let start_pos = match self.start_state.get(&slot) {
                Some(pos) => *pos,
                _ => continue,
            };

            if self.point_outside_of_ellipse(pos.x as f64, pos.y as f64, start_pos.x as f64, start_pos.y as f64, 0.1) {
                let direction = self.point_side_in_ellipse(pos.x as f64, pos.y as f64, start_pos.x as f64, start_pos.y as f64);

                if !self.update_last_step(slot, &direction) {
                    self.performed_sequence.push(PerformedSequenceStep::Move { slots: HashSet::from([slot]), direction });
                }

                if let Some(p) = self.start_state.get_mut(&slot) {
                    p.x = pos.x;
                    p.y = pos.y;
                }
            }
        }

        if state.len() > self.previous_state.len()
            && !self.performed_sequence.is_empty()
       {
            let new_slot = *state.keys().find(|k| !self.previous_state.contains_key(k)).unwrap();
            if let Some(PerformedSequenceStep::TouchDown { slots }) = self.performed_sequence.last_mut() {
                slots.insert(new_slot);
            } else {
                self.performed_sequence.push(PerformedSequenceStep::TouchDown { slots: HashSet::from([new_slot]) });
            }

            // Check for repeated gestures
            if matches!(&self.performed_sequence[self.performed_sequence.len() - 2..], [PerformedSequenceStep::TouchUp { .. }, PerformedSequenceStep::TouchDown { .. }]) {
                // TODO: Find a way to get rid of this abomination
                let repeated_gesture = self.repeated_gesture;
                self.repeated_gesture = true;
                let matched = self.match_gestures();
                if !matched {
                    self.repeated_gesture = repeated_gesture;
                }
            }
        }

        self.previous_state = state;
    }

    pub fn point_outside_of_ellipse(&self, x: f64, y: f64, h: f64, k: f64, eps: f64) -> bool {
        let nx = (x - h) / self.move_threshold_units.x as f64;
        let ny = (y - k) / self.move_threshold_units.y as f64;
        let v = nx * nx + ny * ny;

        let point_on_ellipse = (v - 1.0).abs() <= eps;
        let point_inside_ellipse = v < 1.0;
        !(point_on_ellipse || point_inside_ellipse)
    }

    pub fn point_side_in_ellipse(&self, x: f64, y: f64, h: f64, k: f64) -> Direction {
        let dx = x - h;
        let dy = y - k;

        let nx = dx / self.move_threshold_units.x as f64;
        let ny = dy / self.move_threshold_units.y as f64;

        if nx.abs() > ny.abs() {
            if dx >= 0.0 {
                Direction::Right
            } else {
                Direction::Left
            }
        } else if dy < 0.0 {
            Direction::Up
        } else {
            Direction::Down
        }
    }

    fn match_gestures(&mut self) -> bool {
        // Temporarily remove all trailing touch up and down steps for matching
        let trailing_count = self.performed_sequence.iter()
            .rev()
            .take_while(|step| matches!(step, PerformedSequenceStep::TouchDown { .. } | PerformedSequenceStep::TouchUp { .. }))
            .count();
        let trailing_steps = self.performed_sequence.split_off(self.performed_sequence.len() - trailing_count);

        if !self.performed_sequence.is_empty() && self.args.verbose {
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
            .filter(|g| self.does_gesture_match(g))
            .cloned()
            .collect::<Vec<_>>();

        if !matching_gestures.is_empty() {
            let names = matching_gestures.iter().map(|g| &g.name).collect::<Vec<_>>();
            if self.args.verbose {
                println!("Matched gestures: {:?}", names);
            }

            for gesture in &matching_gestures {
                self.run_command(&gesture.command);
            }

            return true;
        }

        self.performed_sequence.extend(trailing_steps);

        false
    }

    fn does_gesture_match(&self, gesture: &Gesture) -> bool {
        if self.repeated_gesture && !gesture.repeatable
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
                (DefinedSequenceStep::TouchDown { fingers }, PerformedSequenceStep::TouchDown { slots }) => {
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
