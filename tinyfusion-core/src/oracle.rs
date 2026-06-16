/// Oracle module — runs verification commands and captures their output.
///
/// The oracle executes workspace verify commands and determines success/failure
/// based on exit codes, feeding results back into the retry loop.

use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

/// Result of a verification command execution.
#[derive(Debug)]
pub struct VerifyResult {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

impl VerifyResult {
    /// Whether the command succeeded (exit code 0 and no timeout).
    pub fn is_success(&self) -> bool {
        !self.timed_out && self.exit_code == Some(0)
    }
}

/// Run a verification command with timeout.
///
/// Returns the captured stdout, stderr, and exit code.
pub async fn run_verify(
    command: &str,
    working_dir: &str,
    timeout_secs: u64,
) -> Result<VerifyResult, std::io::Error> {
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", command]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", command]);
        c
    };

    cmd.current_dir(working_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let result = timeout(Duration::from_secs(timeout_secs), cmd.output()).await;

    match result {
        Ok(Ok(output)) => Ok(VerifyResult {
            exit_code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            timed_out: false,
        }),
        Ok(Err(e)) => Err(e),
        Err(_) => {
            // Timeout — process is killed by timeout
            Ok(VerifyResult {
                exit_code: None,
                stdout: String::new(),
                stderr: format!("Command timed out after {} seconds", timeout_secs),
                timed_out: true,
            })
        }
    }
}

/// Format a verification failure as a message to inject into the conversation.
pub fn format_error_message(result: &VerifyResult) -> String {
    let exit_info = match result.exit_code {
        Some(code) => format!("exit code {}", code),
        None => "timeout".into(),
    };

    format!(
        "Local verification failed with {}. Stderr:\n```\n{}\n```\nPlease re-analyze.",
        exit_info, result.stderr
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_successful_command() {
        let result = run_verify("echo hello", "/tmp", 10).await.unwrap();
        assert!(result.is_success());
        assert!(result.stdout.contains("hello"));
    }

    #[tokio::test]
    async fn test_failed_command() {
        let result = run_verify("exit 1", "/tmp", 10).await.unwrap();
        assert!(!result.is_success());
        assert_eq!(result.exit_code, Some(1));
    }

    #[tokio::test]
    async fn test_command_timeout() {
        let result = run_verify("sleep 30", "/tmp", 1).await.unwrap();
        assert!(result.timed_out);
        assert!(!result.is_success());
    }

    #[test]
    fn test_error_message_formatting() {
        let result = VerifyResult {
            exit_code: Some(1),
            stdout: String::new(),
            stderr: "error: something broke".into(),
            timed_out: false,
        };
        let msg = format_error_message(&result);
        assert!(msg.contains("exit code 1"));
        assert!(msg.contains("something broke"));
        assert!(msg.contains("Please re-analyze"));
    }
}
