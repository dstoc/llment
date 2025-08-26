use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::tool::{Parameters, ToolRouter},
    model::{CallToolResult, Content},
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::time::sleep;

mod shell;
pub use shell::ContainerShell;
use shell::{Exit, RunHandle};

const OUTPUT_LIMIT: usize = 10_000;
const TIME_LIMIT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct ShellServer {
    tool_router: ToolRouter<Self>,
    shell: Arc<ContainerShell>,
    run: Arc<Mutex<Option<CommandState>>>,
    time_limit: Duration,
}

struct CommandState {
    stdout_rx: mpsc::Receiver<String>,
    stderr_rx: mpsc::Receiver<String>,
    done_rx: oneshot::Receiver<Exit>,
    pid: i32,
    stdout: String,
    stderr: String,
    stdout_pos: usize,
    stderr_pos: usize,
    truncated: bool,
    additional_output: bool,
    exit_code: Option<i32>,
    stdout_closed: bool,
    stderr_closed: bool,
}

impl CommandState {
    fn new(handle: RunHandle) -> Self {
        let (stdout_rx, stderr_rx, done_rx, pid) = handle.into_parts();
        Self {
            stdout_rx,
            stderr_rx,
            done_rx,
            pid,
            stdout: String::new(),
            stderr: String::new(),
            stdout_pos: 0,
            stderr_pos: 0,
            truncated: false,
            additional_output: false,
            exit_code: None,
            stdout_closed: false,
            stderr_closed: false,
        }
    }

    fn total_len(&self) -> usize {
        self.stdout.len() + self.stderr.len()
    }
}

#[derive(Deserialize, JsonSchema)]
pub struct RunParams {
    /// Command to run in the shell
    command: String,
    /// Optional stdin to pass to the command
    stdin: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct WaitParams {
    // no parameters
}

#[derive(Deserialize, JsonSchema)]
pub struct TerminateParams {
    // no parameters
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct WaitResult {
    /// Newly collected stdout since the last poll
    #[serde(default, skip_serializing_if = "String::is_empty")]
    stdout: String,
    /// Newly collected stderr since the last poll
    #[serde(default, skip_serializing_if = "String::is_empty")]
    stderr: String,
    /// Exit code if the process has finished
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_code: Option<i32>,
    /// True if the 10 second time limit elapsed before completion
    #[serde(default, skip_serializing_if = "is_false")]
    timed_out: bool,
    /// True if output exceeded the 10k character limit
    #[serde(default, skip_serializing_if = "is_false")]
    output_truncated: bool,
    /// True if additional output was produced after the limit was reached
    #[serde(default, skip_serializing_if = "is_false")]
    additional_output: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[tool_router]
impl ShellServer {
    /// Connect to a local bash shell. Useful for tests and non-container use.
    pub async fn new_local() -> Result<Self> {
        Self::new_local_with_limit(TIME_LIMIT).await
    }

    /// Connect to a local bash shell with a custom time limit.
    pub async fn new_local_with_limit(limit: Duration) -> Result<Self> {
        let shell = ContainerShell::connect_local().await?;
        Ok(Self {
            tool_router: Self::tool_router(),
            shell: Arc::new(shell),
            run: Arc::new(Mutex::new(None)),
            time_limit: limit,
        })
    }

    /// Connect to bash running inside a Podman container.
    pub async fn new_podman(container: impl Into<String>) -> Result<Self> {
        Self::new_podman_with_limit(container, TIME_LIMIT).await
    }

    /// Connect to Podman bash with a custom time limit.
    pub async fn new_podman_with_limit(
        container: impl Into<String>,
        limit: Duration,
    ) -> Result<Self> {
        let shell = ContainerShell::connect_podman(container).await?;
        Ok(Self {
            tool_router: Self::tool_router(),
            shell: Arc::new(shell),
            run: Arc::new(Mutex::new(None)),
            time_limit: limit,
        })
    }

    #[tool(description = "Run a shell command")]
    pub async fn run(
        &self,
        Parameters(params): Parameters<RunParams>,
    ) -> Result<CallToolResult, McpError> {
        let RunParams { command, stdin } = params;
        let mut run_slot = self.run.lock().await;
        if run_slot.is_some() {
            return Err(McpError::invalid_params(
                "command already running".to_string(),
                None,
            ));
        }
        let handle = self
            .shell
            .run(command, stdin.map(|s| s.into_bytes()))
            .await
            .map_err(|e| McpError::internal_error(format!("spawn failed: {e}"), None))?;
        let mut state = CommandState::new(handle);
        let timed_out = collect_output(&mut state, self.time_limit).await;
        let stdout = state.stdout.clone();
        let stderr = state.stderr.clone();
        let exit_code = state.exit_code;
        let truncated = state.truncated;
        let additional = state.additional_output;
        if timed_out {
            state.stdout_pos = state.stdout.len();
            state.stderr_pos = state.stderr.len();
            state.additional_output = false;
            *run_slot = Some(state);
        } else {
            *run_slot = None;
        }
        drop(run_slot);
        let result = WaitResult {
            stdout,
            stderr,
            exit_code,
            timed_out,
            output_truncated: truncated,
            additional_output: additional,
        };
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&result).unwrap(),
        )]))
    }

    #[tool(description = "Wait for a running command for up to 10s")]
    pub async fn wait(
        &self,
        Parameters(_params): Parameters<WaitParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut run_slot = self.run.lock().await;
        let state = run_slot
            .as_mut()
            .ok_or_else(|| McpError::invalid_params("no running command".to_string(), None))?;
        let timed_out = collect_output(state, self.time_limit).await;
        let stdout = state.stdout[state.stdout_pos..].to_string();
        let stderr = state.stderr[state.stderr_pos..].to_string();
        state.stdout_pos = state.stdout.len();
        state.stderr_pos = state.stderr.len();
        let exit_code = state.exit_code;
        let truncated = state.truncated;
        let additional = state.additional_output;
        state.additional_output = false;
        let finished = state.stdout_closed && state.stderr_closed && exit_code.is_some();
        if finished {
            *run_slot = None;
        }
        drop(run_slot);
        let result = WaitResult {
            stdout,
            stderr,
            exit_code,
            timed_out,
            output_truncated: truncated,
            additional_output: additional,
        };
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&result).unwrap(),
        )]))
    }

    #[tool(description = "Terminate a running command")]
    pub async fn terminate(
        &self,
        Parameters(_params): Parameters<TerminateParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut run_slot = self.run.lock().await;
        let state = run_slot
            .take()
            .ok_or_else(|| McpError::invalid_params("no running command".to_string(), None))?;
        kill(Pid::from_raw(state.pid), Signal::SIGTERM)
            .map_err(|e| McpError::internal_error(format!("terminate failed: {e}"), None))?;
        let result = serde_json::json!({});
        Ok(CallToolResult::success(vec![Content::text(
            result.to_string(),
        )]))
    }
}

