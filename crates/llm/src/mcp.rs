use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use rmcp::{
    ClientHandler,
    model::{CallToolRequestParam, RawContent},
    service::{NotificationContext, RoleClient, RunningService, ServerSink, ServiceExt},
    transport::TokioChildProcess,
};
use serde::Deserialize;
use serde_json::Value;
use tokio::{process::Command, sync::watch};

use crate::{Schema, ToolInfo, tools::ToolExecutor};

#[derive(Default)]
pub struct McpContext {
    pub tools: HashMap<String, ServerSink>,
    pub tool_infos: Vec<ToolInfo>,
}

impl McpContext {
    /// Merge another context into this one, extending tool mappings and metadata.
    pub fn merge(&mut self, other: McpContext) {
        self.tools.extend(other.tools);
        self.tool_infos.extend(other.tool_infos);
    }
}

#[derive(Deserialize)]
struct McpConfig {
    #[serde(rename = "mcpServers")]
    mcp_servers: HashMap<String, McpServer>,
}

#[derive(Deserialize)]
struct McpServer {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
}

pub struct McpClient {
    server_name: String,
    ctx: Arc<RwLock<McpContext>>,
    needs_update: watch::Sender<bool>,
}

impl ClientHandler for McpClient {
    fn on_tool_list_changed(
        &self,
        context: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        let server = self.server_name.clone();
        let ctx = self.ctx.clone();
        let needs_update = self.needs_update.clone();
        async move {
            if let Ok(tools) = context.peer.list_tools(Default::default()).await {
                let mut new_tools = HashMap::new();
                let mut new_infos = Vec::new();
                for tool in tools.tools {
                    let prefixed_name = format!("{server}.{}", tool.name);
                    new_tools.insert(prefixed_name.clone(), context.peer.clone());
                    if let Ok(schema) = serde_json::from_value(tool.schema_as_json_value()) {
                        let description = tool.description.clone().unwrap_or_default().to_string();
                        new_infos.push(ToolInfo {
                            name: prefixed_name,
                            description,
                            parameters: schema,
                        });
                    }
                }
                {
                    let mut guard = ctx.write().unwrap();
                    guard
                        .tools
                        .retain(|name, _| !name.starts_with(&format!("{server}.")));
                    guard
                        .tool_infos
                        .retain(|info| !info.name.starts_with(&format!("{server}.")));
                    guard.tools.extend(new_tools);
                    guard.tool_infos.extend(new_infos);
                }
                let _ = needs_update.send(true);
            }
        }
    }
}

pub async fn load_mcp_servers(
    path: &str,
    needs_update: watch::Sender<bool>,
) -> Result<
    (
        Arc<RwLock<McpContext>>,
        Vec<RunningService<RoleClient, McpClient>>,
    ),
    Box<dyn std::error::Error + Send + Sync>,
> {
    let data = tokio::fs::read_to_string(path).await?;
    let config: McpConfig = serde_json::from_str(&data)?;
    let mut services = Vec::new();
    let ctx = Arc::new(RwLock::new(McpContext::default()));
    for (server_name, server) in config.mcp_servers.iter() {
        let mut cmd = Command::new(&server.command);
        cmd.args(&server.args);
        for (k, v) in &server.env {
            cmd.env(k, v);
        }
        let process = TokioChildProcess::new(cmd)?;
        let handler = McpClient {
            server_name: server_name.clone(),
            ctx: ctx.clone(),
            needs_update: needs_update.clone(),
        };
        let service = handler.serve(process).await?;
        let tools = service.list_tools(Default::default()).await?;
        {
            let mut guard = ctx.write().unwrap();
            for tool in tools.tools {
                let prefixed_name = format!("{server_name}.{}", tool.name);
                guard
                    .tools
                    .insert(prefixed_name.clone(), service.peer().clone());
                let schema: Schema = serde_json::from_value(tool.schema_as_json_value())?;
                let description = tool.description.clone().unwrap_or_default().to_string();
                guard.tool_infos.push(ToolInfo {
                    name: prefixed_name,
                    description,
                    parameters: schema,
                });
            }
        }
        services.push(service);
    }
    Ok((ctx, services))
}

pub struct McpToolExecutor {
    ctx: Arc<RwLock<McpContext>>,
}

impl McpToolExecutor {
    pub fn new(ctx: Arc<RwLock<McpContext>>) -> Self {
        Self { ctx }
    }
}

#[async_trait::async_trait]
impl ToolExecutor for McpToolExecutor {
    async fn call(
        &self,
        name: &str,
        args: Value,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let peer = {
            let guard = self.ctx.read().unwrap();
            guard.tools.get(name).cloned()
        }
        .ok_or_else(|| format!("Tool {name} not found"))?;

        let tool_name = name.rsplit_once('.').map(|(_, t)| t).unwrap_or(name);

        let result = peer
            .call_tool(CallToolRequestParam {
                name: tool_name.to_string().into(),
                arguments: args.as_object().cloned(),
            })
            .await?;

        if let Some(content) = result.content {
            let text = content
                .into_iter()
                .filter_map(|c| match c.raw {
                    RawContent::Text(t) => Some(t.text),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            Ok(text)
        } else if let Some(value) = result.structured_content {
            Ok(value.to_string())
        } else {
            Ok(String::new())
        }
    }
}
