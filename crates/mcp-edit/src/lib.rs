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
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
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
    /// Path to the file to modify.
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
    // Deprecated: `.gitignore` files are always respected.
}

#[derive(Deserialize, JsonSchema)]
struct ReadFileParams {
    /// Path to the file to read.
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
    /// Glob patterns of file paths to read.
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
}

#[derive(Deserialize, JsonSchema)]
struct CreateFileParams {
    /// Path where the file will be created.
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
    mount_point: PathBuf,
    modification_disabled: bool,
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

    fn resolve(&self, path: &str) -> Result<PathBuf, String> {
        let p = Path::new(path);
        let joined = if p.is_absolute() {
            match p.strip_prefix(&self.mount_point) {
                Ok(rel) => self.workspace_root.join(rel),
                Err(_) => p.to_path_buf(),
            }
        } else {
            self.workspace_root.join(p)
        };
        let normalized = Self::normalize(&joined);
        if !normalized.starts_with(&self.workspace_root) {
            return Err("path must be within the workspace".to_string());
        }
        let canonical = fs::canonicalize(&normalized)
            .map_err(|_| format!("path '{}' does not exist", self.display_path(&normalized)))?;
        if !canonical.starts_with(&self.workspace_root) {
            return Err("path must be within the workspace".to_string());
        }
        Ok(canonical)
    }

    fn resolve_for_write(&self, path: &str) -> Result<PathBuf, String> {
        let p = Path::new(path);
        let canonical_parent = match p.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => {
                let parent_str = parent
                    .to_str()
                    .ok_or_else(|| "parent path must be valid UTF-8".to_string())?;
                self.resolve(parent_str)?
            }
            _ => self.workspace_root.clone(),
        };
        let file_name = p
            .file_name()
            .ok_or_else(|| "file_path must have a file name".to_string())?;
        Ok(canonical_parent.join(file_name))
    }

    fn display_path(&self, path: &Path) -> String {
        if let Ok(rel) = path.strip_prefix(&self.workspace_root) {
            self.mount_point.join(rel).display().to_string()
        } else {
            path.display().to_string()
        }
    }

    fn tool_error(msg: impl Into<String>) -> CallToolResult {
        CallToolResult::error(vec![Content::text(msg.into())])
    }
}

