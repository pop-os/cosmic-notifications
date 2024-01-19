use crate::subscriptions::notifications;
use cosmic::app::{Core, Settings};
use cosmic::cosmic_config::{Config, CosmicConfigEntry};
use cosmic::iced::wayland::actions::layer_surface::{
    IcedMargin, IcedOutput, SctkLayerSurfaceSettings,
};
use cosmic::iced::wayland::layer_surface::{
    destroy_layer_surface, get_layer_surface, Anchor, KeyboardInteractivity,
};
use cosmic::iced::widget::{container, text, Column};
use cosmic::iced::{self, Length, Limits, Subscription};
use cosmic::iced_runtime::core::window::Id as SurfaceId;
use cosmic::iced_style::application;
use cosmic::iced_widget::{column, row, vertical_space};
use cosmic::widget::button;
use cosmic::widget::icon;
use cosmic::{app::Command, Element, Theme};
use cosmic_notifications_config::NotificationsConfig;
use cosmic_notifications_util::{CloseReason, Notification};
use cosmic_panel_config::{CosmicPanelConfig, CosmicPanelOuput, PanelAnchor};
use cosmic_time::{anim, id, Instant, Timeline};
use iced::wayland::Appearance;
use iced::{Alignment, Color};
use once_cell::sync::Lazy;
use std::borrow::Cow;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{info, warn};

static WINDOW_ID: Lazy<SurfaceId> = Lazy::new(|| SurfaceId::unique());
static NOTIFICATIONS_APPLET: &str = "com.system76.CosmicAppletNotifications";

pub fn run() -> cosmic::iced::Result {
    cosmic::app::run::<CosmicNotifications>(
        Settings::default()
            .antialiasing(true)
            .client_decorations(true)
            .debug(false)
            .default_text_size(16.0)
            .scale_factor(1.0)
            .no_main_window(true)
            .exit_on_close(false),
        (),
    )?;
    Ok(())
}

#[derive(Default)]
struct CosmicNotifications {
    core: Core,
    active_surface: bool,
    cards: Vec<(id::Cards, Vec<Notification>)>,
    notifications_tx: Option<mpsc::Sender<notifications::Input>>,
    config: NotificationsConfig,
    dock_config: CosmicPanelConfig,
    panel_config: CosmicPanelConfig,
    anchor: Option<(Anchor, Option<String>)>,
    timeline: Timeline,
}

#[derive(Debug, Clone)]
enum Message {
    ActivateNotification(u32),
    Dismissed(u32),
    Notification(notifications::Event),
    Timeout(u32),
    Config(NotificationsConfig),
    PanelConfig(CosmicPanelConfig),
    DockConfig(CosmicPanelConfig),
    Frame(Instant),
    Ignore,
}

impl CosmicNotifications {
    fn close(&mut self, i: u32, reason: CloseReason) -> Option<Command<Message>> {
        let Some((c_pos, j)) = self
            .cards
            .iter()
            .enumerate()
            .find_map(|(c_pos, n)| n.1.iter().position(|n| n.id == i).map(|j| (c_pos, j)))
        else {
            warn!("Notification not found for id {i}");
            return None;
        };

        let notification = self.cards[c_pos].1.remove(j);
        self.cards.retain(|(_, n)| !n.is_empty());

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

        if self.cards.is_empty() && self.active_surface {
            self.active_surface = false;
            info!("Destroying layer surface");
            Some(destroy_layer_surface(WINDOW_ID.clone()))
        } else {
            Some(Command::none())
        }
    }

