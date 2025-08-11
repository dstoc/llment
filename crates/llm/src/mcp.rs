use std::{collections::HashMap, sync::Arc};

use rmcp::{
    model::{CallToolRequestParam, RawContent},
    service::{RoleClient, RunningService, ServerSink, ServiceExt},
    transport::TokioChildProcess,
};
use serde::Deserialize;
use serde_json::Value;
use tokio::{process::Command, sync::Mutex};

use crate::{Schema, ToolFunctionInfo, ToolInfo, ToolType, tools::ToolExecutor};

#[derive(Default)]
pub struct McpContext {
    pub tools: Mutex<HashMap<String, ServerSink>>,
    pub tool_infos: Mutex<Vec<ToolInfo>>,
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
) -> Result<
    (McpContext, Vec<RunningService<RoleClient, ()>>),
    Box<dyn std::error::Error + Send + Sync>,
> {
    let data = tokio::fs::read_to_string(path).await?;
    let config: McpConfig = serde_json::from_str(&data)?;
    let mut services = Vec::new();
    let ctx = McpContext::default();
    for server in config.mcp_servers.values() {
        let mut cmd = Command::new(&server.command);
        cmd.args(&server.args);
        for (k, v) in &server.env {
            cmd.env(k, v);
        }
        let process = TokioChildProcess::new(cmd)?;
        let service = ().serve(process).await?;
        let tools = service.list_tools(Default::default()).await?;
        {
            let mut map = ctx.tools.lock().await;
            let mut infos = ctx.tool_infos.lock().await;
            for tool in tools.tools {
                map.insert(tool.name.to_string(), service.peer().clone());
                let schema: Schema = serde_json::from_value(tool.schema_as_json_value())?;
                let description = tool.description.clone().unwrap_or_default().to_string();
                infos.push(ToolInfo {
                    tool_type: ToolType::Function,
                    function: ToolFunctionInfo {
                        name: tool.name.to_string(),
                        description,
                        parameters: schema,
                    },
                });
            }
        }
        services.push(service);
    }
    Ok((ctx, services))
}

pub struct McpToolExecutor {
    ctx: Arc<McpContext>,
}

impl McpToolExecutor {
    pub fn new(ctx: Arc<McpContext>) -> Self {
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
            let map = self.ctx.tools.lock().await;
            map.get(name).cloned()
        }
        .ok_or_else(|| format!("Tool {name} not found"))?;

        let result = peer
            .call_tool(CallToolRequestParam {
                name: name.to_string().into(),
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
