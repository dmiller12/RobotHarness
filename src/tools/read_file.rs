use std::fs;
use async_trait::async_trait;
use schemars::{JsonSchema, schema_for};
use serde::Deserialize;

use crate::tools::AgentTool;

#[derive(Deserialize, JsonSchema)]
struct ReadFileArgs {
    /// The relative or absolute path to the file.
    path: String,
}

pub struct ReadFileTool;

#[async_trait]
impl AgentTool for ReadFileTool {
    fn name(&self) -> &'static str { "read_file" }
    
    fn description(&self) -> &'static str { 
        "Read the contents of a file from the local filesystem." 
    }
    
    fn parameters(&self) -> serde_json::Value {
        serde_json::to_value(schema_for!(ReadFileArgs)).unwrap()
    }

    async fn execute(&self, args: &str) -> Result<String, String> {
        let parsed: ReadFileArgs = serde_json::from_str(args)
            .map_err(|e| format!("Invalid JSON arguments: {}", e))?;
            
        fs::read_to_string(&parsed.path)
            .map_err(|e| format!("Failed to read file at {}: {}", parsed.path, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_file_api_tool_output() {
        let tool = ReadFileTool;
        let api_tool = tool.as_api_tool();

        // Serialize the Tool struct into pretty-printed JSON for inspection
        let json_output = serde_json::to_string_pretty(&api_tool)
            .expect("Failed to serialize api_tool");

        println!("--- ReadFileTool API Schema ---");
        println!("{}", json_output);
        
        // Assert basic fields exist
        assert_eq!(api_tool.function.name, "read_file");
    }
}
