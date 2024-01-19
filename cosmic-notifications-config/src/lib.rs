use cosmic_config::{cosmic_config_derive::CosmicConfigEntry, CosmicConfigEntry};

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

#[derive(
    Debug, Default, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, CosmicConfigEntry,
)]
#[version = 1]
pub struct NotificationsConfig {
    pub do_not_disturb: bool,
    pub anchor: Anchor,
}
