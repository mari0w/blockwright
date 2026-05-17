pub mod auth;
pub mod blueprints;
pub mod chat;
pub mod health;
pub mod minecraft;
pub mod robot;

use axum::Router;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(health::api_router())
        .merge(minecraft::router())
        .merge(robot::router())
        .merge(chat::router())
        .merge(blueprints::router())
}
