# ollama-tui-test
Terminal chat interface to Ollama with MCP tool integration.

## Dependencies
- clap: parse command-line arguments.
- ollama-rs (dstoc fork): communicate with Ollama using streaming and tools.
- tokio and tokio-stream: asynchronous runtime and streaming.
- ratatui and crossterm: terminal UI and input handling.
- rmcp: connect to MCP servers.
- serde and serde_json: load MCP server configuration.
- once_cell: shared state for loaded tools.
- textwrap: wrap chat history.

## Features, Requirements and Constraints
- Streams assistant responses and displays incremental "thinking" tokens before assistant messages.
- Loads MCP servers from configuration, exposes their tools, and executes tool calls with error handling.
- Chat history is wrapped and scrollable with scrollbar and mouse support.
- Thinking traces, tool calls, and tool results are collapsible.
- Allows specifying the Ollama host via CLI option.
