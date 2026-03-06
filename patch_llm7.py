with open("crates/llm/src/gemini_rust.rs", "r") as f:
    code = f.read()

# Replace construction with unsafe modification or json serialize/deserialize
code = code.replace(
    "let function = FunctionDeclaration { name: t.name, description: t.description, behavior: None, parameters: Some(params) };",
    "let function: FunctionDeclaration = serde_json::from_value(serde_json::json!({\n                    \"name\": t.name,\n                    \"description\": t.description,\n                    \"parameters\": params\n                })).unwrap();"
)

with open("crates/llm/src/gemini_rust.rs", "w") as f:
    f.write(code)
