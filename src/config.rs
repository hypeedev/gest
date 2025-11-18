use std::collections::HashMap;
use std::path::Path;
use regex::Regex;
use bitflags::bitflags;
use crate::sequence_step::{DefinedSequenceStep, DefinedSequenceStepRaw};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
    None,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

bitflags! {
    #[derive(Debug, Clone, PartialEq, Default)]
    pub struct RepeatMode: u8 {
        const None = 0b00;
        const Tap = 0b01;
        const Slide = 0b10;
    }
}

impl<'de> serde::Deserialize<'de> for RepeatMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let mut repeat_mode = RepeatMode::None;
        let s: String = serde::Deserialize::deserialize(deserializer)?;
        for mode in s.split_whitespace().collect::<Vec<_>>() {
            match mode.to_lowercase().as_str() {
                "none" => {},
                "tap" => { repeat_mode.insert(RepeatMode::Tap); },
                "slide" => { repeat_mode.insert(RepeatMode::Slide); },
                _ => return Err(serde::de::Error::custom(format!("Invalid repeat mode: {}", mode))),
            }
        }
        Ok(repeat_mode)
    }
}

#[derive(Debug, Clone)]
pub struct Gesture {
    pub name: String,
    pub sequence: Vec<DefinedSequenceStep>,
    pub edge: Option<Edge>,
    pub repeat_mode: RepeatMode,
    pub command: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct GestureRaw {
    pub name: String,
    pub sequence: Vec<DefinedSequenceStepRaw>,
    pub edge: Option<Edge>,
    #[serde(default)]
    pub repeat_mode: RepeatMode,
    pub command: String,
}

impl Gesture {
    pub fn from_raw(raw: GestureRaw, distances: &HashMap<String, f32>) -> Self {
        Gesture {
            name: raw.name,
            sequence: raw.sequence.into_iter().map(|step_raw| {
                DefinedSequenceStep::from_raw(step_raw, distances)
            }).collect(),
            edge: raw.edge,
            repeat_mode: raw.repeat_mode,
            command: raw.command,
        }
    }
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct EdgeOptions {
    #[serde(default = "EdgeOptions::default_threshold")]
    pub threshold: f32,
    #[serde(default = "EdgeOptions::default_sensitivity")]
    pub sensitivity: f32,
}

impl EdgeOptions {
    fn default_threshold() -> f32 { 0.05 }

    fn default_sensitivity() -> f32 { 0.5 }
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct Options {
    #[serde(default = "Options::default_move_threshold")]
    pub move_threshold: f32,
    #[serde(default)]
    pub edge: EdgeOptions,
    #[serde(default)]
    pub run_all_matches: bool,
    #[serde(default)]
    pub distance: HashMap<String, f32>,
}

impl Options {
    fn default_move_threshold() -> f32 { 0.15 }
}

type ApplicationGesturesRaw = HashMap<String, Vec<GestureRaw>>;

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
    pub gestures: Vec<GestureRaw>,
}

#[derive(Debug)]
pub struct Config {
    pub options: Options,
    pub gestures: Vec<Gesture>,
    pub application_gestures: ApplicationGestures,
}

// TODO: clean this up
impl Config {
    pub fn from_raw<P: AsRef<Path>>(path: P, config_raw: ConfigRaw) -> Result<Self, Box<dyn std::error::Error>> {
        let mut gestures = config_raw.gestures.iter().map(|g_raw| {
            Gesture::from_raw(g_raw.clone(), &config_raw.options.distance)
        }).collect::<Vec<_>>();

        let mut application_gestures = ApplicationGestures::default();

        if !config_raw.import.is_empty() {
            let parent_path = path.as_ref().parent().unwrap_or_else(|| Path::new("."));
            for import_path in &config_raw.import {
                let import_content = std::fs::read_to_string(parent_path.join(import_path))?;
                let imported_config: ImportedConfigRaw = serde_yaml::from_str(&import_content)?;

                if let Some(imported_gestures) = &imported_config.gestures {
                    let imported_gestures = imported_gestures.iter().map(|g_raw| {
                        Gesture::from_raw(g_raw.clone(), &config_raw.options.distance)
                    });
                    gestures.extend(imported_gestures);
                }

                if let Some(app_gestures) = imported_config.application_gestures {
                    for (app_name, gestures) in app_gestures {
                        let gestures = gestures.iter().map(|g_raw| {
                            Gesture::from_raw(g_raw.clone(), &config_raw.options.distance)
                        }).collect::<Vec<_>>();

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

        Ok(Config {
            options: config_raw.options,
            gestures,
            application_gestures,
        })
    }
}

#[derive(Debug, serde::Deserialize)]
struct ImportedConfigRaw {
    pub gestures: Option<Vec<GestureRaw>>,
    pub application_gestures: Option<ApplicationGesturesRaw>,
}

fn are_gestures_conflicting(g1: &Gesture, g2: &Gesture) -> bool {
    if g1.sequence.len() != g2.sequence.len()
        || g1.edge != g2.edge
    {
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
            (DefinedSequenceStep::Move { fingers: f1, direction: d1, distance: dst1 }, DefinedSequenceStep::Move { fingers: f2, direction: d2, distance: dst2 }) => {
                if f1 != f2 || d1 != d2 || dst1 != dst2 {
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
        let main_config_raw: ConfigRaw = serde_yaml::from_str(&content)?;
        let main_config = Config::from_raw(path, main_config_raw)?;

        let all_gestures = main_config.gestures
            .iter()
            .chain(main_config.application_gestures.by_title.iter().flat_map(|(_, gestures)| gestures))
            .chain(main_config.application_gestures.by_class.iter().flat_map(|(_, gestures)| gestures))
            .collect::<Vec<_>>();

        // Check for conflicting gestures
        for i in 0..main_config.gestures.len() {
            for &gesture in all_gestures.iter().skip(i + 1) {
                let g1 = &main_config.gestures[i];
                let g2 = gesture;
                if are_gestures_conflicting(g1, g2) {
                    // TODO: improve error reporting to show file and line numbers
                    log::warn!("Warning: Conflicting gestures found: '{}' and '{}'", g1.name, g2.name);
                }
            }
        }

        // Check for sequence steps with distance less than threshold
        for gesture in &all_gestures {
            for step in &gesture.sequence {
                if let DefinedSequenceStep::Move { distance, .. } = step
                    && let Some(distance) = distance
                    && *distance < main_config.options.move_threshold
                {
                    log::warn!(
                        "Gesture '{}' has a move step with distance {} which is less than the configured move_threshold of {}",
                        gesture.name, distance, main_config.options.move_threshold
                    );
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
