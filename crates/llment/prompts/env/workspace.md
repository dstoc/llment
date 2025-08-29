# Workspace Overview

All commands, tools, and shell interactions executed by this project are
performed **relative to the workspace root**:

```
/home/user/workspace
```

When you use tools or run shell commands via the `shell.run` tool, the
current working directory defaults to the workspace root.  This means:

* `apply_patch <<'PATCH'` operates on files relative to the workspace.
* `shell.run` without a `workdir` argument runs in `/home/user/workspace`.

Feel free to change the working directory in your commands using the
`workdir` option, but keep in mind that relative paths will then be
resolved against the specified directory.
