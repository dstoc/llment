with open("crates/llm/src/gemini_rust.rs", "r") as f:
    code = f.read()

# Let's figure out what FunctionDeclaration::new actually takes
import re
code = code.replace("Some(gemini_rust::tools::Behavior::new(params))", "Some(params)")

with open("crates/llm/src/gemini_rust.rs", "w") as f:
    f.write(code)
