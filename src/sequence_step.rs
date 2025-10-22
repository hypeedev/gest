use std::fmt::{Formatter, Debug};
use std::collections::HashSet;
use crate::config::{Direction, Edge};

pub enum PerformedSequenceStep {
    Move { slots: HashSet<u8>, direction: Direction },
    TouchUp { slots: HashSet<u8> },
    TouchDown { slots: HashSet<u8> },
    MoveEdge { slots: HashSet<u8>, edge: Edge, direction: Direction },
}

impl Debug for PerformedSequenceStep {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TouchDown { slots } => write!(f, "TouchDown({})", slots.len()),
            Self::TouchUp { slots } => write!(f, "TouchUp({})", slots.len()),
            Self::Move { slots, direction } => write!(f, "Move{:?}({})", direction, slots.len()),
            Self::MoveEdge { slots, edge, direction } => write!(f, "MoveEdge{:?}-{:?}({})", edge, direction, slots.len()),
        }
    }
}

#[derive(Debug, Clone)]
pub enum DefinedSequenceStep {
    TouchDown { fingers: u8 },
    TouchUp { fingers: u8 },
    Move { fingers: u8, direction: Direction },
    MoveEdge { fingers: u8, edge: Edge, direction: Direction },
}

impl<'de> serde::Deserialize<'de> for DefinedSequenceStep {
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
            "touch_down" | "touch down" => DefinedSequenceStep::TouchDown { fingers },
            "touch_up" | "touch up" => DefinedSequenceStep::TouchUp { fingers },
            "move_up" | "move up" => {
                if let Some(edge) = edge {
                    DefinedSequenceStep::MoveEdge { fingers, edge, direction: Direction::Up }
                } else {
                    DefinedSequenceStep::Move { fingers, direction: Direction::Up }
                }
            },
            "move_down" | "move down" => {
                if let Some(edge) = edge {
                    DefinedSequenceStep::MoveEdge { fingers, edge, direction: Direction::Down }
                } else {
                    DefinedSequenceStep::Move { fingers, direction: Direction::Down }
                }
            },
            "move_left" | "move left" => {
                if let Some(edge) = edge {
                    DefinedSequenceStep::MoveEdge { fingers, edge, direction: Direction::Left }
                } else {
                    DefinedSequenceStep::Move { fingers, direction: Direction::Left }
                }
            },
            "move_right" | "move right" => {
                if let Some(edge) = edge {
                    DefinedSequenceStep::MoveEdge { fingers, edge, direction: Direction::Right }
                } else {
                    DefinedSequenceStep::Move { fingers, direction: Direction::Right }
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
            (DefinedSequenceStep::MoveEdge { fingers, edge, direction }, PerformedSequenceStep::MoveEdge { slots, edge: e, direction: dir }) => {
                if *fingers as usize != slots.len() || edge != e || direction != dir {
                    return false;
                }
            }
            _ => return false,
        }
        true
    }
}
