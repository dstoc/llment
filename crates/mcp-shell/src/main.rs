use anyhow::Result;
use clap::{ArgGroup, Parser};
use mcp_shell::ShellServer;
use rmcp::{ServiceExt, transport::stdio};
use tracing_subscriber::{self, EnvFilter};

/// Run the Shell MCP server over stdio.
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

    tracing::info!("Starting mcp-shell server");

    let server = if let Some(name) = args.container {
        ShellServer::new_podman(name, args.workdir).await?
    } else if args.unsafe_local_access {
        ShellServer::new_local(args.workdir).await?
    } else {
        unreachable!()
    };

    let service = server.serve(stdio()).await.map_err(|e| {
        tracing::error!("serving error: {:?}", e);
        e
    })?;

    service.waiting().await?;
    Ok(())
}

#[derive(Parser)]
#[command(group(
    ArgGroup::new("required")
        .required(true)
        .multiple(false)
        .args(&["container", "unsafe_local_access"]),
))]
struct Args {
    /// Run commands inside a Podman container
    #[arg(long)]
    container: Option<String>,
    /// Run commands inside a local shell. Unsafe.
    #[arg(long)]
    unsafe_local_access: bool,
    /// Working directory for command execution
    #[arg(long)]
    workdir: String,
    /// Show trace
    #[arg(long)]
    trace: bool,
}
