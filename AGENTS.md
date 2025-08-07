# Agent Guide

This repository contains several Rust crates within the `crates/` workspace directory. Each crate has a brief overview, key dependencies, and the paths to its primary entry points.

## mcp-edit
A file-system based MCP server offering tooling for reading, writing, searching, and modifying files under a workspace root.

- **Entry points:** `crates/mcp-edit/src/lib.rs`, `crates/mcp-edit/src/main.rs`
- **Key dependencies:**
  - `rmcp` for server and tool abstractions.
  - `serde` and `schemars` for parameter serialization and schema generation.
  - `tokio` for asynchronous execution.
  - `tracing` and `tracing-subscriber` for logging.
  - `ignore`, `globset`, and `regex` for file system traversal and matching.

## mcp-hello
A minimal MCP server exposing a single `hello` tool that returns a friendly greeting. Includes basic tests demonstrating tool wiring.

- **Entry points:** `crates/mcp-hello/src/lib.rs`, `crates/mcp-hello/src/main.rs`
- **Key dependencies:**
  - `rmcp` for server and tool routing.
  - `tokio` for async test support.
  - `tracing` and `tracing-subscriber` for logging.

## ollama-tui-test
A terminal UI experiment that streams chat completions from an Ollama model and exercises tool-calling through the `ollama-rs` crate.

- **Entry points:** `crates/ollama-tui-test/src/main.rs`
- **Key dependencies:**
  - `ollama-rs` for model interaction and tool registration.
  - `tokio` and `tokio-stream` for async streaming.
  - `ratatui` and `crossterm` for terminal rendering and input handling.
  - `clap` for command-line argument parsing.
