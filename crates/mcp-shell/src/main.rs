use std::env;

use anyhow::Result;
use mcp_shell::ShellServer;
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

    let container = env::args().nth(1);
    let server = if let Some(name) = container {
        ShellServer::new_podman(name).await?
    } else {
        ShellServer::new_local().await?
    };

    let service = server.serve(stdio()).await.map_err(|e| {
        tracing::error!("serving error: {:?}", e);
        e
    })?;

    service.waiting().await?;
    Ok(())
}
