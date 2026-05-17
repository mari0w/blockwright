use tokio::process::Command;

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

    #[allow(dead_code)]
    pub async fn ask(&self, prompt: &str) -> Result<Option<String>, std::io::Error> {
        if !self.config.enabled {
            return Ok(None);
        }

        let output = Command::new(&self.config.command)
            .arg("exec")
            .arg("--")
            .arg(prompt)
            .output()
            .await?;

        Ok(Some(
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
        ))
    }
}
