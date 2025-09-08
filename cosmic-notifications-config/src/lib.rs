use cosmic_config::{CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};

pub const ID: &str = "com.system76.CosmicNotifications";

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Anchor {
    #[default]
    Top,
    Bottom,
    Right,
    Left,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, CosmicConfigEntry)]
#[version = 1]
pub struct NotificationsConfig {
    pub do_not_disturb: bool,
    pub anchor: Anchor,
    /// The maximum number of notifications that can be displayed at once.
    pub max_notifications: u32,
    /// The maximum number of notifications that can be displayed per app if not urgent and constrained by `max_notifications`.
    pub max_per_app: u32,
    /// Max time in milliseconds a critical notification can be displayed before being removed.
    pub max_timeout_urgent: Option<u32>,
    /// Max time in milliseconds a normal notification can be displayed before being removed.
    pub max_timeout_normal: Option<u32>,
    /// Max time in milliseconds a low priority notification can be displayed before being removed.
    pub max_timeout_low: Option<u32>,
}

impl Default for NotificationsConfig {
    fn default() -> Self {
        Self {
            do_not_disturb: false,
            anchor: Anchor::default(),
            max_notifications: 3,
            max_per_app: 2,
            max_timeout_urgent: None,
            max_timeout_normal: Some(5000),
            max_timeout_low: Some(3000),
        }
    }
}
