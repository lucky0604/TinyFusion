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
    api_key: Option<&str>,
) -> Result<reqwest::Response, reqwest::Error> {
    tracing::info!("Forwarding request to upstream: {}", upstream_url);

    let mut req = client
        .post(upstream_url)
        .json(body);
    if let Some(key) = api_key.filter(|k| !k.is_empty()) {
        req = req.header("Authorization", format!("Bearer {}", key));
    }
    let response = req.send().await?;

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
    api_key: Option<&str>,
) -> Result<(axum::http::StatusCode, HeaderMap, Body), (axum::http::StatusCode, String)> {
    tracing::info!("Forwarding chat completion to upstream: {}", upstream_url);

    let response = match forward_request(client, upstream_url, body, api_key).await {
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
    use axum::routing::{get, post};
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

    #[test]
    fn test_stream_to_body_converts_stream() {
        use futures::stream;
        let chunks: Vec<Result<Bytes, reqwest::Error>> = vec![
            Ok(Bytes::from("Hello, ")),
            Ok(Bytes::from("world!")),
        ];
        let body = stream_to_body(stream::iter(chunks));
        let _ = body;
    }

    #[tokio::test]
    async fn test_forward_passthrough_connection_refused() {
        let client = Client::builder().timeout(std::time::Duration::from_secs(1)).build().unwrap();
        let url = "http://127.0.0.1:19999/chat/completions";
        let result = forward_passthrough(&client, url, &json!({"model":"t","messages":[]}), None).await;
        assert!(result.is_err(), "Expected error for refused connection");
    }

    struct MockServer {
        addr: SocketAddr,
    }

    impl MockServer {
        async fn start() -> Self {
            let app = axum::Router::new()
                .route("/health", get(|| async { "ok" }))
                .route("/chat/completions", post(|| async {
                    axum::response::Response::builder()
                        .status(200)
                        .header("content-type", "text/event-stream")
                        .header("cache-control", "no-cache")
                        .body(Body::from("data: {\"c\":\"hello\"}\n\ndata: [DONE]\n\n"))
                        .unwrap()
                }));

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();

            tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });

            let client = Client::builder().timeout(std::time::Duration::from_secs(2)).build().unwrap();
            let health_url = format!("http://{}/health", addr);
            for _ in 0..40 {
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                if client.get(&health_url).send().await.map(|r| r.status().is_success()).unwrap_or(false) { break; }
            }
            MockServer { addr }
        }
    }

    #[tokio::test]
    async fn test_sse_passthrough_forwards_chunks() {
        let server = MockServer::start().await;
        let client = Client::builder().timeout(std::time::Duration::from_secs(3)).build().unwrap();
        let url = format!("http://{}/chat/completions", server.addr);
        let (status, headers, body) = forward_passthrough(&client, &url, &json!({"model":"t","messages":[{"role":"user","content":"hi"}]}), None).await.expect("SSE passthrough should succeed");
        assert_eq!(status, axum::http::StatusCode::OK);
        assert!(headers.get("content-type").unwrap().to_str().unwrap().contains("text/event-stream"));
        assert_eq!(headers.get("cache-control").unwrap().to_str().unwrap(), "no-cache");
        let bytes = axum::body::to_bytes(body, 65536).await.unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("\"c\":\"hello\""), "missing chunk: {}", text);
        assert!(text.contains("[DONE]"), "missing done marker: {}", text);
    }

    #[tokio::test]
    async fn test_sse_passthrough_stream_closes() {
        let server = MockServer::start().await;
        let client = Client::builder().timeout(std::time::Duration::from_secs(3)).build().unwrap();
        let url = format!("http://{}/chat/completions", server.addr);
        let (_, _, body) = forward_passthrough(&client, &url, &json!({"model":"t","messages":[]}), None).await.expect("should succeed");
        let bytes = axum::body::to_bytes(body, 65536).await.unwrap();
        assert!(!bytes.is_empty(), "SSE body should not be empty");
    }
}