#[tool_handler]
impl ServerHandler for ShellServer {}

async fn collect_output(state: &mut CommandState, limit: Duration) -> bool {
    let timeout_fut = sleep(limit);
    tokio::pin!(timeout_fut);
    loop {
        tokio::select! {
            chunk = state.stdout_rx.recv(), if !state.stdout_closed => {
                match chunk {
                    Some(line) => handle_chunk(state, true, line),
                    None => state.stdout_closed = true,
                }
            },
            chunk = state.stderr_rx.recv(), if !state.stderr_closed => {
                match chunk {
                    Some(line) => handle_chunk(state, false, line),
                    None => state.stderr_closed = true,
                }
            },
            _ = &mut timeout_fut => { break; }
        }
        if state.stdout_closed && state.stderr_closed {
            break;
        }
    }
    if let Ok(exit) = state.done_rx.try_recv() {
        state.exit_code = Some(exit.code);
    }
    !(state.stdout_closed && state.stderr_closed && state.exit_code.is_some())
}

fn handle_chunk(state: &mut CommandState, is_stdout: bool, chunk: String) {
    if state.truncated {
        if !chunk.is_empty() {
            state.additional_output = true;
        }
        return;
    }
    let remaining = OUTPUT_LIMIT.saturating_sub(state.total_len());
    if remaining == 0 {
        state.truncated = true;
        if !chunk.is_empty() {
            state.additional_output = true;
        }
        return;
    }
    let take = remaining.min(chunk.len());
    let part = &chunk[..take];
    if is_stdout {
        state.stdout.push_str(part);
    } else {
        state.stderr.push_str(part);
    }
    if take < chunk.len() {
        state.truncated = true;
        state.additional_output = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn captures_all_output_fields() -> Result<()> {
        let server = ShellServer::new_local().await?;
        let params = RunParams {
            command: "bash -c 'echo out; echo err >&2'".into(),
            stdin: None,
        };
        let run_res: CallToolResult = server.run(Parameters(params)).await.unwrap();
        let text = &run_res.content[0].as_text().unwrap().text;
        let json: serde_json::Value = serde_json::from_str(text).unwrap();
        assert!(json.get("timed_out").is_none());
        assert!(json.get("output_truncated").is_none());
        assert!(json.get("additional_output").is_none());
        let value: WaitResult = serde_json::from_str(text).unwrap();
        assert_eq!(value.stdout.trim(), "out");
        assert_eq!(value.stderr.trim(), "err");
        assert_eq!(value.exit_code, Some(0));
        assert!(!value.timed_out);
        assert!(!value.output_truncated);
        assert!(!value.additional_output);
        Ok(())
    }

    #[tokio::test]
    async fn sequential_runs_without_wait() -> Result<()> {
        let server = ShellServer::new_local().await?;

        let first = RunParams {
            command: "echo first".into(),
            stdin: None,
        };
        let first_res: CallToolResult = server.run(Parameters(first)).await.unwrap();
        let first_value: WaitResult =
            serde_json::from_str(&first_res.content[0].as_text().unwrap().text).unwrap();
        assert!(first_value.stdout.contains("first"));
        assert_eq!(first_value.exit_code, Some(0));

        let second = RunParams {
            command: "echo second".into(),
            stdin: None,
        };
        let second_res: CallToolResult = server.run(Parameters(second)).await.unwrap();
        let second_value: WaitResult =
            serde_json::from_str(&second_res.content[0].as_text().unwrap().text).unwrap();
        assert!(second_value.stdout.contains("second"));
        assert_eq!(second_value.exit_code, Some(0));
        Ok(())
    }

    #[tokio::test]
    async fn omits_empty_output_fields() -> Result<()> {
        let server = ShellServer::new_local().await?;
        let params = RunParams {
            command: "true".into(),
            stdin: None,
        };
        let run_res: CallToolResult = server.run(Parameters(params)).await.unwrap();
        let text = &run_res.content[0].as_text().unwrap().text;
        let json: serde_json::Value = serde_json::from_str(text).unwrap();
        assert!(json.get("stdout").is_none());
        assert!(json.get("stderr").is_none());
        let value: WaitResult = serde_json::from_str(text).unwrap();
        assert_eq!(value.exit_code, Some(0));
        Ok(())
    }

    #[tokio::test]
    async fn timeout_and_wait() -> Result<()> {
        let server = ShellServer::new_local_with_limit(Duration::from_millis(100)).await?;

        let params = RunParams {
            command: "sleep 0.2; echo done".into(),
            stdin: None,
        };
        let run_res: CallToolResult = server.run(Parameters(params)).await.unwrap();
        let run_value: WaitResult =
            serde_json::from_str(&run_res.content[0].as_text().unwrap().text).unwrap();
        assert!(run_value.timed_out);
        assert!(run_value.exit_code.is_none());

        let wait_res: CallToolResult = server.wait(Parameters(WaitParams {})).await.unwrap();
        let wait_value: WaitResult =
            serde_json::from_str(&wait_res.content[0].as_text().unwrap().text).unwrap();
        assert!(wait_value.timed_out);
        assert!(wait_value.exit_code.is_none());

        tokio::time::sleep(Duration::from_millis(200)).await;
        let wait_res: CallToolResult = server.wait(Parameters(WaitParams {})).await.unwrap();
        let wait_value: WaitResult =
            serde_json::from_str(&wait_res.content[0].as_text().unwrap().text).unwrap();
        assert_eq!(wait_value.exit_code, Some(0));
        assert!(wait_value.stdout.contains("done"));
        assert!(!wait_value.timed_out);
        Ok(())
    }

    #[tokio::test]
    async fn terminate_allows_new_run() -> Result<()> {
        let server = ShellServer::new_local_with_limit(Duration::from_millis(100)).await?;

        let params = RunParams {
            command: "sleep 5".into(),
            stdin: None,
        };
        let run_res: CallToolResult = server.run(Parameters(params)).await.unwrap();
        let run_value: WaitResult =
            serde_json::from_str(&run_res.content[0].as_text().unwrap().text).unwrap();
        assert!(run_value.timed_out);

        let _ = server
            .terminate(Parameters(TerminateParams {}))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;

        let wait_err = server.wait(Parameters(WaitParams {})).await;
        assert!(wait_err.is_err());
        Ok(())
    }
}
