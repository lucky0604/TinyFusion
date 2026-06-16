/// Chat Completions API — request parsing, response formatting, and streaming.
///
/// Handles POST /v1/chat/completions: parses and validates incoming
/// OpenAI-compatible chat completion requests, returns responses in the
/// standard OpenAI format (both streaming via SSE and non-streaming).

use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::time::{SystemTime, UNIX_EPOCH};

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

/// A single message in a chat completion response (assistant reply).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMessage {
    pub role: String,
    pub content: String,
}

/// A single choice in the response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    pub index: usize,
    pub message: ResponseMessage,
    pub finish_reason: String,
}

/// Token usage statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
}

/// Standard OpenAI-compatible chat completion response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Usage,
}

/// A single delta chunk for streaming responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaMessage {
    pub role: Option<String>,
    pub content: Option<String>,
}

/// A single streaming choice.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingChoice {
    pub index: usize,
    pub delta: DeltaMessage,
    pub finish_reason: Option<String>,
}

/// A single SSE chunk in a streaming response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<StreamingChoice>,
}

impl ChatCompletionResponse {
    /// Construct a response with a single assistant choice.
    pub fn new(model: &str, content: &str) -> Self {
        let id = generate_id();
        let created = current_timestamp();

        // Simple token estimation: ~1 token per 4 chars of English text
        let prompt_tokens = 0; // Not tracked at this stage
        let completion_tokens = content.len().div_ceil(4).max(1);

        Self {
            id,
            object: "chat.completion".to_string(),
            created,
            model: model.to_string(),
            choices: vec![Choice {
                index: 0,
                message: ResponseMessage {
                    role: "assistant".to_string(),
                    content: content.to_string(),
                },
                finish_reason: "stop".to_string(),
            }],
            usage: Usage {
                prompt_tokens,
                completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
            },
        }
    }
}

/// Generate a unique chat completion ID.
fn generate_id() -> String {
    // Simple ID: chatcmpl-<timestamp_hex>
    let ts = current_timestamp();
    format!("chatcmpl-{:x}", ts)
}

/// Current UNIX timestamp in seconds.
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Generate an SSE event for a streaming chunk.
///
/// Returns the final `[DONE]` event when `finish_reason` is provided.
fn streaming_event(
    id: &str,
    created: u64,
    model: &str,
    content: Option<&str>,
    index: usize,
    finish_reason: Option<&str>,
) -> Event {
    let chunk = ChatCompletionChunk {
        id: id.to_string(),
        object: "chat.completion.chunk".to_string(),
        created,
        model: model.to_string(),
        choices: vec![StreamingChoice {
            index,
            delta: DeltaMessage {
                role: if content.is_some() {
                    Some("assistant".to_string())
                } else {
                    None
                },
                content: content.map(String::from),
            },
            finish_reason: finish_reason.map(String::from),
        }],
    };

    if finish_reason.is_some() {
        Event::default().event("chat.completion.chunk").json_data(&chunk).unwrap()
    } else {
        Event::default().event("chat.completion.chunk").json_data(&chunk).unwrap()
    }
}

