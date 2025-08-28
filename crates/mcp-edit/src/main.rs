use anyhow::Result;
use clap::Parser;
use mcp_edit::FsServer;
use rmcp::{ServiceExt, transport::stdio};
use std::path::PathBuf;
use tracing_subscriber::{self, EnvFilter};

/// Run the Edit MCP server over stdio.
#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    if args.trace {
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::from_default_env().add_directive(tracing::Level::DEBUG.into()),
            )
            .with_writer(std::io::stderr)
            .with_ansi(false)
            .init();
    }

    tracing::info!("Starting mcp-edit server");

    let mut server = FsServer::new_with_mount_point(args.workspace_root, args.mount_point);
    if !args.allow_modification {
        server.disable_modification_tools();
    }
    let service = server.serve(stdio()).await.map_err(|e| {
        tracing::error!("serving error: {:?}", e);
        e
    })?;

    service.waiting().await?;
    Ok(())
}

#[derive(Parser)]
struct Args {
    /// Workspace root directory (default: current directory)
    #[arg(default_value_os = ".")]
    workspace_root: PathBuf,

    /// Mount point path used in responses (default: `/home/user/workspace`)
    #[arg(default_value_os = "/home/user/workspace")]
    mount_point: PathBuf,

    /// Show trace
    #[arg(long)]
    trace: bool,

    /// Allow tools that modify files
    #[arg(long)]
    allow_modification: bool,
}
