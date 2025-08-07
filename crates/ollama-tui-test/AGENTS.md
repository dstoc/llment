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
- termimad: render markdown.

## Features, Requirements and Constraints
- Streams assistant responses and displays incremental "thinking" tokens before assistant messages.
- Loads MCP servers from configuration, exposes their tools, and executes tool calls with error handling.
- Chat history is wrapped and scrollable with scrollbar and mouse support.
- Groups all reasoning and tool steps into a single "Thinking" block that shows "Thinking" while in progress and summarizes as "Thought for …" when complete.
- Allows specifying the Ollama host via CLI option.
- User prompts render inside a boxed region with a 5-character left margin followed by a blank line; thinking blocks are flush left with wrapped lines indented by two spaces and end with a blank line.
- Thinking steps start with a bullet; tool names are italicized while tool arguments and results render as plain text.
- Markdown rendering via termimad preserves code block styling and tables with padding and Unicode borders and is covered by tests.
