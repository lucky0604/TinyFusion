use serde::{Deserialize, Serialize};

/// A single message in a chat completion request (OpenAI-compatible).
///
/// Extends the standard schema with `tool_calls` and `tool_call_id`
/// to support the Fusion deliberation pipeline's tool-calling orchestration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    pub fn user(content: &str) -> Self {
        Self {
            role: "user".into(),
            content: Some(content.into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn assistant(content: &str) -> Self {
        Self {
            role: "assistant".into(),
            content: Some(content.into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn system(content: &str) -> Self {
        Self {
            role: "system".into(),
            content: Some(content.into()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn tool(tool_call_id: &str, content: &str) -> Self {
        Self {
            role: "tool".into(),
            content: Some(content.into()),
            name: None,
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }

    /// Helper: get content as &str, defaulting to "" if None.
    pub fn content_str(&self) -> &str {
        self.content.as_deref().unwrap_or("")
    }
}

/// A tool call emitted by the model in an assistant message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

/// The function name and arguments within a tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

/// Arguments for the `tinyfusion_deliberate` tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliberateArgs {
    pub analysis_models: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub judge_model: Option<String>,
}

/// Depth guard header extracted from the request.
/// When `is_subrequest` is true, the request is an internal sub-call
/// and must be forced into the single-model passthrough path to prevent recursion.
#[derive(Debug, Clone, Copy)]
pub struct FusionGuard {
    pub is_subrequest: bool,
}

impl FusionGuard {
    pub const HEADER_NAME: &'static str = "x-tinyfusion-subrequest";

    pub fn from_headers(headers: &axum::http::HeaderMap) -> Self {
        let is_subrequest = headers
            .get(Self::HEADER_NAME)
            .and_then(|v| v.to_str().ok())
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        Self { is_subrequest }
    }
}

/// Model alias for the Fusion deliberation pipeline.
pub const FUSION_MODEL_ALIAS: &str = "tinyfusion";
/// Tool name used for the Fusion deliberation pipeline.
pub const FUSION_TOOL_NAME: &str = "tinyfusion_deliberate";

/// Structured analysis output from the Judge model.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StructuredAnalysis {
    #[serde(default)]
    pub consensus: Vec<String>,
    #[serde(default)]
    pub contradictions: Vec<Contradiction>,
    #[serde(default)]
    pub partial_coverage: Vec<PartialCoverage>,
    #[serde(default)]
    pub unique_insights: Vec<UniqueInsight>,
    #[serde(default)]
    pub blind_spots: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contradiction {
    pub topic: String,
    pub stances: Vec<Stance>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stance {
    pub model: String,
    pub stance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialCoverage {
    pub models: Vec<String>,
    pub point: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniqueInsight {
    pub model: String,
    pub insight: String,
}

/// A single panel model's response.
#[derive(Debug, Clone)]
pub struct PanelResponse {
    pub model_name: String,
    pub content: Result<String, String>,
    pub latency_ms: u64,
    pub token_count: Option<u64>,
}

/// Pipeline metrics for a single request lifecycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestMetrics {
    pub request_id: String,
    pub timestamp: u64,
    pub request_type: String,
    pub total_latency_ms: u64,
    pub outer_model: String,
    pub panel_models: Vec<String>,
    pub judge_model: String,
    pub panel_latencies_ms: Vec<u64>,
    pub judge_latency_ms: u64,
    pub refiner_latency_ms: u64,
    pub consensus_count: usize,
    pub contradiction_count: usize,
    pub blind_spot_count: usize,
    pub panel_success_count: usize,
    pub panel_failure_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_user() {
        let msg = Message::user("hello");
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content_str(), "hello");
        assert!(msg.tool_calls.is_none());
    }

    #[test]
    fn test_message_tool() {
        let msg = Message::tool("call_123", "result data");
        assert_eq!(msg.role, "tool");
        assert_eq!(msg.tool_call_id.as_deref(), Some("call_123"));
        assert_eq!(msg.content_str(), "result data");
    }

    #[test]
    fn test_message_serialization_skips_none() {
        let msg = Message::user("test");
        let json = serde_json::to_string(&msg).unwrap();
        assert!(!json.contains("tool_calls"));
        assert!(!json.contains("tool_call_id"));
        assert!(!json.contains("name"));
    }

    #[test]
    fn test_message_deserialization_with_tool_calls() {
        let json = r#"{
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "call_abc",
                "type": "function",
                "function": {
                    "name": "tinyfusion_deliberate",
                    "arguments": "{\"analysis_models\":[\"gpt-4o\",\"claude-3-5-sonnet\"]}"
                }
            }]
        }"#;
        let msg: Message = serde_json::from_str(json).unwrap();
        assert_eq!(msg.role, "assistant");
        assert!(msg.content.is_none());
        let tc = msg.tool_calls.as_ref().unwrap();
        assert_eq!(tc.len(), 1);
        assert_eq!(tc[0].function.name, "tinyfusion_deliberate");
    }

    #[test]
    fn test_deliberate_args_deserialization() {
        let json = r#"{"analysis_models":["gpt-4o","claude-3-5-sonnet"],"judge_model":"gpt-4o"}"#;
        let args: DeliberateArgs = serde_json::from_str(json).unwrap();
        assert_eq!(args.analysis_models.len(), 2);
        assert_eq!(args.judge_model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    fn test_structured_analysis_deserialization() {
        let json = r#"{
            "consensus": ["point1"],
            "contradictions": [],
            "partial_coverage": [],
            "unique_insights": [{"model": "gpt-4o", "insight": "edge case"}],
            "blind_spots": ["missing auth check"]
        }"#;
        let analysis: StructuredAnalysis = serde_json::from_str(json).unwrap();
        assert_eq!(analysis.consensus.len(), 1);
        assert_eq!(analysis.unique_insights.len(), 1);
        assert_eq!(analysis.blind_spots.len(), 1);
    }

    #[test]
    fn test_fusion_guard_no_header() {
        let headers = axum::http::HeaderMap::new();
        let guard = FusionGuard::from_headers(&headers);
        assert!(!guard.is_subrequest);
    }

    #[test]
    fn test_fusion_guard_with_header() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-tinyfusion-subrequest", "1".parse().unwrap());
        let guard = FusionGuard::from_headers(&headers);
        assert!(guard.is_subrequest);
    }

    #[test]
    fn test_fusion_guard_true_string() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-tinyfusion-subrequest", "true".parse().unwrap());
        let guard = FusionGuard::from_headers(&headers);
        assert!(guard.is_subrequest);
    }

    #[test]
    fn test_fusion_guard_false_string() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-tinyfusion-subrequest", "0".parse().unwrap());
        let guard = FusionGuard::from_headers(&headers);
        assert!(!guard.is_subrequest);
    }

    #[test]
    fn test_fusion_guard_invalid_value() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-tinyfusion-subrequest", "yes".parse().unwrap());
        let guard = FusionGuard::from_headers(&headers);
        assert!(!guard.is_subrequest);
    }

    #[test]
    fn test_fusion_guard_empty_value() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-tinyfusion-subrequest", "".parse().unwrap());
        let guard = FusionGuard::from_headers(&headers);
        assert!(!guard.is_subrequest);
    }
}
