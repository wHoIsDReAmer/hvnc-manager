mod config;
mod quic;
mod session;
mod transport;

use std::sync::Arc;

use anyhow::Result;
use config::ServerConfig;
use session::SessionManager;
use tokio::signal;
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

    tokio::select! {
        result = quic::run_quic(cfg, sessions) => {
            result?;
        }
        _ = shutdown_signal() => {
            info!("Shutdown signal received, stopping server...");
        }
    }

    info!("Server stopped gracefully");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .try_init();
    info!("relay starting");
}
