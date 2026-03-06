import os

for path in ["crates/mcp-edit/src/lib.rs", "crates/mcp-hello/src/lib.rs"]:
    with open(path, "r") as f:
        code = f.read()

    code = code.replace("content.unwrap()", "content")

    with open(path, "w") as f:
        f.write(code)
