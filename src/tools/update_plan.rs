use std::sync::Arc;
use tokio::sync::Mutex;
use crate::{session::{SessionData, Task}, tools::AgentTool};
use async_trait::async_trait;
use schemars::{JsonSchema, schema_for};
use serde::Deserialize;

#[derive(Deserialize, JsonSchema)]
struct UpdatePlanArgs {
    tasks: Vec<Task>,
}

pub struct UpdatePlanTool {
    pub session: Arc<Mutex<SessionData>>,
}

#[async_trait]
impl AgentTool for UpdatePlanTool {
    fn name(&self) -> &'static str { "update_plan" }
    
    fn description(&self) -> &'static str { 
        "Update the current execution plan and task statuses." 
    }
    
    fn parameters(&self) -> serde_json::Value {
        serde_json::to_value(schema_for!(UpdatePlanArgs)).unwrap()
    }

    async fn execute(&self, args: &str) -> Result<String, String> {
        let parsed: UpdatePlanArgs = serde_json::from_str(args)
            .map_err(|e| format!("Invalid JSON arguments: {}", e))?;
            
        let mut session = self.session.lock().await;
        session.plan = parsed.tasks;
        
        Ok("Plan updated successfully.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_plan_api_tool_output() {
        let session_data = SessionData::new("You are an assistant");

        let session = Arc::new(Mutex::new(session_data));
        let tool = UpdatePlanTool { session }; 
        let api_tool = tool.as_api_tool();

        // Serialize the Tool struct into pretty-printed JSON for inspection
        let json_output = serde_json::to_string_pretty(&api_tool)
            .expect("Failed to serialize api_tool");

        println!("--- UpdatePlanTool API Schema ---");
        println!("{}", json_output);
        
        // Assert basic fields exist
        assert_eq!(api_tool.function.name, "update_plan");
    }
}
