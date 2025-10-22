use std::collections::HashMap;
use std::path::Path;
use regex::Regex;
use crate::sequence_step::DefinedSequenceStep;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
    None,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
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

#[derive(Debug, Clone, PartialEq, serde::Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RepeatMode {
    #[default]
    None,
    Tap,
    Slide,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Gesture {
    pub name: String,
    pub sequence: Vec<DefinedSequenceStep>,
    #[serde(default)]
    pub repeat_mode: RepeatMode,
    pub command: String,
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct EdgeOptions {
    #[serde(default = "EdgeOptions::default_threshold")]
    pub threshold: f32,
    #[serde(default = "EdgeOptions::default_sensitivity")]
    pub sensitivity: f32,
}

impl EdgeOptions {
    fn default_threshold() -> f32 { 0.1 }

    fn default_sensitivity() -> f32 { 0.5 }
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct Options {
    #[serde(default = "Options::default_move_threshold")]
    pub move_threshold: f32,
    #[serde(default)]
    pub edge: EdgeOptions,
}

impl Options {
    fn default_move_threshold() -> f32 { 0.15 }
}

type ApplicationGesturesRaw = HashMap<String, Vec<Gesture>>;

#[derive(Debug, Default)]
pub struct ApplicationGestures {
    pub by_title: Vec<(Regex, Vec<Gesture>)>,
    pub by_class: Vec<(Regex, Vec<Gesture>)>,
}

#[derive(Debug, serde::Deserialize)]
pub struct ConfigRaw {
    #[serde(default)]
    pub import: Vec<String>,
    #[serde(default)]
    pub options: Options,
    #[serde(default)]
    pub gestures: Vec<Gesture>,
    #[serde(default)]
    pub application_gestures: ApplicationGesturesRaw,
}

#[derive(Debug)]
pub struct Config {
    pub options: Options,
    pub gestures: Vec<Gesture>,
    pub application_gestures: ApplicationGestures,
}

#[derive(Debug, serde::Deserialize)]
struct ImportedConfigRaw {
    pub gestures: Option<Vec<Gesture>>,
    pub application_gestures: Option<ApplicationGesturesRaw>,
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

impl Config {
    pub fn parse_from_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(&path)?;
        let mut main_config: ConfigRaw = serde_yaml::from_str(&content)?;

        let mut application_gestures = ApplicationGestures::default();

        if !main_config.import.is_empty() {
            let parent_path = path.as_ref().parent().unwrap_or_else(|| Path::new("."));
            for import_path in &main_config.import {
                let import_content = std::fs::read_to_string(parent_path.join(import_path))?;
                let imported_config: ImportedConfigRaw = serde_yaml::from_str(&import_content)?;

                if let Some(gestures) = &imported_config.gestures {
                    main_config.gestures.extend(gestures.clone());
                }

                if let Some(app_gestures) = imported_config.application_gestures {
                    for (app_name, gestures) in app_gestures {
                        if let Some(regex_str) = app_name.strip_prefix("title:") {
                            let regex = Regex::new(regex_str)?;
                            application_gestures.by_title.push((regex, gestures));
                        } else if let Some(regex_str) = app_name.strip_prefix("class:") {
                            let regex = Regex::new(regex_str)?;
                            application_gestures.by_class.push((regex, gestures));
                        } else {
                            // Treat as class
                            let regex = Regex::new(&app_name)?;
                            application_gestures.by_class.push((regex, gestures));
                        }
                    }
                }
            }
        }

        let all_gestures = main_config.gestures
            .iter()
            .chain(main_config.application_gestures.values().flatten())
            .collect::<Vec<_>>();

        // Check for conflicting gestures
        for i in 0..main_config.gestures.len() {
            for gesture in all_gestures.iter().skip(i + 1) {
                if are_gestures_conflicting(&main_config.gestures[i], gesture) {
                    // TODO: improve error reporting to show file and line numbers
                    eprintln!("Warning: Conflicting gestures found: '{}' and '{}'", main_config.gestures[i].name, gesture.name);
                }
            }
        }

        Ok(Config {
            options: main_config.options,
            gestures: main_config.gestures,
            application_gestures,
        })
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
