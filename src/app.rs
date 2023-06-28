use crate::config;
use crate::subscriptions::{notifications, panel};
use cosmic::cosmic_config::{config_subscription, CosmicConfigEntry};
use cosmic::cosmic_theme::util::CssColor;
use cosmic::iced::wayland::actions::layer_surface::{IcedMargin, SctkLayerSurfaceSettings};
use cosmic::iced::wayland::layer_surface::{
    destroy_layer_surface, get_layer_surface, Anchor, KeyboardInteractivity,
};
use cosmic::iced::wayland::InitialSurface;
use cosmic::iced::widget::{container, horizontal_space, image, text, Column};
use cosmic::iced::{self, Application, Command, Length, Limits, Subscription};
use cosmic::iced_runtime::core::window::Id as SurfaceId;
use cosmic::iced_style::{application, button::StyleSheet};
use cosmic::iced_widget::{column, row, vertical_space};
use cosmic::theme::Button;
use cosmic::widget::{button, icon};
use cosmic::{settings, Element, Theme};
use cosmic_notifications_util::{AppletEvent, CloseReason, Notification};
use iced::wayland::Appearance;
use iced::{Alignment, Color};
use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::info;

const WINDOW_ID: SurfaceId = SurfaceId(1);

pub fn run() -> cosmic::iced::Result {
    let mut settings = settings();
    settings.exit_on_close_request = false;
    settings.initial_surface = InitialSurface::None;
    CosmicNotifications::run(settings)
}

#[derive(Default)]
struct CosmicNotifications {
    active_notifications: Vec<Notification>,
    theme: Theme,
    notifications_tx: Option<mpsc::Sender<notifications::Input>>,
    panel_tx: Option<mpsc::Sender<panel::Input>>,
}

fn theme() -> Theme {
    let Ok(helper) = cosmic::cosmic_config::Config::new(
        cosmic::cosmic_theme::NAME,
        cosmic::cosmic_theme::Theme::<CssColor>::version(),
    ) else {
        return cosmic::theme::Theme::dark();
    };
    let t = cosmic::cosmic_theme::Theme::get_entry(&helper)
        .map(|t| t.into_srgba())
        .unwrap_or_else(|(errors, theme)| {
            for err in errors {
                tracing::error!("{:?}", err);
            }
            theme.into_srgba()
        });
    cosmic::theme::Theme::custom(Arc::new(t))
}

#[derive(Debug, Clone)]
enum Message {
    ActivateNotification(u32),
    Dismissed(u32),
    Notification(notifications::Event),
    Panel(panel::Event),
    ClosedSurface(SurfaceId),
    Theme(Theme),
    Timeout(u32),
}

impl CosmicNotifications {
    fn close(&mut self, i: usize, reason: CloseReason) -> Command<Message> {
        info!("Closing notification");
        let notification = self.active_notifications.remove(i);

        if let Some(ref sender) = &self.notifications_tx {
            let _res = sender.blocking_send(notifications::Input::Closed(notification.id, reason));
        }

        if self.active_notifications.is_empty() {
            info!("Destroying layer surface");
            destroy_layer_surface(WINDOW_ID)
        } else {
            Command::none()
        }
    }

