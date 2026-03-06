import re

with open("crates/llm/src/mcp.rs", "r") as f:
    code = f.read()

code = code.replace(
    ".call_tool(CallToolRequestParams::builder().name(tool_name.to_string().into()).arguments(args.as_object().cloned().unwrap_or_default()).build())",
    ".call_tool(CallToolRequestParams::new(tool_name.to_string()).with_arguments(args.as_object().cloned().unwrap_or_default()))"
)

with open("crates/llm/src/mcp.rs", "w") as f:
    f.write(code)

with open("crates/llm/src/gemini_rust.rs", "r") as f:
    code = f.read()

code = code.replace("FunctionDeclaration::new(t.name, t.description, params)", "FunctionDeclaration::new(t.name, t.description, None)")
code = code.replace("gemini_rust::ThinkingConfig {", "gemini_rust::ThinkingConfig { thinking_level: gemini_rust::ThinkingLevel::Standard,")

with open("crates/llm/src/gemini_rust.rs", "w") as f:
    f.write(code)
