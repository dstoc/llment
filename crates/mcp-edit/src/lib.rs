use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use globset::{Glob, GlobBuilder, GlobSetBuilder};
use grep::{
    regex::RegexMatcher,
    searcher::{Searcher, sinks::UTF8},
};
use ignore::WalkBuilder;
use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::tool::{Parameters, ToolRouter},
    model::{CallToolResult, Content},
    tool, tool_handler, tool_router,
};
use std::{
    cmp::Ordering,
    fs,
    path::{Component, Path, PathBuf},
    time::SystemTime,
};

mod replace_in_content;
use replace_in_content::replace_in_content;

use rmcp::{schemars::JsonSchema, serde::Deserialize};

fn default_expected_replacements() -> Option<usize> {
    Some(1)
}

fn default_offset() -> Option<usize> {
    Some(0)
}

fn default_true() -> Option<bool> {
    Some(true)
}

fn default_false() -> Option<bool> {
    Some(false)
}

#[derive(Deserialize, JsonSchema)]
struct ReplaceParams {
    /// Absolute path to the file to modify.
    file_path: String,
    /// Text to search for in the file.
    old_string: String,
    /// Replacement text.
    new_string: String,
    /// Optional. Number of replacements required. Defaults to 1.
    #[serde(default = "default_expected_replacements")]
    #[schemars(default = "default_expected_replacements")]
    expected_replacements: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct ListDirectoryParams {
    /// Directory path to list.
    path: String,
    /// Optional. Glob patterns to ignore.
    #[serde(default)]
    #[schemars(default)]
    ignore: Option<Vec<String>>,
    /// Optional. Whether to respect `.gitignore` files. Defaults to true.
    #[serde(default = "default_true")]
    #[schemars(default = "default_true")]
    _respect_git_ignore: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
struct ReadFileParams {
    /// Absolute path to the file to read.
    path: String,
    /// Optional. Line offset to start reading from. Defaults to 0.
    #[serde(default = "default_offset")]
    #[schemars(default = "default_offset")]
    offset: Option<usize>,
    /// Optional. Maximum number of lines to read. Reads to end of file when omitted.
    #[serde(default)]
    #[schemars(default)]
    limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct ReadManyFilesParams {
    /// Glob patterns of absolute paths to read.
    paths: Vec<String>,
    /// Optional. Additional include glob patterns.
    #[serde(default)]
    #[schemars(default)]
    include: Option<Vec<String>>,
    /// Optional. Additional exclude glob patterns.
    #[serde(default)]
    #[schemars(default)]
    exclude: Option<Vec<String>>,
    /// Optional. Recurse into directories. Defaults to true.
    #[serde(default = "default_true")]
    #[schemars(default = "default_true")]
    recursive: Option<bool>,
    /// Optional. Whether to respect `.gitignore` files. Defaults to true.
    #[serde(default = "default_true")]
    #[schemars(default = "default_true")]
    respect_git_ignore: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
struct WriteFileParams {
    /// Absolute path where the file will be written.
    file_path: String,
    /// Content to write to the file.
    content: String,
}

#[derive(Deserialize, JsonSchema)]
struct GlobParams {
    /// Glob pattern to match files.
    pattern: String,
    /// Optional. Directory to search within. Defaults to the workspace root.
    #[serde(default)]
    #[schemars(default)]
    path: Option<String>,
    /// Optional. Enable case-sensitive matching. Defaults to false.
    #[serde(default = "default_false")]
    #[schemars(default = "default_false")]
    case_sensitive: Option<bool>,
    /// Optional. Whether to respect `.gitignore` files. Defaults to true.
    #[serde(default = "default_true")]
    #[schemars(default = "default_true")]
    respect_git_ignore: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
struct SearchFileContentParams {
    /// Regex pattern to search for.
    pattern: String,
    /// Optional. Directory to search within. Defaults to the workspace root.
    #[serde(default)]
    #[schemars(default)]
    path: Option<String>,
    /// Optional. Glob pattern for files to include.
    #[serde(default)]
    #[schemars(default)]
    include: Option<String>,
}

#[derive(Clone)]
pub struct FsServer {
    tool_router: ToolRouter<Self>,
    workspace_root: PathBuf,
}

impl FsServer {
    fn normalize(path: &Path) -> PathBuf {
        let mut normalized = PathBuf::new();
        for comp in path.components() {
            match comp {
                Component::ParentDir => {
                    normalized.pop();
                }
                Component::CurDir => {}
                other => normalized.push(other.as_os_str()),
            }
        }
        normalized
    }

    fn resolve(&self, path: &str) -> Result<PathBuf, McpError> {
        let p = Path::new(path);
        if !p.is_absolute() {
            return Err(McpError::invalid_params(
                "path must be an absolute path".to_string(),
                None,
            ));
        }
        let normalized = Self::normalize(p);
        if !normalized.starts_with(&self.workspace_root) {
            return Err(McpError::invalid_params(
                "path must be within the workspace".to_string(),
                None,
            ));
        }
        let canonical = fs::canonicalize(&normalized).map_err(|e| {
            McpError::invalid_params(format!("failed to canonicalize path: {e}"), None)
        })?;
        if !canonical.starts_with(&self.workspace_root) {
            return Err(McpError::invalid_params(
                "path must be within the workspace".to_string(),
                None,
            ));
        }
        Ok(canonical)
    }

    fn resolve_for_write(&self, path: &str) -> Result<PathBuf, McpError> {
        let p = Path::new(path);
        if !p.is_absolute() {
            return Err(McpError::invalid_params(
                "path must be an absolute path".to_string(),
                None,
            ));
        }
        let parent = p.parent().ok_or_else(|| {
            McpError::invalid_params("file_path must have a parent directory".to_string(), None)
        })?;
        let normalized_parent = Self::normalize(parent);
        if !normalized_parent.starts_with(&self.workspace_root) {
            return Err(McpError::invalid_params(
                "path must be within the workspace".to_string(),
                None,
            ));
        }
        let canonical_parent = fs::canonicalize(&normalized_parent).map_err(|e| {
            McpError::invalid_params(format!("failed to canonicalize path: {e}"), None)
        })?;
        if !canonical_parent.starts_with(&self.workspace_root) {
            return Err(McpError::invalid_params(
                "path must be within the workspace".to_string(),
                None,
            ));
        }
        Ok(canonical_parent.join(p.file_name().unwrap()))
    }
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
        let canonical_path = self.resolve(&file_path)?;
        let content = fs::read_to_string(&canonical_path)
            .map_err(|e| McpError::internal_error(format!("failed to read file: {e}"), None))?;
        let updated = replace_in_content(&content, &old_string, &new_string, expected_replacements)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        fs::write(&canonical_path, updated)
            .map_err(|e| McpError::internal_error(format!("failed to write file: {e}"), None))?;
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
        let canonical_path = self.resolve(&path)?;
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
        for entry in fs::read_dir(&canonical_path)
            .map_err(|e| McpError::internal_error(format!("failed to read dir: {e}"), None))?
        {
            let entry = entry
                .map_err(|e| McpError::internal_error(format!("dir entry error: {e}"), None))?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if ignore_set.is_match(name_str.as_ref()) {
                continue;
            }
            let is_dir = entry
                .file_type()
                .map_err(|e| {
                    McpError::internal_error(format!("failed to get file type: {e}"), None)
                })?
                .is_dir();
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
        let canonical_path = self.resolve(&path)?;
        let data = fs::read(&canonical_path)
            .map_err(|e| McpError::internal_error(format!("failed to read file: {e}"), None))?;
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

    #[tool(description = "Read multiple files and concatenate their contents.")]
    pub async fn read_many_files(
        &self,
        Parameters(params): Parameters<ReadManyFilesParams>,
    ) -> Result<CallToolResult, McpError> {
        let ReadManyFilesParams {
            paths,
            include,
            exclude,
            recursive,
            respect_git_ignore,
        } = params;
        if paths.is_empty() {
            return Err(McpError::invalid_params(
                "paths must not be empty".to_string(),
                None,
            ));
        }

        let include_set =
            if let Some(pats) = include {
                let mut builder = GlobSetBuilder::new();
                for p in pats {
                    builder.add(Glob::new(&p).map_err(|e| {
                        McpError::invalid_params(format!("invalid include glob: {e}"), None)
                    })?);
                }
                Some(builder.build().map_err(|e| {
                    McpError::internal_error(format!("include build error: {e}"), None)
                })?)
            } else {
                None
            };

        let mut exclude_patterns = exclude.unwrap_or_default();
        exclude_patterns.extend([
            "**/node_modules/**".into(),
            "**/.git/**".into(),
            "**/target/**".into(),
        ]);
        let exclude_set =
            if exclude_patterns.is_empty() {
                None
            } else {
                let mut builder = GlobSetBuilder::new();
                for p in exclude_patterns {
                    builder.add(Glob::new(&p).map_err(|e| {
                        McpError::invalid_params(format!("invalid exclude glob: {e}"), None)
                    })?);
                }
                Some(builder.build().map_err(|e| {
                    McpError::internal_error(format!("exclude build error: {e}"), None)
                })?)
            };

        let mut file_paths = Vec::new();
        for pattern in paths {
            if !Path::new(&pattern).is_absolute() {
                return Err(McpError::invalid_params(
                    "paths must be absolute".to_string(),
                    None,
                ));
            }
            let glob_iter = glob::glob(&pattern).map_err(|e| {
                McpError::invalid_params(format!("invalid glob pattern: {e}"), None)
            })?;
            for entry in glob_iter {
                let path = entry
                    .map_err(|e| McpError::internal_error(format!("glob error: {e}"), None))?;
                let canonical = fs::canonicalize(&path).map_err(|e| {
                    McpError::internal_error(format!("failed to canonicalize path: {e}"), None)
                })?;
                if !canonical.starts_with(&self.workspace_root) {
                    return Err(McpError::invalid_params(
                        "path must be within the workspace".to_string(),
                        None,
                    ));
                }
                if canonical.is_file() {
                    file_paths.push(canonical);
                } else if canonical.is_dir() {
                    let mut builder = WalkBuilder::new(&canonical);
                    builder.standard_filters(true);
                    builder.git_ignore(respect_git_ignore.unwrap_or(true));
                    if !recursive.unwrap_or(true) {
                        builder.max_depth(Some(1));
                    }
                    for result in builder.build() {
                        let entry = result.map_err(|e| {
                            McpError::internal_error(format!("walk error: {e}"), None)
                        })?;
                        if !entry.file_type().map_or(false, |ft| ft.is_file()) {
                            continue;
                        }
                        let canon = fs::canonicalize(entry.path()).map_err(|e| {
                            McpError::internal_error(
                                format!("failed to canonicalize path: {e}"),
                                None,
                            )
                        })?;
                        file_paths.push(canon);
                    }
                }
            }
        }

        file_paths.sort();
        file_paths.dedup();

        let mut text_output = String::new();
        let mut contents = Vec::new();
        for file in file_paths {
            let rel = file.strip_prefix(&self.workspace_root).unwrap_or(&file);
            if let Some(ref inc) = include_set {
                if !inc.is_match(rel) {
                    continue;
                }
            }
            if let Some(ref exc) = exclude_set {
                if exc.is_match(rel) {
                    continue;
                }
            }
            let data = fs::read(&file)
                .map_err(|e| McpError::internal_error(format!("failed to read file: {e}"), None))?;
            if let Ok(content) = String::from_utf8(data.clone()) {
                text_output.push_str(&format!("===== {} =====\n{}\n\n", rel.display(), content));
            } else {
                let ext = file
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
                    contents.push(Content::image(encoded, mime.to_string()));
                } else {
                    text_output
                        .push_str(&format!("===== {} =====\n[binary file]\n\n", rel.display()));
                }
            }
        }

        if !text_output.is_empty() {
            contents.insert(0, Content::text(text_output));
        }
        Ok(CallToolResult::success(contents))
    }

    #[tool(description = "Write content to a file at an absolute path, creating it if necessary.")]
    pub async fn write_file(
        &self,
        Parameters(params): Parameters<WriteFileParams>,
    ) -> Result<CallToolResult, McpError> {
        let WriteFileParams { file_path, content } = params;
        let canonical_path = self.resolve_for_write(&file_path)?;
        if let Some(parent) = canonical_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                McpError::internal_error(format!("failed to create parent dirs: {e}"), None)
            })?;
        }
        fs::write(&canonical_path, content)
            .map_err(|e| McpError::internal_error(format!("failed to write file: {e}"), None))?;
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
            self.resolve(&p)?
        } else {
            self.workspace_root.clone()
        };
        let mut builder = WalkBuilder::new(&root);
        builder.git_ignore(respect_git_ignore.unwrap_or(true));
        builder.standard_filters(true);
        let glob = GlobBuilder::new(&pattern)
            .case_insensitive(!case_sensitive.unwrap_or(false))
            .build()
            .map_err(|e| McpError::invalid_params(format!("invalid glob pattern: {e}"), None))?
            .compile_matcher();
        let mut matches = Vec::new();
        for result in builder.build() {
            let entry =
                result.map_err(|e| McpError::internal_error(format!("walk error: {e}"), None))?;
            if !entry.file_type().map_or(false, |ft| ft.is_file()) {
                continue;
            }
            let canonical = match fs::canonicalize(entry.path()) {
                Ok(p) => p,
                Err(_) => continue,
            };
            if !canonical.starts_with(&self.workspace_root) {
                continue;
            }
            let rel = canonical.strip_prefix(&root).unwrap_or(&canonical);
            if glob.is_match(rel) {
                matches.push(canonical);
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
            self.resolve(&p)?
        } else {
            self.workspace_root.clone()
        };
        let matcher = RegexMatcher::new(&pattern)
            .map_err(|e| McpError::invalid_params(format!("invalid regex: {e}"), None))?;
        let include_matcher = if let Some(ref inc) = include {
            Some(
                Glob::new(inc)
                    .map_err(|e| {
                        McpError::invalid_params(format!("invalid include glob: {e}"), None)
                    })?
                    .compile_matcher(),
            )
        } else {
            None
        };
        let mut builder = WalkBuilder::new(&root);
        builder.git_ignore(true);
        builder.standard_filters(true);
        let mut results = Vec::new();
        let mut searcher = Searcher::new();
        for result in builder.build() {
            let entry =
                result.map_err(|e| McpError::internal_error(format!("walk error: {e}"), None))?;
            if !entry.file_type().map_or(false, |ft| ft.is_file()) {
                continue;
            }
            let canonical = match fs::canonicalize(entry.path()) {
                Ok(p) => p,
                Err(_) => continue,
            };
            if !canonical.starts_with(&self.workspace_root) {
                continue;
            }
            let rel = canonical.strip_prefix(&root).unwrap_or(&canonical);
            if let Some(matcher) = &include_matcher {
                if !matcher.is_match(rel) {
                    continue;
                }
            }
            let rel_display = rel.display().to_string();
            if let Err(err) = searcher.search_path(
                &matcher,
                &canonical,
                UTF8(|ln, line| {
                    results.push(format!("File: {}\nL{}: {}", rel_display, ln, line));
                    Ok(true)
                }),
            ) {
                return Err(McpError::internal_error(
                    format!("search error: {err}"),
                    None,
                ));
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
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
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
    async fn read_many_files_reads_multiple() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "hello").unwrap();
        fs::write(dir.path().join("b.txt"), "world").unwrap();
        let server = FsServer::new(dir.path());
        let pattern = format!("{}/**/*.txt", dir.path().display());
        let result = server
            .read_many_files(Parameters(ReadManyFilesParams {
                paths: vec![pattern],
                include: None,
                exclude: None,
                recursive: Some(true),
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
        assert!(text.contains("hello"));
        assert!(text.contains("world"));
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

    #[cfg(unix)]
    #[tokio::test]
    async fn glob_ignores_files_outside_workspace() {
        let workspace = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join("a.rs");
        fs::write(&outside_file, "").unwrap();
        let link_path = workspace.path().join("link.rs");
        symlink(&outside_file, &link_path).unwrap();
        let server = FsServer::new(workspace.path());
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
        assert!(!text.contains("link.rs"));
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

    #[cfg(unix)]
    #[tokio::test]
    async fn search_file_content_ignores_files_outside_workspace() {
        let workspace = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join("a.txt");
        fs::write(&outside_file, "foo").unwrap();
        let link = workspace.path().join("link.txt");
        symlink(&outside_file, &link).unwrap();
        let server = FsServer::new(workspace.path());
        let result = server
            .search_file_content(Parameters(SearchFileContentParams {
                pattern: "foo".into(),
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
        assert!(!text.contains("link.txt"));
    }

    #[tokio::test]
    async fn read_file_outside_workspace_masks_existence() {
        let workspace = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let existing = outside.path().join("exists.txt");
        fs::write(&existing, "hello").unwrap();
        let missing = outside.path().join("missing.txt");
        let server = FsServer::new(workspace.path());
        let err_existing = server
            .read_file(Parameters(ReadFileParams {
                path: existing.to_string_lossy().to_string(),
                offset: None,
                limit: None,
            }))
            .await
            .unwrap_err();
        let err_missing = server
            .read_file(Parameters(ReadFileParams {
                path: missing.to_string_lossy().to_string(),
                offset: None,
                limit: None,
            }))
            .await
            .unwrap_err();
        assert_eq!(err_existing.message, err_missing.message);
        assert_eq!(err_existing.message, "path must be within the workspace");
    }

    #[tokio::test]
    async fn write_file_outside_workspace_masks_existence() {
        let workspace = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let existing = outside.path().join("exists.txt");
        fs::write(&existing, "old").unwrap();
        let missing = outside.path().join("missing.txt");
        let server = FsServer::new(workspace.path());
        let err_existing = server
            .write_file(Parameters(WriteFileParams {
                file_path: existing.to_string_lossy().to_string(),
                content: "new".into(),
            }))
            .await
            .unwrap_err();
        let err_missing = server
            .write_file(Parameters(WriteFileParams {
                file_path: missing.to_string_lossy().to_string(),
                content: "new".into(),
            }))
            .await
            .unwrap_err();
        assert_eq!(err_existing.message, err_missing.message);
        assert_eq!(err_existing.message, "path must be within the workspace");
    }
}
