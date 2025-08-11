# llm
Trait-based LLM client implementations for multiple providers.

## Dependencies
- async-trait
  - async trait abstraction
- serde_json
  - schema sanitization and parsing
- tokio-stream
  - stream response handling
- ollama-rs (dstoc fork)
  - communicate with Ollama using streaming and tools
- async-openai
  - connect to OpenAI models
- gemini-rs
  - connect to Gemini models
- rmcp
  - connect to MCP servers

## Features, Requirements and Constraints
- LLM clients
  - `LlmClient` trait streams chat responses
  - implementations for Ollama, OpenAI, and Gemini
- Tool schemas
  - `to_openapi_schema` strips `$schema` and converts unsigned ints to signed formats
- Responses
  - chunks include optional content, tool calls, and optional thinking text
- Tool orchestration
  - `tools` module exposes a `ToolExecutor` trait
  - `run_tool_loop` streams responses, executes tools, and issues follow-up requests
  - `McpContext` stores MCP tool mappings and metadata
    - tool call chunks insert assistant messages immediately before execution
    - accumulated streamed content is appended as an assistant message after the stream completes
