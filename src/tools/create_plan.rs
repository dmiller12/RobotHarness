use crate::{
    session::{SessionData, StreamEvent, Task, TaskStatus},
    tools::AgentTool,
};
use async_trait::async_trait;
use schemars::{JsonSchema, schema_for};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NewTask {
    /// A clear, concise summary of the task to be performed.
    pub description: String,
}

#[derive(Deserialize, JsonSchema)]
struct UpdatePlanArgs {
    tasks: Vec<NewTask>,
}

pub struct CreatePlanTool {
    pub session: Arc<Mutex<SessionData>>,
}

#[async_trait]
impl AgentTool for CreatePlanTool {
    fn name(&self) -> &'static str {
        "create_plan"
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

        let mut final_tasks = Vec::new();
        for (index, new_task) in parsed.tasks.into_iter().enumerate() {
            final_tasks.push(Task {
                id: format!("task_{}", index + 1),
                description: new_task.description,
                status: TaskStatus::Pending,
            });
        }

        tx.send(StreamEvent::PlanUpdated(final_tasks.clone()));
        let mut session = self.session.lock().await;
        session.plan = final_tasks;

        Ok("Plan updated successfully.".to_string())
    }

    fn roundtrip_on_success(&self) -> bool {
        false
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateTaskStatusArgs {
    pub task_id: String,
    pub status: TaskStatus,
}

pub struct UpdateTaskStatusTool {
    pub session: Arc<Mutex<SessionData>>,
}

#[async_trait]
impl AgentTool for UpdateTaskStatusTool {
    fn name(&self) -> &'static str {
        "update_task_status"
    }

    fn description(&self) -> &'static str {
        "Updates the status of a single existing task by its unique ID. Use this to transition a task to 'in_progress', 'completed', or 'failed'."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::to_value(schema_for!(UpdateTaskStatusArgs)).unwrap()
    }

    async fn execute(
        &self,
        args: &str,
        tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
    ) -> Result<String, String> {
        let parsed: UpdateTaskStatusArgs =
            serde_json::from_str(args).map_err(|e| format!("Invalid JSON arguments: {}", e))?;

        let mut session = self.session.lock().await;

        // Constraint Check: Enforce maximum one 'in_progress' task
        if parsed.status == TaskStatus::InProgress {
            let active_tasks = session
                .plan
                .iter()
                .filter(|t| t.status == TaskStatus::InProgress && t.id != parsed.task_id)
                .count();

            if active_tasks > 0 {
                return Err(format!(
                    "Constraint violation: Another task is currently 'in_progress'. You must update it to 'completed' or 'pending' before starting task '{}'.",
                    parsed.task_id
                ));
            }
        }

        // Locate and mutate the target task
        let task = session
            .plan
            .iter_mut()
            .find(|t| t.id == parsed.task_id)
            .ok_or_else(|| format!("Task ID '{}' not found in the current plan.", parsed.task_id))?;

        task.status = parsed.status.clone();
        let updated_plan = session.plan.clone();
        
        drop(session); // Release the lock before broadcasting

        let _ = tx.send(StreamEvent::PlanUpdated(updated_plan));

        Ok(format!("Successfully updated task '{}' to '{:?}'.", parsed.task_id, parsed.status))
    }

    fn roundtrip_on_success(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_plan_api_tool_output() {
        let session_data = SessionData::new("You are an assistant");

        let session = Arc::new(Mutex::new(session_data));
        let tool = CreatePlanTool { session };
        let api_tool = tool.as_api_tool();

        // Serialize the Tool struct into pretty-printed JSON for inspection
        let json_output =
            serde_json::to_string_pretty(&api_tool).expect("Failed to serialize api_tool");

        println!("--- UpdatePlanTool API Schema ---");
        println!("{}", json_output);

        // Assert basic fields exist
        assert_eq!(api_tool.function.name, "create_plan");
    }
}
