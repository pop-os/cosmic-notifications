mod app;
mod config;
mod localize;
mod subscriptions;

use config::APP_ID;
use tracing::{info, metadata::LevelFilter};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use localize::localize;

use crate::config::VERSION;

fn main() -> anyhow::Result<()> {
    color_backtrace::install();
    let trace = tracing_subscriber::registry();

    let env_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::WARN.into())
        .from_env_lossy();
    #[cfg(feature = "systemd")]
    if let Ok(journald) = tracing_journald::layer() {
        trace
            .with(journald)
            .with(fmt::layer())
            .with(env_filter)
            .try_init()?;
    } else {
        trace.with(fmt::layer()).with(env_filter).try_init()?;
        tracing::warn!("Failed to connect to journald")
    }

    #[cfg(not(feature = "systemd"))]
    trace.with(fmt::layer()).with(env_filter).try_init()?;

    info!("cosmic-notifications ({})", APP_ID);
    info!("Version: {} ({})", VERSION, config::profile());

    // Prepare i18n
    localize();

    app::run()?;
    Ok(())
}
