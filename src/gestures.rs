use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use crate::config::{Config, Direction, Edge, Gesture, RepeatMode};
use crate::Window;
use crate::args::Args;
use crate::sequence_step::{DefinedSequenceStep, PerformedSequenceStep};

#[derive(Debug, Clone, Copy)]
pub struct Position {
    pub x: u16,
    pub y: u16,
}

impl Position {
    pub fn distance(&self, other: &Position) -> Position {
        Position {
            x: self.x.abs_diff(other.x),
            y: self.y.abs_diff(other.y),
        }
    }
}

pub type State = HashMap<u8, Position>;

#[derive(Debug, Clone, Copy)]
pub struct MoveThresholdUnits {
    pub x: u16,
    pub y: u16,
}

#[derive(Debug)]
pub struct GesturesManager<'a> {
    pub config: Config,
    /// positions of fingers in the previous update
    previous_state: State,
    /// initial positions when fingers touch down
    touch_down_state: State,
    /// initial positions when fingers touch down for the very first time
    initial_touch_down_state: State,
    performed_sequence: Vec<PerformedSequenceStep>,
    repeat_mode: RepeatMode,
    move_threshold_units: MoveThresholdUnits,
    touchpad_size: MoveThresholdUnits,
    active_window: Arc<Mutex<Window>>,
    args: &'a Args,
    slots_outside_ellipse: HashSet<u8>,
    direction: Direction,
}

impl<'a> GesturesManager<'a> {
    pub fn new(config: Config, active_window: Arc<Mutex<Window>>, move_threshold_units: MoveThresholdUnits, touchpad_size: MoveThresholdUnits, args: &'a Args) -> Self {
        Self {
            config,
            previous_state: HashMap::new(),
            touch_down_state: HashMap::new(),
            initial_touch_down_state: HashMap::new(),
            performed_sequence: Vec::new(),
            repeat_mode: RepeatMode::None,
            move_threshold_units,
            touchpad_size,
            active_window,
            args,
            slots_outside_ellipse: HashSet::new(),
            direction: Direction::None,
        }
    }

    fn update_last_step(&mut self, slot: u8, direction: &Direction, distance: f32) -> bool {
        match self.performed_sequence.last_mut() {
            Some(PerformedSequenceStep::Move { slots, direction: dir, distance: dst }) if dir == direction => {
                slots.insert(slot);
                let size = match direction {
                    Direction::Up | Direction::Down => self.touchpad_size.y,
                    Direction::Left | Direction::Right => self.touchpad_size.x,
                    Direction::None => return false,
                };
                *dst = distance / size as f32;
                true
            }
            _ => false
        }
    }

    fn is_at_edge(&self, pos: &Position) -> Option<Edge> {
        let edge_threshold_x = (self.touchpad_size.x as f32 * self.config.options.edge.threshold) as u16;
        let edge_threshold_y = (self.touchpad_size.y as f32 * self.config.options.edge.threshold) as u16;
        if pos.x <= edge_threshold_x {
            Some(Edge::Left)
        } else if pos.x >= self.touchpad_size.x - edge_threshold_x {
            Some(Edge::Right)
        } else if pos.y <= edge_threshold_y {
            Some(Edge::Top)
        } else if pos.y >= self.touchpad_size.y - edge_threshold_y {
            Some(Edge::Bottom)
        } else {
            None
        }
    }

