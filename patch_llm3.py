with open("crates/llm/src/gemini_rust.rs", "r") as f:
    code = f.read()

code = code.replace("ThinkingLevel::Standard", "ThinkingLevel::ThinkingLevelUnspecified")
code = code.replace("FunctionDeclaration::new(t.name, t.description, None)", "FunctionDeclaration::new(t.name, t.description, Some(gemini_rust::Behavior::new(params)))")

with open("crates/llm/src/gemini_rust.rs", "w") as f:
    f.write(code)
