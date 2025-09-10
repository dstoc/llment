use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::tool::ToolRouter,
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
};

#[derive(Clone)]
pub struct HelloServer {
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl HelloServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Return a friendly greeting")]
    pub async fn hello(&self) -> Result<CallToolResult, McpError> {
        Ok(CallToolResult::success(vec![Content::text(
            "Hello, world!",
        )]))
    }
}

#[tool_handler]
impl ServerHandler for HelloServer {
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

    #[tokio::test]
    async fn hello_returns_greeting() {
        let server = HelloServer::new();
        let result = server.hello().await.unwrap();
        let content = result.content.unwrap();
        let text = content[0].as_text().unwrap().text.clone();
        assert_eq!(text, "Hello, world!");
    }

    #[test]
    fn tool_list_contains_hello() {
        let server = HelloServer::new();
        let tools = server.tool_router.list_all();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "hello");
    }

    #[test]
    fn get_info_enables_tools() {
        let server = HelloServer::new();
        assert!(
            ServerHandler::get_info(&server)
                .capabilities
                .tools
                .is_some()
        );
    }
}
