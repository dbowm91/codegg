use codegg::skills::{Skill, SkillIndex};
use std::path::PathBuf;

#[test]
fn test_skill_index_new() {
    let index = SkillIndex::new();
    assert_eq!(index.list().len(), 0);
}

#[test]
fn test_skill_index_get_nonexistent() {
    let index = SkillIndex::new();
    assert!(index.get("nonexistent").is_none());
}

#[test]
fn test_skill_index_find_matching_empty() {
    let index = SkillIndex::new();
    let results = index.find_matching("test");
    assert!(results.is_empty());
}

#[test]
fn test_skill_build_system_prompt_empty() {
    let index = SkillIndex::new();
    let prompt = index.build_system_prompt();
    assert!(prompt.is_empty());
}

#[test]
fn test_skill_activate_nonexistent() {
    let index = SkillIndex::new();
    let result = index.activate("nonexistent");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_skill_index_load_empty_dir() {
    let mut index = SkillIndex::new();
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let result = index.load(temp_dir.path().to_str().unwrap()).await;
    assert!(result.is_ok());
    assert_eq!(index.list().len(), 0);
}

#[test]
fn test_skill_struct_creation() {
    let skill = Skill {
        name: "test-skill".to_string(),
        description: "A test skill".to_string(),
        version: None,
        tags: vec!["testing".to_string(), "unit".to_string()],
        body: "This is the skill body.\nIt has multiple lines.\n".to_string(),
        source: PathBuf::from("/test/skill"),
    };

    assert_eq!(skill.name, "test-skill");
    assert_eq!(skill.description, "A test skill");
    assert_eq!(skill.tags.len(), 2);
}
