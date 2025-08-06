use anyhow::Result;
use mcp_hello::HelloServer;
use rmcp::{ServiceExt, transport::stdio};
use tracing_subscriber::{self, EnvFilter};

/// Run the Hello MCP server over stdio.
#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging to stderr without ANSI color codes.
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::DEBUG.into()))
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    tracing::info!("Starting mcp-hello server");

    let service = HelloServer::new().serve(stdio()).await.map_err(|e| {
        tracing::error!("serving error: {:?}", e);
        e
    })?;

    service.waiting().await?;
    Ok(())
}
