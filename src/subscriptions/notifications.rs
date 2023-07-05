use crate::config::VERSION;
use cosmic::{
    iced::{
        futures::{self, SinkExt},
        subscription,
    },
    iced_futures::Subscription,
};
use cosmic_notifications_util::{ActionId, CloseReason, Hint, Notification};
use fast_image_resize as fr;
use std::{collections::HashMap, fmt::Debug, num::NonZeroU32, path::PathBuf, time::SystemTime};
use tokio::sync::mpsc::{channel, Receiver, Sender};

use zbus::{
    dbus_interface,
    zvariant::{Signature, Structure},
    Connection, ConnectionBuilder, SignalContext,
};

#[derive(Debug)]
pub enum State {
    Starting,
    Waiting(Connection, Receiver<Input>),
    Finished,
}

#[derive(Debug, Clone)]
pub enum Input {
    Notification(Notification),
    Replace(Notification),
    CloseNotification(u32),
    Closed(u32, CloseReason),
}

#[derive(Debug, Clone)]
pub enum Event {
    Ready(Sender<Input>),
    Notification(Notification),
    Replace(Notification),
    CloseNotification(u32),
}

pub fn notifications() -> Subscription<Event> {
    struct SomeWorker;

    subscription::channel(
        std::any::TypeId::of::<SomeWorker>(),
        100,
        |mut output| async move {
            let mut state = State::Starting;

            loop {
                match &mut state {
                    State::Starting => {
                        // Create channel
                        let (tx, rx) = channel(100);
                        if let Some(conn) = ConnectionBuilder::session()
                            .ok()
                            .and_then(|conn| conn.name("org.freedesktop.Notifications").ok())
                            .and_then(|conn| {
                                conn.serve_at(
                                    "/org/freedesktop/Notifications",
                                    Notifications(tx.clone(), NonZeroU32::new(1).unwrap()),
                                )
                                .ok()
                            })
                            .map(ConnectionBuilder::build)
                        {
                            if let Ok(conn) = conn.await {
                                // Send the sender back to the application
                                _ = output.send(Event::Ready(tx)).await;

                                // We are ready to receive messages
                                state = State::Waiting(conn, rx);
                            }
                        } else {
                            tracing::error!("Failed to create the dbus server");
                            state = State::Finished;
                        }
                    }
                    State::Waiting(conn, rx) => {
                        // Read next input sent from `Application`
                        if let Some(next) = rx.recv().await {
                            match next {
                                Input::Closed(id, reason) => {
                                    let object_server = conn.object_server();
                                    if let Ok(iface_ref) = object_server
                                        .interface::<_, Notifications>(
                                            "/org/freedesktop/Notifications",
                                        )
                                        .await
                                    {
                                        _ = Notifications::notification_closed(
                                            iface_ref.signal_context(),
                                            id,
                                            reason as u32,
                                        )
                                        .await;
                                    }
                                }
                                Input::Notification(notification) => {
                                    _ = output.send(Event::Notification(notification)).await;
                                }
                                Input::Replace(notification) => {
                                    _ = output.send(Event::Replace(notification)).await;
                                }
                                Input::CloseNotification(id) => {
                                    _ = output.send(Event::CloseNotification(id)).await;
                                }
                            }
                        } else {
                            // The channel was closed, so we are done
                            state = State::Finished;
                        }
                    }
                    State::Finished => {
                        let () = futures::future::pending().await;
                    }
                }
            }
        },
    )
}

pub struct Notifications(Sender<Input>, NonZeroU32);

#[dbus_interface(name = "org.freedesktop.Notifications")]
impl Notifications {
    async fn close_notification(&self, id: u32) {
        if let Err(err) = self.0.send(Input::CloseNotification(id)).await {
            tracing::error!("Failed to send close notification: {}", err);
        }
    }

