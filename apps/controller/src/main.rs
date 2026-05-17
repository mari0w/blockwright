mod app;
mod config;
mod domain;
mod http;
mod integrations;
mod services;
mod state;

use state::AppState;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    dotenvy::dotenv().ok();
    init_tracing();

    let config = config::load()?;
    let port = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(config.server.port);
    let bind_addr = format!("{}:{port}", config.server.host);

    let state = AppState::new(config).await?;
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;

    tracing::info!("blockwright controller listening on http://{bind_addr}");
    axum::serve(listener, app::build_app(state)).await?;
    Ok(())
}

fn init_tracing() {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info,tower_http=debug".into());

    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
}
