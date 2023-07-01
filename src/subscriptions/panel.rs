use crate::util;
use cosmic::{
    iced::{
        futures::{self, SinkExt},
        subscription,
    },
    iced_futures::Subscription,
};
use cosmic_notifications_util::{AppletEvent, PanelRequest};
use nix::fcntl;
use sendfd::SendWithFd;
use std::{
    fmt::Debug,
    os::{
        fd::IntoRawFd,
        unix::io::{AsRawFd, OwnedFd},
    },
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    sync::mpsc::{channel, Receiver, Sender},
};
use tracing::{info, warn};

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
                                        info!("Socket is readable");
                                        let mut reader = BufReader::new(s);
                                        // todo read messages
                                        let mut request_buf = String::with_capacity(32);
                                        if let Err(err) = reader.read_line(&mut request_buf).await {
                                            tracing::error!("Failed to read line from socket {}", err);
                                            continue;
                                        }
                                        let s = reader.into_inner();
                                        match ron::de::from_str::<PanelRequest>(request_buf.as_str()) {
                                            Ok(panel_request) => {
                                                match panel_request {
                                                    PanelRequest::Init => {
                                                        applets.clear();
                                                    }
                                                    PanelRequest::NewNotificationsClient{ id } => {
                                                        info!("New notifications client {}", id);
                                                        let Ok((mine, theirs)) = UnixStream::pair() else {
                                                            tracing::error!("Failed to create new socket pair");
                                                            continue;
                                                        };
                                                        let Ok(my_std_stream) = mine.into_std() else {
                                                            tracing::error!("Failed to convert new socket to std socket");
                                                            continue;
                                                        };
                                                        if let Err(err) = my_std_stream.set_nonblocking(false) {
                                                            tracing::error!("Failed to mark new socket as non-blocking {}", err);
                                                            continue;
                                                        }


                                                        let theirs = {
                                                            let Ok(their_std_stream) = theirs
                                                                .into_std() else {
                                                                    tracing::error!("Failed to convert new socket to std socket");
                                                                    continue;
                                                                };
                                                            if let Err(err) = their_std_stream
                                                                .set_nonblocking(false) {
                                                                    tracing::error!("Failed to mark new socket as non-blocking {}", err);
                                                                    continue;
                                                                }
                                                            OwnedFd::from(their_std_stream)
                                                        };
                                                        // set CLOEXEC
                                                        let flags = fcntl::fcntl(theirs.as_raw_fd(), fcntl::FcntlArg::F_GETFD);
                                                        if let Err(err) = flags
                                                            .map(|f: i32| fcntl::FdFlag::from_bits(f).unwrap() | fcntl::FdFlag::FD_CLOEXEC)
                                                            .and_then(|f| fcntl::fcntl(theirs.as_raw_fd(), fcntl::FcntlArg::F_SETFD(f))) {
                                                                tracing::error!("Failed to set CLOEXEC on new socket {}", err);
                                                                continue;
                                                            }

                                                        // actually send the fd
                                                        let msg = id;
                                                        if let Err(err) = s.send_with_fd(bytemuck::bytes_of(&msg), &[theirs.as_raw_fd()]) {
                                                            tracing::error!("Failed to send fd to applet {}", err);
                                                        } else if let Ok(mine) = UnixStream::from_std(my_std_stream) {
                                                            applets.push(mine);
                                                        }
                                                    }
                                                }
                                            }
                                            Err(err) => {
                                                tracing::error!("Failed to deserialize panel request: {} {}", request_buf.as_str(), err);
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
