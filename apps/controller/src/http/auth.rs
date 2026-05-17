use axum::{
    extract::{Request, State},
    http::{header::HeaderName, StatusCode},
    middleware::Next,
    response::Response,
};

use crate::state::AppState;

static BLOCKWRIGHT_TOKEN_HEADER: HeaderName = HeaderName::from_static("x-blockwright-token");

pub async fn require_token(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if !state.config.security.require_token {
        return Ok(next.run(request).await);
    }

    let request_token = request
        .headers()
        .get(&BLOCKWRIGHT_TOKEN_HEADER)
        .and_then(|value| value.to_str().ok());

    if request_token == Some(state.config.security.shared_token.as_str()) {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}
