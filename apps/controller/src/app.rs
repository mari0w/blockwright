use axum::{middleware, Router};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::{http, state::AppState};

pub fn build_app(state: AppState) -> Router {
    let api_router = http::router().layer(middleware::from_fn_with_state(
        state.clone(),
        http::auth::require_token,
    ));

    Router::new()
        .merge(http::health::router())
        .merge(http::web::router())
        .nest("/api", api_router)
        .with_state(state)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}
