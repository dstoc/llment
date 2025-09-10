# mcp-shell
MCP server exposing shell command execution.

## Dependencies
- rmcp
  - build MCP server and tools
- tokio
  - asynchronous runtime
- nix
  - send signals to processes
- clap
  - parse CLI flags
- serde, schemars
  - define tool parameters and results
- anyhow
  - error handling
- tracing, tracing-subscriber
  - structured logging

## Features
- runs commands in a fresh bash process locally or in a Podman container
  - `--container` flag selects container name
  - `--workdir` flag sets default working directory
- tools
  - `run`
    - executes a single command with optional stdin
    - accepts optional `workdir` overriding the server's default
  - `wait`
    - reports if extra output was produced after the limit
  - `terminate`
    - sends SIGTERM to abort the running command
- tool results return a status string: "still running, call wait or terminate" or "finished"
- tool results omit false flags (`output_truncated`, `additional_output`)
- tool results omit empty `stdout` and `stderr` fields
- announces available tools to clients via MCP initialize capabilities and `list_tools`
- parameter metadata
  - tool parameters include descriptions and default values via rmcp
  - optional parameters prefix descriptions with "Optional."
- tool errors reported via `CallToolResult::error`
  - state issues like missing or running commands return execution errors

## Constraints
- working directory must already exist; it is not created automatically
- `run` returns up to 10k characters of combined stdout/stderr
- output truncation respects UTF-8 character boundaries
- `run` waits at most 10 seconds for output or completion (limit configurable)
- `wait` waits up to another 10 seconds for additional output (limit configurable)
- once the 10k output limit is reached, further output is not returned
- only one command may run at a time
  - finished commands free the slot immediately, allowing sequential runs
- timed-out commands keep running until waited or terminated
  - subsequent `run` calls error with "command already running"
