use std::fs;
use std::path::Path;

use crate::session::{Session, Task};

pub fn execute_tool(name: &str, args_str: &str, session: &mut Session) -> String {
    match name {
        "read_file" => {
            let parsed: Result<serde_json::Value, _> = serde_json::from_str(args_str);
            match parsed {
                Ok(val) => {
                    if let Some(path_str) = val.get("path").and_then(|v| v.as_str()) {
                        if Path::new(path_str).exists() {
                            match fs::read_to_string(path_str) {
                                Ok(contents) => contents,
                                Err(e) => format!("Error reading file: {}", e),
                            }
                        } else {
                            format!("Error: File not found at path '{}'", path_str)
                        }
                    } else {
                        "Error: Missing 'path' parameter".to_string()
                    }
                }
                Err(e) => format!("Error parsing tool arguments: {}", e),
            }
        }
        "update_plan" => {
            #[derive(serde::Deserialize)]
            struct Args {
                tasks: Vec<Task>,
            }

            if let Ok(args) = serde_json::from_str::<Args>(args_str) {
                session.plan = args.tasks;
                "Plan updated successfully.".to_string()
            } else {
                "Failed to parse plan arguments.".to_string()
            }
        }
        _ => format!("Error: Unknown tool '{}'", name),
    }
}
