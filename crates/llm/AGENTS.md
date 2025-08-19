# llm
Trait-based LLM client implementations for multiple providers.

## Dependencies
- async-trait
  - async trait abstraction
- serde_json
  - schema sanitization and parsing
- tokio-stream
  - stream response handling
- clap
  - parse provider enum for CLI usage
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
  - `LlmClient` trait streams chat responses and lists supported model names
  - implementations for Ollama, OpenAI, and Gemini
- Provider selection
  - `Provider` enum lists supported backends
  - `client_from` builds a client for the given provider and model
    - stores provider and model names for later retrieval
    - uses provider-specific default host when none is supplied
- Tool schemas
  - `to_openapi_schema` strips `$schema` and converts unsigned ints to signed formats
- Responses
  - chunks include optional content, tool calls, optional thinking text, and usage metrics on the final chunk
  - OpenAI client converts assistant history messages with tool calls into request `tool_calls` and stitches streaming tool call deltas into complete tool calls
  - OpenAI client parses `reasoning_content` from streamed responses into thinking text
  - deprecated `function_call` streaming is no longer supported
- Tool orchestration
  - `tools` module exposes a `ToolExecutor` trait
  - `run_tool_loop` streams responses, executes tools, and issues follow-up requests
  - `tool_event_stream` spawns the loop and yields `ToolEvent`s with a join handle for updated history
  - `mcp` module
    - `load_mcp_servers` starts configured MCP servers and collects tool schemas
    - `McpToolExecutor` implements `ToolExecutor` for MCP calls
    - `McpContext` stores MCP tool mappings and metadata
      - tool call chunks insert assistant messages immediately before execution
      - accumulated streamed content is appended as an assistant message after the stream completes
- Test utilities
  - `TestProvider` implements `LlmClient`
    - captures `ChatMessageRequest`s for assertions
    - streams queued `ResponseChunk`s for iterative testing
