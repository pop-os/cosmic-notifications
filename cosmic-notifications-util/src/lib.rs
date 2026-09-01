#[cfg(feature = "image")]
pub mod image;
#[cfg(feature = "image")]
pub use image::*;

pub mod markup;

use cosmic::desktop::fde;
use cosmic::widget::{Icon, icon};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    convert::Infallible,
    fmt,
    path::{Path, PathBuf},
    str::FromStr,
    sync::OnceLock,
    time::SystemTime,
};

/// Desktop entries of the system, loaded once and reused for `desktop-entry` lookups.
fn desktop_entries() -> &'static [fde::DesktopEntry] {
    static ENTRIES: OnceLock<Vec<fde::DesktopEntry>> = OnceLock::new();

    ENTRIES.get_or_init(|| {
        let locales = fde::get_languages_from_env();
        fde::Iter::new(fde::default_paths())
            .filter_map(|path| fde::DesktopEntry::from_path(path, Some(&locales)).ok())
            .collect()
    })
}

/// Value of the `Icon` key of the desktop entry with the given app id.
fn desktop_entry_icon(app_id: &str) -> Option<String> {
    fde::find_app_by_id(desktop_entries(), fde::unicase::Ascii::new(app_id))
        .and_then(fde::DesktopEntry::icon)
        .map(ToString::to_string)
}

/// The `image-path` hint is specified as a URI, but applications commonly pass a bare
/// absolute path or a plain icon name instead. Accept all three.
fn image_from_path_hint(value: String) -> Image {
    if let Some(path) = url::Url::parse(&value)
        .ok()
        .and_then(|url| url.to_file_path().ok())
    {
        return Image::File(path);
    }

    let path = Path::new(&value);
    if path.is_absolute() && path.exists() {
        return Image::File(path.to_path_buf());
    }

    Image::Name(value)
}

/// The `app_icon` argument of `Notify`, the `image-path` hint and the `Icon` key of a
/// desktop entry may each be either an icon name or an absolute path to an image file.
fn icon_from_name_or_path(value: &str) -> Icon {
    let path = Path::new(value);
    if path.is_absolute() && path.exists() {
        icon::from_path(path.to_path_buf()).icon()
    } else {
        icon::from_name(value).icon()
    }
}

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
    pub time: SystemTime,
}

impl Notification {
    #[allow(clippy::too_many_arguments)]
    #[cfg(feature = "zbus_notifications")]
    pub fn new(
        app_name: &str,
        id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: Vec<&str>,
        hints: HashMap<&str, zbus::zvariant::Value<'_>>,
        expire_timeout: i32,
    ) -> Self {
        let actions = actions
            .chunks_exact(2)
            .map(|a| (a[0].parse().unwrap(), a[1].to_string()))
            .collect();

        let hints: Vec<Hint> = hints
            .into_iter()
            .filter_map(|(k, v)| match k {
                "action-icons" => bool::try_from(v).map(Hint::ActionIcons).ok(),
                "category" => String::try_from(v).map(Hint::Category).ok(),
                "desktop-entry" => String::try_from(v).map(Hint::DesktopEntry).ok(),
                "resident" => bool::try_from(v).map(Hint::Resident).ok(),
                "sound-file" => String::try_from(v)
                    .map(|s| Hint::SoundFile(PathBuf::from(s)))
                    .ok(),
                "sound-name" => String::try_from(v).map(Hint::SoundName).ok(),
                "suppress-sound" => bool::try_from(v).map(Hint::SuppressSound).ok(),
                "transient" => bool::try_from(v).map(Hint::Transient).ok(),
                "x" => i32::try_from(v).map(Hint::X).ok(),
                "y" => i32::try_from(v).map(Hint::Y).ok(),
                "urgency" => u8::try_from(v).map(Hint::Urgency).ok(),
                "image-path" | "image_path" => String::try_from(v)
                    .ok()
                    .map(|s| Hint::Image(image_from_path_hint(s))),
                "image-data" | "image_data" | "icon_data" => match v {
                    zbus::zvariant::Value::Structure(v) => match ImageData::try_from(v) {
                        Ok(mut image) => Some({
                            image = image.into_rgba();
                            Hint::Image(Image::Data {
                                width: image.width,
                                height: image.height,
                                data: image.data,
                            })
                        }),
                        Err(err) => {
                            tracing::warn!("Invalid image data: {}", err);
                            None
                        }
                    },
                    _ => {
                        tracing::warn!("Invalid value for hint: {}", k);
                        None
                    }
                },
                _ => {
                    tracing::warn!("Unknown hint: {}", k);
                    None
                }
            })
            .collect();

        // Applications that supply neither `app_icon` nor an image hint commonly
        // identify themselves with the `desktop-entry` hint instead. Fall back to the
        // `Icon` key of that desktop entry so those notifications are not left iconless.
        let mut app_icon = app_icon.to_string();
        if app_icon.is_empty()
            && !hints.iter().any(|h| matches!(h, Hint::Image(_)))
            && let Some(desktop_entry) = hints.iter().find_map(|h| match h {
                Hint::DesktopEntry(s) => Some(s.as_str()),
                _ => None,
            })
            && let Some(icon) = desktop_entry_icon(desktop_entry)
        {
            app_icon = icon;
        }

        Notification {
            id,
            app_name: app_name.to_string(),
            app_icon,
            summary: summary.to_string(),
            body: body.to_string(),
            actions,
            hints,
            expire_timeout,
            time: SystemTime::now(),
        }
    }

    pub fn transient(&self) -> bool {
        self.hints.contains(&Hint::Transient(true))
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

    pub fn notification_icon(&self) -> Option<Icon> {
        match self.image() {
            Some(Image::File(path)) => Some(icon::from_path(PathBuf::from(path)).icon()),
            Some(Image::Name(name)) => Some(icon::from_name(name.as_str()).icon()),
            Some(Image::Data {
                width,
                height,
                data,
            }) => Some(icon::from_raster_pixels(*width, *height, data.clone()).icon()),
            None => {
                if !self.app_icon.is_empty() {
                    // Handle file:// URLs in app_icon
                    if self.app_icon.starts_with("file://")
                        && let Ok(url) = url::Url::parse(&self.app_icon)
                        && let Ok(path) = url.to_file_path()
                    {
                        return Some(icon::from_path(path).icon());
                    }
                    // Otherwise treat as an icon name or an absolute path
                    Some(icon_from_name_or_path(self.app_icon.as_str()))
                } else {
                    None
                }
            }
        }
    }

    pub fn duration_since(&self) -> Option<std::time::Duration> {
        SystemTime::now().duration_since(self.time).ok()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ActionId {
    Default,
    Custom(String),
}

impl fmt::Display for ActionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActionId::Default => write!(f, "default"),
            ActionId::Custom(value) => write!(f, "{}", value),
        }
    }
}

impl FromStr for ActionId {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "default" => ActionId::Default,
            s => ActionId::Custom(s.to_string()),
        })
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
