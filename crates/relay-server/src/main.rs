mod config;
mod quic;
mod session;
mod transport;

use std::sync::Arc;

use anyhow::Result;
use config::ServerConfig;
use session::SessionManager;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let cfg = if std::env::var("RELAY_AUTH_TOKEN").is_ok() {
        ServerConfig::from_env()
    } else {
        info!("Using default dev config (set RELAY_AUTH_TOKEN for production)");
        ServerConfig::default()
    };

    info!(
        "Auth token configured: {}...",
        &cfg.auth_token[..cfg.auth_token.len().min(8)]
    );

    let sessions = Arc::new(SessionManager::new());
    quic::run_quic(cfg, sessions).await?;

    Ok(())
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .try_init();
    info!("relay starting");
}
