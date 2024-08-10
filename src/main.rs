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
    let trace = tracing_subscriber::registry();
    #[cfg(feature = "systemd")]
    let trace = trace.with(tracing_journald::layer()?);
    trace
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
