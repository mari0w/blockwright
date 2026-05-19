use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    process::{ExitStatus, Output, Stdio},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::{Mutex, OwnedMutexGuard},
    time::{sleep, Duration, Instant as TokioInstant},
};

use crate::{config::CodexConfig, services::progress::ProgressStore};

const CODEX_PROGRESS_INTERVAL_SECONDS: u64 = 10;

#[derive(Clone)]
pub struct CodexClient {
    config: CodexConfig,
    sessions: CodexSessionStore,
    runtime_home: Option<Arc<PathBuf>>,
    progress: Option<ProgressStore>,
}

#[derive(Clone)]
struct CodexSessionStore {
    path: Option<Arc<PathBuf>>,
    sessions: Arc<Mutex<HashMap<String, String>>>,
    key_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

#[derive(Debug, Clone, Copy)]
pub enum CodexResponseSchema {
    Plan,
}

impl CodexResponseSchema {
    fn label(self) -> &'static str {
        match self {
            CodexResponseSchema::Plan => "plan",
        }
    }

    fn path(self) -> PathBuf {
        let file_name = match self {
            CodexResponseSchema::Plan => "plan.schema.json",
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
            progress: None,
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
            progress: None,
        }
    }

    pub fn with_progress(mut self, progress: ProgressStore) -> Self {
        self.progress = Some(progress);
        self
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    pub async fn ask(
        &self,
        prompt: &str,
    ) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
        self.ask_inner(prompt, None, None, None).await
    }

    pub async fn ask_with_schema(
        &self,
        prompt: &str,
        schema: CodexResponseSchema,
        session_key: Option<&str>,
    ) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
        self.ask_inner(prompt, Some(schema), session_key, None)
            .await
    }

    pub async fn ask_with_schema_and_progress(
        &self,
        prompt: &str,
        schema: CodexResponseSchema,
        session_key: Option<&str>,
        progress_id: Option<&str>,
    ) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
        self.ask_inner(prompt, Some(schema), session_key, progress_id)
            .await
    }

    async fn ask_inner(
        &self,
        prompt: &str,
        schema: Option<CodexResponseSchema>,
        session_key: Option<&str>,
        progress_id: Option<&str>,
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
        self.record_progress(progress_id, schema_start_phase(schema_label), None);
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
            self.progress.clone(),
            progress_id.map(str::to_string),
            run_codex_exec(
                program,
                &args,
                prompt,
                &last_message_path,
                schema_path.as_deref(),
                resume_thread_id.as_deref(),
                session_key.is_some(),
                self.runtime_home.as_deref().map(PathBuf::as_path),
                &self.config.command,
                schema_label,
                session_key.as_deref().unwrap_or("ephemeral"),
                self.progress.clone(),
                progress_id.map(str::to_string),
            ),
        )
        .await?;
        let elapsed_ms = started_at.elapsed().as_millis();

        if !output.status.success() {
            let _ = tokio::fs::remove_file(&last_message_path).await;
            let failure = codex_failure_message(output.status, &output.stderr);
            tracing::warn!(
                command = %self.config.command,
                schema = schema_label,
                session_key = session_key.as_deref().unwrap_or("ephemeral"),
                elapsed_ms,
                status = %output.status,
                error = %failure,
                "codex cli request failed"
            );
            return Err(failure.into());
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
        self.record_progress(progress_id, schema_finish_phase(schema_label), None);
        Ok(Some(answer.to_string()))
    }

    fn record_progress(&self, progress_id: Option<&str>, phase: &str, detail: Option<String>) {
        let (Some(progress), Some(progress_id)) = (self.progress.as_ref(), progress_id) else {
            return;
        };
        progress.record(progress_id, phase, detail);
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
    progress: Option<ProgressStore>,
    progress_id: Option<String>,
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
                if let (Some(progress), Some(progress_id)) = (progress.as_ref(), progress_id.as_deref()) {
                    progress.record(
                        progress_id,
                        "Codex 仍在处理，本次请求还没有返回",
                        Some(format!("已用 {elapsed_seconds}s，剩余超时 {remaining_seconds}s")),
                    );
                }
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
    command_for_log: &str,
    schema_label: &str,
    session_key: &str,
    progress: Option<ProgressStore>,
    progress_id: Option<String>,
) -> std::io::Result<Output> {
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

    let stdout = child.stdout.take().ok_or_else(|| {
        io::Error::new(io::ErrorKind::Other, "failed to capture codex stdout pipe")
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        io::Error::new(io::ErrorKind::Other, "failed to capture codex stderr pipe")
    })?;
    let stdout_context = CodexEventLogContext {
        command: command_for_log.to_string(),
        schema_label: schema_label.to_string(),
        session_key: session_key.to_string(),
        progress,
        progress_id,
    };
    let stdout_task =
        tokio::spawn(async move { collect_stdout_with_progress(stdout, stdout_context).await });
    let stderr_task = tokio::spawn(async move { collect_reader(stderr).await });

    let status = child.wait().await?;
    let stdout = stdout_task
        .await
        .map_err(|error| io::Error::new(io::ErrorKind::Other, error.to_string()))??;
    let stderr = stderr_task
        .await
        .map_err(|error| io::Error::new(io::ErrorKind::Other, error.to_string()))??;

    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

struct CodexEventLogContext {
    command: String,
    schema_label: String,
    session_key: String,
    progress: Option<ProgressStore>,
    progress_id: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
struct CodexProgressEvent {
    event_type: String,
    phase: &'static str,
    detail: Option<String>,
}

async fn collect_stdout_with_progress<R>(
    reader: R,
    context: CodexEventLogContext,
) -> io::Result<Vec<u8>>
where
    R: AsyncRead + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut output = Vec::new();
    let mut line = Vec::new();

    loop {
        line.clear();
        let read = reader.read_until(b'\n', &mut line).await?;
        if read == 0 {
            break;
        }
        output.extend_from_slice(&line);
        log_codex_json_event(&line, &context);
    }

    Ok(output)
}

async fn collect_reader<R>(reader: R) -> io::Result<Vec<u8>>
where
    R: AsyncRead + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut output = Vec::new();
    reader.read_to_end(&mut output).await?;
    Ok(output)
}

