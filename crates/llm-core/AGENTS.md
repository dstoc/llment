# llm-core
Shared LLM abstraction and backend implementations.

## Dependencies
- async-trait
  - async trait interface
- ollama-rs (dstoc fork)
  - common chat and tool types
  - streaming Ollama backend
- async-openai
  - OpenAI backend
- gemini-rs
  - Gemini backend
- tokio-stream
  - stream trait for response chunks
- serde / serde_json
  - convert schemas to OpenAPI JSON

## Features, Requirements and Constraints
- exposes `LlmClient` trait with streaming chat API
- re-exports chat and tool types from `ollama-rs`
- implementations for Ollama, OpenAI, and Gemini
- sanitizes schemas for OpenAPI compatibility
