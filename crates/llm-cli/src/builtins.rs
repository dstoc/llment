use std::sync::{Arc, Mutex};

use llm::{ChatMessage, ToolInfo, mcp::McpContext};
use rmcp::{
    ServerHandler,
    handler::server::router::tool::ToolRouter,
    model::{ServerCapabilities, ServerInfo},
    service::ServiceExt,
    tool, tool_handler, tool_router,
};
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use tokio::io::duplex;

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct GetMessageCountParams {}

#[derive(Clone)]
struct BuiltinTools {
    chat_history: Arc<Mutex<Vec<ChatMessage>>>,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl BuiltinTools {
    fn new(chat_history: Arc<Mutex<Vec<ChatMessage>>>) -> Self {
        Self {
            chat_history,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        name = "get_message_count",
        description = "Returns the number of chat messages"
    )]
    fn get_message_count(&self) -> String {
        self.chat_history.lock().unwrap().len().to_string()
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for BuiltinTools {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

pub async fn setup_builtin_tools(
    chat_history: Arc<Mutex<Vec<ChatMessage>>>,
    mcp_context: &mut McpContext,
) {
    let builtins = BuiltinTools::new(chat_history);
    let (server_transport, client_transport) = duplex(64);
    let server = builtins
        .clone()
        .serve(server_transport)
        .await
        .expect("builtin server");
    tokio::spawn(async move {
        let _ = server.waiting().await;
    });
    let client_service = ().serve(client_transport).await.expect("builtin client");
    mcp_context
        .tools
        .insert("get_message_count".into(), client_service.peer().clone());
    mcp_context.tool_infos.push(ToolInfo {
        name: "get_message_count".into(),
        description: "Returns the number of chat messages".into(),
        parameters: schema_for!(GetMessageCountParams),
    });
}
