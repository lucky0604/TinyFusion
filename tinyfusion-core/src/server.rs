use axum::{
    routing::get,
    Router,
    Json,
};
use serde_json::json;
use tracing::info;

/// Run the Axum HTTP server on the configured address.
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let app = Router::new()
        .route("/health", get(health_check));

    let addr = "127.0.0.1:9999";
    let listener = tokio::net::TcpListener::bind(addr).await?;

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
