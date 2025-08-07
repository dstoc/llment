# Commits
* Always use conventional commit format, with prefix feat/fix/refactor/chore
  * Specify the crate if relevant, e.g. fix(mcp-hello) ...

# Crates
The project is divided into crates:
* crates/mcp-hello
* crates/mcp-edit
* crates/ollama-tui-test

# AGENTS.md protocol
Each crate/component has its own AGENTS.md file that summarizes the component and the features/requirements/constraints that have been established so far.

At the end of each task, for the corresponding AGENTS.md files:
* Check that no listed features/requirements/constraints have been accidentally removed or violated.
* Update the list to add/remove/update any features/requirements/constraints involved in this specific task.

The list should always be formatted as brief bullet points. Minor/unimportant details should be omitted.

A template for component-specific AGENTS.md files follows:

# Component Name
Brief description of the component.

## Dependencies
A bullet-point list of key dependencies and the reason they are needed. Minor dependencies are omitted.

# Features, Requirements and Constraints
A bullet-point list of the component's features.
