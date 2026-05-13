use axum::{
    body::Body,
    extract::State,
    http::{header, Request},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::{error::AppError, state::AppState};

/// Bearer token 鉴权中间件。当 `AppState.config.auth_token()` 为 None 时跳过。
pub async fn auth(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let Some(expected) = state.config.auth_token() else {
        return next.run(req).await;
    };

    let header_val = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let token = header_val.strip_prefix("Bearer ").unwrap_or("").trim();

    if token.is_empty() || token != expected {
        return AppError::Unauthorized.into_response();
    }

    next.run(req).await
}