    pub fn update_state(&mut self, state: State) {
        // All fingers lifted
        if state.is_empty() {
            if self.repeat_mode == RepeatMode::None {
                self.match_gestures(RepeatMode::None);
            } else {
                self.repeat_mode = RepeatMode::None;
            }

            self.previous_state.clear();
            self.touch_down_state.clear();
            self.initial_touch_down_state.clear();
            self.performed_sequence.clear();
            self.slots_outside_ellipse.clear();
            return;
        }

        let mut slots_at_edge = HashSet::new();
        let mut slots_edge = Edge::None;

        for (slot, pos) in &state {
            if !self.touch_down_state.contains_key(slot) {
                self.touch_down_state.insert(*slot, *pos);
            }

            if !self.initial_touch_down_state.contains_key(slot) {
                self.initial_touch_down_state.insert(*slot, *pos);

                if let Some(edge) = self.is_at_edge(pos) {
                    if let Some(PerformedSequenceStep::MoveEdge { slots, .. }) = self.performed_sequence.last_mut() {
                        slots.insert(*slot);
                    } else {
                        slots_at_edge.insert(*slot);
                        slots_edge = edge;
                    }
                }
            }
        }

        // Perform edge move if all slots are at the edge
        if slots_at_edge.len() == state.len() {
            self.performed_sequence.push(PerformedSequenceStep::MoveEdge { slots: slots_at_edge, edge: slots_edge, direction: Direction::None });
        }

        let lifted_slots = self.touch_down_state
            .extract_if(|slot, _| !state.contains_key(slot))
            .map(|(slot, _)| slot)
            .collect::<Vec<_>>();
        for slot in lifted_slots {
            if let Some(PerformedSequenceStep::TouchUp { slots }) = self.performed_sequence.last_mut() {
                slots.insert(slot);
            } else {
                self.performed_sequence.push(PerformedSequenceStep::TouchUp { slots: HashSet::from([slot]) });
                // Reset start positions for all slots
                for (slot, pos) in &state {
                    self.touch_down_state.insert(*slot, *pos);
                }

                self.slots_outside_ellipse.remove(&slot);
            }
        }

        if matches!(self.performed_sequence.last(), Some(PerformedSequenceStep::MoveEdge { .. })) {
            for (slot, pos) in &state {
                let start_pos = match self.touch_down_state.get(slot) {
                    Some(pos) => *pos,
                    _ => continue,
                };

                if self.point_outside_of_ellipse(pos.x as f64, pos.y as f64, start_pos.x as f64, start_pos.y as f64, true) {
                    let direction = self.point_side_in_ellipse(pos.x as f64, pos.y as f64, start_pos.x as f64, start_pos.y as f64);
                    if let Some(PerformedSequenceStep::MoveEdge { direction: dir, .. }) = self.performed_sequence.last_mut() {
                        *dir = direction;
                    }

                    self.match_gestures(RepeatMode::Slide);

                    // Reset start positions for all slots
                    for (slot, pos) in &state {
                        self.touch_down_state.insert(*slot, *pos);
                    }
                }
            }
        }

        for (&slot, pos) in &state {
            let start_pos = match self.touch_down_state.get(&slot) {
                Some(pos) => *pos,
                _ => continue,
            };

            if self.point_outside_of_ellipse(pos.x as f64, pos.y as f64, start_pos.x as f64, start_pos.y as f64, false) {
                let direction = self.point_side_in_ellipse(pos.x as f64, pos.y as f64, start_pos.x as f64, start_pos.y as f64);
                self.direction = direction;

                let distance = if let Some(initial_touch_down_position) = self.initial_touch_down_state.get(&slot) {
                    let distance = pos.distance(initial_touch_down_position);
                    match direction {
                        Direction::Up | Direction::Down => distance.y,
                        Direction::Left | Direction::Right => distance.x,
                        Direction::None => 0,
                    }
                } else {
                    0
                };

                if !self.update_last_step(slot, &direction, distance as f32) {
                    let size = match direction {
                        Direction::Up | Direction::Down => self.touchpad_size.y,
                        Direction::Left | Direction::Right => self.touchpad_size.x,
                        Direction::None => continue,
                    };
                    let distance = distance as f32 / size as f32;
                    self.performed_sequence.push(PerformedSequenceStep::Move { slots: HashSet::from([slot]), direction, distance });
                }

                if let Some(p) = self.touch_down_state.get_mut(&slot) {
                    p.x = pos.x;
                    p.y = pos.y;
                }

                self.slots_outside_ellipse.insert(slot);
            }

            if let Some(initial_touch_down_position) = self.initial_touch_down_state.get(&slot) {
                let distance = pos.distance(initial_touch_down_position);
                let distance = match self.direction {
                    Direction::Up | Direction::Down => distance.y,
                    Direction::Left | Direction::Right => distance.x,
                    Direction::None => continue,
                };
                let direction = self.direction;
                self.update_last_step(slot, &direction, distance as f32);
            }
        }

        if self.slots_outside_ellipse.len() == state.len()
            && self.slots_outside_ellipse.iter().all(|slot| state.contains_key(slot))
        {
            self.slots_outside_ellipse.clear();
            self.match_gestures(RepeatMode::Slide);
        }

        if state.len() > self.previous_state.len() && !self.performed_sequence.is_empty() {
            let is_edge = matches!(self.performed_sequence.last(), Some(PerformedSequenceStep::MoveEdge { .. }));

            let new_slot = *state.keys().find(|k| !self.previous_state.contains_key(k)).unwrap();
            if let Some(PerformedSequenceStep::TouchDown { slots }) = self.performed_sequence.last_mut() {
                slots.insert(new_slot);
            } else if !is_edge {
                self.performed_sequence.push(PerformedSequenceStep::TouchDown { slots: HashSet::from([new_slot]) });
            }

            // Check for repeated gestures
            if is_edge || matches!(&self.performed_sequence[self.performed_sequence.len() - 2..], [PerformedSequenceStep::TouchUp { .. }, PerformedSequenceStep::TouchDown { .. }]) {
                self.match_gestures(RepeatMode::Tap);
            }
        }

        self.previous_state = state;
    }

