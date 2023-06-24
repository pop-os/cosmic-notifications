mod app;
mod config;
mod localize;
mod subscriptions;
use config::APP_ID;
use tracing::info;

use localize::localize;

use crate::config::VERSION;

fn main() -> cosmic::iced::Result {
    // Initialize logger
    if std::env::var("TOKIO_CONSOLE").as_deref() == Ok("1") {
        std::env::set_var("RUST_LOG", "trace");
        console_subscriber::init();
    } else {
        tracing_subscriber::fmt::init();
    }

    info!("cosmic-notifications ({})", APP_ID);
    info!("Version: {} ({})", VERSION, config::profile());

    // Prepare i18n
    localize();

    app::run()
}