    fn push_notification(&mut self, notification: Notification) -> Command<Message> {
        info!("Pushing notification");
        let timeout = notification.expire_timeout;
        let mut commands = vec![if notification.urgency() == 2 {
            if timeout > 0 {
                Command::perform(
                    tokio::time::sleep(Duration::from_millis(timeout as u64)),
                    move |_| Message::Timeout(notification.id),
                )
            } else {
                Command::none()
            }
        } else {
            Command::perform(
                tokio::time::sleep(Duration::from_millis(if timeout < 0 {
                    timeout.max(10000) as u64
                } else {
                    5000
                })),
                move |_| Message::Timeout(notification.id),
            )
        }];

        if let Some(ref sender) = &self.panel_tx {
            let sender = sender.clone();
            let notification = notification.clone();
            tokio::spawn(async move {
                sender
                    .send(panel::Input::AppletEvent(AppletEvent::Notification(
                        notification,
                    )))
                    .await
            });
        }

        if self.active_notifications.is_empty() {
            info!("Creating layer surface");
            commands.push(get_layer_surface(SctkLayerSurfaceSettings {
                id: WINDOW_ID,
                anchor: Anchor::TOP,
                exclusive_zone: 0,
                keyboard_interactivity: KeyboardInteractivity::None,
                namespace: "notifications".to_string(),
                margin: IcedMargin {
                    top: 8,
                    right: 8,
                    bottom: 0,
                    left: 0,
                },
                size_limits: Limits::NONE
                    .min_width(300.0)
                    .min_height(1.0)
                    .max_height(1920.0)
                    .max_width(1080.0),
                ..Default::default()
            }))
        };

        self.active_notifications.push(notification);

        // TODO: send to fd

        Command::batch(commands)
    }

    fn replace_notification(&mut self, notification: Notification) -> Command<Message> {
        info!("Replacing notification");
        if let Some(notif) = self
            .active_notifications
            .iter_mut()
            .find(|n| n.id == notification.id)
        {
            *notif = notification;
            if let Some(ref sender) = &self.panel_tx {
                let sender = sender.clone();
                let notification = notif.clone();
                tokio::spawn(async move {
                    sender
                        .send(panel::Input::AppletEvent(AppletEvent::Replace(
                            notification,
                        )))
                        .await
                });
            }
            Command::none()
        } else {
            tracing::error!("Notification not found... pushing instead");
            self.push_notification(notification)
        }
    }
}

impl Application for CosmicNotifications {
    type Message = Message;
    type Theme = Theme;
    type Executor = cosmic::executor::single::Executor;
    type Flags = ();

    fn new(_flags: ()) -> (Self, Command<Message>) {
        (
            CosmicNotifications {
                theme: theme(),
                ..Default::default()
            },
            Command::none(),
        )
    }

    fn title(&self) -> String {
        config::APP_ID.to_string()
    }

    #[allow(clippy::too_many_lines)]
    fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::Theme(t) => {
                self.theme = t;
            }
            // TODO
            Message::ActivateNotification(_) => {}
            Message::Notification(e) => match e {
                notifications::Event::Ready(tx) => {
                    self.notifications_tx = Some(tx);
                }
                notifications::Event::Notification(n) => {
                    return self.push_notification(n);
                }
                notifications::Event::Replace(n) => {
                    return self.replace_notification(n);
                }
                notifications::Event::CloseNotification(id) => {
                    if let Some(i) = self.active_notifications.iter().position(|n| n.id == id) {
                        return self.close(i, CloseReason::CloseNotification);
                    }
                }
            },

