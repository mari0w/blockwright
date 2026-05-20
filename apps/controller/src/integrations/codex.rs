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
    io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader},
    process::Command,
    sync::{mpsc, Mutex, OwnedMutexGuard},
    time::{sleep, Duration, Instant as TokioInstant},
};

use crate::{config::CodexConfig, services::progress::ProgressStore};

const CODEX_PROGRESS_INTERVAL_SECONDS: u64 = 10;
const CODEX_MAX_REQUEST_ATTEMPTS: u32 = 2;

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

    #[cfg(test)]
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
        let mut resume_thread_id = if let Some(session_key) = session_key.as_deref() {
            self.sessions.get(session_key).await
        } else {
            None
        };
        let schema_label = schema.map(CodexResponseSchema::label).unwrap_or("none");
        self.record_progress(progress_id, schema_start_phase(schema_label), None);
        let (program, args) = command_parts(&self.config.command)?;
        tracing::info!(
            command = %self.config.command,
            schema = schema_label,
            session_key = session_key.as_deref().unwrap_or("ephemeral"),
            resume_thread_id = resume_thread_id.as_deref().unwrap_or("new"),
            timeout_seconds = self.config.timeout_seconds,
            "starting codex cli request"
        );
        tracing::info!(
            runtime_home = self
                .runtime_home
                .as_deref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "default".to_string()),
            proxy_env = %codex_proxy_env_summary(),
            "codex cli environment diagnostics"
        );

        let started_at = std::time::Instant::now();
        let mut attempt = 1;
        let mut context_session_reset = false;
        let (output, last_message_path) = loop {
            let last_message_path = codex_last_message_path();
            let attempt_started_at = std::time::Instant::now();
            // Codex CLI 0.130.0 在 gpt-5.5 + --output-schema 下会稳定 stream disconnected。
            // controller 保留 schema label 作为本地解析语义，但不把 schema 交给 CLI。
            let structured_output = false;
            tracing::info!(
                command = %self.config.command,
                schema = schema_label,
                session_key = session_key.as_deref().unwrap_or("ephemeral"),
                attempt,
                structured_output,
                "starting codex cli request attempt"
            );
            let result = run_codex_exec_with_progress(
                attempt_started_at,
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
                    None,
                    resume_thread_id.as_deref(),
                    session_key.is_some(),
                    self.runtime_home.as_deref().map(PathBuf::as_path),
                    &self.config.command,
                    schema_label,
                    session_key.as_deref().unwrap_or("ephemeral"),
                    self.progress.clone(),
                    progress_id.map(str::to_string),
                    structured_output,
                ),
            )
            .await;

            let elapsed_ms = attempt_started_at.elapsed().as_millis();
            match result {
                Ok(output) if output.status.success() => break (output, last_message_path),
                Ok(output) => {
                    let _ = tokio::fs::remove_file(&last_message_path).await;
                    let failure =
                        codex_failure_message(output.status, &output.stderr, &output.stdout);
                    tracing::warn!(
                        command = %self.config.command,
                        schema = schema_label,
                        session_key = session_key.as_deref().unwrap_or("ephemeral"),
                        attempt,
                        elapsed_ms,
                        status = %output.status,
                        error = %failure,
                        "codex cli request failed"
                    );
                    if self
                        .reset_session_after_context_overflow(
                            &session_key,
                            &mut resume_thread_id,
                            &mut context_session_reset,
                            attempt,
                            &failure,
                            progress_id,
                        )
                        .await?
                    {
                        attempt += 1;
                        continue;
                    }
                    if attempt < CODEX_MAX_REQUEST_ATTEMPTS && codex_failure_is_retriable(&failure)
                    {
                        attempt += 1;
                        continue;
                    }
                    return Err(failure.into());
                }
                Err(error) => {
                    let _ = tokio::fs::remove_file(&last_message_path).await;
                    let failure = error.to_string();
                    tracing::warn!(
                        command = %self.config.command,
                        schema = schema_label,
                        session_key = session_key.as_deref().unwrap_or("ephemeral"),
                        attempt,
                        elapsed_ms,
                        error = %failure,
                        "codex cli request failed before process exit"
                    );
                    if self
                        .reset_session_after_context_overflow(
                            &session_key,
                            &mut resume_thread_id,
                            &mut context_session_reset,
                            attempt,
                            &failure,
                            progress_id,
                        )
                        .await?
                    {
                        attempt += 1;
                        continue;
                    }
                    if attempt < CODEX_MAX_REQUEST_ATTEMPTS && codex_failure_is_retriable(&failure)
                    {
                        attempt += 1;
                        continue;
                    }
                    return Err(error);
                }
            }
        };
        let elapsed_ms = started_at.elapsed().as_millis();

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

    async fn reset_session_after_context_overflow(
        &self,
        session_key: &Option<String>,
        resume_thread_id: &mut Option<String>,
        context_session_reset: &mut bool,
        attempt: u32,
        failure: &str,
        progress_id: Option<&str>,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let Some(session_key) = session_key.as_deref() else {
            return Ok(false);
        };
        if *context_session_reset
            || resume_thread_id.is_none()
            || attempt >= CODEX_MAX_REQUEST_ATTEMPTS
            || !codex_failure_is_context_window_full(failure)
        {
            return Ok(false);
        }

        self.sessions.remove(session_key).await?;
        *resume_thread_id = None;
        *context_session_reset = true;
        tracing::warn!(
            command = %self.config.command,
            session_key = %session_key,
            attempt,
            "codex session context window is full; cleared saved thread and retrying fresh"
        );
        self.record_progress(progress_id, "Codex 会话历史过长，已自动换新会话重试", None);
        Ok(true)
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

        self.persist_snapshot(&snapshot).await
    }

    async fn remove(&self, key: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let snapshot = {
            let mut sessions = self.sessions.lock().await;
            sessions.remove(key);
            sessions.clone()
        };

        self.persist_snapshot(&snapshot).await
    }

    async fn persist_snapshot(
        &self,
        snapshot: &HashMap<String, String>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let Some(path) = self.path.as_ref() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let json = serde_json::to_string_pretty(snapshot)?;
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
    structured_output: bool,
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
    let (terminal_error_tx, mut terminal_error_rx) = mpsc::unbounded_channel();
    let stdout_context = CodexEventLogContext {
        command: command_for_log.to_string(),
        schema_label: schema_label.to_string(),
        session_key: session_key.to_string(),
        progress,
        progress_id,
        terminal_error_tx: Some(terminal_error_tx),
        early_terminal_reconnect: structured_output,
    };
    let stderr_context = CodexEventLogContext {
        command: command_for_log.to_string(),
        schema_label: schema_label.to_string(),
        session_key: session_key.to_string(),
        progress: stdout_context.progress.clone(),
        progress_id: stdout_context.progress_id.clone(),
        terminal_error_tx: None,
        early_terminal_reconnect: false,
    };
    let stdout_task =
        tokio::spawn(async move { collect_stdout_with_progress(stdout, stdout_context).await });
    let stderr_task =
        tokio::spawn(async move { collect_stderr_with_diagnostics(stderr, stderr_context).await });

    let status = loop {
        tokio::select! {
            status = child.wait() => break status?,
            terminal_error = terminal_error_rx.recv() => {
                let Some(terminal_error) = terminal_error else {
                    continue;
                };
                let _ = child.start_kill();
                let _ = child.wait().await;
                let _ = stdout_task.await;
                let _ = stderr_task.await;
                return Err(io::Error::new(io::ErrorKind::Other, terminal_error));
            }
        }
    };
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
    terminal_error_tx: Option<mpsc::UnboundedSender<String>>,
    early_terminal_reconnect: bool,
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

async fn collect_stderr_with_diagnostics<R>(
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
        log_codex_stderr_diagnostic(&line, &context);
    }

    Ok(output)
}

