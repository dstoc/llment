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
- glob
  - expand glob patterns for reading many files
- tracing
  - logging
  - uses `tracing-subscriber` for output formatting

## Features, Requirements and Constraints
- workspace root via CLI
  - all paths must be absolute within this directory
  - paths outside the workspace return the same error regardless of file existence
- tools
  - `replace`
    - enforces the expected number of string replacements
  - `list_directory`
  - `read_file`
    - supports offset/limit and base64-encoded images
  - `read_many_files`
    - reads and concatenates multiple files using glob patterns
  - `write_file`
    - creates parent directories as needed
  - `glob`
    - respects git ignore and optional case sensitivity
  - `search_file_content`
    - runs regex searches with optional include filters
- parameter metadata
  - tool parameters include descriptions and default values via rmcp
  - optional parameters prefix descriptions with "Optional."