fn log_codex_json_event(line: &[u8], context: &CodexEventLogContext) {
    let Ok(line) = std::str::from_utf8(line) else {
        return;
    };
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return;
    };
    let Some(event) = codex_progress_event(&value) else {
        return;
    };

    tracing::info!(
        command = %context.command,
        schema = %context.schema_label,
        session_key = %context.session_key,
        event_type = %event.event_type,
        phase = event.phase,
        detail = event.detail.as_deref().unwrap_or(""),
        "codex cli progress event"
    );
    if let (Some(progress), Some(progress_id)) =
        (context.progress.as_ref(), context.progress_id.as_deref())
    {
        progress.record(progress_id, event.phase, event.detail.clone());
    }
}

fn schema_start_phase(schema_label: &str) -> &'static str {
    match schema_label {
        "plan" => "Codex 正在理解需求并决定下一步",
        _ => "Codex 正在处理请求",
    }
}

fn schema_finish_phase(schema_label: &str) -> &'static str {
    match schema_label {
        "plan" => "Codex 已给出回复和下一步动作",
        _ => "Codex 已完成本阶段处理",
    }
}

fn codex_progress_event(value: &Value) -> Option<CodexProgressEvent> {
    let event_type = value.get("type").and_then(Value::as_str)?;
    if event_type.ends_with(".delta") || event_type.ends_with("_delta") {
        return None;
    }

    let phase = codex_progress_phase(event_type)?;
    Some(CodexProgressEvent {
        event_type: event_type.to_string(),
        phase,
        detail: codex_progress_detail(value),
    })
}

fn codex_progress_phase(event_type: &str) -> Option<&'static str> {
    match event_type {
        "thread.started" => Some("会话已准备好，开始承接本次请求"),
        "turn.started" => Some("开始分析玩家需求并准备生成结构化结果"),
        "turn.completed" => Some("本轮 Codex 处理完成，准备整理最终结果"),
        "agent_message.started" => Some("开始生成最终结构化回复"),
        "agent_message.completed" => Some("最终结构化回复已经生成"),
        "error" | "turn.failed" => Some("Codex 处理失败，准备返回错误"),
        value if value.contains("reasoning") && value.ends_with(".started") => {
            Some("开始分析需求和可执行方案")
        }
        value if value.contains("reasoning") && value.ends_with(".completed") => {
            Some("需求分析阶段完成")
        }
        value if value.contains("tool") && value.ends_with(".started") => {
            Some("准备调用工具获取上下文或执行辅助步骤")
        }
        value if value.contains("tool") && value.ends_with(".completed") => {
            Some("工具调用完成，继续整理结果")
        }
        value if value.contains("command") && value.ends_with(".started") => {
            Some("准备执行内部命令或检查步骤")
        }
        value if value.contains("command") && value.ends_with(".completed") => {
            Some("内部命令或检查步骤完成")
        }
        value if value.ends_with(".started") => Some("进入新的处理阶段"),
        value if value.ends_with(".completed") => Some("当前处理阶段完成"),
        _ => None,
    }
}