    pub fn point_outside_of_ellipse(&self, x: f64, y: f64, h: f64, k: f64, is_edge: bool) -> bool {
        let sensitivity = if is_edge { 1.0 - self.config.options.edge.sensitivity } else { 1.0 };
        let nx = (x - h) / (self.move_threshold_units.x as f64 * sensitivity as f64);
        let ny = (y - k) / (self.move_threshold_units.y as f64 * sensitivity as f64);
        let v = nx * nx + ny * ny;
        v > 1.0
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

    fn match_gestures(&mut self, repeat_mode: RepeatMode) -> bool {
        // Temporarily remove all trailing touch up and down steps for matching
        let trailing_count = self.performed_sequence.iter()
            .rev()
            .take_while(|step| matches!(step, PerformedSequenceStep::TouchDown { .. } | PerformedSequenceStep::TouchUp { .. }))
            .count();
        let trailing_steps = self.performed_sequence.split_off(self.performed_sequence.len() - trailing_count);

        if !self.performed_sequence.is_empty() && self.args.verbose {
            println!("Performed sequence: {:?}", self.performed_sequence);
        }

        let active_window = &self.active_window.lock().unwrap();

        let mut app_gestures_by_class = Vec::new();
        for (regex, gestures) in &self.config.application_gestures.by_class {
            if regex.is_match(&active_window.class) {
                app_gestures_by_class.extend(gestures);
            }
        }

        let mut app_gestures_by_title = Vec::new();
        for (regex, gestures) in &self.config.application_gestures.by_title {
            if regex.is_match(&active_window.title) {
                app_gestures_by_title.extend(gestures);
            }
        }

        let app_gestures = app_gestures_by_class
            .into_iter()
            .chain(app_gestures_by_title);
        let matching_gestures = self.config.gestures
            .iter()
            .chain(app_gestures)
            .filter(|g| self.does_gesture_match(g, &repeat_mode))
            .cloned()
            .collect::<Vec<_>>();

        if !matching_gestures.is_empty() {
            if self.config.options.run_all_matches {
                let names = matching_gestures.iter().map(|g| &g.name).collect::<Vec<_>>();
                if self.args.verbose {
                    println!("Matched gestures: {:?}", names);
                }

                for gesture in &matching_gestures {
                    self.run_command(&gesture.command);
                }
            } else {
                let mut matched_gesture = &matching_gestures[0];
                let mut distance = 0.0f32;

                for gesture in matching_gestures.iter().skip(1) {
                    for step in &gesture.sequence {
                        if let DefinedSequenceStep::Move { distance: dst, .. } = step
                            && let Some(dst) = dst
                            && *dst > distance
                        {
                            distance = *dst;
                            matched_gesture = gesture;
                        }
                    }
                }

                if self.args.verbose {
                    println!("Matched gesture: {:?}", matched_gesture.name);
                }

                self.run_command(&matched_gesture.command);
            }

            self.repeat_mode = repeat_mode;
            return true;
        }

        self.performed_sequence.extend(trailing_steps);

        false
    }

    fn does_gesture_match(&self, gesture: &Gesture, repeat_mode: &RepeatMode) -> bool {
        if gesture.sequence.len() != self.performed_sequence.len()
            || gesture.repeat_mode != RepeatMode::Slide && *repeat_mode == RepeatMode::Slide
        {
            return false;
        }

        for (i, defined_step) in gesture.sequence.iter().enumerate() {
            let performed_step = &self.performed_sequence[i];
            if defined_step != performed_step {
                return false;
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
