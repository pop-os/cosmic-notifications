use anyhow::{bail, Result};
use cosmic_notifications_util::DAEMON_NOTIFICATIONS_FD;
use nix::fcntl;
use std::os::unix::io::{FromRawFd, RawFd};
use tokio::net::UnixStream;

pub async fn setup_panel_socket() -> Result<UnixStream> {
    if let Ok(fd_num) = std::env::var(DAEMON_NOTIFICATIONS_FD) {
        if let Ok(fd) = fd_num.parse::<RawFd>() {
            // set CLOEXEC
            let flags = fcntl::fcntl(fd, fcntl::FcntlArg::F_GETFD);
            flags
                .map(|f: i32| fcntl::FdFlag::from_bits(f).unwrap() | fcntl::FdFlag::FD_CLOEXEC)
                .and_then(|f| fcntl::fcntl(fd, fcntl::FcntlArg::F_SETFD(f)))?;

            let unix_stream = unsafe { std::os::unix::net::UnixStream::from_raw_fd(fd) };
            let unix_stream: UnixStream = UnixStream::from_std(unix_stream)?;

            // XXX first read to end during setup to make sure we have no leftover data after a restart
            let mut buf = [0u8; 1024];
            loop {
                match unix_stream.try_read(&mut buf) {
                    Ok(0) => {
                        // EOF
                        break;
                    }
                    Ok(_) => {
                        // read more
                    }
                    _ => {
                        break;
                    }
                }
            }

            Ok(unix_stream)
        } else {
            bail!("DAEMON_NOTIFICATIONS_FD is not a valid RawFd.");
        }
    } else {
        bail!("DAEMON_NOTIFICATIONS_FD is not set.");
    }
}
