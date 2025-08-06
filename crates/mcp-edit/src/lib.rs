use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use globset::{Glob, GlobBuilder, GlobSetBuilder};
use ignore::WalkBuilder;
use regex::Regex;
use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::tool::{Parameters, ToolRouter},
    model::{CallToolResult, Content},
    tool, tool_handler, tool_router,
};
use std::{
    cmp::Ordering,
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

mod replace_in_content;
use replace_in_content::replace_in_content;

use rmcp::{schemars::JsonSchema, serde::Deserialize};

#[derive(Deserialize, JsonSchema)]
struct ReplaceParams {
    file_path: String,
    old_string: String,
    new_string: String,
    expected_replacements: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct ListDirectoryParams {
    path: String,
    ignore: Option<Vec<String>>,
    _respect_git_ignore: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
struct ReadFileParams {
    path: String,
    offset: Option<usize>,
    limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct WriteFileParams {
    file_path: String,
    content: String,
}

#[derive(Deserialize, JsonSchema)]
struct GlobParams {
    pattern: String,
    path: Option<String>,
    case_sensitive: Option<bool>,
    respect_git_ignore: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
struct SearchFileContentParams {
    pattern: String,
    path: Option<String>,
    include: Option<String>,
}

#[derive(Clone)]
pub struct FsServer {
    tool_router: ToolRouter<Self>,
    workspace_root: PathBuf,
}

impl FsServer {
    fn resolve(&self, path: &str) -> Result<PathBuf, String> {
        if !Path::new(path).is_absolute() {
            return Err("path must be an absolute path".to_string());
        }
        let canonical =
            fs::canonicalize(path).map_err(|e| format!("failed to canonicalize path: {e}"))?;
        if !canonical.starts_with(&self.workspace_root) {
            return Err("path must be within the workspace".to_string());
        }
        Ok(canonical)
    }

    fn resolve_for_write(&self, path: &str) -> Result<PathBuf, String> {
        let p = Path::new(path);
        if !p.is_absolute() {
            return Err("path must be an absolute path".to_string());
        }
        let parent = p
            .parent()
            .ok_or_else(|| "file_path must have a parent directory".to_string())?;
        let canonical_parent =
            fs::canonicalize(parent).map_err(|e| format!("failed to canonicalize path: {e}"))?;
        if !canonical_parent.starts_with(&self.workspace_root) {
            return Err("path must be within the workspace".to_string());
        }
        Ok(canonical_parent.join(p.file_name().unwrap()))
    }
}

fn text_result(msg: impl Into<String>) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![Content::text(msg.into())]))
}

#[tool_router]
impl FsServer {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        let workspace_root = fs::canonicalize(workspace_root.into())
            .expect("workspace path must exist and be canonicalizable");
        Self {
            tool_router: Self::tool_router(),
            workspace_root,
        }
    }

    #[tool(
        description = "Replace text in a file at an absolute path. By default replaces one occurrence of `old_string`; set `expected_replacements` to require a specific number of matches."
    )]
    pub async fn replace(
        &self,
        Parameters(params): Parameters<ReplaceParams>,
    ) -> Result<CallToolResult, McpError> {
        let ReplaceParams {
            file_path,
            old_string,
            new_string,
            expected_replacements,
        } = params;
        let canonical_path = match self.resolve(&file_path) {
            Ok(p) => p,
            Err(e) => return text_result(e),
        };
        let content = match fs::read_to_string(&canonical_path) {
            Ok(c) => c,
            Err(e) => return text_result(format!("failed to read file: {e}")),
        };
        let updated =
            match replace_in_content(&content, &old_string, &new_string, expected_replacements) {
                Ok(u) => u,
                Err(e) => return text_result(e.to_string()),
            };
        if let Err(e) = fs::write(&canonical_path, updated) {
            return text_result(format!("failed to write file: {e}"));
        }
        Ok(CallToolResult::success(vec![Content::text(
            "Replaced text in file.".to_string(),
        )]))
    }

    #[tool(description = "List the contents of a directory at an absolute path.")]
    pub async fn list_directory(
        &self,
        Parameters(params): Parameters<ListDirectoryParams>,
    ) -> Result<CallToolResult, McpError> {
        let ListDirectoryParams { path, ignore, .. } = params;
        let canonical_path = match self.resolve(&path) {
            Ok(p) => p,
            Err(e) => return text_result(e),
        };
        let mut builder = GlobSetBuilder::new();
        if let Some(patterns) = ignore {
            for pat in patterns {
                if let Ok(glob) = Glob::new(&pat) {
                    builder.add(glob);
                }
            }
        }
        let ignore_set = builder
            .build()
            .unwrap_or_else(|_| GlobSetBuilder::new().build().unwrap());
        let mut entries = Vec::new();
        let read_dir = match fs::read_dir(&canonical_path) {
            Ok(rd) => rd,
            Err(e) => return text_result(format!("failed to read dir: {e}")),
        };
        for entry in read_dir {
            let entry = match entry {
                Ok(en) => en,
                Err(e) => return text_result(format!("dir entry error: {e}")),
            };
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if ignore_set.is_match(name_str.as_ref()) {
                continue;
            }
            let is_dir = match entry.file_type() {
                Ok(ft) => ft.is_dir(),
                Err(e) => return text_result(format!("failed to get file type: {e}")),
            };
            entries.push((is_dir, name_str.to_string()));
        }
        entries.sort_by(|a, b| match (a.0, b.0) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => a.1.cmp(&b.1),
        });
        let listing = entries
            .into_iter()
            .map(|(is_dir, name)| {
                if is_dir {
                    format!("[DIR] {name}")
                } else {
                    name
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let output = format!(
            "Directory listing for {}:\n{}",
            canonical_path.display(),
            listing
        );
        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    #[tool(description = "Read a file at an absolute path.")]
    pub async fn read_file(
        &self,
        Parameters(params): Parameters<ReadFileParams>,
    ) -> Result<CallToolResult, McpError> {
        let ReadFileParams {
            path,
            offset,
            limit,
        } = params;
        let canonical_path = match self.resolve(&path) {
            Ok(p) => p,
            Err(e) => return text_result(e),
        };
        let data = match fs::read(&canonical_path) {
            Ok(d) => d,
            Err(e) => return text_result(format!("failed to read file: {e}")),
        };
        if let Ok(content) = String::from_utf8(data.clone()) {
            let lines: Vec<&str> = content.lines().collect();
            let start = offset.unwrap_or(0);
            if start >= lines.len() {
                return Ok(CallToolResult::success(vec![Content::text("".to_string())]));
            }
            let end = limit.map_or(lines.len(), |l| (start + l).min(lines.len()));
            let slice = lines[start..end].join("\n");
            let truncated = end < lines.len();
            let result = if truncated {
                format!(
                    "[File content truncated: showing lines {}-{} of {} total lines...]\n{}",
                    start + 1,
                    end,
                    lines.len(),
                    slice
                )
            } else {
                slice
            };
            Ok(CallToolResult::success(vec![Content::text(result)]))
        } else {
            let ext = canonical_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            let mime = match ext.as_str() {
                "png" => Some("image/png"),
                "jpg" | "jpeg" => Some("image/jpeg"),
                "gif" => Some("image/gif"),
                "webp" => Some("image/webp"),
                "svg" => Some("image/svg+xml"),
                "bmp" => Some("image/bmp"),
                "pdf" => Some("application/pdf"),
                _ => None,
            };
            if let Some(mime) = mime {
                let encoded = BASE64.encode(data);
                Ok(CallToolResult::success(vec![Content::image(
                    encoded,
                    mime.to_string(),
                )]))
            } else {
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Cannot display content of binary file: {}",
                    canonical_path.display()
                ))]))
            }
        }
    }

    #[tool(description = "Write content to a file at an absolute path, creating it if necessary.")]
    pub async fn write_file(
        &self,
        Parameters(params): Parameters<WriteFileParams>,
    ) -> Result<CallToolResult, McpError> {
        let WriteFileParams { file_path, content } = params;
        let canonical_path = match self.resolve_for_write(&file_path) {
            Ok(p) => p,
            Err(e) => return text_result(e),
        };
        if let Some(parent) = canonical_path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                return text_result(format!("failed to create parent dirs: {e}"));
            }
        }
        if let Err(e) = fs::write(&canonical_path, content) {
            return text_result(format!("failed to write file: {e}"));
        }
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Wrote file: {}",
            canonical_path.display()
        ))]))
    }

    #[tool(description = "Find files matching a glob pattern.")]
    pub async fn glob(
        &self,
        Parameters(params): Parameters<GlobParams>,
    ) -> Result<CallToolResult, McpError> {
        let GlobParams {
            pattern,
            path,
            case_sensitive,
            respect_git_ignore,
        } = params;
        let root = if let Some(p) = path {
            match self.resolve(&p) {
                Ok(r) => r,
                Err(e) => return text_result(e),
            }
        } else {
            self.workspace_root.clone()
        };
        let mut builder = WalkBuilder::new(&root);
        builder.git_ignore(respect_git_ignore.unwrap_or(true));
        builder.standard_filters(true);
        let glob = match GlobBuilder::new(&pattern)
            .case_insensitive(!case_sensitive.unwrap_or(false))
            .build()
        {
            Ok(g) => g,
            Err(e) => return text_result(format!("invalid glob pattern: {e}")),
        }
        .compile_matcher();
        let mut matches = Vec::new();
        for result in builder.build() {
            let entry = match result {
                Ok(e) => e,
                Err(e) => return text_result(format!("walk error: {e}")),
            };
            if !entry.file_type().map_or(false, |ft| ft.is_file()) {
                continue;
            }
            let rel = entry.path().strip_prefix(&root).unwrap_or(entry.path());
            if glob.is_match(rel) {
                matches.push(entry.path().to_path_buf());
            }
        }
        matches.sort_by_key(|p| {
            fs::metadata(p)
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH)
        });
        matches.reverse();
        let paths = matches
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let output = format!(
            "Found {} file(s) matching \"{}\" within {}:\n{}",
            matches.len(),
            pattern,
            root.display(),
            paths
        );
        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    #[tool(description = "Search for a regex pattern in files within a directory.")]
    pub async fn search_file_content(
        &self,
        Parameters(params): Parameters<SearchFileContentParams>,
    ) -> Result<CallToolResult, McpError> {
        let SearchFileContentParams {
            pattern,
            path,
            include,
        } = params;
        let root = if let Some(p) = path {
            match self.resolve(&p) {
                Ok(r) => r,
                Err(e) => return text_result(e),
            }
        } else {
            self.workspace_root.clone()
        };
        let regex = match Regex::new(&pattern) {
            Ok(r) => r,
            Err(e) => return text_result(format!("invalid regex: {e}")),
        };
        let include_matcher = if let Some(ref inc) = include {
            Some(
                match Glob::new(inc) {
                    Ok(g) => g,
                    Err(e) => return text_result(format!("invalid include glob: {e}")),
                }
                .compile_matcher(),
            )
        } else {
            None
        };
        let mut builder = WalkBuilder::new(&root);
        builder.git_ignore(true);
        builder.standard_filters(true);
        let mut results = Vec::new();
        for result in builder.build() {
            let entry = match result {
                Ok(e) => e,
                Err(e) => return text_result(format!("walk error: {e}")),
            };
            if !entry.file_type().map_or(false, |ft| ft.is_file()) {
                continue;
            }
            let rel = entry.path().strip_prefix(&root).unwrap_or(entry.path());
            if let Some(matcher) = &include_matcher {
                if !matcher.is_match(rel) {
                    continue;
                }
            }
            let content = match fs::read_to_string(entry.path()) {
                Ok(c) => c,
                Err(_) => continue,
            };
            for (idx, line) in content.lines().enumerate() {
                if regex.is_match(line) {
                    results.push(format!("File: {}\nL{}: {}", rel.display(), idx + 1, line));
                }
            }
        }
        let mut output = format!(
            "Found {} match(es) for pattern \"{}\" in path \"{}\"{}:",
            results.len(),
            pattern,
            root.display(),
            include
                .as_ref()
                .map(|s| format!(" (filter: \"{}\")", s))
                .unwrap_or_default()
        );
        if !results.is_empty() {
            output.push_str("\n---\n");
            output.push_str(&results.join("\n---\n"));
        }
        Ok(CallToolResult::success(vec![Content::text(output)]))
    }
}

