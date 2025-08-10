# llm-cli
Parallel reimplementation of the LLM terminal UI using tui-realm components.

## Dependencies
- ratatui
  - terminal rendering
- tuirealm
  - component framework
- tui-realm-stdlib
  - reusable widgets
- crossterm
  - terminal events
- tokio / tokio-stream
  - async runtime and event forwarding
- llm-core
  - shared LLM abstraction

## Features, Requirements and Constraints
- UI built from tui-realm components for app, chat, history, input, and history items
- thinking blocks collapse/expand logic lives in history item component
- event producers forward into a single channel using custom `ChatEvent` carrying LLM `ResponseChunk`s
- chat history appends streamed assistant/thinking content; pressing `t` toggles the last thinking block
