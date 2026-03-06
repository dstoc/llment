use rmcp::{
    tool, tool_handler, tool_router, ErrorData, ServerHandler,
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
};
use serde::Deserialize;

#[derive(Deserialize, rmcp::schemars::JsonSchema)]
pub struct Params {}

pub struct MyServer {}

#[tool_router]
impl MyServer {
    #[tool(description = "Test tool")]
    pub async fn my_tool(&self, params: Params) -> Result<CallToolResult, ErrorData> {
        Ok(CallToolResult::success(vec![Content::text("ok".to_string())]))
    }
}
#[tool_handler]
impl ServerHandler for MyServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            name: "MyServer".to_string(),
            version: "0.1.0".to_string(),
        }
    }
}
