use anyhow::Result;
use mcp_edit::EditServer;
use rmcp::{ServiceExt, transport::stdio};
use std::env;
use tracing_subscriber::{self, EnvFilter};

/// Run the Edit MCP server over stdio.
#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging to stderr without ANSI color codes.
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::DEBUG.into()))
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    tracing::info!("Starting mcp-edit server");

    let workspace_root = env::current_dir()?;

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
