# mcp-shell
MCP server exposing shell command execution.

## Dependencies
- rmcp
  - build MCP server and tools
- tokio
  - asynchronous runtime
- nix
  - send signals to processes
- serde, schemars
  - define tool parameters and results
- anyhow
  - error handling
- tracing, tracing-subscriber
  - structured logging

## Features, Requirements and Constraints
- connects to bash locally or in a Podman container
  - container name may be passed as the first CLI argument; defaults to local bash
- tools
  - `run`
    - executes a command with optional stdin
    - returns up to 10k characters of combined stdout/stderr
    - waits at most 10 seconds for output or completion
  - `wait`
    - waits up to another 10 seconds for additional output
    - once the 10k output limit is reached, further output is not returned
    - reports if extra output was produced after the limit
  - `terminate`
    - sends SIGTERM to the running command
- only one command may run at a time
