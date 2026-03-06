import re

with open("crates/llm/src/mcp.rs", "r") as f:
    code = f.read()

code = code.replace("model::{CallToolRequestParam, RawContent}", "model::{CallToolRequestParams, RawContent}")
code = re.sub(
    r"\.call_tool\(CallToolRequestParam \{\s*name: ([^,]+),\s*arguments: ([^\n]+),\s*\}\)",
    r".call_tool(CallToolRequestParams::builder().name(\1).arguments(\2.unwrap_or_default()).build())",
    code
)
code = re.sub(
    r"let text = if let Some\(content\) = result\.content \{\s*content",
    r"let text = if true {\n            result.content",
    code
)

with open("crates/llm/src/mcp.rs", "w") as f:
    f.write(code)

with open("crates/llm/src/gemini_rust.rs", "r") as f:
    code = f.read()

code = code.replace("FunctionCallingMode, FunctionDeclaration, FunctionParameters, Gemini", "FunctionCallingMode, FunctionDeclaration, Gemini")
code = code.replace("let params: FunctionParameters = serde_json::from_value(params_value)?;", "let params: serde_json::Map<String, serde_json::Value> = serde_json::from_value(params_value)?;")
code = code.replace("builder = builder.with_function_response(\n                            t.tool_name,\n                            serde_json::json!({ \"output\": content }),\n                        );", "builder = builder.with_function_response(\n                            t.tool_name,\n                            serde_json::json!({ \"output\": content }),\n                        ).unwrap();")
code = code.replace("builder = builder.with_function_response(\n                            t.tool_name,\n                            serde_json::json!({ \"error\": error }),\n                        );", "builder = builder.with_function_response(\n                            t.tool_name,\n                            serde_json::json!({ \"error\": error }),\n                        ).unwrap();")
code = code.replace("builder = builder.with_thinking_config(gemini_rust::ThinkingConfig {\n                thinking_budget_tokens: Some(budget),\n            });", "let mut thinking = gemini_rust::ThinkingConfig::default();\n            thinking.thinking_budget_tokens = Some(budget);\n            builder = builder.with_thinking_config(thinking);")

with open("crates/llm/src/gemini_rust.rs", "w") as f:
    f.write(code)
