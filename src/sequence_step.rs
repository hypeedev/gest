use std::fmt::{Formatter, Debug};
use std::collections::{HashMap, HashSet};
use crate::config::{Direction, Edge};

#[derive(Debug, Clone)]
pub enum Distance {
    Variable(String),
    Fixed(f32),
}

pub enum PerformedSequenceStep {
    Move { slots: HashSet<u8>, direction: Direction, distance: f32 },
    TouchUp { slots: HashSet<u8> },
    TouchDown { slots: HashSet<u8> },
    MoveEdge { slots: HashSet<u8>, edge: Edge, direction: Direction, distance: f32 },
}

impl Debug for PerformedSequenceStep {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TouchDown { slots } => write!(f, "TouchDown({})", slots.len()),
            Self::TouchUp { slots } => write!(f, "TouchUp({})", slots.len()),
            Self::Move { slots, direction, distance } => write!(f, "Move{:?}({}, {})", direction, slots.len(), distance),
            Self::MoveEdge { slots, edge, direction, distance } => write!(f, "MoveEdge{:?}-{:?}({}, {})", edge, direction, slots.len(), distance),
        }
    }
}

#[derive(Debug, Clone)]
pub enum DefinedSequenceStep {
    TouchDown { fingers: u8 },
    TouchUp { fingers: u8 },
    Move { fingers: u8, direction: Direction, distance: Option<f32> },
    MoveEdge { fingers: u8, edge: Edge, direction: Direction, distance: Option<f32> },
}

#[derive(Debug, Clone)]
pub enum DefinedSequenceStepRaw {
    TouchDown { fingers: u8 },
    TouchUp { fingers: u8 },
    Move { fingers: u8, direction: Direction, distance: Option<Distance> },
    MoveEdge { fingers: u8, edge: Edge, direction: Direction, distance: Option<Distance> },
}

impl DefinedSequenceStep {
    pub fn from_raw(raw: DefinedSequenceStepRaw, distances: &HashMap<String, f32>) -> Self {
        match raw {
            DefinedSequenceStepRaw::TouchDown { fingers } => DefinedSequenceStep::TouchDown { fingers },
            DefinedSequenceStepRaw::TouchUp { fingers } => DefinedSequenceStep::TouchUp { fingers },
            DefinedSequenceStepRaw::Move { fingers, direction, distance } => {
                let distance = match distance {
                    Some(Distance::Variable(name)) => {
                        match distances.get(&name) {
                            Some(d) => Some(*d),
                            None => {
                                log::error!("Unknown distance: \"{}\"", name);
                                None
                            }
                        }
                    }
                    Some(Distance::Fixed(d)) => Some(d),
                    _ => None,
                };
                DefinedSequenceStep::Move { fingers, direction, distance }
            }
            DefinedSequenceStepRaw::MoveEdge { fingers, edge, direction, distance } => {
                let distance = match distance {
                    Some(Distance::Variable(name)) => {
                        match distances.get(&name) {
                            Some(d) => Some(*d),
                            None => {
                                log::error!("Unknown distance: \"{}\"", name);
                                None
                            }
                        }
                    }
                    Some(Distance::Fixed(d)) => Some(d),
                    _ => None,
                };
                DefinedSequenceStep::MoveEdge { fingers, edge, direction, distance }
            }
        }
    }
}

impl<'de> serde::Deserialize<'de> for DefinedSequenceStepRaw {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>
    {
        let map = serde_yaml::Value::deserialize(deserializer)?;
        let fingers = map.get("fingers")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| serde::de::Error::custom("Missing or invalid 'fingers' field"))? as u8;
        let action = map.get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| serde::de::Error::custom("Missing or invalid 'action' field"))?;
        let edge = map.get("edge")
            .and_then(|v| v.as_str());
        let distance = map.get("distance")
            .and_then(|v| {
                v.as_str().map(|s| Distance::Variable(s.to_string()))
                    .or_else(|| v.as_f64().map(|f| Distance::Fixed(f as f32)))
            });

        if let Some(Distance::Fixed(d)) = distance
            && !(0f32..=1f32).contains(&d)
        {
            return Err(serde::de::Error::custom(format!("Distance must be between 0 and 1, got {}", d)));
        }

        let edge = if let Some(edge_str) = edge {
            match edge_str {
                "top" => Some(Edge::Top),
                "bottom" => Some(Edge::Bottom),
                "left" => Some(Edge::Left),
                "right" => Some(Edge::Right),
                _ => return Err(serde::de::Error::custom(format!("Unknown edge: {}", edge_str))),
            }
        } else {
            None
        };

        let step = match action {
            "touch_down" | "touch down" => DefinedSequenceStepRaw::TouchDown { fingers },
            "touch_up" | "touch up" => DefinedSequenceStepRaw::TouchUp { fingers },
            "move_up" | "move up" => {
                if let Some(edge) = edge {
                    DefinedSequenceStepRaw::MoveEdge { fingers, edge, direction: Direction::Up, distance }
                } else {
                    DefinedSequenceStepRaw::Move { fingers, direction: Direction::Up, distance }
                }
            },
            "move_down" | "move down" => {
                if let Some(edge) = edge {
                    DefinedSequenceStepRaw::MoveEdge { fingers, edge, direction: Direction::Down, distance }
                } else {
                    DefinedSequenceStepRaw::Move { fingers, direction: Direction::Down, distance }
                }
            },
            "move_left" | "move left" => {
                if let Some(edge) = edge {
                    DefinedSequenceStepRaw::MoveEdge { fingers, edge, direction: Direction::Left, distance }
                } else {
                    DefinedSequenceStepRaw::Move { fingers, direction: Direction::Left, distance }
                }
            },
            "move_right" | "move right" => {
                if let Some(edge) = edge {
                    DefinedSequenceStepRaw::MoveEdge { fingers, edge, direction: Direction::Right, distance }
                } else {
                    DefinedSequenceStepRaw::Move { fingers, direction: Direction::Right, distance }
                }
            },
            _ => return Err(serde::de::Error::custom(format!("Unknown action: {}", action))),
        };

        Ok(step)
    }
}

impl PartialEq<PerformedSequenceStep> for DefinedSequenceStep {
    fn eq(&self, other: &PerformedSequenceStep) -> bool {
        match (self, other) {
            (DefinedSequenceStep::Move { fingers, direction, distance }, PerformedSequenceStep::Move { slots, direction: dir, distance: dst }) => {
                if *fingers as usize != slots.len() || direction != dir {
                    return false;
                }

                if let Some(d) = distance
                    && dst < d
                {
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
            (DefinedSequenceStep::MoveEdge { fingers, edge, direction, distance }, PerformedSequenceStep::MoveEdge { slots, edge: e, direction: dir, distance: dst }) => {
                if *fingers as usize != slots.len() || edge != e || direction != dir {
                    return false;
                }

                if let Some(d) = distance
                    && dst < d
                {
                    return false;
                }
            }
            _ => return false,
        }
        true
    }
}
