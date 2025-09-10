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
- globset, ignore, grep
  - globbing and pattern search
- glob
  - expand glob patterns for reading many files
- tracing
  - logging
  - uses `tracing-subscriber` for output formatting

## Features
- workspace root via CLI
  - paths may be absolute or relative to this directory
- mount point hides actual workspace path in responses
  - defaults to `/home/user/workspace`
  - error messages include mount point paths for missing or invalid files
  - accepts mount-point paths with or without a trailing slash
- tools
  - `replace`
    - enforces the expected number of string replacements
  - `list_directory`
    - respects git ignore
  - `read_file`
    - supports offset/limit and base64-encoded images
  - `read_many_files`
    - reads and concatenates multiple files using glob patterns
  - `create_file`
    - creates parent directories as needed
    - allows paths in the workspace root
  - `glob`
    - always respects git ignore
    - optional case sensitivity
  - `search_file_content`
    - uses `grep` crate for regex searches with optional include filters
    - respects git ignore
- parameter metadata
  - tool parameters include descriptions and default values via rmcp
  - optional parameters prefix descriptions with "Optional."
- tool errors reported via `CallToolResult::error`
  - operations return execution errors instead of protocol errors

## Constraints
- paths outside the workspace return the same error regardless of file existence
- file modification tools disabled unless `--allow-modification` flag is passed
  - modification functions assert they are enabled
- `create_file` errors if file already exists
- `glob` validates matched paths are within the workspace
- `search_file_content` validates matches are within the workspace
