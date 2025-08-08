# mcp-edit
MCP server offering file system editing utilities.

## Dependencies
- rmcp
  - build MCP server and tools
- serde
  - define tool parameter types
  - uses `schemars` to generate JSON schemas
- tokio
  - asynchronous runtime and test framework
- base64
  - encode binary file data
- globset, ignore, regex
  - globbing and pattern search
- tracing
  - logging
  - uses `tracing-subscriber` for output formatting

## Features, Requirements and Constraints
- workspace root via CLI
  - all paths must be absolute within this directory
- tools
  - `replace`
    - enforces the expected number of string replacements
  - `list_directory`
  - `read_file`
    - supports offset/limit and base64-encoded images
  - `write_file`
    - creates parent directories as needed
  - `glob`
    - respects git ignore and optional case sensitivity
  - `search_file_content`
    - runs regex searches with optional include filters
