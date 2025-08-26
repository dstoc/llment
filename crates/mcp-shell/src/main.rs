use std::env;

use anyhow::Result;
use mcp_shell::{DEFAULT_WORKDIR, ShellServer};
use rmcp::{ServiceExt, transport::stdio};
use tracing_subscriber::{self, EnvFilter};

/// Run the Shell MCP server over stdio.
#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging to stderr without ANSI color codes.
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::DEBUG.into()))
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    tracing::info!("Starting mcp-shell server");

    let mut args = env::args().skip(1);
    let mut container: Option<String> = None;
    let mut workdir = DEFAULT_WORKDIR.to_string();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--container" => {
                if let Some(name) = args.next() {
                    container = Some(name);
                }
            }
            "--workdir" => {
                if let Some(dir) = args.next() {
                    workdir = dir;
                }
            }
            _ => {}
        }
    }
    let server = if let Some(name) = container {
        ShellServer::new_podman_with_workdir(name, workdir).await?
    } else {
        ShellServer::new_local_with_workdir(workdir).await?
    };

    let service = server.serve(stdio()).await.map_err(|e| {
        tracing::error!("serving error: {:?}", e);
        e
    })?;

    service.waiting().await?;
    Ok(())
}
