use anyhow::Result;
use clap::Parser;
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

    let args = Args::parse();
    let server = if let Some(name) = args.container {
        ShellServer::new_podman(name, args.workdir).await?
    } else {
        ShellServer::new_local(args.workdir).await?
    };

    let service = server.serve(stdio()).await.map_err(|e| {
        tracing::error!("serving error: {:?}", e);
        e
    })?;

    service.waiting().await?;
    Ok(())
}

#[derive(Parser)]
struct Args {
    /// Run commands inside a Podman container
    #[arg(long)]
    container: Option<String>,
    /// Working directory for command execution
    #[arg(long)]
    workdir: String,
}
