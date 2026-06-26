use axum::{
    extract::State,
    response::{sse::Sse, IntoResponse},
    routing::{get, post},
    Router,
    Json,
    http::StatusCode,
};
use serde_json::json;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

use crate::chat::{self, AppState};
use crate::config::Config;
use crate::events;

/// Run the Axum HTTP server on the configured address and port.
pub async fn run(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    let port = config.port;
    let app = app(config);

    let addr: std::net::SocketAddr = format!("127.0.0.1:{}", port).parse()?;
    let socket = tokio::net::TcpSocket::new_v4()?;
    socket.set_reuseaddr(true)?;
    socket.bind(addr)?;
    let listener = socket.listen(1024)?;

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

    // Build fusion.models for frontend
    let mut fusion_models = serde_json::Map::new();
    for (name, entry) in &config.fusion.models {
        fusion_models.insert(name.clone(), serde_json::json!({
            "provider": entry.provider,
            "endpoint": entry.endpoint,
            "model_id": entry.model_id,
            "api_key": entry.api_key,
            "tier": entry.tier,
            "is_local": entry.is_local,
            "chat_path": entry.chat_path,
        }));
    }

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
        "fusion": {
            "models": fusion_models,
            "routing": config.fusion.routing,
            "budget": config.fusion.budget,
            "classifier": config.fusion.classifier,
        },
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

async fn get_sessions(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let sessions = state.session_manager.list();
    let mut list = Vec::new();
    for s in sessions {
        let name = if let Some(first_msg) = s.messages.first() {
            let content = &first_msg.content;
            if content.len() > 30 {
                let end = content
                    .char_indices()
                    .map(|(i, _)| i)
                    .take_while(|&i| i <= 30)
                    .last()
                    .unwrap_or(0);
                format!("{}...", &content[..end])
            } else {
                content.to_string()
            }
        } else {
            format!("#{}", &s.id[..4.min(s.id.len())])
        };

        let duration_secs = std::time::SystemTime::now()
            .duration_since(s.created_at)
            .unwrap_or_default()
            .as_secs();

        let duration_str = if duration_secs < 60 {
            format!("{}s", duration_secs)
        } else {
            format!("{}m {}s", duration_secs / 60, duration_secs % 60)
        };

        let token_count = s.messages.iter().map(|m| m.content.len() / 4).sum::<usize>();

        list.push(json!({
            "id": s.id.clone(),
            "name": name,
            "state": format!("{:?}", s.state), // Diagnostic, Execution, Verify, Done
            "retryCount": s.retry_count,
            "maxRetries": 3,
            "workers": s.models.join(", "),
            "duration": duration_str,
            "requests": s.retry_count + 1,
            "tokens": token_count,
            "createdAt": s.created_at
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() * 1000,
        }));
    }
    Json(json!(list))
}

async fn delete_session(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Json<serde_json::Value> {
    let removed = state.session_manager.remove_and_persist(&id);
    Json(json!({ "status": "ok", "removed": removed.is_some() }))
}

async fn get_budget(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let snap = state.budget.snapshot();
    Json(json!({
        "daily_tokens": snap.daily_tokens,
        "daily_limit": snap.daily_limit,
        "monthly_tokens": snap.monthly_tokens,
        "monthly_limit": snap.monthly_limit,
    }))
}

async fn get_metrics() -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    let path = std::path::PathBuf::from(home)
        .join(".tinyfusion")
        .join("metrics.jsonl");

    if !path.exists() {
        return Ok(Json(json!([])));
    }

    let content = std::fs::read_to_string(&path).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Failed to read metrics: {}", e) })),
        )
    })?;

    let mut metrics_list = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(trimmed) {
            metrics_list.push(parsed);
        }
    }

    Ok(Json(json!(metrics_list)))
}

pub fn app(config: Config) -> Router {
    let state = AppState::new(config);
    let loaded = state.session_manager.load_snapshot();
    if loaded > 0 {
        tracing::info!("Restored {} sessions from snapshot", loaded);
    }

    let cors = CorsLayer::new()
        .allow_origin([
            "http://localhost:5173".parse().unwrap(),
            "http://127.0.0.1:5173".parse().unwrap(),
            "tauri://localhost".parse().unwrap(),
            "http://tauri.localhost".parse().unwrap(),
        ])
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/health", get(health_check))
        .route("/v1/chat/completions", post(chat::chat_completions))
        .route("/v1/events", get(events_handler))
        .route("/v1/config", get(get_config).post(post_config))
        .route("/v1/sessions", get(get_sessions))
        .route("/v1/sessions/:id", axum::routing::delete(delete_session))
        .route("/v1/budget", get(get_budget))
        .route("/v1/metrics", get(get_metrics))
        .layer(cors)
        .with_state(state)
}

/// Build a test router without real config (for testing).
#[cfg(test)]
pub fn test_app() -> Router {
    let config = Config {
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
        fusion: Default::default(),
    };
    let state = AppState::new(config);
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
