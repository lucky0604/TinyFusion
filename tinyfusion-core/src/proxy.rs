/// Proxy module — forwards requests to upstream model endpoints.
///
/// Handles request forwarding, response streaming, and error propagation.

use axum::body::Body;
use axum::http::HeaderMap;
use futures::TryStreamExt;
use reqwest::Client;
use tokio_util::bytes::Bytes;

/// Forward a request to an upstream endpoint and return the response.
pub async fn forward_request(
    client: &Client,
    upstream_url: &str,
    body: &serde_json::Value,
) -> Result<reqwest::Response, reqwest::Error> {
    tracing::info!("Forwarding request to upstream: {}", upstream_url);

    let response = client
        .post(upstream_url)
        .json(body)
        .send()
        .await?;

    tracing::debug!("Upstream responded with status: {}", response.status());

    Ok(response)
}

/// Forward an error response from upstream to the client with correct status.
pub fn forward_error_status(upstream_status: reqwest::StatusCode) -> axum::http::StatusCode {
    axum::http::StatusCode::from_u16(upstream_status.as_u16())
        .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
}

/// Convert a reqwest byte stream into an Axum response body.
///
/// This enables passthrough streaming: upstream bytes flow directly
/// to the client without buffering or transformation.
pub fn stream_to_body(
    stream: impl futures::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
) -> Body {
    Body::from_stream(stream.map_err(axum::Error::new))
}

/// Forward a chat completion request to the upstream executor and return
/// the response body for direct passthrough to the client.
///
/// Returns `(status, headers, body)` — the caller should forward all three
/// to preserve the upstream response exactly (JSON or SSE).
pub async fn forward_passthrough(
    client: &Client,
    upstream_url: &str,
    body: &serde_json::Value,
) -> Result<(axum::http::StatusCode, HeaderMap, Body), (axum::http::StatusCode, String)> {
    tracing::info!("Forwarding chat completion to upstream: {}", upstream_url);

    let response = match forward_request(client, upstream_url, body).await {
        Ok(resp) => resp,
        Err(e) => {
            tracing::error!("Upstream request failed: {}", e);
            return Err((
                axum::http::StatusCode::BAD_GATEWAY,
                format!("Upstream request failed: {}", e),
            ));
        }
    };

    let status = forward_error_status(response.status());
    let headers = response.headers().clone();
    let body = stream_to_body(response.bytes_stream());

    Ok((status, headers, body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::post;
    use axum::Json;
    use serde_json::json;
    use std::net::SocketAddr;

    #[test]
    fn test_error_status_forward() {
        let status = reqwest::StatusCode::BAD_REQUEST;
        let axum_status = forward_error_status(status);
        assert_eq!(axum_status, axum::http::StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_error_status_non_standard() {
        // Non-standard but valid status codes are forwarded as-is
        let status = reqwest::StatusCode::from_u16(418).unwrap();
        let axum_status = forward_error_status(status);
        assert_eq!(axum_status.as_u16(), 418);
    }

    #[test]
    fn test_error_status_5xx() {
        let status = reqwest::StatusCode::INTERNAL_SERVER_ERROR;
        let axum_status = forward_error_status(status);
        assert_eq!(axum_status, axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    }

    /// Spawn a mock upstream SSE server on a random port.
    async fn spawn_mock_upstream(response_body: &'static str) -> SocketAddr {
        let app = axum::Router::new().route(
            "/chat/completions",
            post(move || async move {
                axum::response::Response::builder()
                    .status(200)
                    .header("content-type", "text/event-stream")
                    .header("cache-control", "no-cache")
                    .header("connection", "keep-alive")
                    .body(Body::from(response_body))
                    .unwrap()
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        // Small delay for server startup
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        addr
    }

    #[tokio::test]
    async fn test_sse_passthrough_forwards_chunks() {
        let addr = spawn_mock_upstream(
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\ndata: [DONE]\n\n",
        )
        .await;

        let client = Client::new();
        let url = format!("http://{}/chat/completions", addr);
        let body = json!({"model": "test", "messages": [{"role": "user", "content": "hi"}]});

        let (status, headers, body) = forward_passthrough(&client, &url, &body)
            .await
            .expect("SSE passthrough should succeed");

        assert_eq!(status, axum::http::StatusCode::OK);

        // SSE Content-Type must be forwarded
        let ct = headers.get("content-type").unwrap().to_str().unwrap();
        assert!(ct.contains("text/event-stream"), "SSE content-type not forwarded");

        // Read all body bytes
        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        let text = String::from_utf8_lossy(&bytes);

        // Should contain upstream SSE data
        assert!(text.contains("data:"), "SSE response missing 'data:' events");
        assert!(text.contains("[DONE]"), "SSE response missing [DONE] marker");
    }

    #[tokio::test]
    async fn test_sse_passthrough_preserves_cache_headers() {
        let addr = spawn_mock_upstream(
            "data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n\ndata: [DONE]\n\n",
        )
        .await;

        let client = Client::new();
        let url = format!("http://{}/chat/completions", addr);

        let (_, headers, _) = forward_passthrough(&client, &url, &json!({"model":"t","messages":[]}))
            .await
            .expect("should succeed");

        assert_eq!(
            headers.get("cache-control").unwrap().to_str().unwrap(),
            "no-cache"
        );
    }

    #[tokio::test]
    async fn test_sse_passthrough_stream_closes_on_upstream_disconnect() {
        // Mock sends a short SSE payload then closes
        let addr = spawn_mock_upstream("data: {\"msg\":\"done\"}\n\n").await;

        let client = Client::new();
        let url = format!("http://{}/chat/completions", addr);

        let (_, _, body) = forward_passthrough(&client, &url, &json!({"model":"t","messages":[]}))
            .await
            .expect("should succeed");

        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("done"), "SSE content not received");
    }

    #[tokio::test]
    async fn test_sse_passthrough_connection_refused() {
        let client = Client::new();
        let url = "http://127.0.0.1:19999/chat/completions"; // Non-existent port

        let result = forward_passthrough(&client, url, &json!({"model":"t","messages":[]})).await;
        assert!(result.is_err(), "Expected error for refused connection");
    }
}