    fn anchor_for_notification_applet(&self) -> (Anchor, Option<String>) {
        self.panel_config
            .plugins_left()
            .iter()
            .find_map(|p| {
                if p.iter().any(|s| s == NOTIFICATIONS_APPLET) {
                    return Some((
                        match self.panel_config.anchor {
                            PanelAnchor::Top => Anchor::TOP.union(Anchor::LEFT),
                            PanelAnchor::Bottom => Anchor::BOTTOM.union(Anchor::LEFT),
                            PanelAnchor::Left => Anchor::LEFT.union(Anchor::TOP),
                            PanelAnchor::Right => Anchor::RIGHT.union(Anchor::TOP),
                        },
                        match self.panel_config.output {
                            CosmicPanelOuput::Name(ref n) => Some(n.clone()),
                            _ => None,
                        },
                    ));
                }
                None
            })
            .or_else(|| {
                self.panel_config.plugins_right().iter().find_map(|p| {
                    if p.iter().any(|s| s == NOTIFICATIONS_APPLET) {
                        return Some((
                            match self.panel_config.anchor {
                                PanelAnchor::Top => Anchor::TOP.union(Anchor::RIGHT),
                                PanelAnchor::Bottom => Anchor::BOTTOM.union(Anchor::RIGHT),
                                PanelAnchor::Left => Anchor::LEFT.union(Anchor::BOTTOM),
                                PanelAnchor::Right => Anchor::RIGHT.union(Anchor::BOTTOM),
                            },
                            match self.panel_config.output {
                                CosmicPanelOuput::Name(ref n) => Some(n.clone()),
                                _ => None,
                            },
                        ));
                    }
                    None
                })
            })
            .or_else(|| {
                self.panel_config.plugins_center().iter().find_map(|p| {
                    if p.iter().any(|s| s == NOTIFICATIONS_APPLET) {
                        return Some((
                            match self.panel_config.anchor {
                                PanelAnchor::Top => Anchor::TOP,
                                PanelAnchor::Bottom => Anchor::BOTTOM,
                                PanelAnchor::Left => Anchor::LEFT,
                                PanelAnchor::Right => Anchor::RIGHT,
                            },
                            match self.panel_config.output {
                                CosmicPanelOuput::Name(ref n) => Some(n.clone()),
                                _ => None,
                            },
                        ));
                    }
                    None
                })
            })
            .or_else(|| {
                self.dock_config.plugins_left().iter().find_map(|p| {
                    if p.iter().any(|s| s == NOTIFICATIONS_APPLET) {
                        return Some((
                            match self.dock_config.anchor {
                                PanelAnchor::Top => Anchor::TOP.union(Anchor::LEFT),
                                PanelAnchor::Bottom => Anchor::BOTTOM.union(Anchor::LEFT),
                                PanelAnchor::Left => Anchor::LEFT.union(Anchor::TOP),
                                PanelAnchor::Right => Anchor::RIGHT.union(Anchor::TOP),
                            },
                            match self.dock_config.output {
                                CosmicPanelOuput::Name(ref n) => Some(n.clone()),
                                _ => None,
                            },
                        ));
                    }
                    None
                })
            })
            .or_else(|| {
                self.dock_config.plugins_right().iter().find_map(|p| {
                    if p.iter().any(|s| s == NOTIFICATIONS_APPLET) {
                        return Some((
                            match self.dock_config.anchor {
                                PanelAnchor::Top => Anchor::TOP.union(Anchor::RIGHT),
                                PanelAnchor::Bottom => Anchor::BOTTOM.union(Anchor::RIGHT),
                                PanelAnchor::Left => Anchor::TOP.union(Anchor::BOTTOM),
                                PanelAnchor::Right => Anchor::RIGHT.union(Anchor::BOTTOM),
                            },
                            match self.dock_config.output {
                                CosmicPanelOuput::Name(ref n) => Some(n.clone()),
                                _ => None,
                            },
                        ));
                    }
                    None
                })
            })
            .or_else(|| {
                self.dock_config.plugins_center().iter().find_map(|p| {
                    if p.iter().any(|s| s == NOTIFICATIONS_APPLET) {
                        return Some((
                            match self.dock_config.anchor {
                                PanelAnchor::Top => Anchor::TOP,
                                PanelAnchor::Bottom => Anchor::BOTTOM,
                                PanelAnchor::Left => Anchor::LEFT,
                                PanelAnchor::Right => Anchor::RIGHT,
                            },
                            match self.dock_config.output {
                                CosmicPanelOuput::Name(ref n) => Some(n.clone()),
                                _ => None,
                            },
                        ));
                    }
                    None
                })
            })
            .unwrap_or((Anchor::TOP, None))
    }

