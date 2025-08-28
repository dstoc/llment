> [!WARNING]  
> This project gives an agent the ability to run shell commands and modify files on your system.
> You should not use it unless you understand the behaviors and precautions taken by the [mcp-edit](./crates/mcp-edit) and [mcp-shell](./crates/mcp-shell) crates.
>
> Also... be aware that a large amount of code in this project was generated.

![explain](https://github.com/user-attachments/assets/c58bd392-a674-4433-a15f-11791cd5ba4e)

## Quick Start
Install with [`cargo`](https://doc.rust-lang.org/cargo/getting-started/installation.html)
```sh
> cargo install --git https://github.com/dstoc/llment llment mcp-edit mcp-shell
```
Run with defaults (preferred):
> * `llama-server` on localhost
> * `gpt-oss:20b`
> * no external MCP servers, only the harmless [builtin](crates/llment/src/builtins.rs) tool `get_message_count`
> ```sh
> > llment
> ```

## Providers
`--provider` can be used to select a different provider:
* `llama-server` (preferred)
* `openai`
  * Status: incomplete for use with official OpenAI models, uses the `chat/` api which does not support thinking 
* `gemini`
  * Status: works but incomplete. Does not send encrypted thinking tokens back. Some API bugs in gemini-rs.
  * Requires GEMINI_API_KEY in env.
* `ollama`
  * Status: unknown, worked previously.
 
`--model` and `--host` can be used to customize further, e.g.

> Ollama (with custom model and endpoint):
> ```sh
> > llment --provider ollama --model qwen3:30b --host https://my-ollama.tailc.ts.net:11434
> ```

## Model Context Protocol servers
> [!WARNING]
> There are currently no approval steps in order for an agent to execute functions exposed by MCP servers.

`--mcp file.json` loads a claude-code like mcp.json file.

For example, the following configuration loads two STDIO based MCP servers.
The functions from `mcp-edit` will be prefixed with `files.` as in `files.create_file`, similarly with `shell.` for `mcp-shell`.
The server commands are launched with the same working directory that `llment` was.

```json
{
  "mcpServers": {
    "files": {
      "command": "mcp-edit"
    },
    "shell": {
      "command": "mcp-shell",
      "args": [
        "--workdir", "/home/user/workspace",
        "--container", "sandbox"
      ]
    }
  }
}
```

No other "mcp.json" options or features beyond those used above are currently supported. 

### mcp-edit
The mcp-edit server provides a set of file system tools similar to [gemini-cli](https://github.com/google-gemini/gemini-cli/blob/main/docs/tools/file-system.md).

> [!WARNING]
> By default, mcp-server will provide access to the current working directory.

```
Usage: mcp-edit [OPTIONS] [WORKSPACE_ROOT] [MOUNT_POINT]

Arguments:
  [WORKSPACE_ROOT]  Workspace root directory (default: current directory) [default: .]
  [MOUNT_POINT]     Mount point path used in responses (default: `/home/user/workspace`) [default: /home/user/workspace]

Options:
      --trace  Show trace
  -h, --help   Print help
```

### mcp-shell
The mcp-shell server provides the ability to execute shell commands inside a container.

An example container configuration is provided in [`./sandbox`](./sandbox).

```
Usage: mcp-shell [OPTIONS] --workdir <WORKDIR> <--container <CONTAINER>|--unsafe-local-access>

Options:
      --container <CONTAINER>  Run commands inside a Podman container
      --unsafe-local-access    Run commands inside a local shell. Unsafe
      --workdir <WORKDIR>      Working directory for command execution
      --trace                  Show trace
  -h, --help                   Print help
```

Commands are executed in the named container using `podman exec`
