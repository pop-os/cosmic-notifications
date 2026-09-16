use cosmic_config::{CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};

pub const ID: &str = "com.system76.CosmicNotifications";

pub const PANEL_NOTIFICATIONS_FD: &str = "PANEL_NOTIFICATIONS_FD";
pub const DAEMON_NOTIFICATIONS_FD: &str = "DAEMON_NOTIFICATIONS_FD";

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

/// Selects the output where notification popups are displayed.
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum NotificationOutput {
    /// Let the compositor select the active output.
    #[default]
    Active,
    /// Display notifications on the output containing the notifications applet.
    ///
    /// The applet's output is taken from the panel or dock config. This only
    /// differs from `Active` when that panel is pinned to a single named output;
    /// if the panel is shown on all outputs or follows the active output, there
    /// is no single applet output and this falls back to `Active`.
    Applet,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, CosmicConfigEntry)]
#[version = 1]
pub struct NotificationsConfig {
    pub do_not_disturb: bool,
    pub anchor: Anchor,
    /// Which output notification popups are displayed on. See [`NotificationOutput`].
    pub output: NotificationOutput,
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
            output: NotificationOutput::default(),
            max_notifications: 3,
            max_per_app: 2,
            max_timeout_urgent: None,
            max_timeout_normal: Some(5000),
            max_timeout_low: Some(3000),
        }
    }
}
