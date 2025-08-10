# llm-cli
Basic terminal chat interface scaffold using tuirealm and ratatui.

## Dependencies
- ratatui
  - terminal UI rendering
- tuirealm
  - component-based TUI framework
- tui-realm-stdlib
  - prebuilt tuirealm components
- textwrap
  - wrap conversation lines

## Features, Requirements and Constraints
- layout
  - scrollable conversation pane
  - text input field at the bottom
  - Tab switches focus between conversation and input
  - Esc exits the application
- conversation items
  - user messages render inside a boxed region
  - assistant messages show working steps and final response
    - working and tool sections toggle with Enter
