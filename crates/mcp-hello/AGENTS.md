# mcp-hello
Simple MCP server that provides a greeting tool.

## Dependencies
- rmcp: implement MCP server and tool definitions.
- tokio: asynchronous runtime for server and tests.
- tracing and tracing-subscriber: structured logging.

## Features, Requirements and Constraints
- Exposes a `hello` tool returning "Hello, world!".
- Executable server runs over stdio.
