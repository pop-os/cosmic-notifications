use crate::config;
use crate::subscriptions::notifications;
use cosmic::cosmic_config::{config_subscription, Config, CosmicConfigEntry};
use cosmic::cosmic_theme::util::CssColor;
use cosmic::iced::wayland::actions::layer_surface::{IcedMargin, SctkLayerSurfaceSettings};
use cosmic::iced::wayland::layer_surface::{
    destroy_layer_surface, get_layer_surface, Anchor, KeyboardInteractivity,
};
use cosmic::iced::wayland::InitialSurface;
use cosmic::iced::widget::{container, image, text, Column};
use cosmic::iced::{self, Application, Command, Length, Limits, Subscription};
use cosmic::iced_runtime::core::window::Id as SurfaceId;
use cosmic::iced_style::{application, button::StyleSheet};
use cosmic::iced_widget::{column, row, vertical_space};
use cosmic::theme::Button;
use cosmic::widget::icon;
use cosmic::{settings, Element, Theme};
use cosmic_notifications_config::NotificationsConfig;
use cosmic_notifications_util::{CloseReason, Notification};
use iced::wayland::Appearance;
use iced::{Alignment, Color};
use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{info, warn};

const WINDOW_ID: SurfaceId = SurfaceId(1);

pub fn run() -> cosmic::iced::Result {
    let mut settings = settings();
    settings.exit_on_close_request = false;
    settings.initial_surface = InitialSurface::None;
    CosmicNotifications::run(settings)
}

#[derive(Default)]
struct CosmicNotifications {
    active_surface: bool,
    active_notifications: Vec<Notification>,
    theme: Theme,
    notifications_tx: Option<mpsc::Sender<notifications::Input>>,
    config: NotificationsConfig,
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
    ClosedSurface(SurfaceId),
    Theme(Theme),
    Timeout(u32),
    Config(NotificationsConfig),
}

impl CosmicNotifications {
    fn close(&mut self, i: u32, reason: CloseReason) -> Option<Command<Message>> {
        let Some(i) = self
            .active_notifications
            .iter()
            .position(|n| n.id == i) else {
                warn!("Notification not found for id {i}");
            return None;
        };

        let notification = self.active_notifications.remove(i);

        if let Some(ref sender) = &self.notifications_tx {
            if !matches!(reason, CloseReason::Expired) {
                let id = notification.id;
                let sender = sender.clone();
                tokio::spawn(async move {
                    let _ = sender.send(notifications::Input::Closed(id, reason));
                });
            }
        }

        if let Some(ref sender) = &self.notifications_tx {
            if !matches!(reason, CloseReason::Expired) {
                let sender = sender.clone();
                let id = notification.id;
                tokio::spawn(async move { sender.send(notifications::Input::Dismissed(id)).await });
            }
        }

        if self.active_notifications.is_empty() && self.active_surface {
            self.active_surface = false;
            info!("Destroying layer surface");
            Some(destroy_layer_surface(WINDOW_ID))
        } else {
            Some(Command::none())
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

        if self.active_notifications.is_empty() && !self.config.do_not_disturb {
            info!("Creating layer surface");
            self.active_surface = true;
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
        let helper = Config::new(
            cosmic_notifications_config::ID,
            NotificationsConfig::version(),
        )
        .ok();

        let config: NotificationsConfig = helper
            .as_ref()
            .map(|helper| {
                NotificationsConfig::get_entry(helper).unwrap_or_else(|(errors, config)| {
                    for err in errors {
                        tracing::error!("{:?}", err);
                    }
                    config
                })
            })
            .unwrap_or_default();
        (
            CosmicNotifications {
                theme: theme(),
                config,
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
                    if let Some(c) = self.close(id, CloseReason::CloseNotification) {
                        return c;
                    }
                }
            },

            Message::Dismissed(id) => {
                if let Some(c) = self.close(id, CloseReason::Dismissed) {
                    return c;
                }
            }
            Message::ClosedSurface(id) => {
                if id == WINDOW_ID {
                    self.active_notifications.clear();
                }
            }
            Message::Timeout(id) => {
                if let Some(c) = self.close(id, CloseReason::Expired) {
                    return c;
                }
            }
            Message::Config(config) => {
                self.config = config;
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

        for n in self.active_notifications.iter().rev() {
            let app_name = text(if n.app_name.len() > 24 {
                Cow::from(format!(
                    "{:.26}...",
                    n.app_name.lines().next().unwrap_or_default()
                ))
            } else {
                Cow::from(&n.app_name)
            })
            .size(12);
            let urgency = n.urgency();

            notifs.push(
                cosmic::widget::button(Button::Custom {
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
                            row![icon(path.as_path(), 16), app_name]
                                .spacing(8)
                                .align_items(Alignment::Center)
                        }
                        Some(cosmic_notifications_util::Image::Name(name)) => {
                            row![icon(name.as_str(), 16), app_name]
                                .spacing(8)
                                .align_items(Alignment::Center)
                        }
                        Some(cosmic_notifications_util::Image::Data {
                            width,
                            height,
                            data,
                        }) => {
                            let handle = image::Handle::from_pixels(*width, *height, data.clone());
                            row![icon(handle, 16), app_name]
                                .spacing(8)
                                .align_items(Alignment::Center)
                        }
                        None => row![app_name],
                    },
                    text(if n.summary.len() > 77 {
                        Cow::from(format!(
                            "{:.80}...",
                            n.summary.lines().next().unwrap_or_default()
                        ))
                    } else {
                        Cow::from(&n.summary)
                    })
                    .size(14)
                    .width(Length::Fixed(300.0)),
                    text(if n.body.len() > 77 {
                        Cow::from(format!(
                            "{:.80}...",
                            n.body.lines().next().unwrap_or_default()
                        ))
                    } else {
                        Cow::from(&n.body)
                    })
                    .size(12)
                    .width(Length::Fixed(300.0)),
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
                .height(Length::Shrink)
                .align_items(Alignment::Center),
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
                config_subscription::<u64, NotificationsConfig>(
                    0,
                    cosmic_notifications_config::ID.into(),
                    NotificationsConfig::version(),
                )
                .map(|(_, res)| match res {
                    Ok(config) => Message::Config(config),
                    Err((errors, config)) => {
                        for err in errors {
                            tracing::error!("{:?}", err);
                        }
                        Message::Config(config)
                    }
                }),
                notifications::notifications().map(Message::Notification),
                // applet::panel().map(Message::Panel),
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
