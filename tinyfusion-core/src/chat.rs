//! Chat Completions API — request parsing, response formatting, and streaming.
//!
//! Handles POST /v1/chat/completions with the new Fusion deliberation pipeline:
//!   1. FusionGuard check (subrequest → forced passthrough)
//!   2. Model alias check (tinyfusion/fusion → Fusion pipeline)
//!   3. Tool presence check (tinyfusion_deliberate in tools → Fusion pipeline)
//!   4. Standard passthrough (all other requests)

use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, Sse};
use futures::stream::Stream;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::Config;
use crate::events::{EventBus, GatewayEvent};
use crate::harness::{PipelineContext, PipelineRunner};
use crate::keepalive;
use crate::proxy::forward_passthrough;
use crate::session::SessionManager;
use crate::types::{
    DeliberateArgs, FusionGuard, Message,
    FUSION_MODEL_ALIAS, FUSION_TOOL_NAME,
};
use tokio_util::sync::CancellationToken;

/// Application state shared across all handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub client: Client,
    pub session_manager: Arc<SessionManager>,
    pub events: Arc<EventBus>,
    pub pipeline_runner: Arc<PipelineRunner>,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        let pipeline_runner = PipelineRunner::from_config(&config);
        let client = Client::builder()
            .user_agent("Mozilla/5.0 (compatible; TinyFusion/1.0)")
            .build()
            .expect("Failed to build HTTP client");
        Self {
            config: Arc::new(config),
            client,
            session_manager: Arc::new(SessionManager::new()),
            events: Arc::new(EventBus::new(256)),
            pipeline_runner: Arc::new(pipeline_runner),
        }
    }
}

/// Parsed and validated chat completion request.
#[derive(Debug, Clone)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub stream: bool,
    pub session_id: Option<String>,
    pub workspace: Option<String>,
    pub tools: Option<Vec<serde_json::Value>>,
}

/// Routing decision for a chat completion request.
#[derive(Debug)]
enum RoutingDecision {
    /// Subrequest: forced single-model passthrough (anti-recursion).
    SubrequestPassthrough,
    /// Fusion pipeline via model alias or tool presence.
    FusionPipeline(DeliberateArgs),
    /// Standard passthrough to executor.
    StandardPassthrough,
    /// Legacy diagnostic/MoA path (backward compat).
    LegacyDiagnostic,
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
    pub fn new(model: &str, content: &str) -> Self {
        let id = generate_id();
        let created = current_timestamp();
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
                prompt_tokens: 0,
                completion_tokens,
                total_tokens: completion_tokens,
            },
        }
    }
}

fn generate_id() -> String {
    let ts = current_timestamp();
    let rand_suffix: u32 = (ts as u32).wrapping_mul(2654435761);
    format!("chatcmpl-{:x}{:04x}", ts, rand_suffix & 0xFFFF)
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

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

    Event::default()
        .event("chat.completion.chunk")
        .json_data(&chunk)
        .unwrap()
}

pub fn build_sse_stream(
    model: &str,
    content_chunks: Vec<String>,
) -> PinBoxStream {
    let id = generate_id();
    let created = current_timestamp();
    let model = model.to_string();

    let stream = async_stream::stream! {
        for chunk in &content_chunks {
            let event = streaming_event(&id, created, &model, Some(chunk), 0, None);
            yield Ok(event);
        }
        let done_event = streaming_event(&id, created, &model, None, 0, Some("stop"));
        yield Ok(done_event);
        yield Ok(Event::default().event("chat.completion.chunk").data("[DONE]"));
    };

    Box::pin(stream)
}

/// Top-level request body as received from the client.
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
    #[serde(default)]
    tools: Option<Vec<serde_json::Value>>,
}

/// Validate a raw request and return a structured ChatCompletionRequest.
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

    for (i, msg) in messages.iter().enumerate() {
        if msg.role.is_empty() {
            return Err(format!(
                "messages[{}]: 'role' must not be empty (expected 'system', 'user', or 'assistant').",
                i
            ));
        }
        // Allow empty content for tool-related messages
        if msg.role != "tool"
            && msg.role != "assistant"
            && msg.content_str().is_empty()
            && msg.tool_calls.is_none()
        {
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
        session_id: raw.session_id,
        workspace: raw.workspace,
        tools: raw.tools,
    })
}

