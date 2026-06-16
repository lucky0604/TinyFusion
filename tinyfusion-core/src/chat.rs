/// Chat Completions API — request parsing and validation.
///
/// Handles POST /v1/chat/completions: parses and validates incoming
/// OpenAI-compatible chat completion requests, rejecting invalid
/// payloads with 400 and descriptive errors.

use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

/// A single message in a chat completion request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Parsed and validated chat completion request, ready for downstream handling.
#[derive(Debug, Clone)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
}

/// Top-level request body as received from the client.
///
/// All fields are optional at the serde level so that we can return
/// targeted validation errors rather than a generic deserialization failure.
#[derive(Debug, Deserialize)]
pub(crate) struct RawChatRequest {
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    messages: Option<Vec<ChatMessage>>,
    #[serde(default)]
    stream: Option<bool>,
}

/// Validate a raw request and return a structured [`ChatCompletionRequest`],
/// or a human-readable error string.
fn validate(raw: RawChatRequest) -> Result<ChatCompletionRequest, String> {
    let model = raw.model.ok_or_else(|| {
        "Missing required field: 'model'. The model name must be specified.".to_string()
    })?;

    if model.is_empty() {
        return Err("'model' must not be empty.".to_string());
    }

    let messages = raw.messages.ok_or_else(|| {
        "Missing required field: 'messages'. At least one message is required.".to_string()
    })?;

    if messages.is_empty() {
        return Err("'messages' must contain at least one message.".to_string());
    }

    // Validate that every message has a non-empty role and content
    for (i, msg) in messages.iter().enumerate() {
        if msg.role.is_empty() {
            return Err(format!(
                "messages[{}]: 'role' must not be empty (expected 'system', 'user', or 'assistant').",
                i
            ));
        }
        if msg.content.is_empty() {
            return Err(format!(
                "messages[{}]: 'content' must not be empty.",
                i
            ));
        }
    }

    let stream = raw.stream.unwrap_or(false);

    Ok(ChatCompletionRequest {
        model,
        messages,
        stream,
    })
}

