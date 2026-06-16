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
}