fn log_codex_stderr_diagnostic(line: &[u8], context: &CodexEventLogContext) {
    let Ok(line) = std::str::from_utf8(line) else {
        return;
    };
    let Some(detail) = codex_stderr_diagnostic(line) else {
        return;
    };

    tracing::warn!(
        command = %context.command,
        schema = %context.schema_label,
        session_key = %context.session_key,
        detail = %detail,
        "codex cli stderr network diagnostic"
    );
    if let (Some(progress), Some(progress_id)) =
        (context.progress.as_ref(), context.progress_id.as_deref())
    {
        progress.record(progress_id, "Codex CLI 网络诊断", Some(detail));
    }
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
    if let (Some(sender), Some(error)) = (
        context.terminal_error_tx.as_ref(),
        codex_terminal_error(&value, context.early_terminal_reconnect),
    ) {
        let _ = sender.send(error);
    }
}

fn schema_start_phase(schema_label: &str) -> &'static str {
    match schema_label {
        "plan" => "AI 正在处理你的请求",
        _ => "AI 正在处理请求",
    }
}

fn schema_finish_phase(schema_label: &str) -> &'static str {
    match schema_label {
        "plan" => "AI 已生成回复和操作结果",
        _ => "AI 已完成本阶段处理",
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
        "thread.started" => Some("AI 会话已准备好"),
        "turn.started" => Some("AI 正在处理你的请求"),
        "turn.completed" => Some("AI 已完成本轮处理"),
        "agent_message.started" => Some("AI 正在整理回复"),
        "agent_message.completed" => Some("AI 回复已经生成"),
        "error" | "turn.failed" => Some("AI 处理失败，准备返回错误"),
        value if value.contains("reasoning") && value.ends_with(".started") => {
            Some("AI 正在分析需求")
        }
        value if value.contains("reasoning") && value.ends_with(".completed") => {
            Some("AI 分析阶段完成")
        }
        value if value.contains("tool") && value.ends_with(".started") => Some("AI 准备调用工具"),
        value if value.contains("tool") && value.ends_with(".completed") => Some("工具调用完成"),
        value if value.contains("command") && value.ends_with(".started") => {
            Some("准备执行内部检查")
        }
        value if value.contains("command") && value.ends_with(".completed") => Some("内部检查完成"),
        value if value.ends_with(".started") => Some("AI 进入新的处理阶段"),
        value if value.ends_with(".completed") => Some("AI 当前处理阶段完成"),
        _ => None,
    }
}

