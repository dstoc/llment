{% if tool_enabled("shell_wait") %}
# Timeout Handling for Shell Commands

When a command launched via `shell_run` exceeds the allotted time, the
operation may either still be making progress or be stalled.  You
should decide whether to continue waiting for a normal exit or to
terminate the process.

## Recommended Workflow

1. **Detect a timeout** – The shell interface returns an error when the
   command times out.
2. **Check for ongoing progress** – If the command is still producing
   output (stdout/stderr) or updating its internal state, it is likely
   alive.  In this case, call `shell_wait` to allow the process to
   continue.
3. **Abort if stalled** – If the command shows no output for a period
   or appears stuck, call `shell_terminate`.
{% endif %}

