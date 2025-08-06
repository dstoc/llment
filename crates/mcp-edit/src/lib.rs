use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::tool::ToolRouter,
    model::{CallToolResult, Content},
    tool, tool_handler, tool_router,
};
use std::fs;
use std::path::{Path, PathBuf};

mod replace_in_content;
use replace_in_content::replace_in_content;

use rmcp::{handler::server::tool::Parameters, schemars::JsonSchema, serde::Deserialize};

#[derive(Deserialize, JsonSchema)]
struct EditParams {
    file_path: String,
    old_string: String,
    new_string: String,
    expected_replacements: Option<usize>,
}

#[derive(Clone)]
pub struct EditServer {
    tool_router: ToolRouter<Self>,
    workspace_root: PathBuf,
}

#[tool_router]
impl EditServer {
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
    pub async fn edit_file(
        &self,
        Parameters(params): Parameters<EditParams>,
    ) -> Result<CallToolResult, McpError> {
        let EditParams {
            file_path,
            old_string,
            new_string,
            expected_replacements,
        } = params;

        if !Path::new(&file_path).is_absolute() {
            return Err(McpError::invalid_params(
                "file_path must be an absolute path".to_string(),
                None,
            ));
        }

        let canonical_path = fs::canonicalize(&file_path).map_err(|e| {
            McpError::invalid_params(format!("failed to canonicalize file_path: {e}"), None)
        })?;

        if !canonical_path.starts_with(&self.workspace_root) {
            return Err(McpError::invalid_params(
                "file_path must be within the workspace".to_string(),
                None,
            ));
        }

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
}

#[tool_handler]
impl ServerHandler for EditServer {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::{NamedTempFile, tempdir};

    #[tokio::test]
    async fn replaces_single_occurrence() {
        let dir = tempdir().unwrap();
        let mut file = NamedTempFile::new_in(dir.path()).unwrap();
        write!(file, "hello world").unwrap();
        let path = file.path().to_path_buf();
        let server = EditServer::new(dir.path());
        server
            .edit_file(Parameters(EditParams {
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
    async fn fails_for_relative_path() {
        let dir = tempdir().unwrap();
        let server = EditServer::new(dir.path());
        assert!(
            server
                .edit_file(Parameters(EditParams {
                    file_path: "relative.txt".into(),
                    old_string: "a".into(),
                    new_string: "b".into(),
                    expected_replacements: None,
                }))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn replaces_multiple_occurrences() {
        let dir = tempdir().unwrap();
        let mut file = NamedTempFile::new_in(dir.path()).unwrap();
        write!(file, "a a a").unwrap();
        let path = file.path().to_path_buf();
        let server = EditServer::new(dir.path());
        server
            .edit_file(Parameters(EditParams {
                file_path: path.to_string_lossy().to_string(),
                old_string: "a".into(),
                new_string: "b".into(),
                expected_replacements: Some(3),
            }))
            .await
            .unwrap();
        let content = fs::read_to_string(path).unwrap();
        assert_eq!(content, "b b b");
    }

    #[tokio::test]
    async fn fails_on_unexpected_count() {
        let dir = tempdir().unwrap();
        let mut file = NamedTempFile::new_in(dir.path()).unwrap();
        write!(file, "x x").unwrap();
        let path = file.path().to_path_buf();
        let server = EditServer::new(dir.path());
        assert!(
            server
                .edit_file(Parameters(EditParams {
                    file_path: path.to_string_lossy().to_string(),
                    old_string: "x".into(),
                    new_string: "y".into(),
                    expected_replacements: Some(3),
                }))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn fails_for_path_outside_workspace() {
        let workspace = tempdir().unwrap();
        let server = EditServer::new(workspace.path());
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "hello").unwrap();
        assert!(
            server
                .edit_file(Parameters(EditParams {
                    file_path: file.path().to_string_lossy().to_string(),
                    old_string: "hello".into(),
                    new_string: "world".into(),
                    expected_replacements: None,
                }))
                .await
                .is_err()
        );
    }
}
