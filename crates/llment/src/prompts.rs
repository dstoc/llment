use globset::Glob;
use minijinja::Environment;
use rust_embed::RustEmbed;
use std::collections::HashSet;

#[derive(RustEmbed)]
#[folder = "prompts"]
pub(crate) struct PromptAssets;

#[cfg(test)]
#[derive(RustEmbed)]
#[folder = "tests/prompts"]
pub(crate) struct TestPromptAssets;

#[cfg(test)]
pub(crate) type Assets = TestPromptAssets;
#[cfg(not(test))]
pub(crate) type Assets = PromptAssets;

pub(crate) fn load_prompt(
    name: &str,
    enabled_tools: impl IntoIterator<Item = String>,
) -> Option<String> {
    let enabled_tools: HashSet<String> = enabled_tools.into_iter().collect();
    let mut env = Environment::new();
    env.set_loader(|name| {
        let mut candidates: Vec<String> = vec![name.to_string()];
        if !name.ends_with(".md") {
            candidates.push(format!("{}.md", name));
        }
        for candidate in candidates {
            if let Some(file) = Assets::get(&candidate) {
                let content = String::from_utf8_lossy(file.data.as_ref()).to_string();
                return Ok(Some(content));
            }
        }
        Ok(None)
    });
    env.add_function(
        "glob",
        |pattern: String| -> Result<Vec<String>, minijinja::Error> {
            let glob = Glob::new(&pattern).map_err(|e| {
                minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, e.to_string())
            })?;
            let matcher = glob.compile_matcher();
            let mut matches: Vec<String> = Assets::iter()
                .map(|f| f.as_ref().to_string())
                .filter(|name| matcher.is_match(name))
                .collect();
            matches.sort();
            Ok(matches)
        },
    );
    env.add_function("tool_enabled", move |t: String| {
        Ok(enabled_tools.contains(&t))
    });
    if let Ok(tmpl) = env.get_template(name) {
        if let Ok(rendered) = tmpl.render(()) {
            return Some(rendered);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::load_prompt;

    #[test]
    fn load_md_prompt() {
        let content = load_prompt("sys/hello", Vec::new()).unwrap();
        assert!(content.contains("You are a helpful assistant."));
    }

    #[test]
    fn load_md_with_include() {
        let content = load_prompt("sys/outer", Vec::new()).unwrap();
        assert!(content.contains("Outer."));
        assert!(content.contains("Inner."));
        assert!(content.contains("Deep."));
    }

    #[test]
    fn load_md_with_glob() {
        let content = load_prompt("sys/glob", Vec::new()).unwrap();
        assert!(content.contains("You are a helpful assistant."));
    }

    #[test]
    fn tool_enabled_fn() {
        let content = load_prompt("sys/tool", vec!["shell.run".to_string()]).unwrap();
        assert!(content.contains("Enabled!"));
        let content = load_prompt("sys/tool", Vec::new()).unwrap();
        assert!(content.contains("Disabled!"));
    }
}
