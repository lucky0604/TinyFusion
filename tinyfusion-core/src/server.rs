use axum::{
    extract::State,
    response::{sse::Sse, IntoResponse},
    routing::{get, post},
    Router,
    Json,
};
use serde_json::json;
use tracing::info;

use crate::chat::{self, AppState};
use crate::config::Config;
use crate::events;

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

async fn health_check() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

async fn get_config(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let config = &state.config;
    let mut workspaces = serde_json::Map::new();
    for (k, v) in &config.workspaces {
        workspaces.insert(k.clone(), serde_json::json!({
            "path": v.path,
            "verify_command": v.verify_command,
            "verify_timeout_seconds": v.verify_timeout_seconds,
            "max_retries": v.max_retries,
        }));
    }
    let workers: Vec<_> = config.workers.iter().map(|m| {
        serde_json::json!({
            "name": m.name,
            "endpoint": m.endpoint,
            "model_id": m.model_id,
            "api_key": m.api_key,
        })
    }).collect();

    Json(serde_json::json!({
        "port": config.port,
        "workers": workers,
        "judge": {
            "name": config.judge.name,
            "endpoint": config.judge.endpoint,
            "model_id": config.judge.model_id,
            "api_key": config.judge.api_key,
        },
        "executor": {
            "name": config.executor.name,
            "endpoint": config.executor.endpoint,
            "model_id": config.executor.model_id,
            "api_key": config.executor.api_key,
        },
        "workspaces": workspaces,
        "error_keywords": &config.error_keywords,
    }))
}

async fn post_config(
    _state: State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<impl IntoResponse, (axum::http::StatusCode, Json<serde_json::Value>)> {
    let path = Config::default_path();
    let json_str = serde_json::to_string_pretty(&body).map_err(|e| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({"error": {"message": format!("Invalid JSON: {}", e), "type": "invalid_request"}})),
        )
    })?;
    std::fs::write(&path, &json_str).map_err(|e| {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("Failed to write config: {}", e), "type": "config_error"}})),
        )
    })?;
    info!("Config saved to {}", path.display());
    Ok(Json(json!({"status": "ok", "message": "Config saved. Restart core to apply."})))
}

async fn events_handler(
    State(state): State<AppState>,
) -> Sse<impl futures::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>> {
    let rx = state.events.subscribe();
    events::event_stream(rx)
}

pub fn app(config: Config) -> Router {
    let state = AppState::new(config);
    let loaded = state.session_manager.load_snapshot();
    if loaded > 0 {
        tracing::info!("Restored {} sessions from snapshot", loaded);
    }

    Router::new()
        .route("/health", get(health_check))
        .route("/v1/chat/completions", post(chat::chat_completions))
        .route("/v1/events", get(events_handler))
        .route("/v1/config", get(get_config).post(post_config))
        .with_state(state)
}

/// Build a test router without real config (for testing).
#[cfg(test)]
pub fn test_app() -> Router {
    use reqwest::Client;
    use crate::session::SessionManager;
    use crate::events::EventBus;
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
            error_keywords: vec![],
        }),
        client: Client::new(),
        session_manager: std::sync::Arc::new(SessionManager::new()),
        events: std::sync::Arc::new(EventBus::new(256)),
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
