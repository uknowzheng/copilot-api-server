mod config;
mod error;
mod handlers;
mod middleware;
mod prompt;
mod state;
mod streaming;
mod types;

use std::net::SocketAddr;
use std::time::Duration;

use axum::{
    middleware::from_fn_with_state,
    routing::{get, post},
    Router,
};
use clap::Parser;
use copilot_sdk::Client;
use tokio::signal;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let config = Config::parse();
    info!(host = %config.host, port = config.port, "starting copilot-openai-server");

    // 启动 Copilot 客户端
    let client = Client::builder().auto_start(true).auto_restart(true).build()?;
    client.start().await?;

    let state = AppState::new(client, config.clone());

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/v1/models", get(handlers::list_models))
        .route("/v1/chat/completions", post(handlers::chat_completions))
        .layer(from_fn_with_state(state.clone(), middleware::auth))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state.clone());

    let addr: SocketAddr = format!("{}:{}", config.host, config.port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("listening on http://{addr}");

    let server = axum::serve(listener, app).with_graceful_shutdown(shutdown_signal());

    let serve_result = server.await;

    // 优雅关闭：先停 HTTP（已发生），再停 Copilot 客户端，超时 30s
    info!("shutting down copilot client...");
    let stop_fut = state.client.stop();
    match tokio::time::timeout(Duration::from_secs(30), stop_fut).await {
        Ok(errs) if errs.is_empty() => info!("client stopped"),
        Ok(errs) => error!("client.stop reported {} error(s): {:?}", errs.len(), errs),
        Err(_) => error!("client.stop timeout (30s)"),
    }

    serve_result?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c().await.expect("install ctrl_c handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => info!("received Ctrl+C"),
        _ = terminate => info!("received SIGTERM"),
    }
}
