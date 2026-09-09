use crate::{
    session::{SessionData, StreamEvent, Task, TaskStatus},
    tools::AgentTool,
};
use async_trait::async_trait;
use schemars::{JsonSchema, schema_for};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Deserialize, JsonSchema)]
struct UpdatePlanArgs {
    tasks: Vec<Task>,
}

pub struct UpdatePlanTool {
    pub session: Arc<Mutex<SessionData>>,
}

#[async_trait]
impl AgentTool for UpdatePlanTool {
    fn name(&self) -> &'static str {
        "update_plan"
    }

    fn description(&self) -> &'static str {
        "Overwrites the active execution plan. You must provide the entire array of tasks, not just the modified ones."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::to_value(schema_for!(UpdatePlanArgs)).unwrap()
    }

    async fn execute(
        &self,
        args: &str,
        tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
    ) -> Result<String, String> {
        let parsed: UpdatePlanArgs =
            serde_json::from_str(args).map_err(|e| format!("Invalid JSON arguments: {}", e))?;
        let in_progress_count = parsed
            .tasks
            .iter()
            .filter(|task| task.status == TaskStatus::InProgress)
            .count();

        if in_progress_count > 1 {
            return Err(format!(
                "Constraint violation: at most one task may be 'in_progress' at a time, but found {}. You must mark previous tasks as 'completed' or 'pending' before advancing the plan.",
                in_progress_count
            ));
        }

        tx.send(StreamEvent::PlanUpdated(parsed.tasks.clone()));
        let mut session = self.session.lock().await;
        session.plan = parsed.tasks;

        Ok("Plan updated successfully.".to_string())
    }

    fn roundtrip_on_success(&self) -> bool {
        false
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
        let json_output =
            serde_json::to_string_pretty(&api_tool).expect("Failed to serialize api_tool");

        println!("--- UpdatePlanTool API Schema ---");
        println!("{}", json_output);

        // Assert basic fields exist
        assert_eq!(api_tool.function.name, "update_plan");
    }
}