/// Axum handler: parse and validate a chat completion request.
pub(crate) async fn chat_completions(
    axum::Json(raw): axum::Json<RawChatRequest>,
) -> Result<axum::Json<serde_json::Value>, (StatusCode, axum::Json<serde_json::Value>)> {
    match validate(raw) {
        Ok(req) => {
            tracing::info!(
                "Chat completion request: model={}, messages={}, stream={}",
                req.model,
                req.messages.len(),
                req.stream
            );

            // Echo back the parsed request for now (US-006a: parsing only)
            Ok(axum::Json(serde_json::json!({
                "status": "ok",
                "model": req.model,
                "message_count": req.messages.len(),
                "stream": req.stream,
            })))
        }
        Err(msg) => {
            tracing::warn!("Invalid chat completion request: {}", msg);
            Err((
                StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({
                    "error": {
                        "message": msg,
                        "type": "invalid_request_error"
                    }
                })),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Method, Request};
    use serde_json::json;
    use tower::ServiceExt;

    /// Build a test router with the chat completions route.
    fn chat_app() -> axum::Router {
        axum::Router::new()
            .route("/v1/chat/completions", axum::routing::post(chat_completions))
    }

    /// Helper: send a JSON POST and return the response.
    async fn post_json(body: serde_json::Value) -> axum::http::Response<Body> {
        chat_app()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn response_body(resp: axum::http::Response<Body>) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    // -- Valid requests --

    #[tokio::test]
    async fn test_minimal_valid_request() {
        let resp = post_json(json!({
            "model": "llama3",
            "messages": [{"role": "user", "content": "Hello"}]
        }))
        .await;

        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body(resp).await;
        assert_eq!(body["status"], "ok");
        assert_eq!(body["model"], "llama3");
        assert_eq!(body["message_count"], 1);
        assert_eq!(body["stream"], false); // stream defaults to false
    }

    #[tokio::test]
    async fn test_valid_request_with_stream() {
        let resp = post_json(json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "You are helpful"},
                {"role": "user", "content": "Hi"}
            ],
            "stream": true
        }))
        .await;

        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body(resp).await;
        assert_eq!(body["stream"], true);
        assert_eq!(body["message_count"], 2);
    }

    #[tokio::test]
    async fn test_valid_request_with_multiple_messages() {
        let resp = post_json(json!({
            "model": "claude-3",
            "messages": [
                {"role": "system", "content": "You are a coding assistant"},
                {"role": "user", "content": "Fix this bug"},
                {"role": "assistant", "content": "Here is the fix"}
            ]
        }))
        .await;

        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body(resp).await;
        assert_eq!(body["message_count"], 3);
    }

    // -- Missing required fields --

    #[tokio::test]
    async fn test_missing_model_returns_400() {
        let resp = post_json(json!({
            "messages": [{"role": "user", "content": "Hello"}]
        }))
        .await;

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = response_body(resp).await;
        assert!(body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("model"));
        assert_eq!(body["error"]["type"], "invalid_request_error");
    }

    #[tokio::test]
    async fn test_empty_model_returns_400() {
        let resp = post_json(json!({
            "model": "",
            "messages": [{"role": "user", "content": "Hello"}]
        }))
        .await;

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = response_body(resp).await;
        assert!(body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("must not be empty"));
    }

    #[tokio::test]
    async fn test_missing_messages_returns_400() {
        let resp = post_json(json!({
            "model": "llama3"
        }))
        .await;

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = response_body(resp).await;
        assert!(body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("messages"));
    }

    #[tokio::test]
    async fn test_empty_messages_returns_400() {
        let resp = post_json(json!({
            "model": "llama3",
            "messages": []
        }))
        .await;

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = response_body(resp).await;
        assert!(body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("at least one"));
    }

    // -- Message validation --

    #[tokio::test]
    async fn test_empty_role_returns_400() {
        let resp = post_json(json!({
            "model": "llama3",
            "messages": [{"role": "", "content": "Hello"}]
        }))
        .await;

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = response_body(resp).await;
        assert!(body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("role"));
    }

    #[tokio::test]
    async fn test_empty_content_returns_400() {
        let resp = post_json(json!({
            "model": "llama3",
            "messages": [{"role": "user", "content": ""}]
        }))
        .await;

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = response_body(resp).await;
        assert!(body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("content"));
    }

    #[tokio::test]
    async fn test_invalid_message_index_reported() {
        let resp = post_json(json!({
            "model": "llama3",
            "messages": [
                {"role": "system", "content": "OK"},
                {"role": "", "content": "Bad"}
            ]
        }))
        .await;

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = response_body(resp).await;
        // Should report the correct message index
        assert!(body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("messages[1]"));
    }

    // -- Edge cases --

    #[tokio::test]
    async fn test_empty_body_returns_400() {
        let resp = post_json(json!({}))
        .await;

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = response_body(resp).await;
        // Should complain about missing model (first validated field)
        assert!(body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("model"));
    }

    #[tokio::test]
    async fn test_stream_defaults_to_false() {
        let resp = post_json(json!({
            "model": "llama3",
            "messages": [{"role": "user", "content": "Hello"}]
            // no "stream" field at all
        }))
        .await;

        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body(resp).await;
        assert_eq!(body["stream"], false);
    }

    // -- Unit tests for validate function directly --

    #[test]
    fn test_validate_returns_parsed_request() {
        let raw = RawChatRequest {
            model: Some("test-model".into()),
            messages: Some(vec![ChatMessage {
                role: "user".into(),
                content: "test".into(),
            }]),
            stream: Some(true),
        };

        let req = validate(raw).unwrap();
        assert_eq!(req.model, "test-model");
        assert_eq!(req.messages.len(), 1);
        assert!(req.stream);
    }

    #[test]
    fn test_validate_descriptive_error_for_missing_model() {
        let raw = RawChatRequest {
            model: None,
            messages: Some(vec![ChatMessage {
                role: "user".into(),
                content: "test".into(),
            }]),
            stream: None,
        };

        let err = validate(raw).unwrap_err();
        assert!(err.contains("model"), "Error should mention 'model': {}", err);
    }

    #[test]
    fn test_validate_descriptive_error_for_missing_messages() {
        let raw = RawChatRequest {
            model: Some("model".into()),
            messages: None,
            stream: None,
        };

        let err = validate(raw).unwrap_err();
        assert!(
            err.contains("messages"),
            "Error should mention 'messages': {}",
            err
        );
    }
}
