use crate::util;
use cosmic::{
    iced::{
        futures::{self, SinkExt},
        subscription,
    },
    iced_futures::Subscription,
};
use cosmic_notifications_util::{AppletEvent, PanelRequest};
use std::{fmt::Debug, num::NonZeroU32};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    sync::mpsc::{channel, Receiver, Sender},
};
use tracing::warn;

#[derive(Debug)]
pub enum State {
    Starting,
    Waiting(UnixStream, Vec<UnixStream>, Receiver<Input>),
    Finished,
}

#[derive(Debug, Clone)]
pub enum Input {
    AppletEvent(AppletEvent),
}

#[derive(Debug, Clone)]
pub enum Event {
    Ready(Sender<Input>),
}

pub fn panel() -> Subscription<Event> {
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
                        match util::setup_panel_socket().await {
                            Ok(s) => {
                                // Send the sender back to the application
                                _ = output.send(Event::Ready(tx)).await;

                                // We are ready to receive messages
                                state = State::Waiting(s, Vec::new(), rx);
                            }
                            Err(err) => {
                                tracing::error!(
                                    "Failed to connect to the socket for the panel server {}",
                                    err
                                );
                                state = State::Finished;
                            }
                        }
                    }
                    State::Waiting(s, applets, rx) => {
                        // Wait for a message or for fd to be readable or we get an input
                        let new_state = tokio::select! {
                            res = s.readable() => {
                                let mut new_state = None;
                                match res {
                                    Err(err) => {
                                        tracing::error!("Failed to wait for socket to be readable {}", err);
                                        new_state = Some(State::Finished);
                                    }
                                    Ok(_) => {
                                        let mut reader = BufReader::new(s);
                                        // todo read messages
                                        let mut request_buf = String::with_capacity(1024);
                                        if let Err(err) = reader.read_line(&mut request_buf).await {
                                            tracing::error!("Failed to read line from socket {}", err);
                                            continue;
                                        }
                                        match ron::de::from_str::<PanelRequest>(request_buf.as_str()) {
                                            Ok(panel_request) => {
                                                match panel_request {
                                                    PanelRequest::Init => {
                                                        applets.clear();
                                                    }
                                                    PanelRequest::NewNotificationsClient{ id } => {
                                                        // todo create new socket, send the fd, and add to applets list
                                                        // applets.push(applet);
                                                    }
                                                }
                                            }
                                            Err(err) => {
                                                tracing::error!("Failed to deserialize panel request {}", err);
                                            }
                                        }
                                    }
                                }
                                new_state
                            },
                            v = rx.recv() => {
                                let mut new_state = None;
                                match v {
                                   Some(Input::AppletEvent(e)) => {
                                        let Ok(event) = ron::to_string(&e) else {
                                            tracing::error!("Failed to serialize applet event {:?}", e);
                                            continue;
                                        };
                                        for a in applets {
                                            if let Err(err) = a.write_all(event.as_bytes()).await {
                                                tracing::error!("Failed to write applet event to socket {}", err);
                                                continue;
                                            }
                                        }
                                    }
                                    _ => {
                                        new_state = Some(State::Finished);
                                        warn!("Channel for messages to applets was closed");
                                    }
                                };
                                new_state
                            },
                        };
                        if let Some(new_state) = new_state {
                            state = new_state;
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

pub struct PanelFd(Sender<Input>, NonZeroU32);
