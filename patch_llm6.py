with open("crates/llm/src/gemini_rust.rs", "r") as f:
    code = f.read()

# Replace FunctionDeclaration::new(t.name, t.description, Some(params)) with manual construction to set parameters
code = code.replace("let params: serde_json::Map<String, serde_json::Value> = serde_json::from_value(params_value)?;", "let params: serde_json::Value = serde_json::from_value(params_value)?;")

code = code.replace("FunctionDeclaration::new(t.name, t.description, Some(params))", "FunctionDeclaration { name: t.name, description: t.description, behavior: None, parameters: Some(params) }")

with open("crates/llm/src/gemini_rust.rs", "w") as f:
    f.write(code)