    fn push_notification(
        &mut self,
        notification: Notification,
    ) -> Command<<CosmicNotifications as cosmic::app::Application>::Message> {
        info!("Pushing notification");
        let timeout = notification.expire_timeout;
        let mut commands = vec![if notification.urgency() == 2 {
            if timeout > 0 {
                iced::Command::perform(
                    tokio::time::sleep(Duration::from_millis(timeout as u64)),
                    move |_| cosmic::app::message::app(Message::Timeout(notification.id)),
                )
            } else {
                iced::Command::none()
            }
        } else {
            iced::Command::perform(
                tokio::time::sleep(Duration::from_millis(if timeout < 0 {
                    5000
                } else {
                    timeout.max(10000) as u64
                })),
                move |_| cosmic::app::message::app(Message::Timeout(notification.id)),
            )
        }];

        if self.cards.is_empty() && !self.config.do_not_disturb {
            info!("Creating layer surface");
            let (anchor, _output) = self.anchor.clone().unwrap_or((Anchor::TOP, None));
            self.active_surface = true;
            commands.push(get_layer_surface(SctkLayerSurfaceSettings {
                id: WINDOW_ID.clone(),
                anchor,
                exclusive_zone: 0,
                keyboard_interactivity: KeyboardInteractivity::None,
                namespace: "notifications".to_string(),
                margin: IcedMargin {
                    top: 8,
                    right: 8,
                    bottom: 8,
                    left: 8,
                },
                output: IcedOutput::Active, // TODO should we only create the notification on the output the applet is on?
                size_limits: Limits::NONE
                    .min_width(300.0)
                    .min_height(1.0)
                    .max_height(1920.0)
                    .max_width(300.0),
                ..Default::default()
            }))
        };

        self.cards.push((
            id::Cards::new(notification.app_name.clone()),
            vec![notification],
        ));

        // TODO: send to fd

        iced::Command::batch(commands)
    }

    fn replace_notification(&mut self, notification: Notification) -> Command<Message> {
        info!("Replacing notification");
        if let Some(notif) = self
            .cards
            .iter_mut()
            .find_map(|n| n.1.iter_mut().find(|n| n.id == notification.id))
        {
            *notif = notification;
            Command::none()
        } else {
            tracing::error!("Notification not found... pushing instead");
            self.push_notification(notification)
        }
    }
}

impl cosmic::Application for CosmicNotifications {
    type Message = Message;
    type Executor = cosmic::executor::single::Executor;
    type Flags = ();
    const APP_ID: &'static str = "com.system76.CosmicNotifications";

