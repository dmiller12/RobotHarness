use gray_matter::{Matter, engine::YAML};
use serde::Deserialize;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;

#[derive(Debug, Deserialize, Clone)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub tools: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Skill {
    pub metadata: SkillMetadata,
    pub instructions: String,
}

pub struct SkillRegistry {
    pub skills: HashMap<String, Skill>,
}

impl SkillRegistry {
    pub fn load_from_directory(path: &str) -> Self {
        let mut skills = HashMap::new();
        let entries = fs::read_dir(path).expect("Failed to read skills directory");
        for entry in entries {
            let entry = entry.expect("Failed to read directory entry");
            let file_path = entry.path();

            let is_markdown = file_path.extension().and_then(OsStr::to_str) == Some("md");

            if file_path.is_file() && is_markdown {
                let contents = fs::read_to_string(file_path).expect("Failed to read file");
                let skill = Self::parse_skill(&contents);

                skills.insert(skill.metadata.name.clone(), skill);
            }
        }
        Self { skills }
    }

    fn parse_skill(file_contents: &str) -> Skill {
        let mut matter = Matter::<YAML>::new();
        let parsed: gray_matter::ParsedEntity<SkillMetadata> = matter
            .parse(file_contents)
            .expect("Failed to parse markdown");

        let metadata: SkillMetadata = parsed.data.expect("Missing frontmatter");

        Skill {
            metadata,
            instructions: parsed.content,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_parse_skill_valid_markdown() {
        let valid_markdown = r#"---
name: test_skill
description: A mock skill for testing
tools: ["create_plan"]
---
# Test Header
This is the instruction body."#;

        let skill = SkillRegistry::parse_skill(valid_markdown);

        assert_eq!(skill.metadata.name, "test_skill");
        assert_eq!(skill.metadata.description, "A mock skill for testing");
        assert_eq!(skill.metadata.tools, vec!["create_plan"]);
        assert_eq!(
            skill.instructions.trim(),
            "# Test Header\nThis is the instruction body."
        );
    }

    #[test]
    fn test_load_from_directory() {
        // 1. Create a temporary directory
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let dir_path = temp_dir.path();

        // 2. Create a valid skill file
        let file_path = dir_path.join("test_skill.md");
        let mut file = fs::File::create(&file_path).expect("Failed to create file");

        let valid_markdown = r#"---
name: planner
description: test planner
tools: []
---
Body"#;
        file.write_all(valid_markdown.as_bytes())
            .expect("Failed to write to file");

        // 3. Create a non-markdown file (should be ignored)
        let txt_path = dir_path.join("ignore_me.txt");
        fs::File::create(txt_path).expect("Failed to create txt file");

        // 4. Load the registry
        let registry = SkillRegistry::load_from_directory(dir_path.to_str().unwrap());

        // Assertions
        assert_eq!(registry.skills.len(), 1);
        assert!(registry.skills.contains_key("planner"));
    }
}
