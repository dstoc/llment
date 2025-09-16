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
> * assumes you have `llama-server` on localhost:8000 serving gpt-oss (without the `--jinja` flag)
> * no external MCP servers, only the harmless [builtin](crates/llment/src/builtins.rs) tool `get_message_count`
> ```sh
> > llment
> ```

## Providers
`--provider` can be used to select a different provider:
* `harmony` (preferred) connects with [openai/harmony](https://github.com/openai/harmony) compatible models via the [llama-server](https://github.com/ggml-org/llama.cpp/tree/master/tools/server) `/completion` API
* `ollama` - uses [ollama-rs](https://crates.io/crates/ollama-rs) to interface with the Ollama API
  * <details>
      <summary>Example</summary>
     
      ```sh
      > llment --host http://localhost:11434 --provider ollama --model qwen3:latest
      ```
    </details>
* `openai-chat` - uses [async-openai](https://crates.io/crates/async-openai) to connect with OpenAI's `/v1/chat/completions` API
  * <details>
      <summary>Example - Ollama via OpenAI compat API</summary>
     
      ```sh
      > llment --host http://localhost:11434 --provider openai-chat --model qwen3:latest
      ```
    </details>
* `gemini-rust` - uses the [gemini-rust](https://crates.io/crates/gemini-rust) crate to interface with the Gemini API
  * Status: functional but incomplete. Reasoning context is not maintained.
  * Requires GEMINI_API_KEY in env.
  * <details>
      <summary>Example</summary>
     
      ```sh
      > GEMINI_API_KEY=... llment --provider gemini-rust --model gemini-2.5-flash
      ```
    </details>
 
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
The functions from `mcp-edit` will be prefixed with `files_` as in `files_create_file`, similarly with `shell_` for `mcp-shell`.
MCP server names must not contain underscores because tool identifiers are generated as `<prefix>_<tool>`.
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
> By default, mcp-edit provides read-only access to the current working directory.

```
Usage: mcp-edit [OPTIONS] [WORKSPACE_ROOT] [MOUNT_POINT]

Arguments:
  [WORKSPACE_ROOT]  Workspace root directory (default: current directory) [default: .]
  [MOUNT_POINT]     Mount point path used in responses (default: `/home/user/workspace`) [default: /home/user/workspace]

Options:
      --trace               Show trace
      --allow-modification  Enable file modification tools
  -h, --help                Print help
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
