pub mod read_file;
pub mod create_plan;

use async_trait::async_trait;
use std::collections::HashMap;
use tokio;

use crate::{
    api::{FunctionDeclaration, Tool},
    session::StreamEvent,
};

#[async_trait]
pub trait AgentTool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn parameters(&self) -> serde_json::Value;
    async fn execute(
        &self,
        args: &str,
        tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
    ) -> Result<String, String>;
    fn roundtrip_on_success(&self) -> bool {
        true
    }

    fn as_api_tool(&self) -> Tool {
        let mut params = self.parameters();

        if let Some(obj) = params.as_object_mut() {
            obj.remove("$schema");
            obj.remove("title");
        }

        enforce_strict_schema(&mut params);
        Tool {
            r#type: "function".to_string(),
            function: FunctionDeclaration {
                name: self.name().to_string(),
                description: self.description().to_string(),
                parameters: params,
                strict: Some(true),
            },
        }
    }
}

fn enforce_strict_schema(val: &mut serde_json::Value) {
    if let Some(obj) = val.as_object_mut() {
        let property_keys: Option<Vec<String>> = obj
            .get("properties")
            .and_then(|p| p.as_object())
            .map(|props| props.keys().cloned().collect());

        if let Some(keys) = property_keys {
            obj.insert(
                "additionalProperties".to_string(),
                serde_json::Value::Bool(false),
            );

            let all_keys: Vec<serde_json::Value> =
                keys.into_iter().map(serde_json::Value::String).collect();

            obj.insert("required".to_string(), serde_json::Value::Array(all_keys));
        }

        for (_, v) in obj.iter_mut() {
            enforce_strict_schema(v);
        }
    } else if let Some(arr) = val.as_array_mut() {
        for v in arr.iter_mut() {
            enforce_strict_schema(v);
        }
    }
}

pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn AgentTool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: Box<dyn AgentTool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn get_tool(&self, name: &str) -> Option<&dyn AgentTool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    pub fn get_api_tools(&self) -> Vec<Tool> {
        self.tools.values().map(|t| t.as_api_tool()).collect()
    }

    pub async fn execute_tool(
        &self,
        name: &str,
        args: &str,
        tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
    ) -> Result<String, String> {
        if let Some(tool) = self.tools.get(name) {
            tool.execute(args, tx).await
        } else {
            Err(format!("Tool '{}' not found in registry", name))
        }
    }
}
