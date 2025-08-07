# mcp-edit
MCP server offering file system editing utilities.

## Dependencies
- rmcp: build MCP server and tools.
- serde and schemars: define tool parameter types and schemas.
- tokio: asynchronous runtime and test framework.
- base64: encode binary file data.
- globset, ignore, regex: globbing and pattern search.
- tracing and tracing-subscriber: logging.

## Features, Requirements and Constraints
- Workspace root is provided via CLI; all paths must be absolute within this directory.
- Tools include `replace`, `list_directory`, `read_file`, `write_file`, `glob`, and `search_file_content`.
- `replace` enforces the expected number of string replacements.
- `read_file` supports offset/limit and base64-encoded images.
- `write_file` creates parent directories as needed.
- `glob` respects git ignore and optional case sensitivity.
- `search_file_content` runs regex searches with optional include filters.
