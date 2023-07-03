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
    os::unix::io::{AsRawFd, OwnedFd},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{unix::OwnedWriteHalf, UnixStream},
    sync::mpsc::{channel, Receiver, Sender},
    task::JoinHandle,
};
use tracing::{error, info, warn};

#[derive(Debug)]
pub enum State {
    Starting,
    Waiting(
        OwnedWriteHalf,
        Vec<UnixStream>,
        Receiver<Input>,
        JoinHandle<()>,
    ),
    Finished,
}

#[derive(Debug, Clone)]
pub enum Input {
    AppletEvent(AppletEvent),
    PanelRequest(PanelRequest),
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
                                let (read, write) = s.into_split();
                                let tx_clone = tx.clone();
                                let jh = tokio::spawn(async move {
                                    let reader: BufReader<tokio::net::unix::OwnedReadHalf> =
                                        BufReader::new(read);
                                    let mut lines = reader.lines();
                                    while let Ok(Some(line)) = lines.next_line().await {
                                        info!("Received line {}", line);
                                        match ron::de::from_str::<PanelRequest>(line.as_str()) {
                                            Ok(event) => {
                                                if let Err(err) =
                                                    tx_clone.send(Input::PanelRequest(event)).await
                                                {
                                                    warn!("Failed to pass panel request {}", err);
                                                }
                                            }
                                            Err(err) => {
                                                warn!(
                                                    "Failed to deserialize panel request: {} {}",
                                                    line, err
                                                );
                                            }
                                        }
                                    }
                                });

                                if let Err(err) = output.send(Event::Ready(tx)).await {
                                    error!(
                                        "Failed to send ready event for the panel subscription {}",
                                        err
                                    );
                                    jh.abort();
                                    state = State::Finished;
                                } else {
                                    // We are ready to receive messages
                                    state = State::Waiting(write, Vec::new(), rx, jh);
                                }
                            }
                            Err(err) => {
                                error!(
                                    "Failed to connect to the socket for the panel server {}",
                                    err
                                );
                                state = State::Finished;
                            }
                        }
                    }
                    State::Waiting(s, applets, rx, jh) => {
                        // Wait for a message or for fd to be readable or we get an input
                        if let Some(msg) = rx.recv().await {
                            match msg {
                                Input::AppletEvent(e) => {
                                    let Ok(event) = ron::to_string(&e) else {
                                        error!("Failed to serialize applet event {:?}", e);
                                        continue;
                                    };
                                    for a in applets {
                                        if let Err(err) = a.write_all(event.as_bytes()).await {
                                            error!(
                                                "Failed to write applet event to socket {}",
                                                err
                                            );
                                            continue;
                                        }
                                    }
                                }
                                Input::PanelRequest(e) => {
                                    match e {
                                        PanelRequest::Init => {
                                            applets.clear();
                                        }
                                        PanelRequest::NewNotificationsClient { id } => {
                                            info!("New notifications client {}", id);
                                            let Ok((mine, theirs)) = UnixStream::pair() else {
                                                                            error!("Failed to create new socket pair");
                                                                            continue;
                                                                        };
                                            let Ok(my_std_stream) = mine.into_std() else {
                                                                            error!("Failed to convert new socket to std socket");
                                                                            continue;
                                                                        };
                                            if let Err(err) = my_std_stream.set_nonblocking(false) {
                                                error!(
                                                    "Failed to mark new socket as non-blocking {}",
                                                    err
                                                );
                                                continue;
                                            }

                                            let theirs = {
                                                let Ok(their_std_stream) = theirs
                                                                                .into_std() else {
                                                                                    error!("Failed to convert new socket to std socket");
                                                                                    continue;
                                                                                };
                                                if let Err(err) =
                                                    their_std_stream.set_nonblocking(false)
                                                {
                                                    error!("Failed to mark new socket as non-blocking {}", err);
                                                    continue;
                                                }
                                                OwnedFd::from(their_std_stream)
                                            };
                                            // set CLOEXEC
                                            let flags = fcntl::fcntl(
                                                theirs.as_raw_fd(),
                                                fcntl::FcntlArg::F_GETFD,
                                            );
                                            if let Err(err) = flags
                                                .map(|f: i32| {
                                                    fcntl::FdFlag::from_bits(f).unwrap()
                                                        | fcntl::FdFlag::FD_CLOEXEC
                                                })
                                                .and_then(|f| {
                                                    fcntl::fcntl(
                                                        theirs.as_raw_fd(),
                                                        fcntl::FcntlArg::F_SETFD(f),
                                                    )
                                                })
                                            {
                                                error!(
                                                    "Failed to set CLOEXEC on new socket {}",
                                                    err
                                                );
                                                continue;
                                            }

                                            // actually send the fd
                                            info!("Waiting for socket to be writable");
                                            let msg = id;
                                            let _ = s.writable().await;
                                            let stream = s.as_ref();
                                            info!("Sending fd to {}", id);

                                            if let Err(err) = stream.send_with_fd(
                                                bytemuck::bytes_of(&msg),
                                                &[theirs.as_raw_fd()],
                                            ) {
                                                error!("Failed to send fd to applet {}", err);
                                            } else if let Ok(mine) =
                                                UnixStream::from_std(my_std_stream)
                                            {
                                                applets.push(mine);
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            jh.abort();
                            state = State::Finished;
                            warn!("Channel for messages to applets was closed");
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