fn codex_progress_detail(value: &Value) -> Option<String> {
    let raw = value
        .get("tool_name")
        .or_else(|| value.get("tool"))
        .or_else(|| value.get("name"))
        .or_else(|| value.get("command"))
        .and_then(Value::as_str)?;

    Some(safe_progress_detail(raw))
}

fn safe_progress_detail(raw: &str) -> String {
    let mut detail = raw.split_whitespace().next().unwrap_or(raw).to_string();
    detail.retain(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':' | '/'));
    if detail.len() > 80 {
        detail.truncate(80);
    }
    detail
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
    if stderr.is_empty() {
        return format!(
            "codex command exited with {status}; no stderr returned. Check Codex login status, model access, network connectivity, and CLI version."
        );
    }

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
{{"reply":"好","summary":"测试","blueprint":null,"site_plan":null,"actions":[{{"type":"chat","message":"好"}}]}}
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
            .ask_with_schema("one", CodexResponseSchema::Plan, Some("Minecraft:Steve"))
            .await
            .unwrap();
        client
            .ask_with_schema("two", CodexResponseSchema::Plan, Some("minecraft:steve"))
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
{{"reply":"好","summary":"测试","blueprint":null,"site_plan":null,"actions":[{{"type":"chat","message":"好"}}]}}
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
            .ask_with_schema("hello", CodexResponseSchema::Plan, Some("minecraft:steve"))
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
        assert!(CodexResponseSchema::Plan.path().exists());
    }

    #[test]
    fn plan_response_schema_exposes_site_plan_and_expanded_blueprint_limits() {
        let schema =
            fs::read_to_string(CodexResponseSchema::Plan.path()).expect("schema should read");
        let schema: Value = serde_json::from_str(&schema).expect("schema should be valid json");
        let required = schema
            .get("required")
            .and_then(Value::as_array)
            .expect("top-level required fields should be listed");

        assert!(required
            .iter()
            .any(|value| value.as_str() == Some("site_plan")));
        assert!(schema.pointer("/$defs/sitePlan").is_some());
        assert_eq!(
            schema
                .pointer("/$defs/blueprint/properties/blocks/maxItems")
                .and_then(Value::as_u64),
            Some(5000)
        );
        assert_eq!(
            schema
                .pointer("/$defs/placeBlocksAction/properties/blocks/maxItems")
                .and_then(Value::as_u64),
            Some(5000)
        );
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
    fn summarizes_codex_json_events_without_leaking_model_text() {
        let event = serde_json::json!({
            "type": "agent_message.completed",
            "message": "这里可能是模型最终正文，不能进阶段日志"
        });

        let progress = codex_progress_event(&event).unwrap();

        assert_eq!(progress.event_type, "agent_message.completed");
        assert_eq!(progress.phase, "最终结构化回复已经生成");
        assert_eq!(progress.detail, None);
    }

    #[test]
    fn summarizes_tool_and_command_progress_with_safe_detail() {
        let tool_event = serde_json::json!({
            "type": "mcp_tool_call.started",
            "tool_name": "blockwright_blueprint_validator"
        });
        let command_event = serde_json::json!({
            "type": "command.completed",
            "command": "python3 /tmp/secret-prompt.py --with-sensitive-args"
        });

        let tool_progress = codex_progress_event(&tool_event).unwrap();
        let command_progress = codex_progress_event(&command_event).unwrap();

        assert_eq!(tool_progress.phase, "准备调用工具获取上下文或执行辅助步骤");
        assert_eq!(
            tool_progress.detail.as_deref(),
            Some("blockwright_blueprint_validator")
        );
        assert_eq!(command_progress.phase, "内部命令或检查步骤完成");
        assert_eq!(command_progress.detail.as_deref(), Some("python3"));
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

    #[test]
    fn failure_message_reports_empty_stderr_hint() {
        let message = codex_failure_message(ExitStatus::from_raw(1), b"");

        assert!(message.contains("no stderr returned"));
        assert!(message.contains("Codex login status"));
        assert!(!message.contains("stderr omitted"));
    }
}
