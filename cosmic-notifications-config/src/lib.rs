use cosmic_config::{
    cosmic_config_derive::CosmicConfigEntry, Config, ConfigGet, ConfigSet, CosmicConfigEntry,
};

pub const ID: &str = "com.system76.CosmicNotifications";

#[derive(
    Debug, Default, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, CosmicConfigEntry,
)]
pub struct NotificationsConfig {
    pub do_not_disturb: bool,
}

impl NotificationsConfig {
    pub fn version() -> u64 {
        1
    }
}
