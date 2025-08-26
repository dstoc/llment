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
- runs commands in a fresh bash process locally or in a Podman container
  - `--container` and `--workdir` flags configure container name and default working directory
  - defaults to local bash in `/home/user/workspace`
- tools
  - `run`
    - executes a single command with optional stdin
    - accepts optional `workdir` overriding the server's default
    - returns up to 10k characters of combined stdout/stderr
    - waits at most 10 seconds for output or completion (limit configurable)
  - `wait`
    - waits up to another 10 seconds for additional output (limit configurable)
    - once the 10k output limit is reached, further output is not returned
    - reports if extra output was produced after the limit
  - `terminate`
    - sends SIGTERM to abort the running command
- only one command may run at a time
  - finished commands free the slot immediately, allowing sequential runs
- tool results omit false flags (`timed_out`, `output_truncated`, `additional_output`)
- tool results omit empty `stdout` and `stderr` fields
- timed-out commands keep running until waited or terminated
  - subsequent run calls error with "command already running"
- announces available tools to clients via MCP `list_tools`
