/// Chat Completions API — request parsing, response formatting, and streaming.
///
/// Handles POST /v1/chat/completions: parses and validates incoming
/// OpenAI-compatible chat completion requests, returns responses in the
/// standard OpenAI format (both streaming via SSE and non-streaming).

use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use futures::stream::Stream;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::Config;
use crate::events::{EventBus, GatewayEvent};
use crate::keepalive;
use crate::moa::{self, WorkerConfig};
use crate::proxy::forward_passthrough;
use crate::session::{Session, SessionManager, SessionState};
use crate::sniffer::{self, Message, RequestState};
use tokio_util::sync::CancellationToken;

/// Application state shared across all handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub client: Client,
    pub session_manager: Arc<SessionManager>,
    pub events: Arc<EventBus>,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        Self {
            config: Arc::new(config),
            client: Client::new(),
            session_manager: Arc::new(SessionManager::new()),
            events: Arc::new(EventBus::new(256)),
        }
    }
}

/// Parsed and validated chat completion request, ready for downstream handling.
#[derive(Debug, Clone)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub stream: bool,
    pub session_id: Option<String>,
    pub workspace: Option<String>,
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

    Event::default().event("chat.completion.chunk").json_data(&chunk).unwrap()
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
    messages: Option<Vec<Message>>,
    #[serde(default)]
    stream: Option<bool>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    workspace: Option<String>,
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
        session_id: raw.session_id.clone(),
        workspace: raw.workspace.clone(),
    })
}

/// Axum handler: parse, validate, sniff state, and orchestrate the MoA pipeline.
pub(crate) async fn chat_completions(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::Json(raw): axum::Json<RawChatRequest>,
) -> Result<ChatResponse, (StatusCode, axum::Json<serde_json::Value>)> {
    let req = match validate(raw) {
        Ok(r) => r,
        Err(msg) => {
            tracing::warn!("Invalid chat completion request: {}", msg);
            return Err((
                StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({
                    "error": {
                        "message": msg,
                        "type": "invalid_request_error"
                    }
                })),
            ));
        }
    };

    tracing::info!(
        "Chat completion: model={}, messages={}, stream={}",
        req.model,
        req.messages.len(),
        req.stream
    );

    let sniff_msgs: Vec<Message> =
        req.messages.iter().map(|m| m.clone()).collect();

    let req_state = sniffer::sniff_state_with_keywords(&sniff_msgs, &state.config.error_keywords);

    let session_id = req.session_id.clone().unwrap_or_else(|| {
        Session::id_from_messages(&sniff_msgs)
    });

    let session_exists = state.session_manager.lookup(&session_id).is_some();
    let (_, collision) = state
        .session_manager
        .get_or_create(session_id.clone(), sniff_msgs);

    if collision {
        tracing::warn!(
            "Potential SHA-256 collision detected for session {} — proceeding with existing session state",
            session_id
        );
    }

    let current = state
        .session_manager
        .lookup(&session_id)
        .unwrap_or_else(|| Session::new(session_id.clone(), vec![]));

    let routing_state = if session_exists {
        current.state.clone()
    } else {
        match req_state {
            RequestState::Execution => SessionState::Execution,
            _ => SessionState::Diagnostic,
        }
    };

    let max_retries: u32 = state
        .config
        .workspaces
        .get(req.workspace.as_deref().unwrap_or("default"))
        .map(|w| w.max_retries)
        .unwrap_or(3);

    match routing_state {
        SessionState::Diagnostic => {
            state.events.emit(
                GatewayEvent::new("phase", "Diagnostic phase started")
                    .with_session(&session_id)
            );
            handle_diagnostic(&state, &req, &session_id, max_retries).await
        }
        SessionState::Execution => {
            state.events.emit(
                GatewayEvent::new("phase", "Execution phase started")
                    .with_session(&session_id)
            );
            handle_execution(&state, &req, &session_id).await
        }
        SessionState::Verify => {
            state.events.emit(
                GatewayEvent::new("phase", "Verify phase started")
                    .with_session(&session_id)
            );
            handle_verify(&state, &req, &session_id).await
        }
        SessionState::Done => {
            handle_execution(&state, &req, &session_id).await
        }
    }
}

