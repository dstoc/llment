# Commits
* always use conventional commit format, with prefix feat/fix/refactor/chore
  * specify the crate if relevant, e.g. `fix(mcp-hello) ...`

# Workflow
* prefer to use `cargo` to modify Cargo.toml files over editing them directly
  * this helps ensure we install the latest version of components etc
* always run `cargo fmt` before committing

# Crates
The project is divided into crates:
* [crates/mcp-hello](crates/mcp-hello/AGENTS.md)
* [crates/mcp-edit](crates/mcp-edit/AGENTS.md)
* [crates/llm](crates/llm/AGENTS.md)
* [crates/llment](crates/llment/AGENTS.md)
* [crates/mcp-shell](crates/mcp-shell/AGENTS.md)

# Glossary
MCP: Model Context Protocol

# AGENTS.md protocol
Each crate/component has its own AGENTS.md file that summarizes the component and the features/requirements/constraints that have been established so far.

At the end of each task, for the corresponding AGENTS.md files:
* check that no listed features/requirements/constraints have been accidentally removed or violated.
* update the list to add/remove/update any features/requirements/constraints involved in this specific task.

The list should always be formatted as brief bullet points with hierarchical structure. Sub-lists may be nested as deeply as necessary. Minor/unimportant details should be omitted. e.g:
* The textbox allows both prompts and commands
  * Commands start with a /
    * Example: `/help` lists commands
  * Ctrl-D will exit

A template for component-specific AGENTS.md files follows:

# Component Name
Brief description of the component.

## Dependencies
A bullet-point list of key dependencies and the reason they are needed. Minor dependencies are omitted.

# Features, Requirements and Constraints
A bullet-point list of the component's features.
