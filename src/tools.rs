use std::fs;
use std::path::Path;

pub fn execute_tool(name: &str, arguments_json: &str) -> String {
    match name {
        "read_file" => {
            let parsed: Result<serde_json::Value, _> = serde_json::from_str(arguments_json);
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
        _ => format!("Error: Unknown tool '{}'", name),
    }
}
