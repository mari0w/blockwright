use std::{
    path::PathBuf,
    process::{ExitStatus, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::Value;
use tokio::{
    io::AsyncWriteExt,
    process::Command,
    time::{timeout, Duration},
};

use crate::config::CodexConfig;

#[derive(Clone)]
pub struct CodexClient {
    config: CodexConfig,
}

impl CodexClient {
    pub fn new(config: CodexConfig) -> Self {
        Self { config }
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    pub async fn ask(
        &self,
        prompt: &str,
    ) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
        if !self.config.enabled {
            return Ok(None);
        }

        let (program, args) = command_parts(&self.config.command)?;
        let last_message_path = codex_last_message_path();
        tracing::info!(
            command = %self.config.command,
            timeout_seconds = self.config.timeout_seconds,
            "starting codex cli request"
        );

        let started_at = std::time::Instant::now();
        let output = timeout(
            Duration::from_secs(self.config.timeout_seconds),
            run_codex_exec(program, &args, prompt, &last_message_path),
        )
        .await
        .map_err(|_| {
            format!(
                "codex command timed out after {} seconds",
                self.config.timeout_seconds
            )
        })??;
        let elapsed_ms = started_at.elapsed().as_millis();

        if !output.status.success() {
            let _ = tokio::fs::remove_file(&last_message_path).await;
            tracing::warn!(
                command = %self.config.command,
                elapsed_ms,
                status = %output.status,
                "codex cli request failed"
            );
            return Err(codex_failure_message(output.status, &output.stderr).into());
        }

        let last_message = tokio::fs::read_to_string(&last_message_path)
            .await
            .unwrap_or_default();
        let _ = tokio::fs::remove_file(&last_message_path).await;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let answer = if last_message.trim().is_empty() {
            stdout.trim()
        } else {
            last_message.trim()
        };

        tracing::info!(
            command = %self.config.command,
            elapsed_ms,
            "finished codex cli request"
        );
        Ok(Some(answer.to_string()))
    }
}

fn command_parts(
    command: &str,
) -> Result<(&str, Vec<&str>), Box<dyn std::error::Error + Send + Sync>> {
    let mut parts = command.split_whitespace();
    let program = parts
        .next()
        .ok_or_else(|| "codex command cannot be empty".to_string())?;
    let mut args: Vec<&str> = parts.collect();
    if args.first() == Some(&"exec") {
        args.remove(0);
    }
    Ok((program, args))
}

async fn run_codex_exec(
    program: &str,
    args: &[&str],
    prompt: &str,
    last_message_path: &PathBuf,
) -> std::io::Result<std::process::Output> {
    let mut command = Command::new(program);
    command
        .arg("exec")
        .args(args)
        .arg("--ephemeral")
        .arg("--output-last-message")
        .arg(last_message_path)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = command.spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(prompt.as_bytes()).await?;
    }

    child.wait_with_output().await
}

fn codex_last_message_path() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!(
        "blockwright-codex-last-message-{}-{nanos}.txt",
        std::process::id()
    ))
}

fn codex_failure_message(status: ExitStatus, stderr: &[u8]) -> String {
    let stderr_text = String::from_utf8_lossy(stderr);
    if let Some(api_error) = extract_codex_api_error(&stderr_text) {
        return format!("codex command exited with {status}: {api_error}");
    }

    format!(
        "codex command exited with {status}; stderr omitted to avoid leaking prompts ({} bytes)",
        stderr.len()
    )
}

fn extract_codex_api_error(stderr: &str) -> Option<String> {
    for line in stderr.lines().rev() {
        let Some(json) = line.trim().strip_prefix("ERROR:") else {
            continue;
        };
        let json = json.trim();
        let value = serde_json::from_str::<Value>(json).ok()?;
        let message = value.pointer("/error/message")?.as_str()?;
        let status = value
            .get("status")
            .and_then(Value::as_i64)
            .map(|status| format!("status={status} "))
            .unwrap_or_default();
        let error_type = value
            .pointer("/error/type")
            .and_then(Value::as_str)
            .unwrap_or("unknown_error");

        return Some(format!("{status}type={error_type}: {message}"));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;

    fn disabled_client() -> CodexClient {
        CodexClient::new(CodexConfig {
            enabled: false,
            command: "codex".to_string(),
            timeout_seconds: 1,
        })
    }

    #[tokio::test]
    async fn disabled_client_returns_none_without_running_command() {
        let output = disabled_client().ask("hello").await.unwrap();

        assert!(output.is_none());
    }

    #[test]
    fn command_parts_supports_cli_options() {
        let (program, args) = command_parts("codex --oss --local-provider ollama").unwrap();

        assert_eq!(program, "codex");
        assert_eq!(args, vec!["--oss", "--local-provider", "ollama"]);
    }

    #[test]
    fn command_parts_tolerates_exec_prefix() {
        let (program, args) = command_parts("codex exec --profile local").unwrap();

        assert_eq!(program, "codex");
        assert_eq!(args, vec!["--profile", "local"]);
    }

    #[test]
    fn failure_message_extracts_api_error_without_prompt() {
        let stderr = r#"user
给我钻石斧头
ERROR: {"type":"error","status":400,"error":{"type":"invalid_request_error","message":"The 'gpt-5.5' model requires a newer version of Codex."}}"#;

        let message = codex_failure_message(ExitStatus::from_raw(1), stderr.as_bytes());

        assert!(message.contains("status=400"));
        assert!(message.contains("gpt-5.5"));
        assert!(!message.contains("给我钻石斧头"));
    }

    #[test]
    fn failure_message_omits_unstructured_stderr() {
        let stderr = "user\n给我钻石斧头\n<html>blocked</html>";

        let message = codex_failure_message(ExitStatus::from_raw(1), stderr.as_bytes());

        assert!(message.contains("stderr omitted"));
        assert!(!message.contains("给我钻石斧头"));
        assert!(!message.contains("<html>"));
    }
}
