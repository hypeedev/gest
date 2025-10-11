use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone)]
pub enum DefinedSequenceStep {
    TouchDown { fingers: u8 },
    TouchUp { fingers: u8 },
    Move { fingers: u8, direction: Direction }
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

        let step = match action {
            "touch_down" | "touch down" => DefinedSequenceStep::TouchDown { fingers },
            "touch_up" | "touch up" => DefinedSequenceStep::TouchUp { fingers },
            "move_up" | "move up" => DefinedSequenceStep::Move { fingers, direction: Direction::Up },
            "move_down" | "move down" => DefinedSequenceStep::Move { fingers, direction: Direction::Down },
            "move_left" | "move left" => DefinedSequenceStep::Move { fingers, direction: Direction::Left },
            "move_right" | "move right" => DefinedSequenceStep::Move { fingers, direction: Direction::Right },
            _ => return Err(serde::de::Error::custom(format!("Unknown action: {}", action))),
        };

        Ok(step)
    }
}

// TODO: Add a `distance` field to `Gesture` that will specify the minimum distance a move must cover to be considered valid.
/*
options:
    distance:
        short: 0.15
        medium: 0.3
        long: 0.5

- fingers: 3
  action: move up
  distance: 0.3

- fingers: 3
  action: move up
  distance: short|medium|long
*/

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Gesture {
    pub name: String,
    pub sequence: Vec<DefinedSequenceStep>,
    #[serde(default)]
    pub repeatable: bool,
    pub command: String,
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct Options {
    #[serde(default = "Options::default_move_threshold")]
    pub move_threshold: f32,
}

impl Options {
    fn default_move_threshold() -> f32 { 0.15 }
}

type ApplicationGestures = HashMap<String, Vec<Gesture>>;

#[derive(Debug, serde::Deserialize)]
pub struct Config {
    pub import: Option<Vec<String>>,
    #[serde(default)]
    pub options: Options,
    #[serde(default)]
    pub gestures: Vec<Gesture>,
    #[serde(default)]
    pub application_gestures: ApplicationGestures,
}

#[derive(Debug, serde::Deserialize)]
struct ImportedConfig {
    pub gestures: Option<Vec<Gesture>>,
    pub application_gestures: Option<ApplicationGestures>,
}

fn are_gestures_conflicting(g1: &Gesture, g2: &Gesture) -> bool {
    if g1.sequence.len() != g2.sequence.len() {
        return false;
    }

    for (step1, step2) in g1.sequence.iter().zip(g2.sequence.iter()) {
        match (step1, step2) {
            (DefinedSequenceStep::TouchDown { fingers: f1 }, DefinedSequenceStep::TouchDown { fingers: f2 }) |
            (DefinedSequenceStep::TouchUp { fingers: f1 }, DefinedSequenceStep::TouchUp { fingers: f2 }) => {
                if f1 != f2 {
                    return false;
                }
            }
            (DefinedSequenceStep::Move { fingers: f1, direction: d1 }, DefinedSequenceStep::Move { fingers: f2, direction: d2 }) => {
                if f1 != f2 || d1 != d2 {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

fn is_repeatable_gesture_invalid(gesture: &Gesture) -> bool {
    gesture.repeatable
        && gesture.sequence.len() >= 2
        && matches!(gesture.sequence[gesture.sequence.len() - 2..], [DefinedSequenceStep::TouchUp { .. }, DefinedSequenceStep::TouchDown { .. }])
}

impl Config {
    pub fn parse_from_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(&path)?;
        let mut main_config: Config = serde_yaml::from_str(&content)?;
        if let Some(imports) = &main_config.import {
            let parent_path = path.as_ref().parent().unwrap_or_else(|| Path::new("."));
            for import_path in imports {
                let import_content = std::fs::read_to_string(parent_path.join(import_path))?;
                let imported_config: ImportedConfig = serde_yaml::from_str(&import_content)?;

                if let Some(gestures) = &imported_config.gestures {
                    main_config.gestures.extend(gestures.clone());
                }

                if let Some(app_gestures) = imported_config.application_gestures {
                    for (app, gestures) in app_gestures {
                        main_config.application_gestures.entry(app).or_default().extend(gestures);
                    }
                }
            }
        }

        // Check for conflicting gestures
        let all_gestures = main_config.gestures
            .iter()
            .chain(main_config.application_gestures.values().flatten())
            .cloned()
            .collect::<Vec<_>>();
        for i in 0..main_config.gestures.len() {
            for gesture in all_gestures.iter().skip(i + 1) {
                if are_gestures_conflicting(&main_config.gestures[i], gesture) {
                    // TODO: improve error reporting to show file and line numbers
                    eprintln!("Conflicting gestures found: '{}' and '{}'", main_config.gestures[i].name, gesture.name);
                }
            }
        }

        Ok(main_config)
    }

    pub fn get_config_path() -> Option<std::path::PathBuf> {
        if let Ok(xdg_config_home) = std::env::var("XDG_CONFIG_HOME") {
            Some(std::path::PathBuf::from(xdg_config_home).join("gest/config.yaml"))
        } else if let Ok(home) = std::env::var("HOME") {
            Some(std::path::PathBuf::from(home).join(".config").join("gest/config.yaml"))
        } else {
            None
        }
    }
}
