use std::collections::HashMap;

use rmcp::service::ServerSink;
use tokio::sync::Mutex;

use crate::ToolInfo;

#[derive(Default)]
pub struct McpContext {
    pub tools: Mutex<HashMap<String, ServerSink>>,
    pub tool_infos: Mutex<Vec<ToolInfo>>,
}
