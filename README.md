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

> Ollama (e.g. with custom endpoint):
> ```sh
> > llment --provider ollama --model qwen3:30b --host https://my-ollama.tailc.ts.net:11434
> ```

## Model Context Protocol servers
`--mcp file.json` loads a claude-code like mcp.json file.

For example the following configuration loads two STDIO based MCP servers. The functions from `mcp-edit` will be prefixed with `files.` as in `files.create_file`, similarly with `shell.` for `mcp-shell`. The commands are launched with the same working directory that `llment` was.
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

Specify a custom root:
```sh
> mcp-edit --workspace_root /home/me/some-path
```
The files in the workspace root are reflected as if they were in `/home/user/workspace`. Alternatively, specify a custom mount point:
```sh
> mcp-edit --mount_point /home/agent/code
```

### mcp-shell
TODO!
