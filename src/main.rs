mod app;
mod config;
mod localize;
mod subscriptions;

use config::APP_ID;
use tracing::{info, metadata::LevelFilter};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use localize::localize;

use crate::config::VERSION;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_journald::layer()?)
        .with(fmt::layer())
        .with(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::WARN.into())
                .from_env_lossy(),
        )
        .try_init()?;
    log_panics::init();

    info!("cosmic-notifications ({})", APP_ID);
    info!("Version: {} ({})", VERSION, config::profile());

    // Prepare i18n
    localize();

    app::run()?;
    Ok(())
}