fn codex_progress_detail(value: &Value) -> Option<String> {
    if matches!(
        value.get("type").and_then(Value::as_str),
        Some("error" | "turn.failed")
    ) {
        return extract_codex_error_from_value(value);
    }

    let raw = value
        .get("tool_name")
        .or_else(|| value.get("tool"))
        .or_else(|| value.get("name"))
        .or_else(|| value.get("command"))
        .and_then(Value::as_str)?;

    Some(safe_progress_detail(raw))
}

fn codex_terminal_error(value: &Value, early_reconnect: bool) -> Option<String> {
    let event_type = value.get("type").and_then(Value::as_str)?;
    if !matches!(event_type, "error" | "turn.failed") {
        return None;
    }

    let detail = extract_codex_error_from_value(value)?;
    if detail.contains("Reconnecting...") && !detail.contains("5/5") {
        if early_reconnect
            && detail.contains("stream disconnected before completion")
            && (detail.contains("2/5") || detail.contains("3/5") || detail.contains("4/5"))
        {
            return Some(format!(
                "codex cli reported early retryable structured-output error: {detail}"
            ));
        }
        return None;
    }

    Some(format!("codex cli reported terminal error: {detail}"))
}

fn codex_failure_is_retriable(message: &str) -> bool {
    message.contains("stream disconnected before completion")
        || message.contains("timeout waiting for child process to exit")
        || message.contains("Reconnecting... 5/5")
}

fn codex_failure_is_context_window_full(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("ran out of room in the model's context window")
        || lower.contains("context window")
            && lower.contains("start a new thread")
            && lower.contains("clear earlier history")
        || lower.contains("context_length_exceeded")
}

