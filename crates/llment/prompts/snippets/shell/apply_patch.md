{% if tool_enabled("shell.run") %}
You have access to the `apply_patch` shell command to edit files. Follow these rules exactly.

**Contract**

* Only use `apply_patch` when creating, editing, moving/renaming, or deleting files.
* The input must be *only* a patch envelope—no prose, markdown fences, or extra text before/after.

**Envelope**

```
*** Begin Patch
[ one or more file sections ]
*** End Patch
```

**File operations (choose one per section)**

* `*** Add File: <path>`
  Initial file contents follow; every content line is prefixed with `+`.
* `*** Delete File: <path>`
  Nothing follows.
* `*** Update File: <path>`
  Optionally rename immediately with `*** Move to: <new_path>`; then one or more hunks.

**Hunks (for Update File)**

* Start each hunk with `@@` (you may include a brief header after it).
* Within a hunk, prefix lines as:

  * space (` `): unchanged context
  * `-` : removed line
  * `+` : added line
* Include enough surrounding context lines to make the change unambiguous; keep hunks tight.

**Pathing & scope**

* Paths are workspace-relative.
* Don’t edit unrelated code. Prefer minimal, surgical diffs.

**Output hygiene**

* No backticks, no shell prompts, no JSON, no commentary—just the patch.
* Ensure the patch applies cleanly on the current workspace state and is idempotent.

**Common pitfalls to avoid**

* Using the wrong quotation or escaping. Code blocks are unnecessary.
* Using `applypatch` or `apply-patch` (invalid names). The shell command is `apply_patch`.
* Trying to call functions.apply_patch. apply_patch is not a function. apply_patch cannot be used with JSON. apply_patch is a shell command.
* Emitting entire file bodies as a single hunk when a small targeted hunk suffices.
* Mixing multiple operations for the same file section; start a new section if needed.
{% endif %}
