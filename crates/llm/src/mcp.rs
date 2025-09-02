use arc_swap::ArcSwap;
use async_trait::async_trait;
use rmcp::{
    ClientHandler,
    model::{CallToolRequestParam, RawContent},
    service::{NotificationContext, RoleClient, RunningService, ServiceExt},
    transport::TokioChildProcess,
};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::process::Command;

use crate::{Schema, ToolInfo, tools::ToolExecutor};

pub struct McpService {
    pub prefix: String,
    pub tools: ArcSwap<Vec<ToolInfo>>,
}

impl ClientHandler for McpService {
    fn on_tool_list_changed(
        &self,
        context: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        async move {
            if let Ok(tools) = context.peer.list_all_tools().await {
                let mut infos = Vec::new();
                for tool in tools {
                    if let Ok(schema) =
                        serde_json::from_value::<Schema>(tool.schema_as_json_value())
                    {
                        let description = tool.description.clone().unwrap_or_default().to_string();
                        infos.push(ToolInfo {
                            name: tool.name.to_string(),
                            description,
                            parameters: schema,
                        });
                    }
                }
                self.tools.store(Arc::new(infos));
            }
        }
    }
}

#[derive(Default, Clone)]
pub struct McpContext {
    services: Arc<Mutex<HashMap<String, RunningService<RoleClient, McpService>>>>,
}

impl McpContext {
    pub fn insert(&self, service: RunningService<RoleClient, McpService>) {
        let prefix = service.service().prefix.clone();
        self.services.lock().unwrap().insert(prefix, service);
    }

    pub fn remove(&self, prefix: &str) {
        self.services.lock().unwrap().remove(prefix);
    }

    pub fn tool_infos(&self) -> Vec<ToolInfo> {
        let mut infos = Vec::new();
        let services = self.services.lock().unwrap();
        for svc in services.values() {
            let prefix = svc.service().prefix.clone();
            let tools = svc.service().tools.load();
            for tool in tools.iter() {
                infos.push(ToolInfo {
                    name: format!("{}.{}", prefix, tool.name),
                    description: tool.description.clone(),
                    parameters: tool.parameters.clone(),
                });
            }
        }
        infos
    }

    pub fn tool_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        let services = self.services.lock().unwrap();
        for svc in services.values() {
            let prefix = svc.service().prefix.clone();
            let tools = svc.service().tools.load();
            for tool in tools.iter() {
                names.push(format!("{}.{}", prefix, tool.name));
            }
        }
        names
    }
}

#[async_trait]
impl ToolExecutor for McpContext {
    async fn call(
        &self,
        name: &str,
        args: Value,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let (prefix, tool_name) = name
            .split_once('.')
            .ok_or_else(|| format!("{name} is not a valid tool name"))?;
        let peer = {
            let services = self.services.lock().unwrap();
            let svc = services
                .get(prefix)
                .ok_or_else(|| format!("{name} is not a valid tool name"))?;
            svc.peer().clone()
        };
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

pub async fn load_mcp_servers(
    path: &str,
) -> Result<McpContext, Box<dyn std::error::Error + Send + Sync>> {
    let data = tokio::fs::read_to_string(path).await?;
    let config: McpConfig = serde_json::from_str(&data)?;
    let ctx = McpContext::default();
    for (server_name, server) in config.mcp_servers.iter() {
        let mut cmd = Command::new(&server.command);
        cmd.args(&server.args);
        for (k, v) in &server.env {
            cmd.env(k, v);
        }
        let process = TokioChildProcess::new(cmd)?;
        let handler = McpService {
            prefix: server_name.clone(),
            tools: ArcSwap::new(Arc::new(Vec::new())),
        };
        let service = handler.serve(process).await?;
        let tools = service.peer().list_all_tools().await?;
        let mut infos = Vec::new();
        for tool in tools {
            let schema: Schema = serde_json::from_value(tool.schema_as_json_value())?;
            let description = tool.description.clone().unwrap_or_default().to_string();
            infos.push(ToolInfo {
                name: tool.name.to_string(),
                description,
                parameters: schema,
            });
        }
        service.service().tools.store(Arc::new(infos));
        ctx.insert(service);
    }
    Ok(ctx)
}
