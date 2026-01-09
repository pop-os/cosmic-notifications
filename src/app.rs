use crate::subscriptions::notifications;
use cosmic::app::{Core, Settings};
use cosmic::cosmic_config::{Config, CosmicConfigEntry};
use cosmic::iced::platform_specific::runtime::wayland::layer_surface::{
    IcedMargin, IcedOutput, SctkLayerSurfaceSettings,
};
use cosmic::iced::platform_specific::shell::wayland::commands::{
    activation,
    layer_surface::{Anchor, KeyboardInteractivity, destroy_layer_surface, get_layer_surface},
};
use cosmic::iced::{self, Length, Limits, Subscription};
use cosmic::iced_runtime::core::window::Id as SurfaceId;
use cosmic::iced_widget::{column, row, vertical_space};
use cosmic::surface;
use cosmic::widget::{autosize, button, container, icon, text};
use cosmic::{Application, Element, app::Task};
use cosmic_notifications_config::NotificationsConfig;
use cosmic_notifications_util::{ActionId, CloseReason, Notification};
use cosmic_panel_config::{CosmicPanelConfig, CosmicPanelOuput, PanelAnchor};
use cosmic_time::{Instant, Timeline, anim, id};
use iced::Alignment;
use std::borrow::Cow;
use std::collections::VecDeque;
use std::time::Duration;
use tokio::sync::mpsc;

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

struct CosmicNotifications {
    core: Core,
    active_surface: bool,
    autosize_id: iced::id::Id,
    window_id: SurfaceId,
    cards: Vec<Notification>,
    hidden: VecDeque<Notification>,
    notifications_id: id::Cards,
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
    ActivationToken(Option<String>, u32, Option<ActionId>),
    Dismissed(u32),
    Notification(notifications::Event),
    Timeout(u32),
    Config(NotificationsConfig),
    PanelConfig(CosmicPanelConfig),
    DockConfig(CosmicPanelConfig),
    Frame(Instant),
    Ignore,
    Surface(surface::Action),
}

impl CosmicNotifications {
    fn expire(&mut self, i: u32) {
        let Some((c_pos, _)) = self.cards.iter().enumerate().find(|(_, n)| n.id == i) else {
            return;
        };

        let notification = self.cards.remove(c_pos);
        self.sort_notifications();
        self.group_notifications();
        self.hidden.push_front(notification);
        self.hidden.truncate(200);
    }