/// Resolve default panel models from config: use the first preset if available,
/// otherwise use all registered models except the judge.
fn resolve_default_panel_models(config: &Config) -> Vec<String> {
    // Try the first preset
    if let Some((_, models)) = config.fusion.presets.iter().next() {
        if !models.is_empty() {
            return models.clone();
        }
    }
    // Fall back: all registered models except the judge
    let judge = &config.fusion.default_judge_model;
    let models: Vec<String> = config
        .fusion
        .models
        .keys()
        .filter(|k| k.as_str() != judge.as_str())
        .cloned()
        .collect();
    if models.is_empty() {
        config.fusion.models.keys().cloned().collect()
    } else {
        models
    }
}

/// Determine routing based on the pure stateless decision tree.
fn decide_route(
    guard: &FusionGuard,
    req: &ChatCompletionRequest,
    config: &Config,
) -> RoutingDecision {
    let has_fusion_models = !config.fusion.models.is_empty();

    // 1. Subrequest guard: prevent recursion
    if guard.is_subrequest {
        return RoutingDecision::SubrequestPassthrough;
    }

    // 2. Model alias: tinyfusion/fusion
    if req.model == FUSION_MODEL_ALIAS {
        let panel_models = resolve_default_panel_models(config);
        return RoutingDecision::FusionPipeline(DeliberateArgs {
            analysis_models: panel_models,
            judge_model: None,
        });
    }

    // 3. Check if tools contain tinyfusion_deliberate
    if let Some(tools) = &req.tools {
        for tool in tools {
            if let Some(func) = tool.get("function") {
                if func.get("name").and_then(|n| n.as_str()) == Some(FUSION_TOOL_NAME) {
                    let panel_models = resolve_default_panel_models(config);
                    return RoutingDecision::FusionPipeline(DeliberateArgs {
                        analysis_models: panel_models,
                        judge_model: None,
                    });
                }
            }
        }
    }

    // 4. Check if the last assistant message contains a tool_call for fusion
    if let Some(last_assistant) = req.messages.iter().rev().find(|m| m.role == "assistant") {
        if let Some(tool_calls) = &last_assistant.tool_calls {
            for tc in tool_calls {
                if tc.function.name == FUSION_TOOL_NAME {
                    if let Ok(args) = serde_json::from_str::<DeliberateArgs>(&tc.function.arguments) {
                        return RoutingDecision::FusionPipeline(args);
                    }
                }
            }
        }
    }

    // 5. Legacy: check for error keywords to route to diagnostic (backward compat)
    if has_fusion_models {
        // If fusion config has models, prefer standard passthrough
        return RoutingDecision::StandardPassthrough;
    }

    // Legacy fallback when no fusion config present
    let sniff_msgs: Vec<crate::sniffer::Message> = req
        .messages
        .iter()
        .map(|m| crate::sniffer::Message {
            role: m.role.clone(),
            content: m.content_str().to_string(),
        })
        .collect();

    let req_state = crate::sniffer::sniff_state(&sniff_msgs);
    match req_state {
        crate::sniffer::RequestState::Diagnostic => RoutingDecision::LegacyDiagnostic,
        crate::sniffer::RequestState::Execution => RoutingDecision::StandardPassthrough,
    }
}

/// Axum handler: parse, validate, route, and orchestrate.
pub(crate) async fn chat_completions(
    axum::extract::State(state): axum::extract::State<AppState>,
    headers: HeaderMap,
    axum::Json(raw): axum::Json<RawChatRequest>,
) -> Result<ChatResponse, (StatusCode, axum::Json<serde_json::Value>)> {
    let guard = FusionGuard::from_headers(&headers);

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
        "Chat completion: model={}, messages={}, stream={}, subrequest={}",
        req.model,
        req.messages.len(),
        req.stream,
        guard.is_subrequest
    );

    let decision = decide_route(&guard, &req, &state.config);

    match decision {
        RoutingDecision::SubrequestPassthrough => {
            tracing::info!("[Route] Subrequest passthrough (anti-recursion)");
            handle_passthrough(&state, &req).await
        }
        RoutingDecision::FusionPipeline(args) => {
            tracing::info!("[Route] Fusion deliberation pipeline: {:?}", args);
            state.events.emit(
                GatewayEvent::new("fusion", "Fusion deliberation pipeline started")
            );
            handle_fusion_pipeline(&state, &req, args).await
        }
        RoutingDecision::StandardPassthrough => {
            tracing::info!("[Route] Standard passthrough");
            handle_passthrough(&state, &req).await
        }
        RoutingDecision::LegacyDiagnostic => {
            tracing::info!("[Route] Legacy diagnostic path");
            handle_legacy_diagnostic(&state, &req).await
        }
    }
}

