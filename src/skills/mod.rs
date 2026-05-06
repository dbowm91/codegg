use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub tags: Vec<String>,
    pub body: String,
    pub source: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SkillFrontmatter {
    pub name: Option<String>,
    pub description: Option<String>,
    pub version: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

pub struct SkillIndex {
    skills: Vec<Skill>,
}

impl Default for SkillIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillIndex {
    pub fn new() -> Self {
        Self { skills: Vec::new() }
    }

    pub async fn load(&mut self, project_dir: &str) -> Result<(), AppError> {
        self.skills.clear();

        let config_dir = dirs::config_dir()
            .map(|d| d.join("codegg").join("skills"))
            .filter(|d| d.is_dir());

        let project_dir = PathBuf::from(project_dir);
        let local_skills = project_dir.join(".codegg").join("skills");
        let local_dir = local_skills.is_dir().then_some(local_skills);

        for dir in config_dir.into_iter().chain(local_dir) {
            self.load_dir(&dir).await?;
        }

        Ok(())
    }

    async fn load_dir(&mut self, dir: &Path) -> Result<(), AppError> {
        let entries = fs::read_dir(dir).map_err(AppError::Io)?;

        for entry in entries {
            let entry = entry.map_err(AppError::Io)?;
            let path = entry.path();

            if path.is_dir() {
                let skill_md = path.join("SKILL.md");
                if skill_md.is_file() {
                    if let Some(skill) = parse_skill_file(&skill_md)? {
                        self.skills.push(skill);
                    }
                }
                continue;
            }

            if path.extension().and_then(|e| e.to_str()) == Some("md") {
                if let Some(skill) = parse_skill_file(&path)? {
                    self.skills.push(skill);
                }
            }
        }

        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|s| s.name == name)
    }

    pub fn list(&self) -> &[Skill] {
        &self.skills
    }

    pub fn find_matching(&self, query: &str) -> Vec<&Skill> {
        let query_lower = query.to_lowercase();
        self.skills
            .iter()
            .filter(|s| {
                s.name.to_lowercase().contains(&query_lower)
                    || s.description.to_lowercase().contains(&query_lower)
                    || s.tags
                        .iter()
                        .any(|t| t.to_lowercase().contains(&query_lower))
            })
            .collect()
    }

    pub fn build_system_prompt(&self) -> String {
        if self.skills.is_empty() {
            return String::new();
        }

        let mut prompt = String::from("## Available Skills\n\n");
        prompt.push_str("The following skills are available. Use /skill:<name> to activate a specific skill.\n\n");

        for skill in &self.skills {
            prompt.push_str(&format!("- **{}**: {}\n", skill.name, skill.description));
        }

        prompt.push('\n');
        prompt
    }

    pub fn activate(&self, name: &str) -> Option<String> {
        self.get(name).map(|s| s.body.clone())
    }
}

fn parse_skill_file(path: &Path) -> Result<Option<Skill>, AppError> {
    let content = fs::read_to_string(path).map_err(AppError::Io)?;
    let Some((frontmatter, body)) = parse_frontmatter(&content) else {
        return Ok(None);
    };

    let fm: SkillFrontmatter =
        serde_yaml::from_str(&frontmatter).map_err(|e| AppError::Other(e.into()))?;

    let name = fm.name.unwrap_or_else(|| {
        path.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default()
    });

    let description = fm.description.unwrap_or_default();

    Ok(Some(Skill {
        name,
        description,
        version: fm.version,
        tags: fm.tags,
        body,
        source: path.to_path_buf(),
    }))
}

fn parse_frontmatter(content: &str) -> Option<(String, String)> {
    let content = content.trim_start();
    if !content.starts_with("---") {
        return None;
    }

    let rest = &content[3..];
    let end = rest.find("---")?;
    let frontmatter = rest[..end].trim().to_string();
    let body = rest[end + 3..].to_string();

    Some((frontmatter, body))
}
