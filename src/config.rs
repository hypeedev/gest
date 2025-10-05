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

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Gesture {
    pub name: String,
    pub sequence: Vec<DefinedSequenceStep>,
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
        Ok(serde_yaml::from_str(&content)?)
    }
}