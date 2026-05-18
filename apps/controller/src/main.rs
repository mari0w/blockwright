use blockwright_controller::{app, config, mcp, state::AppState};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    dotenvy::dotenv().ok();
    let mode = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "serve".to_string());
    init_tracing();

    let config = config::load()?;
    let state = AppState::new(config).await?;

    if mode == "mcp" {
        tracing::info!("blockwright MCP server starting on stdio");
        return mcp::serve_stdio(state).await;
    }

    if mode != "serve" {
        return Err(format!("unknown blockwright-controller mode: {mode}").into());
    }

    let port = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(state.config.server.port);
    let bind_addr = format!("{}:{port}", state.config.server.host);

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;

    tracing::info!("blockwright controller listening on http://{bind_addr}");
    axum::serve(listener, app::build_app(state)).await?;
    Ok(())
}

fn init_tracing() {
    let env_filter =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());

    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();
}