/// Fusion deliberation pipeline: run the harness and return results.
async fn handle_fusion_pipeline(
    state: &AppState,
    req: &ChatCompletionRequest,
    args: DeliberateArgs,
) -> Result<ChatResponse, (StatusCode, axum::Json<serde_json::Value>)> {
    let request_id = generate_id();
    let panel_models = state.config.fusion.resolve_panel_models(&args.analysis_models);
    let judge_model = args
        .judge_model
        .unwrap_or_else(|| state.config.fusion.default_judge_model.clone());

    let mut ctx = PipelineContext::new(
        request_id.clone(),
        req.messages.clone(),
        panel_models,
        judge_model,
        req.stream,
    );

    match state.pipeline_runner.run(&mut ctx, state).await {
        Ok(()) => {
            let analysis = ctx
                .structured_analysis
                .as_ref()
                .cloned()
                .unwrap_or_default();

            let tool_output = serde_json::to_string_pretty(&analysis).unwrap_or_default();

            if req.stream {
                let content_chunks = split_into_chunks(&tool_output, SSE_CHUNK_SIZE);
                Ok(ChatResponse::Stream(Sse::new(build_sse_stream(
                    &req.model,
                    content_chunks,
                ))))
            } else {
                Ok(ChatResponse::Json(axum::Json(
                    ChatCompletionResponse::new(&req.model, &tool_output),
                )))
            }
        }
        Err(e) => {
            tracing::error!("[Fusion] Pipeline failed: {}", e);
            state.events.emit(
                GatewayEvent::new("error", &format!("Fusion pipeline failed: {}", e))
            );
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({
                    "error": {
                        "message": format!("Fusion pipeline failed: {}", e),
                        "type": "fusion_error"
                    }
                })),
            ))
        }
    }
}

/// Standard passthrough: forward to executor/upstream.
async fn handle_passthrough(
    state: &AppState,
    req: &ChatCompletionRequest,
) -> Result<ChatResponse, (StatusCode, axum::Json<serde_json::Value>)> {
    // Try to find the model in fusion registry first
    let (upstream_url, model_id, api_key) = if let Some(entry) = state.config.fusion.get_model(&req.model) {
        (
            crate::proxy::build_chat_url(&entry.endpoint),
            entry.model_id.clone(),
            entry.api_key.clone(),
        )
    } else {
        // Fall back to legacy executor config
        (
            crate::proxy::build_chat_url(&state.config.executor.endpoint),
            state.config.executor.model_id.clone(),
            state.config.executor.api_key.clone(),
        )
    };

    let upstream_body = serde_json::json!({
        "model": model_id,
        "messages": req.messages,
        "stream": req.stream,
    });

    match forward_passthrough(&state.client, &upstream_url, &upstream_body, api_key.as_deref()).await {
        Ok((status, headers, body)) => {
            let mut response = axum::response::Response::new(body);
            *response.status_mut() = status;
            if let Some(ct) = headers.get("content-type") {
                response.headers_mut().insert("content-type", ct.clone());
            }
            Ok(ChatResponse::Raw(response))
        }
        Err((status, error_msg)) => {
            tracing::error!("Passthrough failed: {}", error_msg);
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

/// Legacy diagnostic path (backward compat with old MoA).
async fn handle_legacy_diagnostic(
    state: &AppState,
    req: &ChatCompletionRequest,
) -> Result<ChatResponse, (StatusCode, axum::Json<serde_json::Value>)> {
    let worker_configs: Vec<crate::moa::WorkerConfig> = state
        .config
        .workers
        .iter()
        .map(|m| crate::moa::WorkerConfig {
            endpoint: m.endpoint.clone(),
            model_id: m.model_id.clone(),
            api_key: m.api_key.clone(),
        })
        .collect();

    let worker_msgs: Vec<crate::sniffer::Message> = req
        .messages
        .iter()
        .map(|m| crate::sniffer::Message {
            role: m.role.clone(),
            content: m.content_str().to_string(),
        })
        .collect();

    let worker_responses =
        crate::moa::call_workers(&worker_configs, &worker_msgs, 30).await;

    let original_prompt = req
        .messages
        .iter()
        .map(|m| format!("{}: {}", m.role, m.content_str()))
        .collect::<Vec<_>>()
        .join("\n");

    let judge_prompt = crate::moa::build_judge_prompt(&original_prompt, &worker_responses);

    let judge_config = &state.config.judge;
    let judge_url =
        crate::proxy::build_chat_url(&judge_config.endpoint);

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

    let judge_result = {
        let mut req_builder = state.client.post(&judge_url).json(&judge_body);
        req_builder = crate::proxy::add_bearer_auth(req_builder, judge_config.api_key.as_deref());
        req_builder.send().await
    };
    match judge_result {
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

            tokio::spawn(async move {
                let mut stream = resp.bytes_stream();
                cancel.cancel();
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
                                        let _ = tx.send(Ok(Event::default().data("[DONE]"))).await;
                                        break;
                                    }
                                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data) {
                                        if let Some(content) = parsed["choices"]
                                            .as_array()
                                            .and_then(|c| c.first())
                                            .and_then(|c| c.get("delta"))
                                            .and_then(|d| d.get("content"))
                                            .and_then(|c| c.as_str())
                                        {
                                            let evt = streaming_event(
                                                &sse_id, created, &sse_model,
                                                Some(content), 0, None,
                                            );
                                            let _ = tx.send(Ok(evt)).await;
                                        }
                                    }
                                }
                            }
                        }
                        Err(_) => break,
                    }
                }
                let _ = tx.send(Ok(Event::default().data("[DONE]"))).await;
            });

            let rx_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
            let merged = keepalive
                .map(|e| -> Result<Event, Infallible> { e })
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