fn codex_stderr_diagnostic(raw: &str) -> Option<String> {
    let line = raw.trim();
    if line.is_empty() {
        return None;
    }

    let lower = line.to_ascii_lowercase();
    let tls_or_certificate = lower.contains("tls") || lower.contains("certificate");
    let has_failure_signal = lower.contains("error")
        || lower.contains("fail")
        || lower.contains("invalid")
        || lower.contains("expired")
        || lower.contains("untrusted")
        || lower.contains("unknown issuer")
        || lower.contains("self signed");
    let category = if lower.contains("failed to connect to websocket") {
        "websocket_connect"
    } else if lower.contains("falling back to http") {
        "http_fallback"
    } else if lower.contains("stream disconnected") {
        "stream_disconnected"
    } else if lower.contains("error sending request for url")
        || lower.contains("http/request failed")
    {
        "http_request"
    } else if lower.contains("dns error") || lower.contains("failed to lookup") {
        "dns"
    } else if lower.contains("couldn't connect to server")
        || lower.contains("failed to connect to")
        || lower.contains("connection refused")
    {
        "connect"
    } else if lower.contains("operation not permitted") {
        "permission"
    } else if lower.contains("proxy")
        || lower.contains("socks")
        || lower.contains("port 1080")
        || lower.contains("port 1087")
    {
        "proxy"
    } else if tls_or_certificate && has_failure_signal {
        "tls"
    } else if lower.contains("timeout") {
        "timeout"
    } else {
        return None;
    };

    Some(format!(
        "{category}: {}",
        safe_network_diagnostic_detail(line)
    ))
}

fn safe_progress_detail(raw: &str) -> String {
    let mut detail = raw.split_whitespace().next().unwrap_or(raw).to_string();
    detail.retain(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':' | '/'));
    if detail.len() > 80 {
        detail.truncate(80);
    }
    detail
}

fn compact_log_detail(raw: &str) -> Option<String> {
    let detail = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if detail.is_empty() {
        return None;
    }

    let limit = 300;
    if detail.chars().count() <= limit {
        return Some(detail);
    }

    let mut compact = detail.chars().take(limit).collect::<String>();
    compact.push_str("...");
    Some(compact)
}

fn safe_network_diagnostic_detail(raw: &str) -> String {
    let detail = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let detail = redact_url_credentials(&detail);
    let detail = redact_header_like_secret(&detail);
    let limit = 500;
    if detail.chars().count() <= limit {
        return detail;
    }

    let mut compact = detail.chars().take(limit).collect::<String>();
    compact.push_str("...");
    compact
}

