use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
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
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::time::sleep;

const OUTPUT_LIMIT: usize = 10_000;
const TIME_LIMIT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct ShellServer {
    tool_router: ToolRouter<Self>,
    container: Option<String>,
    run: Arc<Mutex<Option<CommandState>>>,
    time_limit: Duration,
    workdir: String,
}

impl ShellServer {
    async fn new(
        container: Option<String>,
        time_limit: Duration,
        workdir: impl Into<String>,
    ) -> Result<Self> {
        let workdir = workdir.into();
        std::fs::create_dir_all(&workdir).context("create workdir")?;
        Ok(Self {
            tool_router: Self::tool_router(),
            container,
            run: Arc::new(Mutex::new(None)),
            time_limit,
            workdir,
        })
    }

    pub async fn new_local(workdir: impl Into<String>) -> Result<Self> {
        Self::new(None, TIME_LIMIT, workdir).await
    }

    pub async fn new_local_with_limit(limit: Duration, workdir: impl Into<String>) -> Result<Self> {
        Self::new(None, limit, workdir).await
    }

    pub async fn new_podman(container: String, workdir: impl Into<String>) -> Result<Self> {
        Self::new(Some(container), TIME_LIMIT, workdir).await
    }

    pub async fn new_podman_with_limit(
        container: String,
        limit: Duration,
        workdir: impl Into<String>,
    ) -> Result<Self> {
        Self::new(Some(container), limit, workdir).await
    }
}

#[tool_handler]
impl ServerHandler for ShellServer {}