/// Build a stream of SSE events from simulated content chunks.
///
/// This creates an async stream that yields SSE events for each content
/// chunk followed by a final `[DONE]` event. In a real implementation,
/// this would be driven by upstream token streaming.
pub fn build_sse_stream(
    model: &str,
    content_chunks: Vec<String>,
) -> PinBoxStream {
    let id = generate_id();
    let created = current_timestamp();
    let model = model.to_string();

    let stream = async_stream::stream! {
        // Yield a content event for each chunk
        for chunk in &content_chunks {
            let event = streaming_event(&id, created, &model, Some(chunk), 0, None);
            yield Ok(event);
        }

        // Yield the final event with finish_reason
        let done_event = streaming_event(&id, created, &model, None, 0, Some("stop"));
        yield Ok(done_event);

        // Yield the [DONE] marker
        yield Ok(Event::default().event("chat.completion.chunk").data("[DONE]"));
    };

    Box::pin(stream)
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
///
/// For non-streaming requests, returns a standard JSON response.
/// For streaming requests, returns an SSE stream with chunked events.
pub(crate) async fn chat_completions(
    axum::Json(raw): axum::Json<RawChatRequest>,
) -> Result<ChatResponse, (StatusCode, axum::Json<serde_json::Value>)> {
    match validate(raw) {
        Ok(req) => {
            tracing::info!(
                "Chat completion request: model={}, messages={}, stream={}",
                req.model,
                req.messages.len(),
                req.stream
            );

            if req.stream {
                // Build a streaming response from the request content
                // In US-006b we simulate by echoing user message content as chunks
                let content: Vec<String> = req
                    .messages
                    .iter()
                    .filter(|m| m.role == "user")
                    .map(|m| m.content.clone())
                    .collect();

                let stream = build_sse_stream(&req.model, content);
                Ok(ChatResponse::Stream(Sse::new(stream)))
            } else {
                // Build a non-streaming JSON response
                // Use the last user message content for the assistant reply
                let user_content = req
                    .messages
                    .iter()
                    .rev()
                    .find(|m| m.role == "user")
                    .map(|m| m.content.as_str())
                    .unwrap_or("");
                let response = ChatCompletionResponse::new(&req.model, user_content);
                Ok(ChatResponse::Json(axum::Json(response)))
            }
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

/// Response type for the chat completions endpoint.
///
/// Either a JSON response (non-streaming) or an SSE stream (streaming).
pub(crate) enum ChatResponse {
    Json(axum::Json<ChatCompletionResponse>),
    Stream(Sse<PinBoxStream>),
}

pub(crate) type PinBoxStream = std::pin::Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>>;

impl axum::response::IntoResponse for ChatResponse {
    fn into_response(self) -> axum::response::Response {
        match self {
            ChatResponse::Json(json) => json.into_response(),
            ChatResponse::Stream(sse) => sse.into_response(),
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

    // -- Valid requests (non-streaming) --

    #[tokio::test]
    async fn test_minimal_valid_request() {
        let resp = post_json(json!({
            "model": "llama3",
            "messages": [{"role": "user", "content": "Hello"}]
        }))
        .await;

        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body(resp).await;
        assert_eq!(body["object"], "chat.completion");
        assert_eq!(body["model"], "llama3");
        assert_eq!(body["choices"][0]["message"]["role"], "assistant");
        assert_eq!(body["choices"][0]["finish_reason"], "stop");
        assert!(body["usage"]["total_tokens"].as_u64().unwrap() > 0);
    }

    #[tokio::test]
    async fn test_response_has_required_fields() {
        let resp = post_json(json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Test"}]
        }))
        .await;

        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body(resp).await;
        // Required: id, object, created, model, choices, usage
        assert!(body["id"].as_str().is_some(), "response must have 'id'");
        assert_eq!(body["object"], "chat.completion");
        assert!(body["created"].as_u64().is_some(), "response must have 'created'");
        assert_eq!(body["model"], "gpt-4");
        assert!(body["choices"].as_array().is_some(), "response must have 'choices'");
        assert!(body["usage"].is_object(), "response must have 'usage'");
    }

    #[tokio::test]
    async fn test_response_choice_structure() {
        let resp = post_json(json!({
            "model": "claude-3",
            "messages": [{"role": "user", "content": "Hello world"}]
        }))
        .await;

        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body(resp).await;
        let choice = &body["choices"][0];
        assert_eq!(choice["index"], 0);
        assert_eq!(choice["message"]["role"], "assistant");
        assert_eq!(choice["message"]["content"], "Hello world");
        assert_eq!(choice["finish_reason"], "stop");
    }

    #[tokio::test]
    async fn test_response_usage_structure() {
        let resp = post_json(json!({
            "model": "llama3",
            "messages": [{"role": "user", "content": "Test message"}]
        }))
        .await;

        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body(resp).await;
        let usage = &body["usage"];
        assert!(usage["prompt_tokens"].as_u64().is_some());
        assert!(usage["completion_tokens"].as_u64().is_some());
        assert!(usage["total_tokens"].as_u64().is_some());
        assert_eq!(
            usage["total_tokens"].as_u64().unwrap(),
            usage["prompt_tokens"].as_u64().unwrap() + usage["completion_tokens"].as_u64().unwrap()
        );
    }

    #[tokio::test]
    async fn test_response_id_format() {
        let resp = post_json(json!({
            "model": "llama3",
            "messages": [{"role": "user", "content": "Hello"}]
        }))
        .await;

        assert_eq!(resp.status(), StatusCode::OK);
        let body = response_body(resp).await;
        let id = body["id"].as_str().unwrap();
        assert!(
            id.starts_with("chatcmpl-"),
            "response id should start with 'chatcmpl-': {}",
            id
        );
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
        // Content should be from the last user message
        assert_eq!(body["choices"][0]["message"]["content"], "Fix this bug");
    }

    // -- Streaming requests --

    #[tokio::test]
    async fn test_streaming_response_returns_sse() {
        let resp = post_json(json!({
            "model": "llama3",
            "messages": [{"role": "user", "content": "Hello"}],
            "stream": true
        }))
        .await;

        assert_eq!(resp.status(), StatusCode::OK);
        // SSE responses have Content-Type: text/event-stream
        let content_type = resp.headers().get("content-type").unwrap().to_str().unwrap();
        assert!(
            content_type.contains("text/event-stream"),
            "streaming response should be SSE, got: {}",
            content_type
        );
    }

    #[tokio::test]
    async fn test_streaming_response_contains_data_events() {
        let resp = post_json(json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Stream test"}],
            "stream": true
        }))
        .await;

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&body);
        // Should contain SSE data: events
        assert!(
            text.contains("data:"),
            "SSE response should contain 'data:' events"
        );
        assert!(
            text.contains("chat.completion.chunk"),
            "SSE response should contain chunk events"
        );
    }

    #[tokio::test]
    async fn test_streaming_response_contains_done_marker() {
        let resp = post_json(json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Done test"}],
            "stream": true
        }))
        .await;

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&body);
        assert!(
            text.contains("[DONE]"),
            "SSE response should contain [DONE] marker"
        );
    }

    #[tokio::test]
    async fn test_streaming_response_has_finish_reason() {
        let resp = post_json(json!({
            "model": "llama3",
            "messages": [{"role": "user", "content": "Finish test"}],
            "stream": true
        }))
        .await;

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&body);
        assert!(
            text.contains("stop"),
            "SSE response should contain finish_reason 'stop'"
        );
    }

    #[tokio::test]
    async fn test_streaming_chunk_structure() {
        let resp = post_json(json!({
            "model": "test-model",
            "messages": [{"role": "user", "content": "Struct test"}],
            "stream": true
        }))
        .await;

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&body);
        // Each chunk should have id, object, created, model, choices
        assert!(text.contains("\"object\":\"chat.completion.chunk\""));
        assert!(text.contains("\"test-model\""));
    }

    // -- Non-streaming explicitly --

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
    async fn test_stream_defaults_to_false_returns_json() {
        let resp = post_json(json!({
            "model": "llama3",
            "messages": [{"role": "user", "content": "Hello"}]
            // no "stream" field at all
        }))
        .await;

        assert_eq!(resp.status(), StatusCode::OK);
        // Without stream=true, should return JSON (not SSE)
        let content_type = resp.headers().get("content-type").unwrap().to_str().unwrap();
        assert!(
            content_type.contains("json"),
            "non-streaming response should be JSON, got: {}",
            content_type
        );
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
