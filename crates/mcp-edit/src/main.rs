use anyhow::Result;
use mcp_edit::EditServer;
use rmcp::{ServiceExt, transport::stdio};
use std::{env, path::PathBuf};
use tracing_subscriber::{self, EnvFilter};

/// Run the Edit MCP server over stdio.
///
/// The workspace root may be provided as the first command-line argument.
/// If omitted, the current working directory is used.
#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging to stderr without ANSI color codes.
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::DEBUG.into()))
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    tracing::info!("Starting mcp-edit server");

    let workspace_root: PathBuf = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().expect("failed to get current dir"));

    let service = EditServer::new(workspace_root)
        .serve(stdio())
        .await
        .map_err(|e| {
            tracing::error!("serving error: {:?}", e);
            e
        })?;

    service.waiting().await?;
    Ok(())
}