#[tool_handler]
impl ServerHandler for FsServer {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::{NamedTempFile, tempdir};

    #[tokio::test]
    async fn replace_single_occurrence() {
        let dir = tempdir().unwrap();
        let mut file = NamedTempFile::new_in(dir.path()).unwrap();
        write!(file, "hello world").unwrap();
        let path = file.path().to_path_buf();
        let server = FsServer::new(dir.path());
        server
            .replace(Parameters(ReplaceParams {
                file_path: path.to_string_lossy().to_string(),
                old_string: "world".into(),
                new_string: "there".into(),
                expected_replacements: None,
            }))
            .await
            .unwrap();
        let content = fs::read_to_string(path).unwrap();
        assert_eq!(content, "hello there");
    }

    #[tokio::test]
    async fn list_directory_lists_entries() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("file.txt"), "abc").unwrap();
        let server = FsServer::new(dir.path());
        let result = server
            .list_directory(Parameters(ListDirectoryParams {
                path: dir.path().to_string_lossy().to_string(),
                ignore: None,
                _respect_git_ignore: None,
            }))
            .await
            .unwrap();
        let text = result.content.unwrap()[0]
            .raw
            .as_text()
            .unwrap()
            .text
            .clone();
        assert!(text.contains("[DIR] sub"));
        assert!(text.contains("file.txt"));
    }

    #[tokio::test]
    async fn read_file_reads_content() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("a.txt");
        fs::write(&file_path, "first\nsecond\nthird").unwrap();
        let server = FsServer::new(dir.path());
        let result = server
            .read_file(Parameters(ReadFileParams {
                path: file_path.to_string_lossy().to_string(),
                offset: Some(1),
                limit: Some(1),
            }))
            .await
            .unwrap();
        let text = result.content.unwrap()[0]
            .raw
            .as_text()
            .unwrap()
            .text
            .clone();
        assert!(text.contains("second"));
    }

    #[tokio::test]
    async fn read_file_missing_returns_message() {
        let dir = tempdir().unwrap();
        let subdir = dir.path().join("missing");
        fs::create_dir(&subdir).unwrap();
        let server = FsServer::new(dir.path());
        let result = server
            .read_file(Parameters(ReadFileParams {
                path: subdir.to_string_lossy().to_string(),
                offset: None,
                limit: None,
            }))
            .await
            .unwrap();
        let text = result.content.unwrap()[0]
            .raw
            .as_text()
            .unwrap()
            .text
            .clone();
        assert!(text.contains("failed to read file"));
    }

    #[tokio::test]
    async fn read_file_outside_workspace_returns_message() {
        let dir = tempdir().unwrap();
        let outside_file = NamedTempFile::new().unwrap();
        let server = FsServer::new(dir.path());
        let result = server
            .read_file(Parameters(ReadFileParams {
                path: outside_file.path().to_string_lossy().to_string(),
                offset: None,
                limit: None,
            }))
            .await
            .unwrap();
        let text = result.content.unwrap()[0]
            .raw
            .as_text()
            .unwrap()
            .text
            .clone();
        assert!(text.contains("within the workspace"));
    }

    #[tokio::test]
    async fn write_file_writes_content() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("new.txt");
        let server = FsServer::new(dir.path());
        server
            .write_file(Parameters(WriteFileParams {
                file_path: file_path.to_string_lossy().to_string(),
                content: "hello".into(),
            }))
            .await
            .unwrap();
        let content = fs::read_to_string(file_path).unwrap();
        assert_eq!(content, "hello");
    }

    #[tokio::test]
    async fn glob_finds_files() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.rs"), "").unwrap();
        fs::write(dir.path().join("b.txt"), "").unwrap();
        let server = FsServer::new(dir.path());
        let result = server
            .glob(Parameters(GlobParams {
                pattern: "*.rs".into(),
                path: None,
                case_sensitive: None,
                respect_git_ignore: None,
            }))
            .await
            .unwrap();
        let text = result.content.unwrap()[0]
            .raw
            .as_text()
            .unwrap()
            .text
            .clone();
        assert!(text.contains("a.rs"));
        assert!(!text.contains("b.txt"));
    }

    #[tokio::test]
    async fn search_file_content_finds_matches() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("main.txt"), "foo\nbar").unwrap();
        let server = FsServer::new(dir.path());
        let result = server
            .search_file_content(Parameters(SearchFileContentParams {
                pattern: "bar".into(),
                path: None,
                include: Some("*.txt".into()),
            }))
            .await
            .unwrap();
        let text = result.content.unwrap()[0]
            .raw
            .as_text()
            .unwrap()
            .text
            .clone();
        assert!(text.contains("bar"));
    }
}
