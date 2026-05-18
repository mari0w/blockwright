use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::{ExitStatus, Stdio},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::Value;
use tokio::{
    io::AsyncWriteExt,
    process::Command,
    sync::{Mutex, OwnedMutexGuard},
    time::{sleep, Duration, Instant as TokioInstant},
};

use crate::config::CodexConfig;

const CODEX_PROGRESS_INTERVAL_SECONDS: u64 = 10;

#[derive(Clone)]
pub struct CodexClient {
    config: CodexConfig,
    sessions: CodexSessionStore,
    runtime_home: Option<Arc<PathBuf>>,
}

#[derive(Clone)]
struct CodexSessionStore {
    path: Option<Arc<PathBuf>>,
    sessions: Arc<Mutex<HashMap<String, String>>>,
    key_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

#[derive(Debug, Clone, Copy)]
pub enum CodexResponseSchema {
    ActionPlan,
    Blueprint,
}

impl CodexResponseSchema {
    fn label(self) -> &'static str {
        match self {
            CodexResponseSchema::ActionPlan => "action_plan",
            CodexResponseSchema::Blueprint => "blueprint",
        }
    }

    fn path(self) -> PathBuf {
        let file_name = match self {
            CodexResponseSchema::ActionPlan => "action-plan.schema.json",
            CodexResponseSchema::Blueprint => "blueprint.schema.json",
        };
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("schemas")
            .join(file_name)
    }
}

impl CodexClient {
    pub fn new(config: CodexConfig) -> Self {
        Self {
            config,
            sessions: CodexSessionStore::in_memory(),
            runtime_home: None,
        }
    }

    pub fn with_session_path(config: CodexConfig, path: PathBuf) -> Self {
        Self::with_session_path_and_home(config, path, None)
    }

    pub fn with_session_path_and_home(
        config: CodexConfig,
        path: PathBuf,
        runtime_home: Option<PathBuf>,
    ) -> Self {
        Self {
            config,
            sessions: CodexSessionStore::from_path(path),
            runtime_home: runtime_home.map(Arc::new),
        }
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    pub async fn ask(
        &self,
        prompt: &str,
    ) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
        self.ask_inner(prompt, None, None).await
    }

    pub async fn ask_with_schema(
        &self,
        prompt: &str,
        schema: CodexResponseSchema,
        session_key: Option<&str>,
    ) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
        self.ask_inner(prompt, Some(schema), session_key).await
    }

    async fn ask_inner(
        &self,
        prompt: &str,
        schema: Option<CodexResponseSchema>,
        session_key: Option<&str>,
    ) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
        if !self.config.enabled {
            return Ok(None);
        }

        let session_key = session_key
            .map(normalize_session_key)
            .filter(|key| !key.is_empty());
        let _session_guard = if let Some(session_key) = session_key.as_deref() {
            Some(self.sessions.lock_key(session_key).await)
        } else {
            None
        };
        let resume_thread_id = if let Some(session_key) = session_key.as_deref() {
            self.sessions.get(session_key).await
        } else {
            None
        };
        let schema_label = schema.map(CodexResponseSchema::label).unwrap_or("none");
        let schema_path = schema.map(CodexResponseSchema::path);
        let (program, args) = command_parts(&self.config.command)?;
        let last_message_path = codex_last_message_path();
        tracing::info!(
            command = %self.config.command,
            schema = schema_label,
            session_key = session_key.as_deref().unwrap_or("ephemeral"),
            resume_thread_id = resume_thread_id.as_deref().unwrap_or("new"),
            timeout_seconds = self.config.timeout_seconds,
            "starting codex cli request"
        );

        let started_at = std::time::Instant::now();
        let output = run_codex_exec_with_progress(
            started_at,
            self.config.timeout_seconds,
            &self.config.command,
            schema_label,
            session_key.as_deref().unwrap_or("ephemeral"),
            run_codex_exec(
                program,
                &args,
                prompt,
                &last_message_path,
                schema_path.as_deref(),
                resume_thread_id.as_deref(),
                session_key.is_some(),
                self.runtime_home.as_deref().map(PathBuf::as_path),
            ),
        )
        .await?;
        let elapsed_ms = started_at.elapsed().as_millis();

        if !output.status.success() {
            let _ = tokio::fs::remove_file(&last_message_path).await;
            tracing::warn!(
                command = %self.config.command,
                schema = schema_label,
                session_key = session_key.as_deref().unwrap_or("ephemeral"),
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
        if let (Some(session_key), Some(thread_id)) = (
            session_key.as_deref(),
            extract_thread_id_from_json_stdout(&output.stdout),
        ) {
            self.sessions.set(session_key, thread_id.as_str()).await?;
            tracing::info!(
                session_key = %session_key,
                thread_id = %thread_id,
                "saved codex session for speaker"
            );
        }

        tracing::info!(
            command = %self.config.command,
            schema = schema_label,
            session_key = session_key.as_deref().unwrap_or("ephemeral"),
            elapsed_ms,
            "finished codex cli request"
        );
        Ok(Some(answer.to_string()))
    }
}

