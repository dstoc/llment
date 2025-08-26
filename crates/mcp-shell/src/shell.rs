use anyhow::{Context, Result, anyhow};
use bytes::BytesMut;
use nix::sys::signal::{Signal, kill as send_signal};
use nix::unistd::Pid;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::time::{Duration, timeout};

// ========================= Backend abstraction ===============================

/// Anything that can spawn a long-lived bash with piped stdio.
pub trait ShellBackend: Send + Sync + 'static {
    /// Spawn and return a configured tokio::process::Command.
    fn spawn(&self) -> Result<Command>;
}

/// Run bash inside an existing Podman container (Linux).
pub struct PodmanBackend {
    pub container: String,
}

impl ShellBackend for PodmanBackend {
    fn spawn(&self) -> Result<Command> {
        let mut cmd = Command::new("podman");
        cmd.arg("exec")
            .arg("-i")
            .arg(&self.container)
            .arg("bash")
            .arg("--noprofile")
            .arg("--norc")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        Ok(cmd)
    }
}

/// Run a local bash (useful for tests / CI—no containers needed).
pub struct LocalBashBackend;

impl ShellBackend for LocalBashBackend {
    fn spawn(&self) -> Result<Command> {
        let mut cmd = Command::new("bash");
        cmd.arg("--noprofile")
            .arg("--norc")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        Ok(cmd)
    }
}

// =============================== API ========================================

pub struct ContainerShell {
    inner: Arc<Inner>,
}

struct Inner {
    child: Mutex<Child>,
    stdin: Mutex<ChildStdin>,
    stdout_task: tokio::task::JoinHandle<()>,
    stderr_task: tokio::task::JoinHandle<()>,
    run: Arc<Mutex<Option<RunState>>>,
}

struct RunState {
    out_tx: mpsc::Sender<String>,
    err_tx: mpsc::Sender<String>,
    done_tx: oneshot::Sender<Exit>,
}

#[derive(Clone, Debug)]
pub struct Exit {
    pub code: i32,
}

pub struct RunHandle {
    stdout_rx: mpsc::Receiver<String>,
    stderr_rx: mpsc::Receiver<String>,
    done_rx: oneshot::Receiver<Exit>,
    pid: i32,
}

impl RunHandle {
    pub async fn recv_stdout(&mut self) -> Option<String> {
        self.stdout_rx.recv().await
    }
    pub async fn recv_stderr(&mut self) -> Option<String> {
        self.stderr_rx.recv().await
    }
    pub async fn wait(self) -> Result<Exit> {
        self.done_rx.await.map_err(|_| anyhow!("run canceled"))
    }
    pub fn pid(&self) -> i32 {
        self.pid
    }
    pub fn into_parts(
        self,
    ) -> (
        mpsc::Receiver<String>,
        mpsc::Receiver<String>,
        oneshot::Receiver<Exit>,
        i32,
    ) {
        (self.stdout_rx, self.stderr_rx, self.done_rx, self.pid)
    }
    pub fn interrupt(&self) -> Result<()> {
        send_signal(Pid::from_raw(self.pid), Signal::SIGINT)?;
        Ok(())
    }
    pub fn terminate(&self) -> Result<()> {
        send_signal(Pid::from_raw(self.pid), Signal::SIGTERM)?;
        Ok(())
    }
    pub fn kill(&self) -> Result<()> {
        send_signal(Pid::from_raw(self.pid), Signal::SIGKILL)?;
        Ok(())
    }
    /// Try to get the exit status without waiting.
    pub fn try_wait(&mut self) -> Option<Exit> {
        self.done_rx.try_recv().ok()
    }
    pub fn cancel(self) {}
}

const BEGIN: &str = "\u{001E}__BEGIN__";
const BEGIN_PRINT: &[u8] = b"printf '%b\\n' '\\036__BEGIN__'\n";
const END_PREFIX: &str = "\u{001E}__END__:";

impl ContainerShell {
    /// Connect using any backend (Podman or Local).
    pub async fn connect_with<B: ShellBackend>(backend: B) -> Result<Self> {
        let mut child = backend.spawn()?.spawn().context("spawning bash")?;
        let pid = child.id().ok_or_else(|| anyhow!("no child pid"))? as i32;

        let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
        let stderr = child.stderr.take().ok_or_else(|| anyhow!("no stderr"))?;

        let run = Arc::new(Mutex::new(None));

        let stdout_task = tokio::spawn({
            let run = run.clone();
            async move { scanner_task(stdout, true, pid, &run).await }
        });
        let stderr_task = tokio::spawn({
            let run = run.clone();
            async move { scanner_task(stderr, false, pid, &run).await }
        });

        Ok(Self {
            inner: Arc::new(Inner {
                child: Mutex::new(child),
                stdin: Mutex::new(stdin),
                stdout_task,
                stderr_task,
                run,
            }),
        })
    }

