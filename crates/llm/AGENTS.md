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
- schemars
  - define and manipulate tool schemas

## Features
- LLM clients
  - `LlmClient` trait streams chat responses and lists supported model names
  - implementations for Ollama, OpenAI, LlamaServer, and Gemini
- Provider selection
  - `Provider` enum lists supported backends
  - `client_from` builds a client for the given provider and model
    - stores provider and model names for later retrieval
- Tool schemas
  - `to_openapi_schema` strips `$schema` and converts unsigned ints to signed formats
- Core message and tool types defined locally instead of re-exporting from `ollama-rs`
  - tool calls hold name and arguments directly
  - tool info stores name, description, and parameters without wrapper enums
  - chat messages are an enum of `UserMessage`, `AssistantMessage`, `SystemMessage`, and `ToolMessage`, each with only relevant fields
    - tool calls include an `id` string, assigned locally when missing
    - tool messages carry the same `id` and store `content` as `serde_json::Value`
- Chat message, request, and response types serialize to and from JSON
  - skips serializing fields that are `None`, empty strings, or empty arrays
- Responses
  - chunks include optional content, tool calls, optional thinking text, and usage metrics on the final chunk
  - OpenAI client converts assistant history messages with tool calls into request `tool_calls` and stitches streaming tool call deltas into complete tool calls
  - OpenAI client parses `reasoning_content` from streamed responses into thinking text
- Tool orchestration
  - `tools` module exposes a `ToolExecutor` trait
  - `run_tool_loop` streams responses, executes tools, and issues follow-up requests
  - `tool_event_stream` spawns the loop and yields `ToolEvent`s
    - join handle resolves on completion with history updated in place
- `mcp` module
- `load_mcp_servers` starts configured MCP servers and collects tool schemas
  - tool names are prefixed with the server name
  - `McpService` implements `ClientHandler`
    - `on_tool_list_changed` refreshes tool metadata from the service
    - tool metadata stored in an `ArcSwap` for lock-free snapshots
  - `McpContext` stores running service handles keyed by prefix
    - supports runtime insertion and removal of services via internal locking
    - exposes merged `tool_infos` from all services
    - provides a non-blocking `tool_names` snapshot of available tools
    - implements `ToolExecutor` for MCP calls
    - tool call chunks insert assistant messages immediately before execution
      - any accumulated assistant content is included with the tool call
    - accumulated streamed content is appended as an assistant message after the stream completes
- Test utilities
  - `TestProvider` implements `LlmClient`
    - captures `ChatMessageRequest`s for assertions
    - streams queued `ResponseChunk`s for iterative testing

## Constraints
- uses provider-specific default host when none is supplied
  - LlamaServer defaults to `http://localhost:8000/v1` and wraps the OpenAI client
- deprecated `function_call` streaming is no longer supported
