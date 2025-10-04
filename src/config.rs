use serde::Deserializer;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone)]
pub enum Step {
    TouchDown { fingers: u8 },
    TouchUp { fingers: u8 },
    Move { fingers: u8, direction: Direction }
}

impl<'de> serde::Deserialize<'de> for Step {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>
    {
        #[derive(serde::Deserialize)]
        struct StepHelper {
            fingers: u8,
            action: String,
        }

        let helper = StepHelper::deserialize(deserializer)?;
        let step = match helper.action.as_str() {
            "touch_down" | "touch down" => Step::TouchDown { fingers: helper.fingers },
            "touch_up" | "touch up" => Step::TouchUp { fingers: helper.fingers },
            "move_up" | "move up" => Step::Move { fingers: helper.fingers, direction: Direction::Up },
            "move_down" | "move down" => Step::Move { fingers: helper.fingers, direction: Direction::Down },
            "move_left" | "move left" => Step::Move { fingers: helper.fingers, direction: Direction::Left },
            "move_right" | "move right" => Step::Move { fingers: helper.fingers, direction: Direction::Right },
            _ => return Err(serde::de::Error::custom(format!("Unknown action: {}", helper.action))),
        };
        Ok(step)
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Gesture {
    pub name: String,
    pub sequence: Vec<Step>,
    #[serde(default = "Gesture::default_repeatable")]
    pub repeatable: bool,
    pub command: String,
}

impl Gesture {
    fn default_repeatable() -> bool { false }
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct Options {
    #[serde(default = "Options::default_move_threshold")]
    pub move_threshold: f32,
}

impl Options {
    fn default_move_threshold() -> f32 { 0.15 }
}

#[derive(Debug, serde::Deserialize)]
pub struct Config {
    #[serde(default)]
    pub options: Options,
    pub gestures: Vec<Gesture>,
}

impl Config {
    pub fn parse_from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = serde_yaml::from_str(&content)?;
        Ok(config)
    }
}