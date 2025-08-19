use anyhow::Result;
use mcp_edit::FsServer;
use rmcp::{ServiceExt, transport::stdio};
use std::{env, path::PathBuf};
use tracing_subscriber::{self, EnvFilter};

/// Run the Edit MCP server over stdio.
///
/// The workspace root may be provided as the first command-line argument.
/// An optional second argument sets the mount point used in responses.
/// If omitted, the current working directory is used and the mount point
/// defaults to `/home/user/workspace`.
#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging to stderr without ANSI color codes.
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::DEBUG.into()))
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    tracing::info!("Starting mcp-edit server");

    let mut args = env::args().skip(1);
    let workspace_root: PathBuf = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().expect("failed to get current dir"));
    let mount_point: PathBuf = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/home/user/workspace"));

    let service = FsServer::new_with_mount_point(workspace_root, mount_point)
        .serve(stdio())
        .await
        .map_err(|e| {
            tracing::error!("serving error: {:?}", e);
            e
        })?;

    service.waiting().await?;
    Ok(())
}
