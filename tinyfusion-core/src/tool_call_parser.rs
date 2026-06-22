//! Streaming SSE tool-call parser.
//!
//! Incrementally consumes SSE `data:` lines from an upstream model's response,
//! accumulates `tool_calls` deltas, and detects when a complete
//! `tinyfusion_deliberate` tool call has been assembled.

use crate::types::{DeliberateArgs, FunctionCall, ToolCall, FUSION_TOOL_NAME};

/// Accumulated state from streaming SSE chunks.
#[derive(Debug, Clone)]
pub struct StreamingToolCallAccumulator {
    /// Accumulated tool calls indexed by their position in the tool_calls array.
    tool_calls: Vec<PartialToolCall>,
    /// Accumulated text content (for non-tool-call responses).
    pub text_content: String,
    /// Whether the stream has finished (received [DONE]).
    pub done: bool,
    /// The finish_reason from the last chunk.
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct PartialToolCall {
    id: String,
    call_type: String,
    function_name: String,
    function_arguments: String,
}

/// Result of feeding a chunk to the accumulator.
#[derive(Debug)]
pub enum ParseResult {
    /// Regular text content delta.
    TextDelta(String),
    /// A complete tool call for `tinyfusion_deliberate` was detected.
    FusionToolCallComplete(ToolCall, DeliberateArgs),
    /// A non-fusion tool call was completed.
    OtherToolCallComplete(ToolCall),
    /// Stream finished with [DONE].
    Done,
    /// Nothing actionable in this chunk.
    Continue,
}

impl Default for StreamingToolCallAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamingToolCallAccumulator {
    pub fn new() -> Self {
        Self {
            tool_calls: Vec::new(),
            text_content: String::new(),
            done: false,
            finish_reason: None,
        }
    }

    /// Feed a raw SSE line (the part after "data: ") to the accumulator.
    /// Returns a ParseResult indicating what happened.
    pub fn feed_sse_data(&mut self, data: &str) -> ParseResult {
        let data = data.trim();

        if data == "[DONE]" {
            self.done = true;
            return ParseResult::Done;
        }

        let parsed: serde_json::Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => return ParseResult::Continue,
        };

        let choices = match parsed["choices"].as_array() {
            Some(c) => c,
            None => return ParseResult::Continue,
        };

        let choice = match choices.first() {
            Some(c) => c,
            None => return ParseResult::Continue,
        };

        if let Some(fr) = choice.get("finish_reason").and_then(|v| v.as_str()) {
            self.finish_reason = Some(fr.to_string());
        }

        let delta = match choice.get("delta") {
            Some(d) => d,
            None => return ParseResult::Continue,
        };

        if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
            self.text_content.push_str(content);
            return ParseResult::TextDelta(content.to_string());
        }

