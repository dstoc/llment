use async_trait::async_trait;
use rmcp::{
    model::{CallToolRequestParam, RawContent},
    service::{RoleClient, RunningService, ServiceExt},
    transport::TokioChildProcess,
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use tokio::process::Command;

use crate::{Schema, ToolInfo, tools::ToolExecutor};

pub struct McpService {
    pub prefix: String,
    pub tools: Vec<ToolInfo>,
    pub service: RunningService<RoleClient, ()>,
}

#[derive(Default)]
pub struct McpContext {
    services: HashMap<String, McpService>,
}

impl McpContext {
    pub fn insert(&mut self, service: McpService) {
        self.services.insert(service.prefix.clone(), service);
    }

    pub fn merge(&mut self, other: McpContext) {
        self.services.extend(other.services.into_iter());
    }

    pub fn tool_infos(&self) -> Vec<ToolInfo> {
        let mut infos = Vec::new();
        for svc in self.services.values() {
            for tool in &svc.tools {
                infos.push(ToolInfo {
                    name: format!("{}.{}", svc.prefix, tool.name),
                    description: tool.description.clone(),
                    parameters: tool.parameters.clone(),
                });
            }
        }
        infos
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
            .ok_or_else(|| format!("Tool {name} missing prefix"))?;
        let svc = self
            .services
            .get(prefix)
            .ok_or_else(|| format!("Service {prefix} not found"))?;
        let peer = svc.service.peer().clone();
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
    let mut ctx = McpContext::default();
    for (server_name, server) in config.mcp_servers.iter() {
        let mut cmd = Command::new(&server.command);
        cmd.args(&server.args);
        for (k, v) in &server.env {
            cmd.env(k, v);
        }
        let process = TokioChildProcess::new(cmd)?;
        let service = ().serve(process).await?;
        let tools = service.list_tools(Default::default()).await?;
        let mut infos = Vec::new();
        for tool in tools.tools {
            let schema: Schema = serde_json::from_value(tool.schema_as_json_value())?;
            let description = tool.description.clone().unwrap_or_default().to_string();
            infos.push(ToolInfo {
                name: tool.name.to_string(),
                description,
                parameters: schema,
            });
        }
        ctx.insert(McpService {
            prefix: server_name.clone(),
            tools: infos,
            service,
        });
    }
    Ok(ctx)
}
