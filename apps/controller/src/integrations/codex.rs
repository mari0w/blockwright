use tokio::process::Command;
use tokio::time::{timeout, Duration};

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
        let mut command = Command::new(program);
        command.args(args);
        let output = timeout(
            Duration::from_secs(self.config.timeout_seconds),
            command.arg("exec").arg("--").arg(prompt).output(),
        )
        .await??;

        if !output.status.success() {
            return Err(format!(
                "codex command exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            )
            .into());
        }

        Ok(Some(
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
        ))
    }
}

fn command_parts(
    command: &str,
) -> Result<(&str, Vec<&str>), Box<dyn std::error::Error + Send + Sync>> {
    let mut parts = command.split_whitespace();
    let program = parts
        .next()
        .ok_or_else(|| "codex command cannot be empty".to_string())?;
    Ok((program, parts.collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
