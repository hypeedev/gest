// TODO: fix issue with reassigning slots when fingers are lifted and new ones are added

use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use crate::config::{Config, Direction, Edge, Gesture, RepeatMode};
use crate::Window;
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

#[derive(Default, Debug, Clone)]
pub struct State {
    pub positions: HashMap<u8, Position>,
}

impl State {
    pub fn centroid(&self) -> Option<Position> {
        if self.positions.is_empty() {
            return None;
        }

        let (sum_x, sum_y) = self.positions.values().fold((0u32, 0u32), |(acc_x, acc_y), pos| {
            (acc_x + pos.x as u32, acc_y + pos.y as u32)
        });

        let count = self.positions.len() as u32;
        Some(Position {
            x: (sum_x / count) as u16,
            y: (sum_y / count) as u16,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MoveThresholdUnits {
    pub x: u16,
    pub y: u16,
}

#[derive(Debug)]
pub struct GesturesEngine {
    pub config: Config,
    /// positions of fingers in the previous update
    previous_state: State,
    /// initial positions when fingers touch down
    touch_down_state: State,
    /// positions at the start of the current sequence step
    sequence_step_start_state: State,
    performed_sequence: Vec<PerformedSequenceStep>,
    repeat_mode: RepeatMode,
    move_threshold_units: MoveThresholdUnits,
    touchpad_size: MoveThresholdUnits,
    active_window: Arc<Mutex<Window>>,
    previous_direction: Direction,
    starting_edge: Option<Edge>,
    gesture_in_progress: bool,
    state_directions: HashMap<u8, Direction>,
}

impl GesturesEngine {
    pub fn new(config: Config, active_window: Arc<Mutex<Window>>, move_threshold_units: MoveThresholdUnits, touchpad_size: MoveThresholdUnits) -> Self {
        Self {
            config,
            previous_state: State::default(),
            touch_down_state: State::default(),
            sequence_step_start_state: State::default(),
            performed_sequence: Vec::new(),
            repeat_mode: RepeatMode::None,
            move_threshold_units,
            touchpad_size,
            active_window,
            previous_direction: Direction::None,
            starting_edge: None,
            gesture_in_progress: false,
            state_directions: HashMap::new(),
        }
    }

    fn at_edge(&self, pos: &Position) -> Option<Edge> {
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

    fn handle_lift_and_cleanup(&mut self) {
        if self.repeat_mode == RepeatMode::None {
            self.match_gestures(RepeatMode::None);
        } else {
            self.repeat_mode = RepeatMode::None;
        }

        self.previous_state.positions.clear();
        self.touch_down_state.positions.clear();
        self.sequence_step_start_state.positions.clear();
        self.performed_sequence.clear();
        self.previous_direction = Direction::None;
        self.starting_edge = None;
        self.gesture_in_progress = false;
        self.state_directions.clear();
    }

    pub fn update_state(&mut self, state: State) {
        if state.positions.is_empty() {
            self.handle_lift_and_cleanup();
            return;
        }

        for (slot, pos) in &state.positions {
            self.touch_down_state.positions.entry(*slot).or_insert(*pos);
            self.sequence_step_start_state.positions.entry(*slot).or_insert(*pos);

            if !self.gesture_in_progress && let Some(edge) = self.at_edge(pos) {
                self.starting_edge = Some(edge);
            }
        }

        self.gesture_in_progress = true;

        let lifted_slots = self.touch_down_state.positions
            .extract_if(|slot, _| !state.positions.contains_key(slot))
            .map(|(slot, _)| slot)
            .collect::<Vec<_>>();
        for slot in lifted_slots {
            if let Some(PerformedSequenceStep::TouchUp { slots }) = self.performed_sequence.last_mut() {
                slots.insert(slot);
            } else if self.repeat_mode == RepeatMode::None {
                self.performed_sequence.push(PerformedSequenceStep::TouchUp { slots: HashSet::from([slot]) });
                // Reset start positions for all slots
                for (slot, pos) in &state.positions {
                    self.touch_down_state.positions.insert(*slot, *pos);
                }
            }
        }

        if let Some(centroid) = state.centroid() {
            let touch_down_centroid = self.touch_down_state.centroid().unwrap();

            let edge = self.at_edge(&touch_down_centroid);

            let direction = self.point_side_in_ellipse(&centroid, &touch_down_centroid);
            if direction != self.previous_direction {
                // New sequence step, reset start positions
                for (slot, pos) in &state.positions {
                    self.sequence_step_start_state.positions.insert(*slot, *pos);
                }

                if let Some(PerformedSequenceStep::Move { direction: dir, .. }) = self.performed_sequence.last_mut()
                    && *dir != direction
                    && edge.is_some()
                {
                    *dir = direction;
                }
            }
            self.previous_direction = direction;

            if self.point_outside_of_ellipse(&centroid, &touch_down_centroid, edge.is_some()) {
                for slot in state.positions.keys() {
                    self.state_directions.insert(*slot, direction);
                }

                let distance = centroid.distance(&self.sequence_step_start_state.centroid().unwrap());
                let (distance, size) = match direction {
                    Direction::Up | Direction::Down => (distance.y, self.touchpad_size.y),
                    Direction::Left | Direction::Right => (distance.x, self.touchpad_size.x),
                    Direction::None => return,
                };

                let slots = state.positions.keys().cloned().collect::<HashSet<u8>>();

                let norm = distance as f32 / size as f32;
                if let Some(PerformedSequenceStep::Move { slots: s, direction: dir, distance: dst }) = self.performed_sequence.last_mut()
                    && *dir == direction
                {
                    *s = slots;
                    *dst = norm;
                } else {
                    self.performed_sequence.push(PerformedSequenceStep::Move { slots, direction, distance: norm });
                }

                for (&slot, pos) in &state.positions {
                    if let Some(p) = self.touch_down_state.positions.get_mut(&slot) {
                        p.x = pos.x;
                        p.y = pos.y;
                    }
                }

                self.match_gestures(RepeatMode::Slide);
            }
        }

        // Update last move step distances
        for (&slot, pos) in &state.positions {
            if let Some((sequence_step_start_position, direction)) = self.sequence_step_start_state.positions.get(&slot).zip(self.state_directions.get(&slot)) {
                let distance = pos.distance(sequence_step_start_position);
                let (distance, size) = match direction {
                    Direction::Up | Direction::Down => (distance.y, self.touchpad_size.y),
                    Direction::Left | Direction::Right => (distance.x, self.touchpad_size.x),
                    Direction::None => continue,
                };

                match self.performed_sequence.last_mut() {
                    Some(PerformedSequenceStep::Move { slots, direction: dir, distance: dst }) if dir == direction => {
                        slots.insert(slot);
                        *dst = dst.max(distance as f32 / size as f32);
                    }
                    _ => {}
                }
            }
        }

        if state.positions.len() > self.previous_state.positions.len() && !self.performed_sequence.is_empty() {
            let new_slot = *state.positions.keys().find(|k| !self.previous_state.positions.contains_key(k)).unwrap();
            if let Some(PerformedSequenceStep::TouchDown { slots }) = self.performed_sequence.last_mut() {
                slots.insert(new_slot);
            } else {
                self.performed_sequence.push(PerformedSequenceStep::TouchDown { slots: HashSet::from([new_slot]) });
            }

            // Check for repeated gestures
            if matches!(&self.performed_sequence.last(), Some(PerformedSequenceStep::TouchDown { .. })) {
                self.match_gestures(RepeatMode::Tap);
            }
        }

        self.previous_state = state;
    }

    pub fn point_outside_of_ellipse(&self, point: &Position, center: &Position, is_edge: bool) -> bool {
        let sensitivity = if is_edge { 1.0 - self.config.options.edge.sensitivity } else { 1.0 };
        let nx = (point.x as f64 - center.x as f64) / (self.move_threshold_units.x as f64 * sensitivity as f64);
        let ny = (point.y as f64 - center.y as f64) / (self.move_threshold_units.y as f64 * sensitivity as f64);
        let v = nx * nx + ny * ny;
        v > 1.0
    }

    pub fn point_side_in_ellipse(&self, point: &Position, center: &Position) -> Direction {
        let dx = point.x as f64 - center.x as f64;
        let dy = point.y as f64 - center.y as f64;

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

        if !self.performed_sequence.is_empty() {
            if let Some(edge) = self.starting_edge {
                log::debug!("Performed sequence from edge {:?}: {:?}", edge, self.performed_sequence);
            } else {
                log::debug!("Performed sequence: {:?}", self.performed_sequence);
            }
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
                log::debug!("Matched gestures: {:?}", names);

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

                log::debug!("Matched gesture: {:?}", matched_gesture.name);

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
            || !gesture.repeat_mode.contains(RepeatMode::Slide) && *repeat_mode == RepeatMode::Slide
            || gesture.edge != self.starting_edge
        {
            return false;
        }

        gesture.sequence == self.performed_sequence
    }

    fn run_command(&self, command: &str) {
        if let Err(e) = std::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            log::error!("Failed to execute command '{}': {}", command, e);
        }
    }
}