struct CommandState {
    stdout_rx: mpsc::Receiver<String>,
    stderr_rx: mpsc::Receiver<String>,
    done_rx: Option<oneshot::Receiver<Exit>>,
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
            done_rx: Some(done_rx),
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

#[derive(Clone, Debug)]
struct Exit {
    code: i32,
}

struct RunHandle {
    stdout_rx: mpsc::Receiver<String>,
    stderr_rx: mpsc::Receiver<String>,
    done_rx: oneshot::Receiver<Exit>,
    pid: i32,
}

#[allow(dead_code)]
impl RunHandle {
    async fn recv_stdout(&mut self) -> Option<String> {
        self.stdout_rx.recv().await
    }
    async fn recv_stderr(&mut self) -> Option<String> {
        self.stderr_rx.recv().await
    }
    async fn wait(self) -> Result<Exit> {
        self.done_rx.await.context("run canceled")
    }
    fn pid(&self) -> i32 {
        self.pid
    }
    fn into_parts(
        self,
    ) -> (
        mpsc::Receiver<String>,
        mpsc::Receiver<String>,
        oneshot::Receiver<Exit>,
        i32,
    ) {
        (self.stdout_rx, self.stderr_rx, self.done_rx, self.pid)
    }
    fn terminate(&self) -> Result<()> {
        kill(Pid::from_raw(self.pid), Signal::SIGTERM)?;
        Ok(())
    }
}

fn spawn_command(
    container: Option<String>,
    command: String,
    stdin: Option<String>,
    workdir: String,
) -> Result<RunHandle> {
    std::fs::create_dir_all(&workdir).context("create workdir")?;
    let mut cmd = if let Some(c) = container {
        let mut cmd = Command::new("podman");
        cmd.arg("exec").arg("-i");
        cmd.arg("--workdir").arg(&workdir);
        cmd.arg(&c);
        cmd.arg("bash")
            .arg("--noprofile")
            .arg("--norc")
            .arg("-c")
            .arg(&command);
        cmd
    } else {
        let mut cmd = Command::new("bash");
        cmd.arg("--noprofile").arg("--norc").arg("-c").arg(&command);
        cmd.current_dir(Path::new(&workdir));
        cmd
    };
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let mut child: Child = cmd.spawn().context("failed to spawn")?;
    if let Some(input) = stdin {
        if let Some(mut w) = child.stdin.take() {
            tokio::spawn(async move {
                let _ = w.write_all(input.as_bytes()).await;
            });
        }
    }
    let stdout = child.stdout.take().context("missing stdout")?;
    let stderr = child.stderr.take().context("missing stderr")?;
    let pid = child.id().unwrap_or_default() as i32;

    let (out_tx, out_rx) = mpsc::channel(16);
    tokio::spawn(async move {
        let mut reader = tokio::io::BufReader::new(stdout);
        let mut buf = [0u8; 1024];
        loop {
            match reader.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    if out_tx
                        .send(String::from_utf8_lossy(&buf[..n]).to_string())
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let (err_tx, err_rx) = mpsc::channel(16);
    tokio::spawn(async move {
        let mut reader = tokio::io::BufReader::new(stderr);
        let mut buf = [0u8; 1024];
        loop {
            match reader.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    if err_tx
                        .send(String::from_utf8_lossy(&buf[..n]).to_string())
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let (done_tx, done_rx) = oneshot::channel();
    tokio::spawn(async move {
        let code = child
            .wait()
            .await
            .ok()
            .and_then(|s| s.code())
            .unwrap_or_default();
        let _ = done_tx.send(Exit { code });
    });

    Ok(RunHandle {
        stdout_rx: out_rx,
        stderr_rx: err_rx,
        done_rx,
        pid,
    })
}

#[derive(Deserialize, JsonSchema)]
pub struct RunParams {
    command: String,
    stdin: Option<String>,
    #[serde(default)]
    workdir: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct WaitParams {}

#[derive(Deserialize, JsonSchema)]
pub struct TerminateParams {}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct WaitResult {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    stdout: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    stderr: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "is_false")]
    timed_out: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    output_truncated: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    additional_output: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[tool_router]
impl ShellServer {
    #[tool(description = "Run a command in the shell")]
    pub async fn run(
        &self,
        Parameters(params): Parameters<RunParams>,
    ) -> Result<CallToolResult, McpError> {
        let RunParams {
            command,
            stdin,
            workdir,
        } = params;
        let dir = workdir.unwrap_or_else(|| self.workdir.clone());
        let mut run_slot = self.run.lock().await;
        if run_slot.is_some() {
            return Err(McpError::invalid_params("command already running", None));
        }
        let handle = spawn_command(self.container.clone(), command, stdin, dir)
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
        Parameters(_p): Parameters<WaitParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut run_slot = self.run.lock().await;
        let state = run_slot
            .as_mut()
            .ok_or_else(|| McpError::invalid_params("no running command", None))?;
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

    #[tool(description = "Terminate the running command")]
    pub async fn terminate(
        &self,
        Parameters(_p): Parameters<TerminateParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut run_slot = self.run.lock().await;
        let state = run_slot
            .take()
            .ok_or_else(|| McpError::invalid_params("no running command", None))?;
        kill(Pid::from_raw(state.pid), Signal::SIGTERM)
            .map_err(|e| McpError::internal_error(format!("terminate failed: {e}"), None))?;
        drop(run_slot);
        Ok(CallToolResult::success(vec![Content::text("{}")]))
    }
}

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
    if state.stdout_closed && state.stderr_closed {
        if let Some(done_rx) = state.done_rx.take() {
            if let Ok(exit) = done_rx.await {
                state.exit_code = Some(exit.code);
            }
        }
        false
    } else {
        if let Some(done_rx) = state.done_rx.as_mut() {
            if let Ok(exit) = done_rx.try_recv() {
                state.exit_code = Some(exit.code);
            }
        }
        true
    }
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

    const WORKDIR: &str = "/home/user/workspace";

    #[tokio::test]
    async fn run_captures_output() -> Result<()> {
        let server = ShellServer::new_local(WORKDIR).await?;
        let params = RunParams {
            command: "echo hi".into(),
            stdin: None,
            workdir: None,
        };
        let res: CallToolResult = server.run(Parameters(params)).await.unwrap();
        let value: WaitResult =
            serde_json::from_str(&res.content[0].as_text().unwrap().text).unwrap();
        assert_eq!(value.stdout.trim(), "hi");
        assert_eq!(value.exit_code, Some(0));
        Ok(())
    }

    #[tokio::test]
    async fn run_respects_workdir() -> Result<()> {
        use std::fs::{create_dir, write};
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let path = dir.path();
        create_dir(path.join("sub")).unwrap();
        write(path.join("sub/test.txt"), b"ok").unwrap();
        let server = ShellServer::new_local(WORKDIR).await?;
        let params = RunParams {
            command: "cat test.txt".into(),
            stdin: None,
            workdir: Some(path.join("sub").to_string_lossy().into()),
        };
        let res: CallToolResult = server.run(Parameters(params)).await.unwrap();
        let value: WaitResult =
            serde_json::from_str(&res.content[0].as_text().unwrap().text).unwrap();
        assert_eq!(value.stdout.trim(), "ok");
        assert_eq!(value.exit_code, Some(0));
        Ok(())
    }

    #[tokio::test]
    async fn terminate_kills_command() -> Result<()> {
        let server = ShellServer::new_local_with_limit(Duration::from_millis(100), WORKDIR).await?;
        let params = RunParams {
            command: "sleep 5".into(),
            stdin: None,
            workdir: None,
        };
        let run_res: CallToolResult = server.run(Parameters(params)).await.unwrap();
        let run_value: WaitResult =
            serde_json::from_str(&run_res.content[0].as_text().unwrap().text).unwrap();
        assert!(run_value.timed_out);
        server
            .terminate(Parameters(TerminateParams {}))
            .await
            .unwrap();
        Ok(())
    }

    #[tokio::test]
    async fn lists_tools() -> Result<()> {
        use rmcp::ServiceExt;
        let server = ShellServer::new_local(WORKDIR).await?;
        let (client_side, server_side) = tokio::io::duplex(1024);
        let server_task = tokio::spawn(async move {
            let svc = server.serve(server_side).await.unwrap();
            svc.waiting().await.unwrap();
        });
        let client = ().serve(client_side).await.unwrap();
        let tools = client.list_tools(Default::default()).await.unwrap();
        let names: Vec<_> = tools.tools.into_iter().map(|t| t.name).collect();
        assert!(names.contains(&"run".into()));
        assert!(names.contains(&"wait".into()));
        assert!(names.contains(&"terminate".into()));
        client.cancel().await.unwrap();
        server_task.await.unwrap();
        Ok(())
    }
}
