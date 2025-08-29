use std::sync::{Arc, Mutex};

use arc_swap::ArcSwap;
use llm::{ChatMessage, ToolInfo, mcp::McpService};
use rmcp::{
    ServerHandler,
    handler::server::router::tool::ToolRouter,
    model::{ServerCapabilities, ServerInfo},
    service::{RoleClient, RunningService, ServiceExt},
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
) -> RunningService<RoleClient, McpService> {
    let builtins = BuiltinTools::new(chat_history);
    let (server_transport, client_transport) = duplex(64);
    let (server_res, client_res) = tokio::join!(
        builtins.clone().serve(server_transport),
        McpService {
            prefix: "chat".into(),
            tools: ArcSwap::new(Arc::new(vec![ToolInfo {
                name: "get_message_count".into(),
                description: "Returns the number of chat messages".into(),
                parameters: schema_for!(GetMessageCountParams),
            }])),
        }
        .serve(client_transport)
    );
    let server = server_res.expect("builtin server");
    let client_service = client_res.expect("builtin client");
    tokio::spawn(async move {
        let _ = server.waiting().await;
    });
    client_service
}
