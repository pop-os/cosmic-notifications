use nix::fcntl;
use std::{
    collections::HashMap,
    os::fd::{IntoRawFd, RawFd},
};
use tokio::{net::UnixStream, sync::mpsc::Sender};
use tracing::{error, info};
use zbus::{connection::Builder, interface, zvariant::OwnedFd, Connection, Guid, SignalContext};

use super::notifications::Input;

use anyhow::{bail, Result};
use cosmic_notifications_util::DAEMON_NOTIFICATIONS_FD;
use std::os::unix::io::FromRawFd;

pub async fn setup_panel_conn(tx: Sender<Input>) -> Result<Connection> {
    let socket = setup_panel_socket()?;
    info!("Got UnixStream");
    let guid = Guid::generate();
    let conn = tokio::time::timeout(
        tokio::time::Duration::from_secs(1),
        Builder::socket(socket)
            .p2p()
            .server(guid)
            .unwrap()
            .serve_at(
                "/com/system76/NotificationsSocket",
                NotificationsSocket { tx },
            )?
            .build(),
    )
    .await??;
    info!("Created panel connection");

    Ok(conn)
}

pub fn setup_panel_socket() -> Result<UnixStream> {
    if let Ok(fd_num) = std::env::var(DAEMON_NOTIFICATIONS_FD) {
        if let Ok(fd) = fd_num.parse::<RawFd>() {
            info!("Connecting to daemon on fd {}", fd);
            // set CLOEXEC
            let flags = fcntl::fcntl(fd, fcntl::FcntlArg::F_GETFD);
            flags
                .map(|f: i32| fcntl::FdFlag::from_bits(f).unwrap() | fcntl::FdFlag::FD_CLOEXEC)
                .and_then(|f| fcntl::fcntl(fd, fcntl::FcntlArg::F_SETFD(f)))?;

            let unix_stream = unsafe { std::os::unix::net::UnixStream::from_raw_fd(fd) };
            unix_stream.set_nonblocking(true)?;

            let unix_stream: UnixStream = UnixStream::from_std(unix_stream)?;

            Ok(unix_stream)
        } else {
            bail!("DAEMON_NOTIFICATIONS_FD is not a valid RawFd.");
        }
    } else {
        bail!("DAEMON_NOTIFICATIONS_FD is not set.");
    }
}

pub struct NotificationsSocket {
    pub tx: Sender<Input>,
}

#[interface(name = "com.system76.NotificationsSocket")]
impl NotificationsSocket {
    #[zbus(out_args("fd"))]
    async fn get_fd(&self) -> zbus::fdo::Result<OwnedFd> {
        let (mine, theirs) = std::os::unix::net::UnixStream::pair()
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        mine.set_nonblocking(true)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        theirs
            .set_nonblocking(true)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        let mine: UnixStream =
            UnixStream::from_std(mine).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;

        let guid = Guid::generate();

        let tx_clone = self.tx.clone();
        tokio::spawn(async move {
            let conn = match Builder::socket(mine)
                .p2p()
                .server(guid)
                .unwrap()
                .serve_at("/com/system76/NotificationsApplet", NotificationsApplet)
            {
                Ok(conn) => conn,
                Err(err) => {
                    error!("Failed to create applet connection {}", err);
                    return;
                }
            };

            info!("Creating applet connection");
            let conn = match conn.build().await {
                Ok(conn) => conn,
                Err(err) => {
                    error!("Failed to create applet connection {}", err);
                    return;
                }
            };
            info!("Created applet connection");

            if let Err(err) = tx_clone.send(Input::AppletConn(conn)).await {
                error!("Failed to send applet connection {}", err);
                return;
            }
            info!("Sent applet connection");
        });

        let raw = theirs.into_raw_fd();
        info!("Sending fd to applet");

        Ok(unsafe { zbus::zvariant::OwnedFd::from(std::os::fd::OwnedFd::from_raw_fd(raw)) })
    }
}

pub struct NotificationsApplet;

#[interface(name = "com.system76.NotificationsApplet")]
impl NotificationsApplet {
    #[zbus(signal)]
    pub async fn notify(
        signal_ctxt: &SignalContext<'_>,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: Vec<&str>,
        hints: HashMap<&str, zbus::zvariant::Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<()>;
}
