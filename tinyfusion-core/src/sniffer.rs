/// Request state detected by inspecting message history.
#[derive(Debug, Clone, PartialEq)]
pub enum RequestState {
    /// Early conversation or error context — route to MoA diagnostic.
    Diagnostic,
    /// Execution phase — route to executor worker.
    Execution,
}

/// Inspect the messages array of a chat completion request to determine state.
///
/// Rules:
/// - Diagnostic: last message contains error keywords (or messages are empty)
/// - Execution: messages contain </final_plan> tag, or no error keywords found
/// - Default: Execution (simple chat goes to the fast executor path)
pub fn sniff_state(messages: &[Message]) -> RequestState {
    sniff_state_with_keywords(messages, &[])
}

/// Inspect messages using caller-provided error keywords (from config).
pub fn sniff_state_with_keywords(
    messages: &[Message],
    error_keywords: &[String],
) -> RequestState {
    if messages.is_empty() {
        return RequestState::Diagnostic;
    }

    for msg in messages {
        if msg.content.contains("</final_plan>") {
            return RequestState::Execution;
        }
    }

    if let Some(last) = messages.last() {
        let lower = last.content.to_lowercase();
        let matches_keyword = error_keywords
            .iter()
            .any(|kw| lower.contains(&kw.to_lowercase()));
        if matches_keyword {
            return RequestState::Diagnostic;
        }
        let matches_default = DEFAULT_ERROR_KEYWORDS.iter().any(|kw| lower.contains(kw));
        if matches_default {
            return RequestState::Diagnostic;
        }
    }

    RequestState::Execution
}

static DEFAULT_ERROR_KEYWORDS: &[&str] = &[
    "stack trace",
    "compile error",
    "test failed",
    "stacktrace",
    "compilation error",
    "tests failed",
    "build failed",
    "assertion error",
    "panic",
];

use serde::{Deserialize, Serialize};

/// A single message in a chat completion request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_messages_is_diagnostic() {
        assert_eq!(sniff_state(&[]), RequestState::Diagnostic);
    }

    #[test]
    fn test_short_conversation_without_errors_is_execution() {
        let messages = vec![
            Message { role: "system".into(), content: "You are helpful".into() },
            Message { role: "user".into(), content: "How do I fix this?".into() },
        ];
        assert_eq!(sniff_state(&messages), RequestState::Execution);
    }

    #[test]
    fn test_error_keyword_detection() {
        let messages = vec![
            Message { role: "user".into(), content: "I got a stack trace: NullPointerException".into() },
        ];
        assert_eq!(sniff_state(&messages), RequestState::Diagnostic);
    }

    #[test]
    fn test_final_plan_detection() {
        let messages = vec![
            Message { role: "system".into(), content: "Plan it".into() },
            Message { role: "user".into(), content: "Do it".into() },
            Message { role: "assistant".into(), content: "</final_plan> Step 1: ...".into() },
        ];
        assert_eq!(sniff_state(&messages), RequestState::Execution);
    }

    #[test]
    fn test_compile_error_detection() {
        let messages = vec![
            Message { role: "user".into(), content: "compile error: cannot find value `x`".into() },
        ];
        assert_eq!(sniff_state(&messages), RequestState::Diagnostic);
    }

    #[test]
    fn test_build_failed_detection() {
        let messages = vec![
            Message { role: "user".into(), content: "build failed with exit code 1".into() },
        ];
        assert_eq!(sniff_state(&messages), RequestState::Diagnostic);
    }

    #[test]
    fn test_assertion_error_detection() {
        let messages = vec![
            Message { role: "user".into(), content: "assertion error: expected true, got false".into() },
        ];
        assert_eq!(sniff_state(&messages), RequestState::Diagnostic);
    }

    #[test]
    fn test_panic_detection() {
        let messages = vec![
            Message { role: "user".into(), content: "thread 'main' panicked at src/main.rs:42".into() },
        ];
        assert_eq!(sniff_state(&messages), RequestState::Diagnostic);
    }

    #[test]
    fn test_sniff_with_custom_keywords() {
        let messages = vec![
            Message { role: "user".into(), content: "segfault at address 0x0".into() },
        ];
        let custom_kw = vec!["segfault".into()];
        assert_eq!(
            sniff_state_with_keywords(&messages, &custom_kw),
            RequestState::Diagnostic
        );
    }

    #[test]
    fn test_no_error_keywords_routes_to_execution() {
        let messages = vec![
            Message { role: "user".into(), content: "The production is running fine".into() },
        ];
        assert_eq!(sniff_state(&messages), RequestState::Execution);
    }
}
