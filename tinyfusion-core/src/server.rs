use axum::{
    routing::{get, post},
    Router,
    Json,
};
use serde_json::json;
use tracing::info;

use crate::chat::{self, AppState};
use crate::config::Config;

/// Run the Axum HTTP server on the configured address and port.
pub async fn run(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    let port = config.port;
    let app = app(config);

    let addr = format!("127.0.0.1:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    info!("TinyFusion server listening on {}", addr);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

/// Health check endpoint: returns {"status": "ok"}
async fn health_check() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

/// Build the application router with shared state.
pub fn app(config: Config) -> Router {
    let state = AppState::new(config);

    Router::new()
        .route("/health", get(health_check))
        .route("/v1/chat/completions", post(chat::chat_completions))
        .with_state(state)
}

/// Build a test router without real config (for testing).
#[cfg(test)]
pub fn test_app() -> Router {
    use reqwest::Client;
    let state = AppState {
        config: std::sync::Arc::new(Config {
            port: 9999,
            workers: vec![],
            judge: crate::config::ModelConfig {
                name: "judge".into(),
                endpoint: "http://localhost:11434".into(),
                model_id: "llama3".into(),
                api_key: None,
            },
            executor: crate::config::ModelConfig {
                name: "executor".into(),
                endpoint: "http://localhost:11434".into(),
                model_id: "llama3".into(),
                api_key: None,
            },
            workspaces: std::collections::HashMap::new(),
        }),
        client: Client::new(),
    };
    Router::new()
        .route("/health", get(health_check))
        .route("/v1/chat/completions", post(chat::chat_completions))
        .with_state(state)
}

/// Listen for SIGINT / SIGTERM and trigger graceful shutdown.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
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

    info!("Shutting down...");
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_health_check_returns_ok() {
        let app = test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
    }

    #[tokio::test]
    async fn test_unknown_route_returns_404() {
        let app = test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/unknown")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