    /// "action-icons"	Supports using icons instead of text for displaying actions. Using icons for actions must be enabled on a per-notification basis using the "action-icons" hint.
    /// "actions"	The server will provide the specified actions to the user. Even if this cap is missing, actions may still be specified by the client, however the server is free to ignore them.
    /// "body"	Supports body text. Some implementations may only show the summary (for instance, onscreen displays, marquee/scrollers)
    /// "body-hyperlinks"	The server supports hyperlinks in the notifications.
    /// "body-images"	The server supports images in the notifications.
    /// "body-markup"	Supports markup in the body text. If marked up text is sent to a server that does not give this cap, the markup will show through as regular text so must be stripped clientside.
    /// "icon-multi"	The server will render an animation of all the frames in a given image array. The client may still specify multiple frames even if this cap and/or "icon-static" is missing, however the server is free to ignore them and use only the primary frame.
    /// "icon-static"	Supports display of exactly 1 frame of any given image array. This value is mutually exclusive with "icon-multi", it is a protocol error for the server to specify both.
    /// "persistence"	The server supports persistence of notifications. Notifications will be retained until they are acknowledged or removed by the user or recalled by the sender. The presence of this capability allows clients to depend on the server to ensure a notification is seen and eliminate the need for the client to display a reminding function (such as a status icon) of its own.
    /// "sound"	The server supports sounds on notifications. If returned, the server must support the "sound-file" and "suppress-sound" hints.
    async fn get_capabilities(&self) -> Vec<&'static str> {
        // TODO more capabilities
        vec!["body", "icon-static", "persistence"]
    }

    #[dbus_interface(out_args("name", "vendor", "version", "spec_version"))]
    async fn get_server_information(
        &self,
    ) -> (&'static str, &'static str, &'static str, &'static str) {
        ("cosmic-notifications", "System76", VERSION, "1.2")
    }

    ///
    /// app_name	STRING	The optional name of the application sending the notification. Can be blank.
    ///
    /// replaces_id	UINT32	The optional notification ID that this notification replaces. The server must atomically (ie with no flicker or other visual cues) replace the given notification with this one. This allows clients to effectively modify the notification while it's active. A value of value of 0 means that this notification won't replace any existing notifications.
    ///
    /// app_icon	STRING	The optional program icon of the calling application. See Icons and Images. Can be an empty string, indicating no icon.
    ///
    /// summary	STRING	The summary text briefly describing the notification.
    ///
    /// body	STRING	The optional detailed body text. Can be empty.
    ///
    /// actions	as	Actions are sent over as a list of pairs. Each even element in the list (starting at index 0) represents the identifier for the action. Each odd element in the list is the localized string that will be displayed to the user.
    ///
    /// hints	a{sv}	Optional hints that can be passed to the server from the client program. Although clients and servers should never assume each other supports any specific hints, they can be used to pass along information, such as the process PID or window ID, that the server may be able to make use of. See Hints. Can be empty.
    /// expire_timeout	INT32
    ///
    /// The timeout time in milliseconds since the display of the notification at which the notification should automatically close.
    /// If -1, the notification's expiration time is dependent on the notification server's settings, and may vary for the type of notification. If 0, never expire.
    async fn notify(
        &mut self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: Vec<&str>,
        hints: HashMap<&str, zbus::zvariant::Value<'_>>,
        expire_timeout: i32,
    ) -> u32 {
        let id = if replaces_id == 0 {
            let id = self.1;
            self.1 = match self.1.checked_add(1) {
                Some(id) => id,
                None => {
                    tracing::warn!("Notification ID overflowed");
                    NonZeroU32::new(1).unwrap()
                }
            };
            id.get()
        } else {
            replaces_id
        };
        let actions = actions
            .chunks_exact(2)
            .map(|a| (ActionId::from(a[0]), a[1].to_string()))
            .collect();

        let hints = hints
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
                "image-path" | "image_path" | "app_icon" => {
                    String::try_from(v).ok().and_then(|s| {
                        if s.starts_with("file://") {
                            s.strip_prefix("file://").map(|s| {
                                Hint::Image(cosmic_notifications_util::Image::File(PathBuf::from(
                                    s,
                                )))
                            })
                        } else {
                            Some(Hint::Image(cosmic_notifications_util::Image::Name(s)))
                        }
                    })
                }
                "image-data" | "image_data" | "icon_data" => match v {
                    zbus::zvariant::Value::Structure(v) => match ImageData::try_from(v) {
                        Ok(mut image) => Some({
                            image = image.into_rgba();
                            Hint::Image(cosmic_notifications_util::Image::Data {
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

        let n = Notification {
            id,
            app_name: app_name.to_string(),
            app_icon: app_icon.to_string(),
            summary: summary.to_string(),
            body: body.to_string(),
            actions,
            hints,
            expire_timeout,
            time: SystemTime::now(),
        };

        if let Err(err) = self
            .0
            .send(if replaces_id == 0 {
                Input::Notification(n)
            } else {
                Input::Replace(n)
            })
            .await
        {
            tracing::error!("Failed to send notification: {}", err);
        }

        id
    }

    #[dbus_interface(signal)]
    async fn action_invoked(
        signal_ctxt: &SignalContext<'_>,
        id: u32,
        action_key: &str,
    ) -> zbus::Result<()> {
    }

    /// id	UINT32	The ID of the notification that was closed.
    /// reason	UINT32
    ///
    /// The reason the notification was closed.
    ///
    /// 1 - The notification expired.
    ///
    /// 2 - The notification was dismissed by the user.
    ///
    /// 3 - The notification was closed by a call to CloseNotification.
    ///
    /// 4 - Undefined/reserved reasons.
    #[dbus_interface(signal)]
    async fn notification_closed(
        signal_ctxt: &SignalContext<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()> {
    }
}

pub struct ImageData {
    pub width: u32,
    pub height: u32,
    pub rowstride: i32,
    pub has_alpha: bool,
    pub bits_per_sample: i32,
    pub channels: i32,
    pub data: Vec<u8>,
}

impl ImageData {
    fn into_rgba(self) -> Self {
        let rgba = if self.has_alpha {
            self
        } else {
            let mut data = self.data;
            let mut new_data = Vec::with_capacity(data.len() / self.channels as usize * 4);

            for chunk in data.chunks_exact_mut(self.channels as usize) {
                new_data.extend_from_slice(chunk);
                new_data.push(0xFF);
            }

            Self {
                has_alpha: true,
                data: new_data,
                channels: 4,
                rowstride: self.width as i32 * 4,
                ..self
            }
        };

        if rgba.width <= 16 && rgba.height <= 16 {
            return rgba;
        }
        let mut src = fr::Image::from_vec_u8(
            NonZeroU32::try_from(rgba.width).unwrap(),
            NonZeroU32::try_from(rgba.height).unwrap(),
            rgba.data,
            fr::PixelType::U8x4,
        )
        .unwrap();
        // Multiple RGB channels of source image by alpha channel
        // (not required for the Nearest algorithm)
        let alpha_mul_div = fr::MulDiv::default();
        alpha_mul_div
            .multiply_alpha_inplace(&mut src.view_mut())
            .unwrap();
        let dst_width = NonZeroU32::try_from(rgba.width.min(16)).unwrap();
        let dst_height = NonZeroU32::try_from(rgba.height.min(16)).unwrap();
        let mut dst = fr::Image::new(dst_width, dst_height, fr::PixelType::U8x4);
        let mut dst_view = dst.view_mut();
        let mut resizer = fr::Resizer::new(fr::ResizeAlg::Convolution(fr::FilterType::Lanczos3));
        resizer.resize(&src.view(), &mut dst_view).unwrap();
        alpha_mul_div.divide_alpha_inplace(&mut dst_view).unwrap();

        Self {
            width: dst.width().get(),
            height: dst.height().get(),
            data: dst.into_vec(),
            ..rgba
        }
    }
}

impl<'a> TryFrom<Structure<'a>> for ImageData {
    type Error = zbus::Error;

    fn try_from(value: Structure<'a>) -> zbus::Result<Self> {
        if Ok(value.full_signature()) != Signature::from_static_str("(iiibiiay)").as_ref() {
            return Err(zbus::Error::Failure(format!(
                "Invalid ImageData: invalid signature {}",
                value.full_signature().to_string()
            )));
        }

        let mut fields = value.into_fields();

        if fields.len() != 7 {
            return Err(zbus::Error::Failure(
                "Invalid ImageData: missing fields".to_string(),
            ));
        }

        let data = Vec::<u8>::try_from(fields.remove(6))
            .map_err(|e| zbus::Error::Failure(format!("data: {}", e)))?;
        let channels = i32::try_from(fields.remove(5))
            .map_err(|e| zbus::Error::Failure(format!("channels: {}", e)))?;
        let bits_per_sample = i32::try_from(fields.remove(4))
            .map_err(|e| zbus::Error::Failure(format!("bits_per_sample: {}", e)))?;
        let has_alpha = bool::try_from(fields.remove(3))
            .map_err(|e| zbus::Error::Failure(format!("has_alpha: {}", e)))?;
        let rowstride = i32::try_from(fields.remove(2))
            .map_err(|e| zbus::Error::Failure(format!("rowstride: {}", e)))?;
        let height = i32::try_from(fields.remove(1))
            .map_err(|e| zbus::Error::Failure(format!("height: {}", e)))?;
        let width = i32::try_from(fields.remove(0))
            .map_err(|e| zbus::Error::Failure(format!("width: {}", e)))?;

        if width <= 0 {
            return Err(zbus::Error::Failure(
                "Invalid ImageData: width is not positive".to_string(),
            ));
        }

        if height <= 0 {
            return Err(zbus::Error::Failure(
                "Invalid ImageData: height is not positive".to_string(),
            ));
        }

        if bits_per_sample != 8 {
            return Err(zbus::Error::Failure(
                "Invalid ImageData: bits_per_sample is not 8".to_string(),
            ));
        }

        if has_alpha && channels != 4 {
            return Err(zbus::Error::Failure(
                "Invalid ImageData: has_alpha is true but channels is not 4".to_string(),
            ));
        }

        if (width * height * channels) as usize != data.len() {
            return Err(zbus::Error::Failure(
                "Invalid ImageData: data length does not match width * height * channels"
                    .to_string(),
            ));
        }

        if data.len() != (rowstride * height) as usize {
            return Err(zbus::Error::Failure(
                "Invalid ImageData: data length does not match rowstride * height".to_string(),
            ));
        }

        Ok(Self {
            width: width as u32,
            height: height as u32,
            rowstride,
            has_alpha,
            bits_per_sample,
            channels,
            data,
        })
    }
}