/// Diagnostic phase: MoA worker fan-out → judge synthesis → SSE streaming with keepalive.
async fn handle_diagnostic(
    state: &AppState,
    req: &ChatCompletionRequest,
    session_id: &str,
    _max_retries: u32,
) -> Result<ChatResponse, (StatusCode, axum::Json<serde_json::Value>)> {
    tracing::info!("[Diagnostic] session={}", session_id);

    let worker_configs: Vec<WorkerConfig> = state
        .config
        .workers
        .iter()
        .map(|m| WorkerConfig {
            endpoint: m.endpoint.clone(),
            model_id: m.model_id.clone(),
        })
        .collect();

    let worker_msgs: Vec<Message> =
        req.messages.iter().map(|m| m.clone()).collect();
    let worker_responses =
        moa::call_workers(&worker_configs, &worker_msgs, 30).await;

    let original_prompt = req
        .messages
        .iter()
        .map(|m| format!("{}: {}", m.role, m.content))
        .collect::<Vec<_>>()
        .join("\n");

    let judge_prompt = moa::build_judge_prompt(&original_prompt, &worker_responses);

    let judge_config = &state.config.judge;
    let judge_url =
        format!("{}/chat/completions", judge_config.endpoint.trim_end_matches('/'));

    let judge_body = serde_json::json!({
        "model": judge_config.model_id,
        "messages": [
            {"role": "user", "content": judge_prompt}
        ],
        "stream": true,
    });

    let cancel = CancellationToken::new();
    let keepalive_cancel = cancel.clone();
    let keepalive = keepalive::keepalive_stream(keepalive_cancel);

    let chat_id = generate_id();
    let created = current_timestamp();

    match state.client.post(&judge_url).json(&judge_body).send().await {
        Ok(resp) => {
            let status = resp.status();
            if !status.is_success() {
                cancel.cancel();
                let text = resp.text().await.unwrap_or_default();
                return Err((
                    StatusCode::BAD_GATEWAY,
                    axum::Json(serde_json::json!({
                        "error": {
                            "message": format!("Judge returned {}: {}", status.as_u16(), text),
                            "type": "upstream_error"
                        }
                    })),
                ));
            }

            use futures::StreamExt;
            use tokio::sync::mpsc;

            let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(64);

            let sse_id = chat_id.clone();
            let sse_model = req.model.clone();
            let session_id = session_id.to_string();
            let session_mgr = state.session_manager.clone();
            tokio::spawn(async move {
                let mut stream = resp.bytes_stream();
                let mut full_text = String::new();
                let mut first_chunk = true;
                let _ = cancel.cancel();
                while let Some(chunk) = stream.next().await {
                    match chunk {
                        Ok(bytes) => {
                            let text = String::from_utf8_lossy(&bytes);
                            for line in text.lines() {
                                let trimmed = line.trim();
                                if trimmed.is_empty() {
                                    continue;
                                }
                                if let Some(data) = trimmed.strip_prefix("data: ") {
                                    if data == "[DONE]" {
                                        let finish_event = Event::default()
                                            .data("[DONE]");
                                        let _ = tx
                                            .send(Ok(finish_event))
                                            .await;
                                        break;
                                    }
                                    if let Ok(parsed) =
                                        serde_json::from_str::<serde_json::Value>(data)
                                    {
                                        if let Some(choices) =
                                            parsed["choices"].as_array()
                                        {
                                            for choice in choices {
                                                if let Some(delta) =
                                                    choice.get("delta")
                                                {
                                                    if let Some(content) =
                                                        delta["content"].as_str()
                                                    {
                                                        full_text.push_str(content);
                                                        if first_chunk {
                                                            first_chunk = false;
                                                            let start_event =
                                                                streaming_event(
                                                                    &sse_id,
                                                                    created,
                                                                    &sse_model,
                                                                    Some(content),
                                                                    0,
                                                                    None,
                                                                );
                                                            let _ = tx
                                                                .send(Ok(start_event))
                                                                .await;
                                                        } else {
                                                            let delta_str = content.to_string();
                                                            let delta_value = serde_json::json!({
                                                                "id": sse_id,
                                                                "object": "chat.completion.chunk",
                                                                "created": created,
                                                                "model": sse_model,
                                                                "choices": [{
                                                                    "index": 0,
                                                                    "delta": {
                                                                        "content": delta_str
                                                                    },
                                                                    "finish_reason": null
                                                                }]
                                                            });
                                                            let evt = Event::default()
                                                                .json_data(delta_value)
                                                                .unwrap();
                                                            let _ = tx
                                                                .send(Ok(evt))
                                                                .await;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Err(_) => break,
                    }
                }

                let analysis = moa::parse_judge_xml(&full_text);
                if !analysis.final_plan.is_empty() {
                    let judge_msg = Message {
                        role: "assistant".into(),
                        content: full_text.clone(),
                    };
                    session_mgr.append_messages(
                        &session_id,
                        vec![judge_msg],
                    );
                    session_mgr.set_state(
                        &session_id,
                        SessionState::Execution,
                    );
                    session_mgr.save_snapshot();
                }

                let done_event =
                    Event::default().data("[DONE]");
                let _ = tx.send(Ok(done_event)).await;
            });

            let rx_stream =
                tokio_stream::wrappers::ReceiverStream::new(rx);

            let merged = keepalive
                .map(|e| {
                    let evt: Result<Event, Infallible> = e;
                    evt
                })
                .chain(rx_stream);

            Ok(ChatResponse::Stream(Sse::new(Box::pin(merged))))
        }
        Err(e) => {
            cancel.cancel();
            Err((
                StatusCode::BAD_GATEWAY,
                axum::Json(serde_json::json!({
                    "error": {
                        "message": format!("Judge request failed: {}", e),
                        "type": "upstream_error"
                    }
                })),
            ))
        }
    }
}

/// Execution phase: forward request to executor via passthrough.
async fn handle_execution(
    state: &AppState,
    req: &ChatCompletionRequest,
    session_id: &str,
) -> Result<ChatResponse, (StatusCode, axum::Json<serde_json::Value>)> {
    tracing::info!("[Execution] session={}", session_id);

    let upstream_url =
        format!("{}/chat/completions", state.config.executor.endpoint.trim_end_matches('/'));
    let upstream_body = serde_json::json!({
        "model": state.config.executor.model_id,
        "messages": req.messages,
        "stream": req.stream,
    });

    match forward_passthrough(&state.client, &upstream_url, &upstream_body).await {
        Ok((status, headers, body)) => {
            state.session_manager.set_state(session_id, SessionState::Verify);
            state.session_manager.save_snapshot();

            let mut response = axum::response::Response::new(body);
            *response.status_mut() = status;
            if let Some(ct) = headers.get("content-type") {
                response.headers_mut().insert("content-type", ct.clone());
            }
            Ok(ChatResponse::Raw(response))
        }
        Err((status, error_msg)) => {
            tracing::error!("Executor forward failed: {}", error_msg);
            Err((
                status,
                axum::Json(serde_json::json!({
                    "error": {
                        "message": error_msg,
                        "type": "upstream_error"
                    }
                })),
            ))
        }
    }
}

/// Verify phase: run the oracle command and handle retry logic.
async fn handle_verify(
    state: &AppState,
    req: &ChatCompletionRequest,
    session_id: &str,
) -> Result<ChatResponse, (StatusCode, axum::Json<serde_json::Value>)> {
    tracing::info!("[Verify] session={}", session_id);

    let workspace = req.workspace.as_deref().unwrap_or("default");

    let verify_cmd = state
        .config
        .workspaces
        .get(workspace)
        .map(|w| w.verify_command.clone())
        .unwrap_or_else(|| "echo 'No verify command configured'".into());

    let workspace_path = state
        .config
        .workspaces
        .get(workspace)
        .map(|w| w.path.clone())
        .unwrap_or_else(|| ".".into());

    let verify_timeout = state
        .config
        .workspaces
        .get(workspace)
        .map(|w| w.verify_timeout_seconds)
        .unwrap_or(45);

    let result = match crate::oracle::run_verify(&verify_cmd, &workspace_path, verify_timeout).await
    {
        Ok(r) => r,
        Err(e) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({
                    "error": {
                        "message": format!("Verify command failed: {}", e),
                        "type": "oracle_error"
                    }
                })),
            ));
        }
    };

    if result.is_success() {
        state.session_manager.set_state(session_id, SessionState::Done);
        Ok(ChatResponse::Json(axum::Json(
            ChatCompletionResponse::new(&req.model, "Verification passed. Workflow complete."),
        )))
    } else {
        let session = state.session_manager.lookup(session_id);
        let retries = session.map(|s| s.retry_count).unwrap_or(0);
        let max = 3;

        if retries < max {
            let error_msg = crate::oracle::format_error_message(&result);
            let error_sniff = Message {
                role: "user".into(),
                content: error_msg,
            };
            state
                .session_manager
                .append_messages(session_id, vec![error_sniff]);
            state
                .session_manager
                .set_state(session_id, SessionState::Diagnostic);
            state.session_manager.increment_retry(session_id);

            handle_diagnostic(state, req, session_id, max).await
        } else {
            state.session_manager.set_state(session_id, SessionState::Done);
            Ok(ChatResponse::Json(axum::Json(
                ChatCompletionResponse::new(
                    &req.model,
                    &format!(
                        "Max retries ({}) reached. Last error:\n{}",
                        max, result.stderr
                    ),
                ),
            )))
        }
    }
}

/// Response type for the chat completions endpoint.
///
/// Either a JSON response, an SSE stream, or a raw upstream passthrough.
pub(crate) enum ChatResponse {
    Json(axum::Json<ChatCompletionResponse>),
    Stream(Sse<PinBoxStream>),
    Raw(axum::response::Response),
}

pub(crate) type PinBoxStream = std::pin::Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>>;

impl axum::response::IntoResponse for ChatResponse {
    fn into_response(self) -> axum::response::Response {
        match self {
            ChatResponse::Json(json) => json.into_response(),
            ChatResponse::Stream(sse) => sse.into_response(),
            ChatResponse::Raw(resp) => resp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Method, Request};
    use serde_json::json;
    use std::collections::HashMap;
    use tower::ServiceExt;

    use crate::config::ModelConfig;
    use crate::session::SessionManager;

    fn test_state() -> AppState {
        AppState {
            config: Arc::new(Config {
                port: 9999,
                workers: vec![],
                judge: ModelConfig {
                    name: "judge".into(),
                    endpoint: "http://localhost:11434".into(),
                    model_id: "llama3".into(),
                    api_key: None,
                },
                executor: ModelConfig {
                    name: "executor".into(),
                    endpoint: "http://localhost:11434".into(),
                    model_id: "llama3".into(),
                    api_key: None,
                },
                workspaces: HashMap::new(),
                error_keywords: vec![],
            }),
            client: Client::builder().timeout(std::time::Duration::from_secs(2)).build().unwrap(),
            session_manager: Arc::new(SessionManager::new()),
            events: Arc::new(EventBus::new(256)),
        }
    }

    fn chat_app() -> axum::Router {
        axum::Router::new()
            .route("/v1/chat/completions", axum::routing::post(chat_completions))
            .with_state(test_state())
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

    // -- Passthrough forwarding --

    #[tokio::test]
    async fn test_forward_attempts_upstream_connection() {
        // Include </final_plan> to trigger Execution path (passthrough to executor)
        let resp = post_json(json!({
            "model": "llama3",
            "messages": [
                {"role": "user", "content": "Hello"},
                {"role": "system", "content": "You are helpful"},
                {"role": "assistant", "content": "</final_plan> Do this"}
            ]
        }))
        .await;

        // Upstream should fail (no server at localhost:11434 for test default)
        assert!(
            resp.status() == StatusCode::BAD_GATEWAY || resp.status().is_server_error(),
            "Expected upstream error status, got {}",
            resp.status()
        );
        let body = response_body(resp).await;
        assert!(body["error"]["message"].as_str().unwrap().contains("Upstream"));
        assert_eq!(body["error"]["type"], "upstream_error");
    }

    #[tokio::test]
    async fn test_diagnostic_path_judge_error_without_endpoint() {
        let resp = post_json(json!({
            "model": "llama3",
            "messages": [{"role": "user", "content": "Hello"}]
        }))
        .await;

        // Diagnostic path tries judge which also fails without server
        assert!(
            resp.status() == StatusCode::BAD_GATEWAY || resp.status().is_server_error(),
            "Expected upstream error status, got {}",
            resp.status()
        );
        let body = response_body(resp).await;
        assert!(body["error"]["message"].as_str().unwrap().contains("Judge"));
    }

    // -- Validation tests (request parsing, unchanged from US-006a) --

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
    async fn test_stream_defaults_to_false_sends_to_upstream() {
        let resp = post_json(json!({
            "model": "llama3",
            "messages": [
                {"role": "user", "content": "Hello"},
                {"role": "system", "content": "You are helpful"},
                {"role": "assistant", "content": "</final_plan> Plan is ready"}
            ]
            // no "stream" field at all
        }))
        .await;

        // Without stream=true, still forwards to upstream.
        // Upstream is unreachable → expect error, NOT a 200 JSON.
        assert!(
            resp.status() == StatusCode::BAD_GATEWAY || resp.status().is_server_error(),
            "Expected upstream error when no server running"
        );
        let body = response_body(resp).await;
        assert_eq!(body["error"]["type"], "upstream_error");
    }

    // -- Unit tests for validate function directly --

    #[test]
    fn test_validate_returns_parsed_request() {
        let raw = RawChatRequest {
            model: Some("test-model".into()),
            messages: Some(vec![Message {
                role: "user".into(),
                content: "test".into(),
            }]),
            stream: Some(true),
            session_id: None,
            workspace: None,
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
            messages: Some(vec![Message {
                role: "user".into(),
                content: "test".into(),
            }]),
            stream: None,
            session_id: None,
            workspace: None,
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
            session_id: None,
            workspace: None,
        };

        let err = validate(raw).unwrap_err();
        assert!(
            err.contains("messages"),
            "Error should mention 'messages': {}",
            err
        );
    }
}