    /// Connect to bash inside a Podman container.
    pub async fn connect_podman(container_name: impl Into<String>) -> Result<Self> {
        Self::connect_with(PodmanBackend {
            container: container_name.into(),
        })
        .await
    }

    /// Connect to a local bash (good for tests).
    pub async fn connect_local() -> Result<Self> {
        Self::connect_with(LocalBashBackend).await
    }

    pub async fn run(
        &self,
        command: impl AsRef<str>,
        stdin_bytes: impl Into<Option<Vec<u8>>>,
    ) -> Result<RunHandle> {
        let (out_tx, out_rx) = mpsc::channel::<String>(64);
        let (err_tx, err_rx) = mpsc::channel::<String>(64);
        let (done_tx, done_rx) = oneshot::channel::<Exit>();
        {
            let mut slot = self.inner.run.lock().await;
            if slot.is_some() {
                return Err(anyhow!("command already running"));
            }
            *slot = Some(RunState {
                out_tx,
                err_tx,
                done_tx,
            });
        }

        let mut stdin = self.inner.stdin.lock().await;

        // BEGIN
        stdin.write_all(BEGIN_PRINT).await.context("send begin")?;

        // heredoc for stdin
        let heredoc = "__IN__";
        let run_script = format!(
            "set -o pipefail; {{ {cmd}; }} <<'{tag}'\n",
            cmd = command.as_ref(),
            tag = heredoc
        );
        stdin
            .write_all(run_script.as_bytes())
            .await
            .context("send command")?;

        if let Some(bytes) = stdin_bytes.into() {
            stdin.write_all(&bytes).await.context("send stdin")?;
        }
        let close = format!("{tag}\n", tag = heredoc);
        stdin
            .write_all(close.as_bytes())
            .await
            .context("close heredoc")?;

        // END
        let end = format!(
            "status=$?; printf '{END_PREFIX}%s\\n' \"$status\"\n",
            END_PREFIX = END_PREFIX,
        );
        stdin.write_all(end.as_bytes()).await.context("send end")?;

        stdin.flush().await.ok();

        let pid = {
            let child = self.inner.child.lock().await;
            child.id().unwrap_or_default() as i32
        };

        Ok(RunHandle {
            stdout_rx: out_rx,
            stderr_rx: err_rx,
            done_rx,
            pid,
        })
    }

    pub async fn shutdown(self) -> Result<()> {
        let mut stdin = self.inner.stdin.lock().await;
        let _ = stdin.write_all(b"exit\n").await;
        let _ = stdin.flush().await;

        {
            let mut child = self.inner.child.lock().await;
            match timeout(Duration::from_millis(500), child.wait()).await {
                Ok(Ok(_)) => return Ok(()),
                _ => {
                    if let Some(pid) = child.id() {
                        let _ = send_signal(Pid::from_raw(pid as i32), Signal::SIGTERM);
                    }
                }
            }
            match timeout(Duration::from_millis(500), child.wait()).await {
                Ok(Ok(_)) => Ok(()),
                _ => {
                    if let Some(pid) = child.id() {
                        let _ = send_signal(Pid::from_raw(pid as i32), Signal::SIGKILL);
                    }
                    let _ = child.wait().await;
                    Ok(())
                }
            }
        }
    }
}

// ============================= scanner & UTF-8 ===============================

async fn scanner_task(
    mut reader: impl AsyncReadExt + Unpin,
    is_stdout: bool,
    _pid: i32,
    run: &Arc<Mutex<Option<RunState>>>,
) {
    let mut buf = [0u8; 8192];
    let mut acc = BytesMut::new();
    let mut utf = Utf8Accumulator::new();

    loop {
        match reader.read(&mut buf).await {
            Ok(0) => {
                if let Some(run) = run.lock().await.take() {
                    let _ = run.done_tx.send(Exit { code: 255 });
                }
                break;
            }
            Ok(n) => {
                acc.extend_from_slice(&buf[..n]);
                while let Some(pos) = acc.iter().position(|&b| b == b'\n') {
                    let line = acc.split_to(pos + 1).freeze();
                    // Search for BEGIN or END sentinels even if preceded by data.
                    if let Some(idx) = line
                        .windows(BEGIN.len())
                        .position(|w| w == BEGIN.as_bytes())
                    {
                        if idx > 0 {
                            deliver(run, is_stdout, &mut utf, &line[..idx]).await;
                        }
                        // ignore sentinel and anything after it on this line
                        continue;
                    }
                    if let Some(idx) = line
                        .windows(END_PREFIX.len())
                        .position(|w| w == END_PREFIX.as_bytes())
                    {
                        if idx > 0 {
                            deliver(run, is_stdout, &mut utf, &line[..idx]).await;
                        }
                        let status_bytes = &line[idx + END_PREFIX.len()..];
                        if let Ok(status_s) = std::str::from_utf8(status_bytes) {
                            let status = status_s
                                .trim_end_matches('\n')
                                .parse::<i32>()
                                .unwrap_or(255);
                            if let Some(run) = run.lock().await.take() {
                                let _ = run.done_tx.send(Exit { code: status });
                            }
                        }
                        continue;
                    }
                    // normal payload
                    deliver(run, is_stdout, &mut utf, &line).await;
                }
            }
            Err(_) => break,
        }
    }
}

