use std::collections::HashMap;
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
    runs: Arc<Mutex<HashMap<String, CommandState>>>,
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
    /// Identifier returned from `run`
    id: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct TerminateParams {
    /// Identifier returned from `run`
    id: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct WaitResult {
    /// Identifier for the running command
    id: String,
    /// Newly collected stdout since the last poll
    stdout: String,
    /// Newly collected stderr since the last poll
    stderr: String,
    /// Exit code if the process has finished
    exit_code: Option<i32>,
    /// True if the 10 second time limit elapsed before completion
    timed_out: bool,
    /// True if output exceeded the 10k character limit
    output_truncated: bool,
    /// True if additional output was produced after the limit was reached
    additional_output: bool,
}

#[tool_router]
impl ShellServer {
    /// Connect to a local bash shell. Useful for tests and non-container use.
    pub async fn new_local() -> Result<Self> {
        let shell = ContainerShell::connect_local().await?;
        Ok(Self {
            tool_router: Self::tool_router(),
            shell: Arc::new(shell),
            runs: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Connect to bash running inside a Podman container.
    pub async fn new_podman(container: impl Into<String>) -> Result<Self> {
        let shell = ContainerShell::connect_podman(container).await?;
        Ok(Self {
            tool_router: Self::tool_router(),
            shell: Arc::new(shell),
            runs: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    #[tool(description = "Run a shell command")]
    pub async fn run(
        &self,
        Parameters(params): Parameters<RunParams>,
    ) -> Result<CallToolResult, McpError> {
        let RunParams { command, stdin } = params;
        let handle = self
            .shell
            .run(command, stdin.map(|s| s.into_bytes()))
            .await
            .map_err(|e| McpError::internal_error(format!("spawn failed: {e}"), None))?;
        let id = handle.id().to_string();
        let mut state = CommandState::new(handle);
        let timed_out = collect_output(&mut state).await;
        let stdout = state.stdout.clone();
        let stderr = state.stderr.clone();
        let exit_code = state.exit_code;
        let truncated = state.truncated;
        let additional = state.additional_output;
        state.additional_output = false;
        self.runs.lock().await.insert(id.clone(), state);
        let result = WaitResult {
            id,
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
        Parameters(params): Parameters<WaitParams>,
    ) -> Result<CallToolResult, McpError> {
        let id = params.id;
        let mut runs = self.runs.lock().await;
        let state = runs
            .get_mut(&id)
            .ok_or_else(|| McpError::invalid_params("unknown id".to_string(), None))?;
        let timed_out = collect_output(state).await;
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
            runs.remove(&id);
        }
        drop(runs);
        let result = WaitResult {
            id,
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
        Parameters(params): Parameters<TerminateParams>,
    ) -> Result<CallToolResult, McpError> {
        let id = params.id;
        let mut runs = self.runs.lock().await;
        let state = runs
            .remove(&id)
            .ok_or_else(|| McpError::invalid_params("unknown id".to_string(), None))?;
        kill(Pid::from_raw(state.pid), Signal::SIGTERM)
            .map_err(|e| McpError::internal_error(format!("terminate failed: {e}"), None))?;
        let result = serde_json::json!({ "id": id });
        Ok(CallToolResult::success(vec![Content::text(
            result.to_string(),
        )]))
    }
}

#[tool_handler]
impl ServerHandler for ShellServer {}

async fn collect_output(state: &mut CommandState) -> bool {
    let timeout_fut = sleep(TIME_LIMIT);
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

    #[tokio::test]
    async fn echo_works() -> Result<()> {
        let server = ShellServer::new_local().await?;
        let params = RunParams {
            command: "echo hi".into(),
            stdin: None,
        };
        let run_res: CallToolResult = server.run(Parameters(params)).await.unwrap();
        let value: WaitResult =
            serde_json::from_str(&run_res.content[0].as_text().unwrap().text).unwrap();
        assert!(value.stdout.contains("hi"));
        assert_eq!(value.exit_code, Some(0));
        Ok(())
    }
}
