import re

for filename in ["crates/llment/src/builtins.rs", "crates/llment/src/modes/code_agent.rs"]:
    with open(filename, "r") as f:
        code = f.read()

    code = code.replace("tool::Parameters", "wrapper::Parameters")
    code = code.replace("tool::{Parameters", "tool::{ToolRouter}, rmcp::handler::server::wrapper::{Parameters")
    code = code.replace("router::tool::ToolRouter, tool::Parameters", "router::tool::ToolRouter, wrapper::Parameters")

    # Fix ServerInfo literal
    code = re.sub(
        r"ServerInfo \{\s*capabilities: ServerCapabilities::builder\(\)\.enable_tools\(\)\.build\(\),\s*\.\.Default::default\(\)\s*\}",
        "{\n        let mut info = ServerInfo::default();\n        info.capabilities = ServerCapabilities::builder().enable_tools().build();\n        info\n    }",
        code
    )

    with open(filename, "w") as f:
        f.write(code)
