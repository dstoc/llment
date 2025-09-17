# mcp-chatgpt-proxy
HTTP MCP proxy that loads MCP servers from configuration and exposes them over streamable HTTP with the bare minimum amount of OAuth2 to work with ChatGPT.

## Dependencies
- rmcp
  - expose loaded MCP tools over streamable HTTP
- axum
  - HTTP server and routing
- tokio
  - asynchronous runtime
- clap
  - CLI argument parsing
- llm
  - load MCP server configurations
- rand
  - generate authorization codes and tokens
- sha2
  - compute PKCE S256 code challenges
- base64
  - encode PKCE hashes
- serde
  - JSON serialization for OAuth flows

## Features
- CLI arguments
  - `--mcp` loads MCP server definitions from a JSON file
  - `--addr` sets the HTTP bind address (defaults to `127.0.0.1:8080`)
 - OAuth2 flow
   - `/.well-known/oauth-authorization-server` metadata endpoint
   - `/register` dynamic client registration
   - `/authorize` authorization code grant with PKCE
   - `/token` exchanges codes for bearer tokens
 - Re-exports tools from configured MCP servers with their prefixes
 - Serves MCP over streamable HTTP using SSE sessions

## Constraints
- None
