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
- unicode-width
  - measure display width for proper box padding

## Features, Requirements and Constraints
- layout
  - scrollable conversation pane
    - mouse wheel adjusts scroll
    - mouse clicks or scrolls in the conversation area focus it even when input is active
  - text input field at the bottom
  - Tab switches focus between conversation and input
  - Esc exits the application
- conversation items
  - user messages render inside a boxed region
  - assistant messages show working steps and final response
    - working and tool sections toggle with Enter or mouse click
  - items stored as a strongly typed `Node` enum implementing `ConvNode`
    - selection moves between components, not individual lines
    - helper methods append items and steps, bumping `content_rev` for caching
  - partial items are clipped when scrolled
  - line caches invalidate on width or content changes
  - clicking items selects them and toggles collapse
