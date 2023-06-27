use serde::{Deserialize, Serialize};
use std::{os::fd::RawFd, path::PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Notification {
    pub id: u32,
    pub app_name: String,
    pub app_icon: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<(ActionId, String)>,
    pub hints: Vec<Hint>,
    pub expire_timeout: i32,
}

impl Notification {
    pub fn transient(&self) -> bool {
        self.hints
            .iter()
            .any(|h| matches!(h, Hint::Transient(true)))
    }

    pub fn category(&self) -> Option<&str> {
        self.hints.iter().find_map(|h| match h {
            Hint::Category(s) => Some(s.as_str()),
            _ => None,
        })
    }

    pub fn desktop_entry(&self) -> Option<&str> {
        self.hints.iter().find_map(|h| match h {
            Hint::DesktopEntry(s) => Some(s.as_str()),
            _ => None,
        })
    }

    pub fn urgency(&self) -> u8 {
        self.hints
            .iter()
            .find_map(|h| match h {
                Hint::Urgency(u) => Some(*u),
                _ => None,
            })
            .unwrap_or(1)
    }

    pub fn image(&self) -> Option<&Image> {
        self.hints.iter().find_map(|h| match h {
            Hint::Image(i) => Some(i),
            _ => None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ActionId {
    Default,
    Custom(String),
}

impl From<&str> for ActionId {
    fn from(s: &str) -> Self {
        // TODO more actions
        match s {
            "default" => Self::Default,
            s => Self::Custom(s.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Hint {
    ActionIcons(bool),
    Category(String),
    DesktopEntry(String),
    Image(Image),
    IconData(Vec<u8>),
    Resident(bool),
    SoundFile(PathBuf),
    SoundName(String),
    SuppressSound(bool),
    Transient(bool),
    Urgency(u8),
    X(i32),
    Y(i32),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]

pub enum Image {
    Name(String),
    File(PathBuf),
    /// RGBA
    Data {
        width: u32,
        height: u32,
        data: Vec<u8>,
    },
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CloseReason {
    Expired = 1,
    Dismissed = 2,
    CloseNotification = 3,
    Undefined = 4,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PanelRequest {
    /// A new instance of the panel is running, so the daemon can reset its state
    Init,
    NewNotificationsClient {
        id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PanelEvent {
    /// Panel should reset its state because a new instance of the daemon has been started
    Init,
    NewNotificationsClient {
        id: String,
        write_fd: RawFd,
        read_fd: RawFd,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AppletEvent {
    Notification(Notification),
    Replace(Notification),
}

pub const PANEL_NOTIFICATIONS_FD: &'static str = "PANEL_NOTIFICATIONS_FD";
pub const DAEMON_NOTIFICATIONS_FD: &'static str = "DAEMON_NOTIFICATIONS_FD";
