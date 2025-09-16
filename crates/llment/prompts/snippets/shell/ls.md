{% if tool_enabled("shell_run") %}
## Searching and Listing Files

Use `rg` -- ripgrep. Always use `rg` instead of `grep`.

When you need to list files in a directory, prefer using `rg --files` over
`ls`. `rg --files` behaves like `ls -R` (recursive) but respects
`.gitignore`.

```bash
rg --files
```

If you need to limit the search to a subdirectory *or* just a single folder,
`rg -d1 --files` behaves like `ls` (non‑recursive).

```bash
rg -d1 --files src
```

To filter the list of files, use rg recursively.

```bash
rg --files | rg 'md$'
```
{% endif %}
