use axum::{
    body::Body,
    extract::State,
    http::{header, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};

use crate::{
    state::AppState,
    types::{ErrorDetail, ErrorResponse},
};

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
        let body = ErrorResponse {
            error: ErrorDetail {
                message: "missing or invalid api key".to_string(),
                err_type: "authentication_error".to_string(),
                code: None,
            },
        };
        return (StatusCode::UNAUTHORIZED, Json(body)).into_response();
    }

    next.run(req).await
}
