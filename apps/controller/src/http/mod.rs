pub mod auth;
pub mod blueprints;
pub mod builds;
pub mod chat;
pub mod health;
pub mod mcp_bridge;
pub mod minecraft;
pub mod robot;
pub mod web;

use axum::Router;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(health::api_router())
        .merge(minecraft::router())
        .merge(robot::router())
        .merge(chat::router())
        .merge(builds::router())
        .merge(blueprints::router())
        .merge(mcp_bridge::router())
}