#[tool_router]
impl FsServer {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self::new_with_mount_point(workspace_root, "/home/user/workspace")
    }

    pub fn new_with_mount_point(
        workspace_root: impl Into<PathBuf>,
        mount_point: impl Into<PathBuf>,
    ) -> Self {
        let workspace_root = fs::canonicalize(workspace_root.into())
            .expect("workspace path must exist and be canonicalizable");
        Self {
            tool_router: Self::tool_router(),
            workspace_root,
            mount_point: mount_point.into(),
            modification_disabled: false,
        }
    }

    pub fn disable_modification_tools(&mut self) {
        self.modification_disabled = true;
        self.tool_router.remove_route::<(), ()>("replace");
        self.tool_router.remove_route::<(), ()>("create_file");
    }

    #[tool(
        description = "Replace text in a file. By default replaces one occurrence of `old_string`; set `expected_replacements` to require a specific number of matches."
    )]
    pub async fn replace(
        &self,
        Parameters(params): Parameters<ReplaceParams>,
    ) -> Result<CallToolResult, McpError> {
        assert!(
            !self.modification_disabled,
            "replace called when modification tools disabled"
        );
        let ReplaceParams {
            file_path,
            old_string,
            new_string,
            expected_replacements,
        } = params;
        let canonical_path = match self.resolve(&file_path) {
            Ok(p) => p,
            Err(msg) => return Ok(Self::tool_error(msg)),
        };
        let content = match fs::read_to_string(&canonical_path) {
            Ok(c) => c,
            Err(e) => {
                return Ok(Self::tool_error(format!(
                    "failed to read file {}: {e}",
                    self.display_path(&canonical_path)
                )));
            }
        };
        let updated =
            match replace_in_content(&content, &old_string, &new_string, expected_replacements) {
                Ok(u) => u,
                Err(e) => return Ok(Self::tool_error(e.to_string())),
            };
        if let Err(e) = fs::write(&canonical_path, updated) {
            return Ok(Self::tool_error(format!(
                "failed to write file {}: {e}",
                self.display_path(&canonical_path)
            )));
        }
        Ok(CallToolResult::success(vec![Content::text(
            "Replaced text in file.".to_string(),
        )]))
    }

    #[tool(
        description = "List the contents of a directory.",
        annotations(read_only_hint = true)
    )]
    pub async fn list_directory(
        &self,
        Parameters(params): Parameters<ListDirectoryParams>,
    ) -> Result<CallToolResult, McpError> {
        let ListDirectoryParams { path, ignore } = params;
        let canonical_path = match self.resolve(&path) {
            Ok(p) => p,
            Err(msg) => return Ok(Self::tool_error(msg)),
        };
        if !canonical_path.is_dir() {
            return Ok(Self::tool_error(format!(
                "failed to read dir {}: not a directory",
                self.display_path(&canonical_path)
            )));
        }
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
        let mut walk_builder = WalkBuilder::new(&canonical_path);
        walk_builder.git_ignore(true);
        walk_builder.standard_filters(true);
        walk_builder.max_depth(Some(1));
        for result in walk_builder.build() {
            let entry = match result {
                Ok(e) => e,
                Err(e) => return Ok(Self::tool_error(format!("walk error: {e}"))),
            };
            let path = entry.path();
            if path == canonical_path {
                continue;
            }
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };
            if ignore_set.is_match(name) {
                continue;
            }
            let is_dir = entry.file_type().map_or(false, |ft| ft.is_dir());
            entries.push((is_dir, name.to_string()));
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
            self.display_path(&canonical_path),
            listing
        );
        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    #[tool(description = "Read a file.", annotations(read_only_hint = true))]
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
            Err(msg) => return Ok(Self::tool_error(msg)),
        };
        let data = match fs::read(&canonical_path) {
            Ok(d) => d,
            Err(e) => {
                return Ok(Self::tool_error(format!(
                    "failed to read file {}: {e}",
                    self.display_path(&canonical_path)
                )));
            }
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
                    self.display_path(&canonical_path)
                ))]))
            }
        }
    }

    #[tool(
        description = "Read multiple files and concatenate their contents.",
        annotations(read_only_hint = true)
    )]
    pub async fn read_many_files(
        &self,
        Parameters(params): Parameters<ReadManyFilesParams>,
    ) -> Result<CallToolResult, McpError> {
        let ReadManyFilesParams {
            paths,
            include,
            exclude,
            recursive,
        } = params;
        if paths.is_empty() {
            return Ok(Self::tool_error("paths must not be empty".to_string()));
        }

        let include_set = if let Some(pats) = include {
            let mut builder = GlobSetBuilder::new();
            for p in pats {
                match Glob::new(&p) {
                    Ok(g) => {
                        builder.add(g);
                    }
                    Err(e) => return Ok(Self::tool_error(format!("invalid include glob: {e}"))),
                }
            }
            match builder.build() {
                Ok(set) => Some(set),
                Err(e) => return Ok(Self::tool_error(format!("include build error: {e}"))),
            }
        } else {
            None
        };

        let mut exclude_patterns = exclude.unwrap_or_default();
        exclude_patterns.extend([
            "**/node_modules/**".into(),
            "**/.git/**".into(),
            "**/target/**".into(),
        ]);
        let exclude_set = if exclude_patterns.is_empty() {
            None
        } else {
            let mut builder = GlobSetBuilder::new();
            for p in exclude_patterns {
                match Glob::new(&p) {
                    Ok(g) => {
                        builder.add(g);
                    }
                    Err(e) => return Ok(Self::tool_error(format!("invalid exclude glob: {e}"))),
                }
            }
            match builder.build() {
                Ok(set) => Some(set),
                Err(e) => return Ok(Self::tool_error(format!("exclude build error: {e}"))),
            }
        };

        let mut file_paths = Vec::new();
        for pattern in paths {
            let pattern_path = if Path::new(&pattern).is_absolute() {
                PathBuf::from(&pattern)
            } else {
                self.workspace_root.join(&pattern)
            };
            let glob_iter = match glob::glob(pattern_path.to_string_lossy().as_ref()) {
                Ok(g) => g,
                Err(e) => return Ok(Self::tool_error(format!("invalid glob pattern: {e}"))),
            };
            for entry in glob_iter {
                let path = match entry {
                    Ok(p) => p,
                    Err(e) => return Ok(Self::tool_error(format!("glob error: {e}"))),
                };
                let canonical = match fs::canonicalize(&path) {
                    Ok(c) => c,
                    Err(e) => {
                        return Ok(Self::tool_error(format!(
                            "failed to canonicalize path: {e}"
                        )));
                    }
                };
                if !canonical.starts_with(&self.workspace_root) {
                    return Ok(Self::tool_error(
                        "path must be within the workspace".to_string(),
                    ));
                }
                if canonical.is_file() {
                    file_paths.push(canonical);
                } else if canonical.is_dir() {
                    let mut builder = WalkBuilder::new(&canonical);
                    builder.standard_filters(true);
                    builder.git_ignore(true);
                    if !recursive.unwrap_or(true) {
                        builder.max_depth(Some(1));
                    }
                    for result in builder.build() {
                        let entry = match result {
                            Ok(e) => e,
                            Err(e) => return Ok(Self::tool_error(format!("walk error: {e}"))),
                        };
                        if !entry.file_type().map_or(false, |ft| ft.is_file()) {
                            continue;
                        }
                        let canon = match fs::canonicalize(entry.path()) {
                            Ok(c) => c,
                            Err(e) => {
                                return Ok(Self::tool_error(format!(
                                    "failed to canonicalize path: {e}"
                                )));
                            }
                        };
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
            let user_path = self.display_path(&file);
            let data = match fs::read(&file) {
                Ok(d) => d,
                Err(e) => {
                    return Ok(Self::tool_error(format!(
                        "failed to read file {}: {e}",
                        user_path
                    )));
                }
            };
            if let Ok(content) = String::from_utf8(data.clone()) {
                text_output.push_str(&format!("===== {} =====\n{}\n\n", user_path, content));
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
                    text_output.push_str(&format!("===== {} =====\n[binary file]\n\n", user_path));
                }
            }
        }

        if !text_output.is_empty() {
            contents.insert(0, Content::text(text_output));
        }
        Ok(CallToolResult::success(contents))
    }

    #[tool(description = "Create a new file with the given content.")]
    pub async fn create_file(
        &self,
        Parameters(params): Parameters<CreateFileParams>,
    ) -> Result<CallToolResult, McpError> {
        assert!(
            !self.modification_disabled,
            "create_file called when modification tools disabled"
        );
        let CreateFileParams { file_path, content } = params;
        let canonical_path = match self.resolve_for_write(&file_path) {
            Ok(p) => p,
            Err(msg) => return Ok(Self::tool_error(msg)),
        };
        if canonical_path.exists() {
            return Ok(Self::tool_error(format!(
                "file {} already exists",
                self.display_path(&canonical_path)
            )));
        }
        if let Some(parent) = canonical_path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                return Ok(Self::tool_error(format!(
                    "failed to create parent dirs: {e}"
                )));
            }
        }
        if let Err(e) = fs::write(&canonical_path, content) {
            return Ok(Self::tool_error(format!(
                "failed to create file {}: {e}",
                self.display_path(&canonical_path)
            )));
        }
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Created file: {}",
            self.display_path(&canonical_path)
        ))]))
    }

    #[tool(
        description = "Find files matching a glob pattern.",
        annotations(read_only_hint = true)
    )]
    pub async fn glob(
        &self,
        Parameters(params): Parameters<GlobParams>,
    ) -> Result<CallToolResult, McpError> {
        let GlobParams {
            pattern,
            path,
            case_sensitive,
        } = params;
        let root = if let Some(p) = path {
            match self.resolve(&p) {
                Ok(r) => r,
                Err(msg) => return Ok(Self::tool_error(msg)),
            }
        } else {
            self.workspace_root.clone()
        };
        let mut builder = WalkBuilder::new(&root);
        builder.git_ignore(true);
        builder.standard_filters(true);
        let glob = match GlobBuilder::new(&pattern)
            .case_insensitive(!case_sensitive.unwrap_or(false))
            .build()
        {
            Ok(g) => g,
            Err(e) => return Ok(Self::tool_error(format!("invalid glob pattern: {e}"))),
        }
        .compile_matcher();
        let mut matches = Vec::new();
        for result in builder.build() {
            let entry = match result {
                Ok(e) => e,
                Err(e) => return Ok(Self::tool_error(format!("walk error: {e}"))),
            };
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
            .map(|p| self.display_path(p))
            .collect::<Vec<_>>()
            .join("\n");
        let output = format!(
            "Found {} file(s) matching \"{}\" within {}:\n{}",
            matches.len(),
            pattern,
            self.display_path(&root),
            paths
        );
        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    #[tool(
        description = "Search for a regex pattern in files within a directory.",
        annotations(read_only_hint = true)
    )]
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
                Err(msg) => return Ok(Self::tool_error(msg)),
            }
        } else {
            self.workspace_root.clone()
        };
        let matcher = match RegexMatcher::new(&pattern) {
            Ok(m) => m,
            Err(e) => return Ok(Self::tool_error(format!("invalid regex: {e}"))),
        };
        let include_matcher = if let Some(ref inc) = include {
            Some(
                match Glob::new(inc) {
                    Ok(g) => g,
                    Err(e) => return Ok(Self::tool_error(format!("invalid include glob: {e}"))),
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
        let mut searcher = Searcher::new();
        for result in builder.build() {
            let entry = match result {
                Ok(e) => e,
                Err(e) => return Ok(Self::tool_error(format!("walk error: {e}"))),
            };
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
            let user_path = self.display_path(&canonical);
            if let Err(err) = searcher.search_path(
                &matcher,
                &canonical,
                UTF8(|ln, line| {
                    results.push(format!("File: {}\nL{}: {}", user_path, ln, line));
                    Ok(true)
                }),
            ) {
                return Ok(Self::tool_error(format!("search error: {err}")));
            }
        }
        let mut output = format!(
            "Found {} match(es) for pattern \"{}\" in path \"{}\"{}:",
            results.len(),
            pattern,
            self.display_path(&root),
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
impl ServerHandler for FsServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

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
    async fn list_directory_respects_git_ignore() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(dir.path().join("ignored.txt"), "hi").unwrap();
        fs::write(dir.path().join("visible.txt"), "hi").unwrap();
        let server = FsServer::new(dir.path());
        let result = server
            .list_directory(Parameters(ListDirectoryParams {
                path: dir.path().to_string_lossy().to_string(),
                ignore: None,
            }))
            .await
            .unwrap();
        let text = result.content.unwrap()[0]
            .raw
            .as_text()
            .unwrap()
            .text
            .clone();
        assert!(text.contains("visible.txt"));
        assert!(!text.contains("ignored.txt"));
    }

    #[tokio::test]
    async fn list_directory_mount_path_trailing_slash() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("file.txt"), "abc").unwrap();
        let server = FsServer::new(dir.path());

        let paths = ["/home/user/workspace", "/home/user/workspace/"];
        for p in paths {
            let result = server
                .list_directory(Parameters(ListDirectoryParams {
                    path: p.into(),
                    ignore: None,
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
    async fn read_file_supports_relative_paths() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("a.txt");
        fs::write(&file_path, "hello").unwrap();
        let server = FsServer::new(dir.path());
        let result = server
            .read_file(Parameters(ReadFileParams {
                path: "a.txt".into(),
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
        assert!(text.contains("hello"));
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
    async fn create_file_writes_content() {
        let dir = tempdir().unwrap();
        let server = FsServer::new(dir.path());
        server
            .create_file(Parameters(CreateFileParams {
                file_path: "new.txt".into(),
                content: "hello".into(),
            }))
            .await
            .unwrap();
        let content = fs::read_to_string(dir.path().join("new.txt")).unwrap();
        assert_eq!(content, "hello");
    }

    #[tokio::test]
    async fn create_file_errors_if_exists() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("new.txt");
        fs::write(&file_path, "hi").unwrap();
        let server = FsServer::new(dir.path());
        let err = server
            .create_file(Parameters(CreateFileParams {
                file_path: file_path.to_string_lossy().to_string(),
                content: "bye".into(),
            }))
            .await
            .unwrap();
        assert!(err.is_error.unwrap());
        let msg = err.content.unwrap()[0].as_text().unwrap().text.clone();
        assert!(msg.contains("/home/user/workspace/new.txt"));
        let content = fs::read_to_string(file_path).unwrap();
        assert_eq!(content, "hi");
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
    async fn glob_respects_git_ignore() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join(".gitignore"), "ignored.rs\n").unwrap();
        fs::write(dir.path().join("ignored.rs"), "").unwrap();
        fs::write(dir.path().join("visible.rs"), "").unwrap();
        let server = FsServer::new(dir.path());
        let result = server
            .glob(Parameters(GlobParams {
                pattern: "*.rs".into(),
                path: None,
                case_sensitive: None,
            }))
            .await
            .unwrap();
        let text = result.content.unwrap()[0]
            .raw
            .as_text()
            .unwrap()
            .text
            .clone();
        assert!(text.contains("visible.rs"));
        assert!(!text.contains("ignored.rs"));
    }

    #[tokio::test]
    async fn read_file_not_found_uses_mount_point() {
        let dir = tempdir().unwrap();
        let server = FsServer::new(dir.path());
        let err = server
            .read_file(Parameters(ReadFileParams {
                path: "missing.txt".into(),
                offset: None,
                limit: None,
            }))
            .await
            .unwrap();
        assert!(err.is_error.unwrap());
        let msg = err.content.unwrap()[0].as_text().unwrap().text.clone();
        assert!(msg.contains("/home/user/workspace/missing.txt"));
        assert!(!msg.contains(dir.path().to_string_lossy().as_ref()));
    }

    #[tokio::test]
    async fn create_file_parent_missing_uses_mount_point() {
        let dir = tempdir().unwrap();
        let server = FsServer::new(dir.path());
        let err = server
            .create_file(Parameters(CreateFileParams {
                file_path: "subdir/new.txt".into(),
                content: "hi".into(),
            }))
            .await
            .unwrap();
        assert!(err.is_error.unwrap());
        let msg = err.content.unwrap()[0].as_text().unwrap().text.clone();
        assert!(msg.contains("/home/user/workspace/subdir"));
        assert!(!msg.contains(dir.path().to_string_lossy().as_ref()));
    }

    #[tokio::test]
    async fn list_directory_not_found_uses_mount_point() {
        let dir = tempdir().unwrap();
        let server = FsServer::new(dir.path());
        let err = server
            .list_directory(Parameters(ListDirectoryParams {
                path: "missing".into(),
                ignore: None,
            }))
            .await
            .unwrap();
        assert!(err.is_error.unwrap());
        let msg = err.content.unwrap()[0].as_text().unwrap().text.clone();
        assert!(msg.contains("/home/user/workspace/missing"));
        assert!(!msg.contains(dir.path().to_string_lossy().as_ref()));
    }

    #[tokio::test]
    async fn create_file_path_is_directory() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("dir")).unwrap();
        let server = FsServer::new(dir.path());
        let err = server
            .create_file(Parameters(CreateFileParams {
                file_path: "dir".into(),
                content: "hi".into(),
            }))
            .await
            .unwrap();
        assert!(err.is_error.unwrap());
        let msg = err.content.unwrap()[0].as_text().unwrap().text.clone();
        assert!(msg.contains("/home/user/workspace/dir"));
        assert!(!msg.contains(dir.path().to_string_lossy().as_ref()));
    }

    #[tokio::test]
    async fn list_directory_path_is_file() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("f"), "hi").unwrap();
        let server = FsServer::new(dir.path());
        let err = server
            .list_directory(Parameters(ListDirectoryParams {
                path: "f".into(),
                ignore: None,
            }))
            .await
            .unwrap();
        assert!(err.is_error.unwrap());
        let msg = err.content.unwrap()[0].as_text().unwrap().text.clone();
        assert!(msg.contains("/home/user/workspace/f"));
        assert!(!msg.contains(dir.path().to_string_lossy().as_ref()));
    }

    #[tokio::test]
    async fn list_directory_outside_workspace() {
        let workspace = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let server = FsServer::new(workspace.path());
        let err = server
            .list_directory(Parameters(ListDirectoryParams {
                path: outside.path().to_string_lossy().to_string(),
                ignore: None,
            }))
            .await
            .unwrap();
        assert!(err.is_error.unwrap());
        let msg = err.content.unwrap()[0].as_text().unwrap().text.clone();
        assert_eq!(msg, "path must be within the workspace");
    }

    #[tokio::test]
    async fn read_file_outside_workspace() {
        let workspace = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let server = FsServer::new(workspace.path());
        let err = server
            .read_file(Parameters(ReadFileParams {
                path: outside.path().join("a.txt").to_string_lossy().to_string(),
                offset: None,
                limit: None,
            }))
            .await
            .unwrap();
        assert!(err.is_error.unwrap());
        let msg = err.content.unwrap()[0].as_text().unwrap().text.clone();
        assert_eq!(msg, "path must be within the workspace");
    }

    #[tokio::test]
    async fn create_file_outside_workspace() {
        let workspace = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let server = FsServer::new(workspace.path());
        let err = server
            .create_file(Parameters(CreateFileParams {
                file_path: outside.path().join("a.txt").to_string_lossy().to_string(),
                content: "hi".into(),
            }))
            .await
            .unwrap();
        assert!(err.is_error.unwrap());
        let msg = err.content.unwrap()[0].as_text().unwrap().text.clone();
        assert_eq!(msg, "path must be within the workspace");
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

    #[tokio::test]
    async fn search_file_content_respects_git_ignore() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(dir.path().join("ignored.txt"), "foo").unwrap();
        fs::write(dir.path().join("visible.txt"), "foo").unwrap();
        let server = FsServer::new(dir.path());
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
        assert!(text.contains("visible.txt"));
        assert!(!text.contains("ignored.txt"));
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
            .unwrap();
        let err_missing = server
            .read_file(Parameters(ReadFileParams {
                path: missing.to_string_lossy().to_string(),
                offset: None,
                limit: None,
            }))
            .await
            .unwrap();
        let msg_existing = err_existing.content.unwrap()[0]
            .as_text()
            .unwrap()
            .text
            .clone();
        let msg_missing = err_missing.content.unwrap()[0]
            .as_text()
            .unwrap()
            .text
            .clone();
        assert_eq!(msg_existing, msg_missing);
        assert_eq!(msg_existing, "path must be within the workspace");
    }

    #[tokio::test]
    async fn create_file_outside_workspace_masks_existence() {
        let workspace = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let existing = outside.path().join("exists.txt");
        fs::write(&existing, "old").unwrap();
        let missing = outside.path().join("missing.txt");
        let server = FsServer::new(workspace.path());
        let err_existing = server
            .create_file(Parameters(CreateFileParams {
                file_path: existing.to_string_lossy().to_string(),
                content: "new".into(),
            }))
            .await
            .unwrap();
        let err_missing = server
            .create_file(Parameters(CreateFileParams {
                file_path: missing.to_string_lossy().to_string(),
                content: "new".into(),
            }))
            .await
            .unwrap();
        let msg_existing = err_existing.content.unwrap()[0]
            .as_text()
            .unwrap()
            .text
            .clone();
        let msg_missing = err_missing.content.unwrap()[0]
            .as_text()
            .unwrap()
            .text
            .clone();
        assert_eq!(msg_existing, msg_missing);
        assert_eq!(msg_existing, "path must be within the workspace");
    }

    #[test]
    fn disable_modification_tools_removes_routes() {
        let dir = tempdir().unwrap();
        let mut server = FsServer::new(dir.path());
        assert!(server.tool_router.has_route("replace"));
        assert!(server.tool_router.has_route("create_file"));
        server.disable_modification_tools();
        assert!(!server.tool_router.has_route("replace"));
        assert!(!server.tool_router.has_route("create_file"));
    }

    #[test]
    fn get_info_enables_tools() {
        let dir = tempdir().unwrap();
        let server = FsServer::new(dir.path());
        assert!(
            ServerHandler::get_info(&server)
                .capabilities
                .tools
                .is_some()
        );
    }
}
