/// Proxy module — forwards requests to upstream model endpoints.
///
/// Handles request forwarding, response streaming, and error propagation.

use reqwest::Client;

/// Forward a request to an upstream endpoint and return the response.
pub async fn forward_request(
    client: &Client,
    upstream_url: &str,
    body: &serde_json::Value,
) -> Result<reqwest::Response, reqwest::Error> {
    tracing::debug!("Forwarding request to upstream: {}", upstream_url);

    let response = client
        .post(upstream_url)
        .json(body)
        .send()
        .await?;

    Ok(response)
}

/// Forward an error response from upstream to the client with correct status.
pub fn forward_error_status(upstream_status: reqwest::StatusCode) -> axum::http::StatusCode {
    axum::http::StatusCode::from_u16(upstream_status.as_u16())
        .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
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
}