    fn close(&mut self, i: u32, reason: CloseReason) -> Option<Task<Message>> {
        tracing::error!("closed {i}");
        tracing::error!("{:?}", self.cards);
        let c_pos = self.cards.iter().position(|n| n.id == i);
        let notification = c_pos.map(|c_pos| self.cards.remove(c_pos)).or_else(|| {
            self.hidden
                .iter()
                .position(|n| n.id == i)
                .and_then(|pos| self.hidden.remove(pos))
        })?;

        tracing::error!("closed {c_pos:?}");

        if self.cards.is_empty() {
            self.cards.shrink_to(50);
        }
        tracing::error!("closed {:?}", self.cards.is_empty());

        self.sort_notifications();
        self.group_notifications();
        if let Some(sender) = &self.notifications_tx {
            let id = notification.id;
            let sender = sender.clone();
            tokio::spawn(async move {
                _ = sender.send(notifications::Input::Closed(id, reason));
            });
        }

        if let Some(sender) = &self.notifications_tx {
            let sender = sender.clone();
            let id = notification.id;
            tokio::spawn(async move { sender.send(notifications::Input::Dismissed(id)).await });
        }

        if self.cards.is_empty() && self.active_surface {
            self.active_surface = false;
            Some(destroy_layer_surface(self.window_id))
        } else {
            Some(Task::none())
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
    ) -> Task<<CosmicNotifications as cosmic::app::Application>::Message> {
        let mut timeout = u32::try_from(notification.expire_timeout).unwrap_or(3000);
        let max_timeout = if notification.urgency() == 2 {
            self.config.max_timeout_urgent
        } else if notification.urgency() == 1 {
            self.config.max_timeout_normal
        } else {
            self.config.max_timeout_low
        }
        .unwrap_or(u32::try_from(notification.expire_timeout).unwrap_or(3000));
        timeout = timeout.min(max_timeout);

        let mut tasks = vec![if timeout > 0 {
            iced::Task::perform(
                tokio::time::sleep(Duration::from_millis(timeout as u64)),
                move |_| cosmic::action::app(Message::Timeout(notification.id)),
            )
        } else {
            iced::Task::none()
        }];

        if self.cards.is_empty() && !self.config.do_not_disturb {
            let (anchor, _output) = self.anchor.clone().unwrap_or((Anchor::TOP, None));
            self.active_surface = true;
            tasks.push(get_layer_surface(SctkLayerSurfaceSettings {
                id: self.window_id,
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
                size: Some((Some(300), Some(1))),
                output: IcedOutput::Active, // TODO should we only create the notification on the output the applet is on?
                size_limits: Limits::NONE
                    .min_width(300.0)
                    .min_height(1.0)
                    .max_height(1920.0)
                    .max_width(300.0),
                ..Default::default()
            }));
        };

        self.sort_notifications();

        let mut insert_sorted =
            |notification: Notification| match self.cards.binary_search_by(|a| {
                match a.urgency().cmp(&notification.urgency()) {
                    std::cmp::Ordering::Equal => a.time.cmp(&notification.time),
                    other => other,
                }
            }) {
                Ok(pos) => {
                    self.cards[pos] = notification;
                }
                Err(pos) => {
                    self.cards.insert(pos, notification);
                }
            };
        insert_sorted(notification);
        self.group_notifications();

        iced::Task::batch(tasks)
    }

    fn group_notifications(&mut self) {
        if self.config.max_per_app == 0 {
            return;
        }

        let mut extra_per_app = Vec::new();
        let mut cur_count = 0;
        let Some(mut cur_id) = self.cards.first().map(|n| n.app_name.clone()) else {
            return;
        };
        self.cards = self
            .cards
            .drain(..)
            .filter(|n| {
                if n.app_name == cur_id {
                    cur_count += 1;
                } else {
                    cur_count = 1;
                    cur_id = n.app_name.clone();
                }
                if cur_count > self.config.max_per_app {
                    extra_per_app.push(n.clone());
                    false
                } else {
                    true
                }
            })
            .collect();

        for n in extra_per_app {
            if self.cards.len() < self.config.max_notifications as usize {
                self.insert_sorted(n);
            } else {
                self.cards.push(n);
            }
        }
    }

    fn insert_sorted(&mut self, notification: Notification) {
        match self
            .cards
            .binary_search_by(|a| match notification.urgency().cmp(&a.urgency()) {
                std::cmp::Ordering::Equal => notification.time.cmp(&a.time),
                other => other,
            }) {
            Ok(pos) => {
                self.cards[pos] = notification;
            }
            Err(pos) => {
                self.cards.insert(pos, notification);
            }
        }
    }

    fn sort_notifications(&mut self) {
        self.cards
            .sort_by(|a, b| match a.urgency().cmp(&b.urgency()) {
                std::cmp::Ordering::Equal => a.time.cmp(&b.time),
                other => other,
            });
    }

    fn replace_notification(&mut self, notification: Notification) -> Task<Message> {
        if let Some(notif) = self.cards.iter_mut().find(|n| n.id == notification.id) {
            *notif = notification;
            Task::none()
        } else {
            tracing::error!("Notification not found... pushing instead");
            self.push_notification(notification)
        }
    }

    fn request_activation(&mut self, i: u32, action: Option<ActionId>) -> Task<Message> {
        activation::request_token(Some(String::from(Self::APP_ID)), Some(self.window_id)).map(
            move |token| cosmic::Action::App(Message::ActivationToken(token, i, action.clone())),
        )
    }

    fn activate_notification(
        &mut self,
        token: String,
        id: u32,
        action: Option<ActionId>,
    ) -> Option<Task<Message>> {
        if let Some(tx) = self.notifications_tx.as_ref() {
            let c_pos = self.cards.iter().position(|n| n.id == id);
            let notification = c_pos.map(|c_pos| &self.cards[c_pos]).or_else(|| {
                self.hidden
                    .iter()
                    .position(|n| n.id == id)
                    .map(|pos| &self.hidden[pos])
            })?;

            let maybe_action = if action
                .as_ref()
                .is_some_and(|a| notification.actions.iter().any(|(b, _)| b == a))
            {
                action.clone().map(|a| a.to_string())
            } else if notification
                .actions
                .iter()
                .any(|a| matches!(a.0, ActionId::Default))
            {
                Some(ActionId::Default.to_string())
            } else {
                notification.actions.first().map(|a| a.0.to_string())
            };

            let Some(action) = maybe_action else {
                return self.close(id, CloseReason::Dismissed);
            };
            let tx = tx.clone();
            tracing::error!("action for {id} {action}");
            return Some(Task::future(async move {
                _ = tx
                    .send(notifications::Input::Activated { token, id, action })
                    .await;
                tracing::trace!("sent action to sub");
                cosmic::Action::App(Message::Dismissed(id))
            }));
        } else {
            tracing::error!("Failed to activate notification. No channel.");
            None
        }
    }
}

impl cosmic::Application for CosmicNotifications {
    type Message = Message;
    type Executor = cosmic::executor::single::Executor;
    type Flags = ();
    const APP_ID: &'static str = "com.system76.CosmicNotifications";

