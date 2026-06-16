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
/// - Diagnostic: message_count <= 2 OR last message contains error keywords
/// - Execution: messages contain </final_plan> tag
/// - Default: Diagnostic
pub fn sniff_state(messages: &[Message]) -> RequestState {
    if messages.is_empty() {
        return RequestState::Diagnostic;
    }

    // Check for execution marker first (higher priority)
    for msg in messages {
        if msg.content.contains("</final_plan>") {
            return RequestState::Execution;
        }
    }

    // Check for error keywords in the last message
    if let Some(last) = messages.last() {
        let lower = last.content.to_lowercase();
        let error_keywords = [
            "stack trace",
            "compile error",
            "test failed",
            "stacktrace",
            "compilation error",
            "tests failed",
        ];
        if error_keywords.iter().any(|kw| lower.contains(kw)) {
            return RequestState::Diagnostic;
        }
    }

    // Short conversations are diagnostic
    if messages.len() <= 2 {
        return RequestState::Diagnostic;
    }

    RequestState::Diagnostic
}

/// A single message in a chat completion request.
#[derive(Debug, Clone)]
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
    fn test_short_conversation_is_diagnostic() {
        let messages = vec![
            Message { role: "system".into(), content: "You are helpful".into() },
            Message { role: "user".into(), content: "How do I fix this?".into() },
        ];
        assert_eq!(sniff_state(&messages), RequestState::Diagnostic);
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
}
