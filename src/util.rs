use anyhow::{bail, Result};
use cosmic_notifications_util::DAEMON_NOTIFICATIONS_FD;
use nix::fcntl;
use std::os::unix::io::{FromRawFd, RawFd};
use tokio::net::UnixStream;
use tracing::info;

pub async fn setup_panel_socket() -> Result<UnixStream> {
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

            Ok(UnixStream::from_std(unix_stream)?)
        } else {
            bail!("DAEMON_NOTIFICATIONS_FD is not a valid RawFd.");
        }
    } else {
        bail!("DAEMON_NOTIFICATIONS_FD is not set.");
    }
}