    fn init(core: Core, _flags: ()) -> (Self, Command<Message>) {
        let helper = Config::new(
            cosmic_notifications_config::ID,
            NotificationsConfig::VERSION,
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
                core,
                config,
                ..Default::default()
            },
            Command::none(),
        )
    }

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn view(&self) -> Element<Self::Message> {
        unimplemented!();
    }

    #[allow(clippy::too_many_lines)]
    fn update(&mut self, message: Message) -> Command<Self::Message> {
        match message {
            // TODO
            Message::ActivateNotification(_) => {}
            Message::Notification(e) => match e {
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
                notifications::Event::Ready(tx) => {
                    self.notifications_tx = Some(tx);
                }
            },

            Message::Dismissed(id) => {
                if let Some(c) = self.close(id, CloseReason::Dismissed) {
                    return c;
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
            Message::PanelConfig(c) => {
                self.panel_config = c;
                self.anchor = Some(self.anchor_for_notification_applet());
            }
            Message::DockConfig(c) => {
                self.dock_config = c;
                self.anchor = Some(self.anchor_for_notification_applet());
            }
            Message::Frame(now) => {
                self.timeline.now(now);
            }
            Message::Ignore => {}
        }
        Command::none()
    }

    #[allow(clippy::too_many_lines)]
    fn view_window(&self, _: SurfaceId) -> Element<Message> {
        if self.cards.is_empty() {
            return container(vertical_space(Length::Fixed(1.0)))
                .width(Length::Fixed(1.0))
                .height(Length::Fixed(1.0))
                .center_x()
                .center_y()
                .into();
        }

        let mut notifs: Vec<Element<_>> = Vec::with_capacity(self.cards.len());

        for c in self.cards.iter().rev() {
            if c.1.is_empty() {
                continue;
            }
            let notif_elems: Vec<_> =
                c.1.iter()
                    .rev()
                    .map(|n| {
                        let app_name = text(if n.app_name.len() > 24 {
                            Cow::from(format!(
                                "{:.26}...",
                                n.app_name.lines().next().unwrap_or_default()
                            ))
                        } else {
                            Cow::from(&n.app_name)
                        })
                        .size(12)
                        .width(Length::Fill);

                        let close_notif = button(
                            icon::from_name("window-close-symbolic")
                                .size(16)
                                .symbolic(true),
                        )
                        .on_press(Message::Dismissed(n.id))
                        .style(cosmic::theme::Button::Text);
                        Element::from(
                            column!(
                                match n.image() {
                                    Some(cosmic_notifications_util::Image::File(path)) => {
                                        row![
                                            icon::from_path(PathBuf::from(path)).icon().size(16),
                                            app_name,
                                            close_notif
                                        ]
                                        .spacing(8)
                                        .align_items(Alignment::Center)
                                    }
                                    Some(cosmic_notifications_util::Image::Name(name)) => {
                                        row![
                                            icon::from_name(name.as_str()).size(16),
                                            app_name,
                                            close_notif
                                        ]
                                        .spacing(8)
                                        .align_items(Alignment::Center)
                                    }
                                    Some(cosmic_notifications_util::Image::Data {
                                        width,
                                        height,
                                        data,
                                    }) => {
                                        row![
                                            icon::from_raster_pixels(*width, *height, data.clone())
                                                .icon()
                                                .size(16),
                                            app_name,
                                            close_notif
                                        ]
                                        .spacing(8)
                                        .align_items(Alignment::Center)
                                    }
                                    None => row![app_name, close_notif]
                                        .spacing(8)
                                        .align_items(Alignment::Center),
                                },
                                column![
                                    text(n.summary.lines().next().unwrap_or_default())
                                        .width(Length::Fill)
                                        .size(14),
                                    text(n.body.lines().next().unwrap_or_default())
                                        .width(Length::Fill)
                                        .size(12)
                                ]
                            )
                            .width(Length::Fill),
                        )
                    })
                    .collect();

            let card_list = anim!(
                //cards
                c.0.clone(),
                &self.timeline,
                notif_elems,
                Message::Ignore,
                move |_, _| Message::Ignore,
                "",
                "",
                "",
                None,
                true,
            );
            notifs.push(card_list.into());
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
                self.core
                    .watch_config(cosmic_notifications_config::ID.into())
                    .map(|u| {
                        for why in u.errors {
                            tracing::error!(?why, "config load error");
                        }
                        Message::Config(u.config)
                    }),
                self.core
                    .watch_config("com.system76.CosmicPanel.Panel".into())
                    .map(|u| {
                        for why in u.errors {
                            tracing::error!(?why, "panel config load error");
                        }
                        Message::PanelConfig(u.config)
                    }),
                self.core
                    .watch_config("com.system76.CosmicPanel.Dock".into())
                    .map(|u| {
                        for why in u.errors {
                            tracing::error!(?why, "dock config load error");
                        }
                        Message::DockConfig(u.config)
                    }),
                self.timeline
                    .as_subscription()
                    .map(|(_, now)| Message::Frame(now)),
                notifications::notifications().map(Message::Notification),
                // applet::panel().map(Message::Panel),
            ]
            .into_iter(),
        )
    }

    fn style(&self) -> Option<<Theme as application::StyleSheet>::Style> {
        Some(<Theme as application::StyleSheet>::Style::Custom(Box::new(
            |theme| Appearance {
                background_color: Color::from_rgba(0.0, 0.0, 0.0, 0.0),
                text_color: theme.cosmic().on_bg_color().into(),
                icon_color: theme.cosmic().on_bg_color().into(),
            },
        )))
    }
}
