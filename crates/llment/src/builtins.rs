use std::sync::{Arc, Mutex};

use arc_swap::ArcSwap;
use llm::{ChatMessage, JsonResult, ToolInfo, mcp::McpService};
use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, tool::Parameters},
    model::{ServerCapabilities, ServerInfo},
    service::{RoleClient, RunningService, ServiceExt},
    tool, tool_handler, tool_router,
};
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::duplex;

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct GetMessageCountParams {}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct DiscardFunctionResponseParams {
    /// The id of the ToolCall/Tool response to discard
    pub id: String,
}

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

    #[tool(
        name = "discard_function_response",
        description = "Removes the content from a tool response in history by id"
    )]
    fn discard_function_response(
        &self,
        Parameters(params): Parameters<DiscardFunctionResponseParams>,
    ) -> String {
        let mut history = self.chat_history.lock().unwrap();
        if let Some((idx, _)) = history.iter().enumerate().rev().find(|(_, m)| match m {
            ChatMessage::Tool(t) => t.id == params.id,
            _ => false,
        }) {
            if let ChatMessage::Tool(t) = &mut history[idx] {
                t.content = JsonResult::Content {
                    content: Value::String("<response discarded>".into()),
                };
            }
            "ok".into()
        } else {
            format!("Tool response with id '{}' not found", params.id)
        }
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
            tools: ArcSwap::new(Arc::new(vec![
                ToolInfo {
                    name: "get_message_count".into(),
                    description: "Returns the number of chat messages".into(),
                    parameters: schema_for!(GetMessageCountParams),
                },
                ToolInfo {
                    name: "discard_function_response".into(),
                    description: "Removes the content from a tool response in history by id".into(),
                    parameters: schema_for!(DiscardFunctionResponseParams),
                },
            ])),
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
