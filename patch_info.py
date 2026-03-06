import os

files_to_patch = [
    "crates/mcp-edit/src/lib.rs",
    "crates/llment/src/builtins.rs",
    "crates/llment/src/modes/code_agent.rs",
    "crates/mcp-shell/src/lib.rs",
    "crates/mcp-hello/src/lib.rs"
]

for file_path in files_to_patch:
    with open(file_path, "r") as f:
        code = f.read()

    # Simple replace logic since the pattern is identical everywhere
    if "{" in code and "let mut info = ServerInfo::default();" in code:
        code = code.replace(
            "{\n            let mut info = ServerInfo::default();\n            info.capabilities = ServerCapabilities::builder().enable_tools().build();\n            info\n        }",
            "ServerInfo::new(ServerCapabilities::builder().enable_tools().build())"
        )
        code = code.replace(
            "let mut info = ServerInfo::default();\n        info.capabilities = ServerCapabilities::builder().enable_tools().build();\n        info",
            "ServerInfo::new(ServerCapabilities::builder().enable_tools().build())"
        )
    with open(file_path, "w") as f:
        f.write(code)
