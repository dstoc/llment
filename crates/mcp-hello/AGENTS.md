# mcp-hello
Simple MCP server that provides a greeting tool.

## Dependencies
- rmcp
  - implement MCP server and tool definitions
- tokio
  - asynchronous runtime for server and tests
- tracing
  - structured logging
  - uses `tracing-subscriber` for output formatting

## Features
- tools
  - `hello`
    - returns "Hello, world!"
- server
  - runs over stdio

## Constraints
- None
