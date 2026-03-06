with open("crates/llm/src/gemini_rust.rs", "r") as f:
    code = f.read()

code = code.replace("Some(gemini_rust::Behavior::new(params))", "Some(gemini_rust::tools::Behavior::new(params))")
code = code.replace("thinking_level: gemini_rust::ThinkingLevel::ThinkingLevelUnspecified", "thinking_level: Some(gemini_rust::ThinkingLevel::ThinkingLevelUnspecified)")

with open("crates/llm/src/gemini_rust.rs", "w") as f:
    f.write(code)
