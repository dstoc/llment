# llm-cli
Basic terminal chat interface scaffold using tuirealm and ratatui.

## Dependencies
- ratatui
  - terminal UI rendering
- tuirealm
  - component-based TUI framework
- textwrap
  - wrap conversation lines
- unicode-width
  - measure display width for proper box padding
- termimad
  - render markdown in assistant responses
- tui-textarea
  - multiline text input with standard editing

## Features, Requirements and Constraints
- layout
  - scrollable conversation pane
    - mouse wheel adjusts scroll
    - mouse clicks or scrolls in the conversation area focus it even when input is active
  - text input field at the bottom
    - supports multi-line editing with wrapping
    - height expands to fit content
    - Ctrl-J inserts a new line
    - standard shortcuts: Ctrl-W delete previous word, Ctrl-L clears input
    - paste inserts clipboard text
    - clicking the field focuses it
    - cursor hidden when unfocused
    - trailing spaces do not move the cursor to the next line
  - Tab switches focus between conversation and input
  - Esc exits the application
- conversation items
  - user messages render inside a boxed region
  - assistant messages show working steps and final response
    - working and tool sections toggle with Enter or mouse click
    - final responses render markdown via termimad
  - items stored as a strongly typed `Node` enum implementing `ConvNode`
    - selection moves between components, not individual lines
    - helper methods append items and steps, bumping `content_rev` for caching
  - partial items are clipped when scrolled
  - line caches invalidate on width or content changes
  - clicking items selects them and toggles collapse
- code structure
  - conversation resides under `src/conversation` with modules for nodes and mutation helpers