/// Default SSE chunk size in bytes for streaming fusion analysis output.
const SSE_CHUNK_SIZE: usize = 80;

/// Split text into chunks of approximately `size` characters for streaming.
fn split_into_chunks(text: &str, size: usize) -> Vec<String> {
    if size == 0 {
        return vec![text.to_string()];
    }
    text.as_bytes()
        .chunks(size)
        .map(|chunk| String::from_utf8_lossy(chunk).to_string())
        .collect()
}

/// Response type for the chat completions endpoint.
pub(crate) enum ChatResponse {
    Json(axum::Json<ChatCompletionResponse>),
    Stream(Sse<PinBoxStream>),
    Raw(axum::response::Response),
}

pub(crate) type PinBoxStream =
    std::pin::Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>>;

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
    use crate::types::ToolCall;

    fn test_state() -> AppState {
        let config = Config {
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
            fusion: Default::default(),
        };
        AppState::new(config)
    }

    fn chat_app() -> axum::Router {
        axum::Router::new()
            .route(
                "/v1/chat/completions",
                axum::routing::post(chat_completions),
            )
            .with_state(test_state())
    }

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

    async fn post_json_with_header(
        body: serde_json::Value,
        header_name: &str,
        header_value: &str,
    ) -> axum::http::Response<Body> {
        chat_app()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/chat/completions")
                    .header("content-type", "application/json")
                    .header(header_name, header_value)
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

    // --- Validation tests ---

    #[tokio::test]
    async fn test_missing_model_returns_400() {
        let resp = post_json(json!({
            "messages": [{"role": "user", "content": "Hello"}]
        }))
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = response_body(resp).await;
        assert!(body["error"]["message"].as_str().unwrap().contains("model"));
    }

    #[tokio::test]
    async fn test_empty_model_returns_400() {
        let resp = post_json(json!({
            "model": "",
            "messages": [{"role": "user", "content": "Hello"}]
        }))
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_missing_messages_returns_400() {
        let resp = post_json(json!({"model": "llama3"})).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_empty_messages_returns_400() {
        let resp = post_json(json!({"model": "llama3", "messages": []})).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_empty_role_returns_400() {
        let resp = post_json(json!({
            "model": "llama3",
            "messages": [{"role": "", "content": "Hello"}]
        }))
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_empty_content_returns_400() {
        let resp = post_json(json!({
            "model": "llama3",
            "messages": [{"role": "user", "content": ""}]
        }))
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_empty_body_returns_400() {
        let resp = post_json(json!({})).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    // --- Routing tests ---

    fn test_config_with_fusion() -> Config {
        let mut config = Config {
            port: 9999,
            workers: vec![],
            judge: crate::config::ModelConfig {
                name: "judge".into(),
                endpoint: "http://localhost:1234".into(),
                model_id: "judge-id".into(),
                api_key: None,
            },
            executor: crate::config::ModelConfig {
                name: "executor".into(),
                endpoint: "http://localhost:1234".into(),
                model_id: "exec-id".into(),
                api_key: None,
            },
            workspaces: std::collections::HashMap::new(),
            error_keywords: vec![],
            fusion: Default::default(),
        };
        config.fusion.models.insert(
            "model-a".into(),
            crate::config::ModelEntry {
                provider: "test".into(),
                endpoint: "http://localhost:1234/v1".into(),
                model_id: "model-a-id".into(),
                api_key: Some("key-a".into()),
            },
        );
        config.fusion.models.insert(
            "model-b".into(),
            crate::config::ModelEntry {
                provider: "test".into(),
                endpoint: "http://localhost:1234/v1".into(),
                model_id: "model-b-id".into(),
                api_key: Some("key-b".into()),
            },
        );
        config
    }

    #[test]
    fn test_routing_subrequest_passthrough() {
        let guard = FusionGuard { is_subrequest: true };
        let req = ChatCompletionRequest {
            model: "tinyfusion/fusion".into(),
            messages: vec![Message::user("test")],
            stream: false,
            session_id: None,
            workspace: None,
            tools: None,
        };
        let config = test_config_with_fusion();
        let decision = decide_route(&guard, &req, &config);
        assert!(matches!(decision, RoutingDecision::SubrequestPassthrough));
    }

    #[test]
    fn test_routing_fusion_model_alias() {
        let guard = FusionGuard { is_subrequest: false };
        let req = ChatCompletionRequest {
            model: "tinyfusion/fusion".into(),
            messages: vec![Message::user("test")],
            stream: false,
            session_id: None,
            workspace: None,
            tools: None,
        };
        let config = test_config_with_fusion();
        let decision = decide_route(&guard, &req, &config);
        assert!(matches!(decision, RoutingDecision::FusionPipeline(_)));
    }

    #[test]
    fn test_routing_standard_passthrough() {
        let guard = FusionGuard { is_subrequest: false };
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![Message::user("test")],
            stream: false,
            session_id: None,
            workspace: None,
            tools: None,
        };
        let config = test_config_with_fusion();
        let decision = decide_route(&guard, &req, &config);
        assert!(matches!(decision, RoutingDecision::StandardPassthrough));
    }

    #[test]
    fn test_routing_fusion_tool_in_tools_array() {
        let guard = FusionGuard { is_subrequest: false };
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![Message::user("test")],
            stream: false,
            session_id: None,
            workspace: None,
            tools: Some(vec![serde_json::json!({
                "type": "function",
                "function": {
                    "name": "tinyfusion_deliberate",
                    "description": "Fusion tool"
                }
            })]),
        };
        let config = test_config_with_fusion();
        let decision = decide_route(&guard, &req, &config);
        assert!(matches!(decision, RoutingDecision::FusionPipeline(_)));
    }

    #[test]
    fn test_routing_fusion_tool_call_in_assistant_message() {
        let guard = FusionGuard { is_subrequest: false };
        let req = ChatCompletionRequest {
            model: "gpt-4o".into(),
            messages: vec![
                Message::user("test"),
                Message {
                    role: "assistant".into(),
                    content: None,
                    name: None,
                    tool_calls: Some(vec![crate::types::ToolCall {
                        id: "call_1".into(),
                        call_type: "function".into(),
                        function: crate::types::FunctionCall {
                            name: "tinyfusion_deliberate".into(),
                            arguments: r#"{"analysis_models":["model-a","model-b"]}"#.into(),
                        },
                    }]),
                    tool_call_id: None,
                },
            ],
            stream: false,
            session_id: None,
            workspace: None,
            tools: None,
        };
        let config = test_config_with_fusion();
        let decision = decide_route(&guard, &req, &config);
        assert!(matches!(decision, RoutingDecision::FusionPipeline(_)));
    }

    #[test]
    fn test_routing_legacy_diagnostic_without_fusion_models() {
        let guard = FusionGuard { is_subrequest: false };
        let req = ChatCompletionRequest {
            model: "llama3".into(),
            messages: vec![Message::user("I got a compile error")],
            stream: false,
            session_id: None,
            workspace: None,
            tools: None,
        };
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
            error_keywords: vec!["error".into(), "compile".into()],
            fusion: Default::default(),
        };
        let decision = decide_route(&guard, &req, &config);
        assert!(matches!(decision, RoutingDecision::LegacyDiagnostic));
    }

    // --- Integration: subrequest header forces passthrough ---

    #[tokio::test]
    async fn test_subrequest_header_forces_passthrough() {
        let resp = post_json_with_header(
            json!({
                "model": "tinyfusion/fusion",
                "messages": [{"role": "user", "content": "test"}]
            }),
            "x-tinyfusion-subrequest",
            "1",
        )
        .await;
        // Should attempt passthrough (which fails since no server)
        assert!(
            resp.status() == StatusCode::BAD_GATEWAY || resp.status().is_server_error(),
            "Expected upstream error, got {}",
            resp.status()
        );
    }

    // --- Validate function unit tests ---

    #[test]
    fn test_validate_returns_parsed_request() {
        let raw = RawChatRequest {
            model: Some("test-model".into()),
            messages: Some(vec![Message::user("test")]),
            stream: Some(true),
            session_id: None,
            workspace: None,
            tools: None,
        };

        let req = validate(raw).unwrap();
        assert_eq!(req.model, "test-model");
        assert_eq!(req.messages.len(), 1);
        assert!(req.stream);
    }

    #[test]
    fn test_validate_allows_tool_messages_with_empty_content() {
        let raw = RawChatRequest {
            model: Some("model".into()),
            messages: Some(vec![
                Message::user("test"),
                Message {
                    role: "assistant".into(),
                    content: None,
                    name: None,
                    tool_calls: Some(vec![ToolCall {
                        id: "call_1".into(),
                        call_type: "function".into(),
                        function: crate::types::FunctionCall {
                            name: "test".into(),
                            arguments: "{}".into(),
                        },
                    }]),
                    tool_call_id: None,
                },
            ]),
            stream: None,
            session_id: None,
            workspace: None,
            tools: None,
        };

        let req = validate(raw);
        assert!(req.is_ok());
    }

    #[test]
    fn test_split_into_chunks() {
        let text = "Hello, this is a test string for chunking.";
        let chunks = split_into_chunks(text, 10);
        assert!(chunks.len() > 1);
        let reassembled: String = chunks.concat();
        assert_eq!(reassembled, text);
    }

    #[test]
    fn test_split_into_chunks_zero_size() {
        let text = "Hello world";
        let chunks = split_into_chunks(text, 0);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], text);
    }

    #[test]
    fn test_split_into_chunks_empty() {
        let chunks = split_into_chunks("", 10);
        assert!(chunks.is_empty() || (chunks.len() == 1 && chunks[0].is_empty()));
    }

    #[test]
    fn test_split_into_chunks_single_byte() {
        let text = "ABCDEF";
        let chunks = split_into_chunks(text, 1);
        assert_eq!(chunks.len(), 6);
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn test_split_into_chunks_unicode() {
        let text = "こんにちは世界";
        let chunks = split_into_chunks(text, 80);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], text);
    }

    // --- Integration: error path tests (no upstream server) ---

    #[tokio::test]
    async fn test_fusion_pipeline_error_returns_500() {
        // Fusion model alias with no fusion models configured → pipeline fails
        let resp = post_json(json!({
            "model": "tinyfusion/fusion",
            "messages": [{"role": "user", "content": "test"}]
        }))
        .await;
        // Fusion pipeline requires at least one model; without any, it fails
        assert!(
            resp.status().is_server_error() || resp.status() == StatusCode::BAD_GATEWAY,
            "Expected server error or bad gateway, got {}",
            resp.status()
        );
    }

    #[tokio::test]
    async fn test_standard_passthrough_upstream_failure() {
        // Standard model passthrough with no upstream server running
        let resp = post_json(json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "Hello"}]
        }))
        .await;
        // No fusion models configured for "gpt-4o", falls back to executor at localhost:11434
        assert!(
            resp.status() == StatusCode::BAD_GATEWAY || resp.status().is_server_error(),
            "Expected upstream error, got {}",
            resp.status()
        );
    }

    #[tokio::test]
    async fn test_legacy_diagnostic_upstream_failure() {
        // Error keywords should trigger legacy diagnostic path
        let resp = post_json(json!({
            "model": "llama3",
            "messages": [{"role": "user", "content": "I got a compile error"}]
        }))
        .await;
        // Legacy diagnostic tries judge which also fails without server
        assert!(
            resp.status() == StatusCode::BAD_GATEWAY || resp.status().is_server_error(),
            "Expected judge or upstream error, got {}",
            resp.status()
        );
    }
}
