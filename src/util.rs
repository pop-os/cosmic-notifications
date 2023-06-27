use anyhow::{anyhow, Context, Result};
use nix::{fcntl, unistd};
use sendfd::RecvWithFd;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::{Read, Write},
    os::unix::{
        io::{AsRawFd, FromRawFd, RawFd},
        net::UnixStream,
    },
    sync::Arc,
};
use tracing::{error, warn};

pub fn setup_panel_socket() -> Result<()> {
    // XXX first read to end during setup to make sure we have no leftover data after a restart
    // TODO create socket

    if let Ok(fd_num) = std::env::var("COSMIC_SESSION_SOCK") {
        if let Ok(fd) = fd_num.parse::<RawFd>() {
            // set CLOEXEC
            let flags = fcntl::fcntl(fd, fcntl::FcntlArg::F_GETFD);
            let result = flags
                .map(|f| fcntl::FdFlag::from_bits(f).unwrap() | fcntl::FdFlag::FD_CLOEXEC)
                .and_then(|f| fcntl::fcntl(fd, fcntl::FcntlArg::F_SETFD(f)));
            let mut session_socket = match result {
                // CLOEXEC worked and we can startup with session IPC
                Ok(_) => unsafe { UnixStream::from_raw_fd(fd) },
                // CLOEXEC didn't work, something is wrong with the fd, just close it
                Err(err) => {
                    let _ = unistd::close(fd);
                    return Err(err).with_context(|| "Failed to setup session socket");
                }
            };

            // let bytes = message.into_bytes();
            // let len = (bytes.len() as u16).to_ne_bytes();
            // session_socket
            //     .write_all(&len)
            //     .with_context(|| "Failed to write message len")?;
            // session_socket
            //     .write_all(&bytes)
            //     .with_context(|| "Failed to write message bytes")?;
        } else {
            error!(socket = fd_num, "COSMIC_SESSION_SOCK is no valid RawFd.");
        }
    };

    Ok(())
}
