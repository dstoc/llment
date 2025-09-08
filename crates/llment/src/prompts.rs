use globset::Glob;
use minijinja::Environment;
use rust_embed::RustEmbed;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

fn collect_files(dir: &Path) -> Vec<String> {
    fn recurse(path: &Path, base: &Path, out: &mut Vec<String>) {
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    recurse(&path, base, out);
                } else if path.is_file() {
                    if let Ok(rel) = path.strip_prefix(base) {
                        out.push(rel.to_string_lossy().replace('\\', "/"));
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    recurse(dir, dir, &mut out);
    out
}

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
    role: Option<&str>,
    enabled_tools: impl IntoIterator<Item = String>,
    prompt_dir: Option<&Path>,
) -> Option<String> {
    let enabled_tools: HashSet<String> = enabled_tools.into_iter().collect();
    let mut env = Environment::new();
    let prompt_dir = prompt_dir.map(PathBuf::from);
    let loader_dir = prompt_dir.clone();
    env.set_loader(move |name| {
        let mut candidates: Vec<String> = vec![name.to_string()];
        if !name.ends_with(".md") {
            candidates.push(format!("{}.md", name));
        }
        for candidate in candidates {
            if let Some(dir) = &loader_dir {
                let path = dir.join(&candidate);
                if let Ok(content) = std::fs::read_to_string(&path) {
                    return Ok(Some(content));
                }
            }
            if let Some(file) = Assets::get(&candidate) {
                let content = String::from_utf8_lossy(file.data.as_ref()).to_string();
                return Ok(Some(content));
            }
        }
        Ok(None)
    });
    let glob_dir = prompt_dir;
    env.add_function(
        "glob",
        move |pattern: String| -> Result<Vec<String>, minijinja::Error> {
            let glob = Glob::new(&pattern).map_err(|e| {
                minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, e.to_string())
            })?;
            let matcher = glob.compile_matcher();
            let mut set: HashSet<String> = HashSet::new();
            if let Some(dir) = &glob_dir {
                for name in collect_files(dir) {
                    if matcher.is_match(&name) {
                        set.insert(name);
                    }
                }
            }
            for name in Assets::iter().map(|f| f.as_ref().to_string()) {
                if matcher.is_match(&name) {
                    set.insert(name);
                }
            }
            let mut matches: Vec<String> = set.into_iter().collect();
            matches.sort();
            Ok(matches)
        },
    );
    env.add_function("tool_enabled", move |t: String| {
        Ok(enabled_tools.contains(&t))
    });

    let role_content = role
        .and_then(|r| {
            if let Ok(tmpl) = env.get_template(&format!("roles/{}", r)) {
                tmpl.render(()).ok()
            } else {
                None
            }
        })
        .unwrap_or_default();
    let role_clone = role_content.clone();
    env.add_function("role", move || Ok(role_clone.clone()));

    if let Ok(tmpl) = env.get_template(&format!("system/{}", name)) {
        if let Ok(rendered) = tmpl.render(()) {
            return Some(rendered);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::load_prompt;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn load_md_prompt() {
        let content = load_prompt("hello", None, Vec::new(), None).unwrap();
        assert!(content.contains("You are a helpful assistant."));
    }

    #[test]
    fn load_md_with_include() {
        let content = load_prompt("outer", None, Vec::new(), None).unwrap();
        assert!(content.contains("Outer."));
        assert!(content.contains("Inner."));
        assert!(content.contains("Deep."));
    }

    #[test]
    fn load_md_with_glob() {
        let content = load_prompt("glob", None, Vec::new(), None).unwrap();
        assert!(content.contains("You are a helpful assistant."));
    }

    #[test]
    fn glob_merges_override_and_assets() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("system")).unwrap();
        fs::write(dir.path().join("system/custom.md"), "Custom").unwrap();

        let content = load_prompt("glob", None, Vec::new(), Some(dir.path())).unwrap();
        assert!(content.contains("Custom"));
        assert!(content.contains("You are a helpful assistant."));
    }

    #[test]
    fn tool_enabled_fn() {
        let content = load_prompt("tool", None, vec!["shell.run".to_string()], None).unwrap();
        assert!(content.contains("Enabled!"));
        let content = load_prompt("tool", None, Vec::new(), None).unwrap();
        assert!(content.contains("Disabled!"));
    }

    #[test]
    fn load_prompt_from_dir_overrides_assets() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("system")).unwrap();
        fs::write(dir.path().join("system/hello.md"), "Override").unwrap();

        let content = load_prompt("hello", None, Vec::new(), Some(dir.path())).unwrap();
        assert_eq!(content, "Override");
    }

    #[test]
    fn load_include_from_dir_overrides_assets() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("system")).unwrap();
        fs::write(
            dir.path().join("system/inner.md"),
            "Override Inner. {% include \"system/deep\" %}",
        )
        .unwrap();

        let content = load_prompt("outer", None, Vec::new(), Some(dir.path())).unwrap();
        assert!(content.contains("Outer."));
        assert!(content.contains("Override Inner."));
        assert!(content.contains("Deep."));
    }

    #[test]
    fn load_role_from_dir() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("system")).unwrap();
        fs::write(dir.path().join("system/role.md"), "Role: {{ role() }}").unwrap();
        fs::create_dir_all(dir.path().join("roles")).unwrap();
        fs::write(dir.path().join("roles/custom.md"), "custom role").unwrap();

        let content = load_prompt("role", Some("custom"), Vec::new(), Some(dir.path())).unwrap();
        assert_eq!(content, "Role: custom role");
    }
}
