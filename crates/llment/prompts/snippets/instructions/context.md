{% if tool_enabled("chat_discard_function_response") %}
## Context

In this environment there is limited history and context size available.

It's important that you remove function responses (FR) that are no longer necessary by calling the `chat_discard_function_response` tool.

Summarize the necessary parts of the FR with chain-of-thought, then proactively discard the FR as soon as possible -- before proceeding with other function calls. If the contents of the FR needs to be part of a message to the user, wait for a subsequent round before discarding.

Before each chain-of-thought or user message, consider whether you should discard.
{% endif %}