fn redact_url_credentials(raw: &str) -> String {
    raw.split_whitespace()
        .map(|part| {
            let Some(scheme_pos) = part.find("://") else {
                return part.to_string();
            };
            let authority_start = scheme_pos + 3;
            let Some(at_relative) = part[authority_start..].find('@') else {
                return part.to_string();
            };
            let at_index = authority_start + at_relative;
            format!("{}***{}", &part[..authority_start], &part[at_index..])
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn redact_header_like_secret(raw: &str) -> String {
    raw.split_whitespace()
        .map(|part| {
            let lower = part.to_ascii_lowercase();
            if lower.starts_with("authorization:")
                || lower.starts_with("bearer")
                || lower.starts_with("token=")
                || lower.starts_with("access_token=")
            {
                "[redacted]".to_string()
            } else {
                part.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn codex_proxy_env_summary() -> String {
    let entries = [
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
        "NO_PROXY",
        "no_proxy",
    ]
    .into_iter()
    .filter_map(|name| {
        let value = std::env::var(name).ok()?;
        if value.trim().is_empty() {
            return None;
        }
        Some(format!("{name}={}", sanitize_proxy_env_value(name, &value)))
    })
    .collect::<Vec<_>>();

    if entries.is_empty() {
        "none".to_string()
    } else {
        entries.join(",")
    }
}

fn sanitize_proxy_env_value(name: &str, value: &str) -> String {
    let value = value.trim();
    if name.eq_ignore_ascii_case("no_proxy") {
        return value.chars().take(160).collect();
    }

    let redacted = redact_url_credentials(value);
    if redacted.chars().count() <= 160 {
        return redacted;
    }
    let mut compact = redacted.chars().take(160).collect::<String>();
    compact.push_str("...");
    compact
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

fn codex_failure_message(status: ExitStatus, stderr: &[u8], stdout: &[u8]) -> String {
    if !stderr.is_empty() {
        let stderr_text = String::from_utf8_lossy(stderr);
        if let Some(api_error) = extract_codex_api_error(&stderr_text) {
            return format!("codex command exited with {status}: {api_error}");
        }
        if let Some(diagnostic) = extract_last_codex_stderr_diagnostic(&stderr_text) {
            return format!("codex command exited with {status}: {diagnostic}");
        }
    }

    if !stdout.is_empty() {
        let stdout_text = String::from_utf8_lossy(stdout);
        if let Some(api_error) = extract_codex_api_error(&stdout_text) {
            return format!("codex command exited with {status}: {api_error}");
        }
    }

    if stderr.is_empty() {
        return format!(
            "codex command exited with {status}; no stderr returned and no structured stdout error found. Check Codex login status, model access, network connectivity, and CLI version."
        );
    }

    format!(
        "codex command exited with {status}; stderr omitted to avoid leaking prompts ({} bytes)",
        stderr.len()
    )
}

fn extract_last_codex_stderr_diagnostic(output: &str) -> Option<String> {
    output.lines().rev().find_map(codex_stderr_diagnostic)
}

fn extract_codex_api_error(output: &str) -> Option<String> {
    for line in output.lines().rev() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(json) = line.strip_prefix("ERROR:") {
            let value = serde_json::from_str::<Value>(json.trim()).ok()?;
            return extract_codex_error_from_value(&value);
        }

        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if !matches!(
            value.get("type").and_then(Value::as_str),
            Some("error" | "turn.failed")
        ) {
            continue;
        }
        if let Some(message) = extract_codex_error_from_value(&value) {
            return Some(message);
        }
    }

    None
}

fn extract_codex_error_from_value(value: &Value) -> Option<String> {
    let message = value
        .pointer("/error/message")
        .and_then(Value::as_str)
        .or_else(|| value.get("error").and_then(Value::as_str))
        .or_else(|| value.get("message").and_then(Value::as_str))
        .or_else(|| value.get("detail").and_then(Value::as_str))?;
    let message = compact_log_detail(message)?;
    let status = value
        .get("status")
        .or_else(|| value.pointer("/error/status"))
        .and_then(Value::as_i64)
        .map(|status| format!("status={status} "))
        .unwrap_or_default();
    let error_type = value
        .pointer("/error/type")
        .or_else(|| value.get("error_type"))
        .or_else(|| value.get("code"))
        .and_then(Value::as_str)
        .unwrap_or("unknown_error");

    Some(format!("{status}type={error_type}: {message}"))
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
    async fn context_window_full_resets_saved_session_and_retries_fresh() {
        let dir = temp_dir("session-context-reset");
        fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("fake-codex-context-reset.sh");
        let args_log = dir.join("args.log");
        let session_path = dir.join("sessions.json");
        fs::write(&session_path, r#"{"minecraft:steve":"thread-old"}"#).unwrap();
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
if [[ "$resume_thread" == "thread-old" ]]; then
  printf '{{"type":"error","error":"Codex ran out of room in the model'\''s context window. Start a new thread or clear earlier history before retrying."}}\n'
  exit 1
fi
printf '{{"type":"thread.started","thread_id":"thread-new"}}\n'
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
            session_path.clone(),
        );

        let output = client
            .ask_with_schema("hello", CodexResponseSchema::Plan, Some("minecraft:steve"))
            .await
            .unwrap()
            .unwrap();

        assert!(output.contains("\"summary\":\"测试\""));
        let args = fs::read_to_string(args_log).unwrap();
        let lines = args.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("resume thread-old"));
        assert!(!lines[1].contains("resume thread-old"));
        assert!(fs::read_to_string(session_path)
            .unwrap()
            .contains("thread-new"));
    }

    #[tokio::test]
    async fn schema_requests_do_not_pass_output_schema_to_cli() {
        let dir = temp_dir("schema-request-without-output-schema");
        fs::create_dir_all(&dir).unwrap();
        let script_path = dir.join("fake-codex-no-output-schema.sh");
        let args_log = dir.join("args.log");
        fs::write(
            &script_path,
            format!(
                r#"#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> '{}'
last_message=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --output-last-message)
      last_message="$2"
      shift 2
      ;;
    --output-schema)
      echo "unexpected --output-schema" >&2
      exit 7
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
printf '{{"type":"thread.started","thread_id":"thread-schema-retry"}}\n'
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

        let client = CodexClient::new(CodexConfig {
            enabled: true,
            command: script_path.to_string_lossy().to_string(),
            timeout_seconds: 5,
        });

        let output = client
            .ask_with_schema("hello", CodexResponseSchema::Plan, None)
            .await
            .unwrap()
            .unwrap();

        assert!(output.contains("\"summary\":\"测试\""));
        let args = fs::read_to_string(args_log).unwrap();
        let lines = args.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 1);
        assert!(!lines[0].contains("--output-schema"));
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
    fn plan_response_schema_exposes_site_plan_without_block_count_caps() {
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
        assert!(schema
            .pointer("/$defs/blueprint/properties/blocks/maxItems")
            .is_none());
        assert!(schema
            .pointer("/$defs/placeBlocksAction/properties/blocks/maxItems")
            .is_none());
        assert!(schema
            .pointer("/$defs/blueprint/properties/materials/items/properties/count/maximum")
            .is_none());
    }

    #[test]
    fn plan_response_schema_uses_strict_closed_objects() {
        let schema =
            fs::read_to_string(CodexResponseSchema::Plan.path()).expect("schema should read");
        let schema: Value = serde_json::from_str(&schema).expect("schema should be valid json");

        assert_schema_objects_are_strict(&schema, "$");
        assert_schema_omits_unsupported_keywords(&schema, "$");
        assert_schema_property_nodes_are_typed(&schema, "$");
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
        assert_eq!(progress.phase, "AI 回复已经生成");
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

        assert_eq!(tool_progress.phase, "AI 准备调用工具");
        assert_eq!(
            tool_progress.detail.as_deref(),
            Some("blockwright_blueprint_validator")
        );
        assert_eq!(command_progress.phase, "内部检查完成");
        assert_eq!(command_progress.detail.as_deref(), Some("python3"));
    }

    #[test]
    fn summarizes_error_progress_with_safe_detail() {
        let event = serde_json::json!({
            "type": "error",
            "status": 400,
            "error": {
                "type": "invalid_request_error",
                "message": "The 'gpt-5.5' model requires a newer version of Codex."
            }
        });

        let progress = codex_progress_event(&event).unwrap();

        assert_eq!(progress.phase, "AI 处理失败，准备返回错误");
        assert_eq!(
            progress.detail.as_deref(),
            Some(
                "status=400 type=invalid_request_error: The 'gpt-5.5' model requires a newer version of Codex."
            )
        );
    }

    #[test]
    fn terminal_error_waits_until_final_reconnect_and_marks_retriable() {
        let reconnecting = serde_json::json!({
            "type": "error",
            "error": "Reconnecting... 4/5 (stream disconnected before completion: request failed)"
        });
        let failed = serde_json::json!({
            "type": "error",
            "error": "Reconnecting... 5/5 (stream disconnected before completion: request failed)"
        });

        assert!(codex_terminal_error(&reconnecting, false).is_none());
        let terminal = codex_terminal_error(&failed, false).unwrap();
        assert!(terminal.contains("Reconnecting... 5/5"));
        assert!(codex_failure_is_retriable(&terminal));
    }

    #[test]
    fn structured_output_reconnect_can_trigger_early_retry() {
        let reconnecting = serde_json::json!({
            "type": "error",
            "error": "Reconnecting... 2/5 (stream disconnected before completion: request failed)"
        });

        assert!(codex_terminal_error(&reconnecting, false).is_none());
        let terminal = codex_terminal_error(&reconnecting, true).unwrap();
        assert!(terminal.contains("early retryable structured-output error"));
        assert!(codex_failure_is_retriable(&terminal));
    }

    #[test]
    fn stderr_diagnostic_summarizes_proxy_failures_without_credentials() {
        let line = "fatal: unable to access 'https://github.com/openai/plugins.git/': Failed to connect to 127.0.0.1 port 1087 after 0 ms: Couldn't connect to server via http://user:secret@127.0.0.1:1087";

        let diagnostic = codex_stderr_diagnostic(line).unwrap();

        assert!(diagnostic.contains("connect"));
        assert!(diagnostic.contains("127.0.0.1"));
        assert!(diagnostic.contains("1087"));
        assert!(!diagnostic.contains("secret"));
    }

    #[test]
    fn stderr_diagnostic_ignores_codex_websocket_trace_info() {
        let line = r#"2026-05-20T11:52:11.956920Z INFO session_loop{thread_id=019e453a}:submission_dispatch{otel.name="op.dispatch.user_input_with_turn_context"}:turn{model=gpt-5.5}:model_client.stream_responses_websocket{model=gpt-5.5 wire_api..."#;

        assert!(codex_stderr_diagnostic(line).is_none());
    }

    #[test]
    fn stderr_diagnostic_ignores_codex_ca_selection_info() {
        let line = "2026-05-20T11:52:11.960952Z INFO codex_client::custom_ca: using system root certificates because no CA override environment variable was selected codex_ca_certificate_configured=false ssl_cert_file_configured=false";

        assert!(codex_stderr_diagnostic(line).is_none());
    }

    #[test]
    fn stderr_diagnostic_keeps_actual_tls_failure() {
        let line = "ERROR request failed: tls certificate verify failed: unknown issuer";

        let diagnostic = codex_stderr_diagnostic(line).unwrap();

        assert!(diagnostic.contains("tls"));
        assert!(diagnostic.contains("unknown issuer"));
    }

    #[test]
    fn failure_message_extracts_network_diagnostic_without_prompt() {
        let stderr = "user\n给我一组红色的砖\nWARN codex_core::session::turn: stream disconnected before completion: error sending request for url (https://chatgpt.com/backend-api/codex/responses)";

        let message = codex_failure_message(ExitStatus::from_raw(1), stderr.as_bytes(), b"");

        assert!(message.contains("stream_disconnected"));
        assert!(message.contains("chatgpt.com/backend-api/codex/responses"));
        assert!(!message.contains("给我一组红色的砖"));
    }

    #[test]
    fn proxy_env_value_redacts_credentials() {
        let value = sanitize_proxy_env_value("HTTPS_PROXY", "http://user:secret@127.0.0.1:1087");

        assert_eq!(value, "http://***@127.0.0.1:1087");
    }

    #[test]
    fn failure_message_extracts_api_error_without_prompt() {
        let stderr = r#"user
给我钻石斧头
ERROR: {"type":"error","status":400,"error":{"type":"invalid_request_error","message":"The 'gpt-5.5' model requires a newer version of Codex."}}"#;

        let message = codex_failure_message(ExitStatus::from_raw(1), stderr.as_bytes(), b"");

        assert!(message.contains("status=400"));
        assert!(message.contains("gpt-5.5"));
        assert!(!message.contains("给我钻石斧头"));
    }

    #[test]
    fn failure_message_extracts_api_error_from_json_stdout() {
        let stdout = br#"{"type":"thread.started","thread_id":"thread-123"}
{"type":"error","status":400,"error":{"type":"invalid_request_error","message":"The 'gpt-5.5' model requires a newer version of Codex."}}
{"type":"turn.failed"}"#;

        let message = codex_failure_message(ExitStatus::from_raw(1), b"", stdout);

        assert!(message.contains("status=400"));
        assert!(message.contains("gpt-5.5"));
        assert!(!message.contains("thread-123"));
    }

    #[test]
    fn failure_message_omits_unstructured_stderr() {
        let stderr = "user\n给我钻石斧头\n<html>blocked</html>";

        let message = codex_failure_message(ExitStatus::from_raw(1), stderr.as_bytes(), b"");

        assert!(message.contains("stderr omitted"));
        assert!(!message.contains("给我钻石斧头"));
        assert!(!message.contains("<html>"));
    }

    #[test]
    fn failure_message_reports_empty_stderr_hint() {
        let message = codex_failure_message(ExitStatus::from_raw(1), b"", b"");

        assert!(message.contains("no stderr returned"));
        assert!(message.contains("Codex login status"));
        assert!(!message.contains("stderr omitted"));
    }

    fn assert_schema_objects_are_strict(value: &Value, path: &str) {
        if schema_node_is_object(value) {
            assert_eq!(
                value.get("additionalProperties").and_then(Value::as_bool),
                Some(false),
                "{path} must set additionalProperties=false"
            );

            if let Some(properties) = value.get("properties").and_then(Value::as_object) {
                let required = value
                    .get("required")
                    .and_then(Value::as_array)
                    .expect("object schemas with properties must list required fields")
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<std::collections::BTreeSet<_>>();
                for property in properties.keys() {
                    assert!(
                        required.contains(property.as_str()),
                        "{path}.properties.{property} must be listed in required"
                    );
                }
            }
        }

        match value {
            Value::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    assert_schema_objects_are_strict(item, &format!("{path}[{index}]"));
                }
            }
            Value::Object(fields) => {
                for (key, child) in fields {
                    assert_schema_objects_are_strict(child, &format!("{path}.{key}"));
                }
            }
            _ => {}
        }
    }

    fn schema_node_is_object(value: &Value) -> bool {
        match value.get("type") {
            Some(Value::String(kind)) => kind == "object",
            Some(Value::Array(kinds)) => kinds.iter().any(|kind| kind.as_str() == Some("object")),
            _ => false,
        }
    }

    fn assert_schema_omits_unsupported_keywords(value: &Value, path: &str) {
        match value {
            Value::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    assert_schema_omits_unsupported_keywords(item, &format!("{path}[{index}]"));
                }
            }
            Value::Object(fields) => {
                for keyword in [
                    "oneOf",
                    "allOf",
                    "not",
                    "dependentRequired",
                    "dependentSchemas",
                    "if",
                    "then",
                    "else",
                ] {
                    assert!(
                        !fields.contains_key(keyword),
                        "{path} contains unsupported schema keyword {keyword}"
                    );
                }
                for (key, child) in fields {
                    assert_schema_omits_unsupported_keywords(child, &format!("{path}.{key}"));
                }
            }
            _ => {}
        }
    }

    fn assert_schema_property_nodes_are_typed(value: &Value, path: &str) {
        match value {
            Value::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    assert_schema_property_nodes_are_typed(item, &format!("{path}[{index}]"));
                }
            }
            Value::Object(fields) => {
                if let Some(properties) = fields.get("properties").and_then(Value::as_object) {
                    for (name, property_schema) in properties {
                        let property_path = format!("{path}.properties.{name}");
                        assert!(
                            property_schema.get("type").is_some()
                                || property_schema.get("$ref").is_some()
                                || property_schema.get("anyOf").is_some(),
                            "{property_path} must include type, $ref, or anyOf"
                        );
                    }
                }
                if fields.contains_key("const") {
                    assert!(
                        fields.contains_key("type"),
                        "{path} uses const and must include an explicit type"
                    );
                }
                for (key, child) in fields {
                    assert_schema_property_nodes_are_typed(child, &format!("{path}.{key}"));
                }
            }
            _ => {}
        }
    }
}