    fn init(core: Core, _flags: ()) -> (Self, Task<Message>) {
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
                        if err.is_err() {
                            tracing::error!("{:?}", err);
                        }
                    }
                    config
                })
            })
            .unwrap_or_default();
        (
            CosmicNotifications {
                core,
                active_surface: false,
                autosize_id: iced::id::Id::new("autosize"),
                window_id: SurfaceId::unique(),
                anchor: None,
                config,
                dock_config: CosmicPanelConfig::default(),
                panel_config: CosmicPanelConfig::default(),
                notifications_id: id::Cards::new("Notifications"),
                notifications_tx: None,
                timeline: Timeline::new(),
                cards: Vec::with_capacity(50),
                hidden: VecDeque::new(),
            },
            Task::none(),
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
    fn update(&mut self, message: Message) -> Task<Self::Message> {
        match message {
            Message::ActivateNotification(id) => {
                tracing::trace!("requesting token for {id}");
                return self.request_activation(id, None);
            }
            Message::ActivationToken(token, id, action) => {
                tracing::trace!("token for {id}");
                if let Some(token) = token {
                    return self
                        .activate_notification(token, id, action)
                        .unwrap_or(Task::none());
                } else {
                    tracing::error!("Failed to get activation token for clicked notification.");
                }
            }
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
                notifications::Event::AppletActivated { id, action } => {
                    tracing::trace!("requesting token for {id}");
                    return self.request_activation(id, Some(action));
                }
            },
            Message::Dismissed(id) => {
                if let Some(c) = self.close(id, CloseReason::Dismissed) {
                    return c;
                }
            }
            Message::Timeout(id) => {
                self.expire(id);
                if self.cards.is_empty() && self.active_surface {
                    self.active_surface = false;
                    return destroy_layer_surface(self.window_id);
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
            Message::Surface(a) => {
                return cosmic::task::message(cosmic::Action::Cosmic(
                    cosmic::app::Action::Surface(a),
                ));
            }
        }
        Task::none()
    }

    #[allow(clippy::too_many_lines)]
    fn view_window(&self, _: SurfaceId) -> Element<Message> {
        if self.cards.is_empty() {
            return container(vertical_space().height(Length::Fixed(1.0)))
                .center_x(Length::Fixed(1.0))
                .center_y(Length::Fixed(1.0))
                .into();
        }

        let (ids, notif_elems): (Vec<_>, Vec<_>) = self
            .cards
            .iter()
            .rev()
            .map(|n| {
                let app_name = text::caption(if n.app_name.len() > 24 {
                    Cow::from(format!(
                        "{:.26}...",
                        n.app_name.lines().next().unwrap_or_default()
                    ))
                } else {
                    Cow::from(&n.app_name)
                })
                .width(Length::Fill);

                let close_notif = button::custom(
                    icon::from_name("window-close-symbolic")
                        .size(16)
                        .symbolic(true),
                )
                .on_press(Message::Dismissed(n.id))
                .class(cosmic::theme::Button::Text);
                let e = Element::from(
                    column!(
                        if let Some(icon) = n.notification_icon() {
                            row![icon.size(16), app_name, close_notif]
                                .spacing(8)
                                .align_y(Alignment::Center)
                        } else {
                            row![app_name, close_notif]
                                .spacing(8)
                                .align_y(Alignment::Center)
                        },
                        column![
                            text::body(n.summary.lines().next().unwrap_or_default())
                                .width(Length::Fill),
                            text::caption(n.body.lines().next().unwrap_or_default())
                                .width(Length::Fill)
                        ]
                    )
                    .width(Length::Fill),
                );
                (n.id, e)
            })
            .take(self.config.max_notifications as usize)
            .unzip();

        let card_list = anim!(
            //cards
            self.notifications_id.clone(),
            &self.timeline,
            notif_elems,
            Message::Ignore,
            None::<fn(cosmic_time::chain::Cards, bool) -> Message>,
            Some(move |id| Message::ActivateNotification(ids[id])),
            "",
            "",
            "",
            None,
            true,
        )
        .width(Length::Fixed(300.));

        autosize::autosize(card_list, self.autosize_id.clone())
            .min_width(200.)
            .min_height(100.)
            .max_width(300.)
            .max_height(1920.)
            .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch(vec![
            self.core
                .watch_config(cosmic_notifications_config::ID)
                .map(|u| {
                    for why in u
                        .errors
                        .into_iter()
                        .filter(cosmic::cosmic_config::Error::is_err)
                    {
                        tracing::error!(?why, "config load error");
                    }
                    Message::Config(u.config)
                }),
            self.core
                .watch_config("com.system76.CosmicPanel.Panel")
                .map(|u| {
                    for why in u
                        .errors
                        .into_iter()
                        .filter(cosmic::cosmic_config::Error::is_err)
                    {
                        tracing::error!(?why, "panel config load error");
                    }
                    Message::PanelConfig(u.config)
                }),
            self.core
                .watch_config("com.system76.CosmicPanel.Dock")
                .map(|u| {
                    for why in u
                        .errors
                        .into_iter()
                        .filter(cosmic::cosmic_config::Error::is_err)
                    {
                        tracing::error!(?why, "dock config load error");
                    }
                    Message::DockConfig(u.config)
                }),
            self.timeline
                .as_subscription()
                .map(|(_, now)| Message::Frame(now)),
            notifications::notifications().map(Message::Notification),
        ])
    }
}