async fn deliver(
    run: &Arc<Mutex<Option<RunState>>>,
    is_stdout: bool,
    utf: &mut Utf8Accumulator,
    bytes: &[u8],
) {
    let guard = run.lock().await;
    if let Some(run) = guard.as_ref() {
        let tx = if is_stdout { &run.out_tx } else { &run.err_tx };
        for chunk in utf.push(bytes) {
            let _ = tx.send(chunk).await;
        }
    }
}

/// yields only valid UTF‑8, buffering up to 3 trailing bytes
struct Utf8Accumulator {
    carry: Vec<u8>,
}
impl Utf8Accumulator {
    fn new() -> Self {
        Self {
            carry: Vec::with_capacity(4),
        }
    }
    fn push(&mut self, data: &[u8]) -> Vec<String> {
        let mut out = Vec::new();
        let mut buf = Vec::with_capacity(self.carry.len() + data.len());
        buf.extend_from_slice(&self.carry);
        buf.extend_from_slice(data);
        self.carry.clear();

        let mut i = 0;
        while i < buf.len() {
            match std::str::from_utf8(&buf[i..]) {
                Ok(valid) => {
                    if !valid.is_empty() {
                        out.push(valid.to_string());
                    }
                    i = buf.len();
                }
                Err(e) => {
                    let ok = e.valid_up_to();
                    if ok > 0 {
                        // safe
                        let s = unsafe { std::str::from_utf8_unchecked(&buf[i..i + ok]) };
                        out.push(s.to_string());
                        i += ok;
                    }
                    // store up to 3 trailing bytes (max UTF-8 tail)
                    let rem = &buf[i..];
                    if !rem.is_empty() {
                        let k = rem.len().min(3);
                        self.carry.extend_from_slice(&rem[rem.len() - k..]);
                    }
                    break;
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_bash_echo_and_status() -> Result<()> {
        let shell = ContainerShell::connect_local().await?;

        // stdout only
        let mut h = shell.run("echo hello", None).await?;
        let mut out = String::new();
        while let Some(chunk) = h.recv_stdout().await {
            out.push_str(&chunk);
        }
        let exit = h.wait().await?;
        assert_eq!(exit.code, 0);
        assert!(out.contains("hello\n"));

        shell.shutdown().await
    }

    #[tokio::test]
    async fn local_bash_stderr_and_nonzero() -> Result<()> {
        let shell = ContainerShell::connect_local().await?;
        // send to stderr and exit 7
        let mut h = shell.run("bash -c 'echo oops 1>&2; exit 7'", None).await?;
        let mut err = String::new();
        while let Some(chunk) = h.recv_stderr().await {
            err.push_str(&chunk);
        }
        let exit = h.wait().await?;
        assert_eq!(exit.code, 7);
        assert!(err.contains("oops\n"));
        shell.shutdown().await
    }

    #[tokio::test]
    async fn local_bash_with_stdin() -> Result<()> {
        let shell = ContainerShell::connect_local().await?;
        let payload = b"hello-from-stdin\n".to_vec();
        let mut h = shell.run("cat", payload).await?;
        let mut out = String::new();
        while let Some(chunk) = h.recv_stdout().await {
            out.push_str(&chunk);
        }
        let exit = h.wait().await?;
        assert_eq!(exit.code, 0);
        assert_eq!(out, "hello-from-stdin\n");
        shell.shutdown().await
    }

    #[tokio::test]
    async fn allows_command_without_trailing_newline() -> Result<()> {
        let shell = ContainerShell::connect_local().await?;

        let mut h = shell.run("printf foo", None).await?;
        let mut out = String::new();
        while let Some(chunk) = h.recv_stdout().await {
            out.push_str(&chunk);
        }
        let exit = h.wait().await?;
        assert_eq!(exit.code, 0);
        assert!(out.starts_with("foo"));

        // ensure the next command can run
        let mut h2 = shell.run("echo bar", None).await?;
        let mut out2 = String::new();
        while let Some(chunk) = h2.recv_stdout().await {
            out2.push_str(&chunk);
        }
        let exit2 = h2.wait().await?;
        assert_eq!(exit2.code, 0);
        assert!(out2.contains("bar"));
        shell.shutdown().await
    }
}