        if let Some(tool_calls) = delta.get("tool_calls").and_then(|tc| tc.as_array()) {
            for tc in tool_calls {
                let index = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;

                while self.tool_calls.len() <= index {
                    self.tool_calls.push(PartialToolCall::default());
                }

                let partial = &mut self.tool_calls[index];

                if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                    partial.id = id.to_string();
                }
                if let Some(t) = tc.get("type").and_then(|v| v.as_str()) {
                    partial.call_type = t.to_string();
                }
                if let Some(func) = tc.get("function") {
                    if let Some(name) = func.get("name").and_then(|v| v.as_str()) {
                        partial.function_name.push_str(name);
                    }
                    if let Some(args) = func.get("arguments").and_then(|v| v.as_str()) {
                        partial.function_arguments.push_str(args);
                    }
                }
            }
        }

        if self.finish_reason.is_some() {
            return self.try_complete_tool_calls();
        }

        ParseResult::Continue
    }

    /// Try to parse completed tool calls when finish_reason is received.
    fn try_complete_tool_calls(&self) -> ParseResult {
        for partial in &self.tool_calls {
            if partial.function_name.is_empty() {
                continue;
            }

            let tool_call = ToolCall {
                id: partial.id.clone(),
                call_type: if partial.call_type.is_empty() {
                    "function".into()
                } else {
                    partial.call_type.clone()
                },
                function: FunctionCall {
                    name: partial.function_name.clone(),
                    arguments: partial.function_arguments.clone(),
                },
            };

            if partial.function_name == FUSION_TOOL_NAME {
                if let Ok(args) = serde_json::from_str::<DeliberateArgs>(&partial.function_arguments) {
                    return ParseResult::FusionToolCallComplete(tool_call, args);
                }
            }

            return ParseResult::OtherToolCallComplete(tool_call);
        }

        ParseResult::Continue
    }

    /// Check whether we have accumulated enough of the arguments to detect
    /// the `analysis_models` array for early fan-out.
    /// Returns the partial list if parseable.
    pub fn try_early_fanout(&self) -> Option<Vec<String>> {
        for partial in &self.tool_calls {
            if partial.function_name != FUSION_TOOL_NAME {
                continue;
            }

            let args = &partial.function_arguments;
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(args) {
                if let Some(models) = parsed.get("analysis_models").and_then(|m| m.as_array()) {
                    let names: Vec<String> = models
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect();
                    if !names.is_empty() {
                        return Some(names);
                    }
                }
            }

            // Partial JSON: try to find a complete array even in incomplete JSON
            if let Some(start) = args.find("[") {
                if let Some(end) = args.find("]") {
                    if end > start {
                        let array_str = &args[start..=end];
                        if let Ok(arr) = serde_json::from_str::<Vec<String>>(array_str) {
                            if !arr.is_empty() {
                                return Some(arr);
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// Get the assembled tool calls (after stream is done).
    pub fn into_tool_calls(self) -> Vec<ToolCall> {
        self.tool_calls
            .into_iter()
            .filter(|p| !p.function_name.is_empty())
            .map(|p| ToolCall {
                id: p.id,
                call_type: if p.call_type.is_empty() {
                    "function".into()
                } else {
                    p.call_type
                },
                function: FunctionCall {
                    name: p.function_name,
                    arguments: p.function_arguments,
                },
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_accumulate_text_content() {
        let mut acc = StreamingToolCallAccumulator::new();

        let r1 = acc.feed_sse_data(r#"{"choices":[{"index":0,"delta":{"role":"assistant","content":"Hello"}}]}"#);
        assert!(matches!(r1, ParseResult::TextDelta(ref s) if s == "Hello"));

        let r2 = acc.feed_sse_data(r#"{"choices":[{"index":0,"delta":{"content":" world"}}]}"#);
        assert!(matches!(r2, ParseResult::TextDelta(ref s) if s == " world"));

        assert_eq!(acc.text_content, "Hello world");
    }

    #[test]
    fn test_accumulate_tool_call_chunks() {
        let mut acc = StreamingToolCallAccumulator::new();

        // First chunk: tool call id + name start
        acc.feed_sse_data(r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_abc","type":"function","function":{"name":"tinyfusion_deliberate","arguments":""}}]}}]}"#);

        // Arguments come in chunks
        acc.feed_sse_data(r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"analysis_models\":"}}]}}]}"#);
        acc.feed_sse_data(r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"[\"gpt-4o\",\"claude\"]}"}}]}}]}"#);

        // Finish
        let result = acc.feed_sse_data(r#"{"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#);

        match result {
            ParseResult::FusionToolCallComplete(tc, args) => {
                assert_eq!(tc.id, "call_abc");
                assert_eq!(tc.function.name, "tinyfusion_deliberate");
                assert_eq!(args.analysis_models, vec!["gpt-4o", "claude"]);
            }
            other => panic!("Expected FusionToolCallComplete, got {:?}", other),
        }
    }

    #[test]
    fn test_early_fanout_detection() {
        let mut acc = StreamingToolCallAccumulator::new();

        acc.feed_sse_data(r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"tinyfusion_deliberate","arguments":""}}]}}]}"#);
        acc.feed_sse_data(r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"analysis_models\":[\"gpt-4o\",\"claude\"]}"}}]}}]}"#);

        let models = acc.try_early_fanout();
        assert_eq!(models, Some(vec!["gpt-4o".to_string(), "claude".to_string()]));
    }

    #[test]
    fn test_done_marker() {
        let mut acc = StreamingToolCallAccumulator::new();
        let result = acc.feed_sse_data("[DONE]");
        assert!(matches!(result, ParseResult::Done));
        assert!(acc.done);
    }

    #[test]
    fn test_malformed_data_returns_continue() {
        let mut acc = StreamingToolCallAccumulator::new();
        let result = acc.feed_sse_data("not json");
        assert!(matches!(result, ParseResult::Continue));
    }
}