            Message::Dismissed(id) => {
                if let Some(i) = self.active_notifications.iter().position(|n| n.id == id) {
                    return self.close(i, CloseReason::Dismissed);
                }
            }
            Message::ClosedSurface(id) => {
                if id == WINDOW_ID {
                    self.active_notifications.clear();
                }
            }
            Message::Timeout(id) => {
                if let Some(i) = self.active_notifications.iter().position(|n| n.id == id) {
                    // Note: we want to persist notifications but stop showing them
                    self.active_notifications.remove(i);
                }
            }
            Message::Panel(panel::Event::Ready(tx)) => {
                self.panel_tx = Some(tx);
            }
        }
        Command::none()
    }

    #[allow(clippy::too_many_lines)]
    fn view(&self, _: SurfaceId) -> Element<Message> {
        if self.active_notifications.is_empty() {
            return container(vertical_space(Length::Fixed(1.0)))
                .width(Length::Fixed(1.0))
                .height(Length::Fixed(1.0))
                .center_x()
                .center_y()
                .into();
        }

        let mut notifs = Vec::with_capacity(self.active_notifications.len());

        for n in &self.active_notifications {
            let summary = text(if n.summary.len() > 24 {
                Cow::from(format!(
                    "{:.26}...",
                    n.summary.lines().next().unwrap_or_default()
                ))
            } else {
                Cow::from(&n.summary)
            })
            .size(18);
            let urgency = n.urgency();

            notifs.push(
                button(Button::Custom {
                    active: Box::new(move |t| {
                        let style = if urgency > 1 {
                            Button::Primary
                        } else {
                            Button::Secondary
                        };
                        let cosmic = t.cosmic();
                        let mut a = t.active(&style);
                        a.border_radius = 8.0.into();
                        a.background = Some(Color::from(cosmic.bg_color()).into());
                        a.border_color = Color::from(cosmic.bg_divider());
                        a.border_width = 1.0;
                        a
                    }),
                    hover: Box::new(move |t| {
                        let style = if urgency > 1 {
                            Button::Primary
                        } else {
                            Button::Secondary
                        };
                        let cosmic = t.cosmic();
                        let mut a = t.hovered(&style);
                        a.border_radius = 8.0.into();
                        a.background = Some(Color::from(cosmic.bg_color()).into());
                        a.border_color = Color::from(cosmic.bg_divider());
                        a.border_width = 1.0;
                        a
                    }),
                })
                .custom(vec![column!(
                    match n.image() {
                        Some(cosmic_notifications_util::Image::File(path)) => {
                            row![icon(path.as_path(), 32), summary]
                                .spacing(8)
                                .align_items(Alignment::Center)
                        }
                        Some(cosmic_notifications_util::Image::Name(name)) => {
                            row![icon(name.as_str(), 32), summary]
                                .spacing(8)
                                .align_items(Alignment::Center)
                        }
                        Some(cosmic_notifications_util::Image::Data {
                            width,
                            height,
                            data,
                        }) => {
                            let handle = image::Handle::from_pixels(*width, *height, data.clone());
                            row![icon(handle, 32), summary]
                                .spacing(8)
                                .align_items(Alignment::Center)
                        }
                        None => row![summary],
                    },
                    text(if n.body.len() > 38 {
                        Cow::from(format!(
                            "{:.40}...",
                            n.body.lines().next().unwrap_or_default()
                        ))
                    } else {
                        Cow::from(&n.summary)
                    })
                    .size(14),
                    horizontal_space(Length::Fixed(300.0)),
                )
                .spacing(8)
                .into()])
                .on_press(Message::Dismissed(n.id))
                .into(),
            );
        }

        container(
            Column::with_children(notifs)
                .spacing(8)
                .width(Length::Shrink)
                .height(Length::Shrink),
        )
        .width(Length::Shrink)
        .height(Length::Shrink)
        .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch(
            vec![
                config_subscription::<u64, cosmic::cosmic_theme::Theme<CssColor>>(
                    0,
                    cosmic::cosmic_theme::NAME.into(),
                    cosmic::cosmic_theme::Theme::<CssColor>::version(),
                )
                .map(|(_, res)| {
                    let theme = res
                        .map(cosmic::cosmic_theme::Theme::into_srgba)
                        .unwrap_or_else(|(errors, theme)| {
                            for err in errors {
                                tracing::error!("{:?}", err);
                            }
                            theme.into_srgba()
                        });
                    Message::Theme(cosmic::theme::Theme::custom(Arc::new(theme)))
                }),
                notifications::notifications().map(Message::Notification),
                panel::panel().map(Message::Panel),
            ]
            .into_iter(),
        )
    }

    fn style(&self) -> <Self::Theme as application::StyleSheet>::Style {
        <Self::Theme as application::StyleSheet>::Style::Custom(Box::new(|theme| Appearance {
            background_color: Color::from_rgba(0.0, 0.0, 0.0, 0.0),
            text_color: theme.cosmic().on_bg_color().into(),
        }))
    }

    fn theme(&self) -> Theme {
        self.theme.clone()
    }

    fn close_requested(&self, id: SurfaceId) -> Self::Message {
        Message::ClosedSurface(id)
    }
}