impl CodexSessionStore {
    fn in_memory() -> Self {
        Self {
            path: None,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            key_locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn from_path(path: PathBuf) -> Self {
        let sessions = std::fs::read_to_string(&path)
            .ok()
            .and_then(|source| serde_json::from_str::<HashMap<String, String>>(&source).ok())
            .unwrap_or_default();
        Self {
            path: Some(Arc::new(path)),
            sessions: Arc::new(Mutex::new(sessions)),
            key_locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn lock_key(&self, key: &str) -> OwnedMutexGuard<()> {
        let lock = {
            let mut locks = self.key_locks.lock().await;
            locks
                .entry(key.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        lock.lock_owned().await
    }

    async fn get(&self, key: &str) -> Option<String> {
        self.sessions.lock().await.get(key).cloned()
    }

    async fn set(
        &self,
        key: &str,
        thread_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let snapshot = {
            let mut sessions = self.sessions.lock().await;
            sessions.insert(key.to_string(), thread_id.to_string());
            sessions.clone()
        };

        let Some(path) = self.path.as_ref() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let json = serde_json::to_string_pretty(&snapshot)?;
        tokio::fs::write(path.as_ref(), json).await?;
        Ok(())
    }
}

async fn run_codex_exec_with_progress(
    started_at: std::time::Instant,
    timeout_seconds: u64,
    command: &str,
    schema_label: &str,
    session_key: &str,
    exec: impl std::future::Future<Output = std::io::Result<std::process::Output>>,
) -> Result<std::process::Output, Box<dyn std::error::Error + Send + Sync>> {
    let timeout_duration = Duration::from_secs(timeout_seconds);
    let deadline = sleep(timeout_duration);
    let next_progress = sleep(Duration::from_secs(CODEX_PROGRESS_INTERVAL_SECONDS));
    tokio::pin!(deadline);
    tokio::pin!(next_progress);
    tokio::pin!(exec);

    loop {
        tokio::select! {
            result = &mut exec => return result.map_err(Into::into),
            _ = &mut deadline => {
                return Err(format!("codex command timed out after {timeout_seconds} seconds").into());
            }
            _ = &mut next_progress => {
                let elapsed_seconds = started_at.elapsed().as_secs();
                let remaining_seconds = timeout_seconds.saturating_sub(elapsed_seconds);
                tracing::info!(
                    command = %command,
                    schema = schema_label,
                    session_key = session_key,
                    elapsed_seconds,
                    remaining_seconds,
                    "codex cli request still running"
                );
                next_progress.as_mut().reset(
                    TokioInstant::now() + Duration::from_secs(CODEX_PROGRESS_INTERVAL_SECONDS)
                );
            }
        }
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
    schema_path: Option<&Path>,
    resume_thread_id: Option<&str>,
    persist_session: bool,
    runtime_home: Option<&Path>,
) -> std::io::Result<std::process::Output> {
    let mut command = Command::new(program);
    if let Some(runtime_home) = runtime_home {
        command.env("CODEX_HOME", runtime_home);
    }
    command.arg("exec").args(args);
    if !persist_session {
        command.arg("--ephemeral");
    }
    command.arg("--json");
    if let Some(schema_path) = schema_path {
        command.arg("--output-schema").arg(schema_path);
    }
    if let Some(thread_id) = resume_thread_id {
        command.arg("resume").arg(thread_id);
    }
    command
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

fn extract_thread_id_from_json_stdout(stdout: &[u8]) -> Option<String> {
    let stdout = String::from_utf8_lossy(stdout);
    for line in stdout.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) != Some("thread.started") {
            continue;
        }
        return value
            .get("thread_id")
            .and_then(Value::as_str)
            .map(ToString::to_string);
    }

    None
}

fn normalize_session_key(key: &str) -> String {
    key.trim().to_lowercase()
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
    use std::{
        fs,
        os::unix::{fs::PermissionsExt, process::ExitStatusExt},
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_DIR_ID: AtomicU64 = AtomicU64::new(1);

    fn disabled_client() -> CodexClient {
        CodexClient::new(CodexConfig {
            enabled: false,
            command: "codex".to_string(),
            timeout_seconds: 1,
        })
    }

    fn temp_dir(name: &str) -> PathBuf {
        let number = NEXT_DIR_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "blockwright-codex-{name}-{}-{number}",
            std::process::id()
        ))
    }

    #[tokio::test]
    async fn disabled_client_returns_none_without_running_command() {
        let output = disabled_client().ask("hello").await.unwrap();

        assert!(output.is_none());
    }

    #[tokio::test]
    async fn session_key_resumes_same_codex_thread() {
        let dir = temp_dir("session-resume");
        fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("fake-codex.sh");
        let args_log = dir.join("args.log");
        fs::write(
            &script_path,
            format!(
                r#"#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> '{}'
last_message=""
resume_thread=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --output-last-message)
      last_message="$2"
      shift 2
      ;;
    resume)
      resume_thread="$2"
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done
cat >/dev/null
if [[ -z "$last_message" ]]; then
  exit 2
fi
thread_id="${{resume_thread:-thread-a}}"
printf '{{"type":"thread.started","thread_id":"%s"}}\n' "$thread_id"
cat > "$last_message" <<'BLOCKWRIGHT_JSON'
{{"reply":"好","summary":"测试","actions":[{{"type":"chat","player":null,"item":null,"count":null,"command":null,"message":"好"}}]}}
BLOCKWRIGHT_JSON
"#,
                args_log.display()
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).unwrap();

        let client = CodexClient::with_session_path(
            CodexConfig {
                enabled: true,
                command: script_path.to_string_lossy().to_string(),
                timeout_seconds: 5,
            },
            dir.join("sessions.json"),
        );

        client
            .ask_with_schema(
                "one",
                CodexResponseSchema::ActionPlan,
                Some("Minecraft:Steve"),
            )
            .await
            .unwrap();
        client
            .ask_with_schema(
                "two",
                CodexResponseSchema::ActionPlan,
                Some("minecraft:steve"),
            )
            .await
            .unwrap();

        let args = fs::read_to_string(args_log).unwrap();
        let lines = args.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 2);
        assert!(!lines[0].contains("resume thread-a"));
        assert!(lines[1].contains("resume thread-a"));
    }

    #[tokio::test]
    async fn runtime_home_is_passed_as_codex_home() {
        let dir = temp_dir("runtime-home");
        fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("fake-codex-home.sh");
        let env_log = dir.join("env.log");
        let runtime_home = dir.join("codex-home");
        fs::write(
            &script_path,
            format!(
                r#"#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "${{CODEX_HOME:-}}" > '{}'
last_message=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --output-last-message)
      last_message="$2"
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done
cat >/dev/null
printf '{{"type":"thread.started","thread_id":"thread-home"}}\n'
cat > "$last_message" <<'BLOCKWRIGHT_JSON'
{{"reply":"好","summary":"测试","actions":[{{"type":"chat","player":null,"item":null,"count":null,"command":null,"message":"好"}}]}}
BLOCKWRIGHT_JSON
"#,
                env_log.display()
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).unwrap();

        let client = CodexClient::with_session_path_and_home(
            CodexConfig {
                enabled: true,
                command: script_path.to_string_lossy().to_string(),
                timeout_seconds: 5,
            },
            dir.join("sessions.json"),
            Some(runtime_home.clone()),
        );

        client
            .ask_with_schema(
                "hello",
                CodexResponseSchema::ActionPlan,
                Some("minecraft:steve"),
            )
            .await
            .unwrap();

        assert_eq!(
            fs::read_to_string(env_log).unwrap().trim(),
            runtime_home.to_string_lossy()
        );
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
    fn response_schema_files_are_packaged_with_controller_source() {
        assert!(CodexResponseSchema::ActionPlan.path().exists());
        assert!(CodexResponseSchema::Blueprint.path().exists());
    }

    #[test]
    fn extracts_thread_id_from_json_stdout() {
        let stdout = br#"{"type":"turn.started"}
{"type":"thread.started","thread_id":"thread-123"}
{"type":"turn.completed"}"#;

        assert_eq!(
            extract_thread_id_from_json_stdout(stdout).as_deref(),
            Some("thread-123")
        );
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
