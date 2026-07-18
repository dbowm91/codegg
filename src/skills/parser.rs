use std::collections::HashMap;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::candidate::{ResourceDescriptor, SkillCandidate};
use super::diagnostic::Diagnostic;
use super::source::{AssetDiscoveryConfig, SourceKind};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct PortableFrontmatter {
    pub name: Option<String>,
    pub description: Option<String>,
    pub license: Option<String>,
    pub compatibility: Option<String>,
    #[serde(default)]
    pub metadata: HashMap<String, serde_yaml::Value>,
    #[serde(rename = "allowed-tools")]
    pub allowed_tools: Option<serde_yaml::Value>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct NativeFrontmatter {
    pub name: Option<String>,
    pub description: Option<String>,
    pub version: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct RawFrontmatter {
    #[serde(flatten)]
    inner: HashMap<String, serde_yaml::Value>,
}

pub fn parse_candidate(
    skill_file: &Path,
    source_kind: SourceKind,
    config: &AssetDiscoveryConfig,
) -> Result<SkillCandidate, Diagnostic> {
    let location = skill_file.display().to_string();

    let raw_content = std::fs::read_to_string(skill_file).map_err(|e| {
        Diagnostic::error(format!("failed to read skill file: {e}"), location.clone())
    })?;

    if raw_content.len() as u64 > config.max_skill_file_size {
        return Err(Diagnostic::error(
            format!(
                "skill file exceeds maximum size ({} bytes)",
                config.max_skill_file_size
            ),
            location.clone(),
        ));
    }

    let (frontmatter_str, body) = parse_frontmatter(&raw_content).ok_or_else(|| {
        Diagnostic::error(
            "missing or malformed YAML frontmatter (expected --- delimiters)".to_string(),
            location.clone(),
        )
    })?;

    if frontmatter_str.len() > config.max_frontmatter_size {
        return Err(Diagnostic::error(
            format!(
                "frontmatter exceeds maximum size ({} bytes)",
                config.max_frontmatter_size
            ),
            location.clone(),
        ));
    }

    let mut diagnostics = Vec::new();

    let (name, description, metadata) = match source_kind {
        SourceKind::CodeGGNativeCompat | SourceKind::CodeGGProject
            if !has_portable_fields(&frontmatter_str) =>
        {
            let fm: NativeFrontmatter = serde_yaml::from_str(&frontmatter_str).map_err(|e| {
                Diagnostic::error(
                    format!("failed to parse frontmatter: {e}"),
                    location.clone(),
                )
            })?;
            let name = fm.name.unwrap_or_else(|| {
                skill_file
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default()
            });
            let description = fm.description.unwrap_or_default();
            let mut meta = HashMap::new();
            if let Some(v) = fm.version {
                meta.insert("version".to_string(), serde_yaml::Value::String(v));
            }
            if !fm.tags.is_empty() {
                meta.insert(
                    "tags".to_string(),
                    serde_yaml::Value::Sequence(
                        fm.tags.into_iter().map(serde_yaml::Value::String).collect(),
                    ),
                );
            }
            (name, description, meta)
        }
        _ => {
            let fm: PortableFrontmatter = serde_yaml::from_str(&frontmatter_str).map_err(|e| {
                Diagnostic::error(
                    format!("failed to parse frontmatter: {e}"),
                    location.clone(),
                )
            })?;
            let name = fm.name.ok_or_else(|| {
                Diagnostic::error("missing required field: name".to_string(), location.clone())
            })?;
            let description = fm.description.ok_or_else(|| {
                Diagnostic::error(
                    "missing required field: description".to_string(),
                    location.clone(),
                )
            })?;
            let mut meta = fm.metadata;
            if let Some(lic) = fm.license {
                meta.insert("license".to_string(), serde_yaml::Value::String(lic));
            }
            if let Some(compat) = fm.compatibility {
                meta.insert(
                    "compatibility".to_string(),
                    serde_yaml::Value::String(compat),
                );
            }
            if let Some(tools) = fm.allowed_tools {
                meta.insert("allowed-tools".to_string(), tools);
                diagnostics.push(Diagnostic::warning(
                    "allowed-tools is preserved as metadata only; it does not grant permissions",
                    location.clone(),
                ));
            }
            (name, description, meta)
        }
    };

    let normalized_name = normalize_name(&name, config)?;

    if description.len() > config.max_description_length {
        diagnostics.push(Diagnostic::warning(
            format!(
                "description exceeds recommended maximum ({} chars)",
                config.max_description_length
            ),
            location.clone(),
        ));
    }

    let package_root = determine_package_root(skill_file, source_kind);
    let resources = inventory_resources(&package_root, config, &location, &mut diagnostics)?;

    let content_digest = compute_digest(&frontmatter_str, &body);

    Ok(SkillCandidate {
        name,
        normalized_name,
        description,
        source_kind,
        source_path: skill_file.to_path_buf(),
        package_root,
        content_digest,
        frontmatter_raw: frontmatter_str,
        body,
        metadata,
        resources,
        diagnostics,
    })
}

fn has_portable_fields(frontmatter: &str) -> bool {
    if let Ok(raw) = serde_yaml::from_str::<RawFrontmatter>(frontmatter) {
        raw.inner.contains_key("name") && raw.inner.contains_key("description")
    } else {
        false
    }
}

fn determine_package_root(skill_file: &Path, source_kind: SourceKind) -> PathBuf {
    match source_kind {
        SourceKind::CodeGGNativeCompat => skill_file
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| skill_file.to_path_buf()),
        _ => skill_file
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| skill_file.to_path_buf()),
    }
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

fn normalize_name(name: &str, config: &AssetDiscoveryConfig) -> Result<String, Diagnostic> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(Diagnostic::error(
            "skill name must not be empty",
            "frontmatter".to_string(),
        ));
    }
    if trimmed.len() > config.max_skill_name_length {
        return Err(Diagnostic::error(
            format!(
                "skill name exceeds maximum length ({} chars)",
                config.max_skill_name_length
            ),
            "frontmatter".to_string(),
        ));
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        return Err(Diagnostic::error(
            "skill name must not contain path separators",
            "frontmatter".to_string(),
        ));
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return Err(Diagnostic::error(
            "skill name must not contain control characters",
            "frontmatter".to_string(),
        ));
    }
    Ok(trimmed.to_lowercase())
}

fn inventory_resources(
    package_root: &Path,
    config: &AssetDiscoveryConfig,
    location: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Vec<ResourceDescriptor>, Diagnostic> {
    let mut resources = Vec::new();
    if !package_root.is_dir() {
        return Ok(resources);
    }

    let entries = std::fs::read_dir(package_root).map_err(|e| {
        Diagnostic::error(
            format!("failed to read package directory: {e}"),
            location.to_string(),
        )
    })?;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if name == "SKILL.md" {
            continue;
        }
        if resources.len() >= config.max_resources_per_skill {
            diagnostics.push(Diagnostic::warning(
                format!(
                    "resource inventory truncated at {} items",
                    config.max_resources_per_skill
                ),
                location.to_string(),
            ));
            break;
        }
        let meta = std::fs::metadata(&path).ok();
        let size = meta.map(|m| m.len()).unwrap_or(0);
        let relative = path
            .strip_prefix(package_root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();
        resources.push(ResourceDescriptor {
            name,
            relative_path: relative,
            size,
        });
    }

    resources.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(resources)
}

pub fn compute_digest(frontmatter: &str, body: &str) -> String {
    let normalized_body = body.replace("\r\n", "\n");
    let mut hasher = Sha256::new();
    hasher.update(frontmatter.as_bytes());
    hasher.update(b"\n");
    hasher.update(normalized_body.as_bytes());
    let result = hasher.finalize();
    hex::encode(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn test_config() -> AssetDiscoveryConfig {
        AssetDiscoveryConfig::default()
    }

    #[test]
    fn parse_frontmatter_valid() {
        let content = "---\nname: test\ndescription: A test skill\n---\nBody content";
        let (fm, body) = parse_frontmatter(content).unwrap();
        assert!(fm.contains("name: test"));
        assert_eq!(body.trim(), "Body content");
    }

    #[test]
    fn parse_frontmatter_missing() {
        assert!(parse_frontmatter("no frontmatter here").is_none());
    }

    #[test]
    fn compute_digest_stability() {
        let fm = "name: test\ndescription: desc";
        let body = "Hello world\n";
        let d1 = compute_digest(fm, body);
        let d2 = compute_digest(fm, body);
        assert_eq!(d1, d2);
    }

    #[test]
    fn compute_digest_crlf_normalization() {
        let fm = "name: test\ndescription: desc";
        let body_lf = "Hello world\n";
        let body_crlf = "Hello world\r\n";
        let d1 = compute_digest(fm, body_lf);
        let d2 = compute_digest(fm, body_crlf);
        assert_eq!(d1, d2);
    }

    #[test]
    fn normalize_name_rejects_empty() {
        let config = test_config();
        assert!(normalize_name("", &config).is_err());
        assert!(normalize_name("   ", &config).is_err());
    }

    #[test]
    fn normalize_name_rejects_path_separators() {
        let config = test_config();
        assert!(normalize_name("a/b", &config).is_err());
        assert!(normalize_name("a\\b", &config).is_err());
    }

    #[test]
    fn normalize_name_rejects_control_chars() {
        let config = test_config();
        assert!(normalize_name("a\x00b", &config).is_err());
        assert!(normalize_name("a\nb", &config).is_err());
    }

    #[test]
    fn normalize_name_lowercases() {
        let config = test_config();
        assert_eq!(normalize_name("MySkill", &config).unwrap(), "myskill");
        assert_eq!(normalize_name("  MySkill  ", &config).unwrap(), "myskill");
    }

    #[test]
    fn parse_candidate_native_compat() {
        let dir = TempDir::new().unwrap();
        let skill_file = dir.path().join("test.md");
        fs::write(
            &skill_file,
            "---\nname: native-skill\nversion: 1.0.0\ntags: [test]\n---\nBody here",
        )
        .unwrap();

        let config = test_config();
        let candidate =
            parse_candidate(&skill_file, SourceKind::CodeGGNativeCompat, &config).unwrap();
        assert_eq!(candidate.name, "native-skill");
        assert_eq!(candidate.normalized_name, "native-skill");
        assert!(candidate.metadata.contains_key("version"));
        assert!(candidate.metadata.contains_key("tags"));
    }

    #[test]
    fn parse_candidate_portable() {
        let dir = TempDir::new().unwrap();
        let skill_file = dir.path().join("SKILL.md");
        fs::write(
            &skill_file,
            "---\nname: portable-skill\ndescription: A portable skill\nlicense: MIT\n---\nBody",
        )
        .unwrap();

        let config = test_config();
        let candidate = parse_candidate(&skill_file, SourceKind::AgentsProject, &config).unwrap();
        assert_eq!(candidate.name, "portable-skill");
        assert!(candidate.metadata.contains_key("license"));
    }

    #[test]
    fn parse_candidate_missing_name_error() {
        let dir = TempDir::new().unwrap();
        let skill_file = dir.path().join("SKILL.md");
        fs::write(&skill_file, "---\ndescription: No name field\n---\nBody").unwrap();

        let config = test_config();
        let result = parse_candidate(&skill_file, SourceKind::AgentsProject, &config);
        assert!(result.is_err());
    }

    #[test]
    fn parse_candidate_missing_description_error() {
        let dir = TempDir::new().unwrap();
        let skill_file = dir.path().join("SKILL.md");
        fs::write(&skill_file, "---\nname: skill-no-desc\n---\nBody").unwrap();

        let config = test_config();
        let result = parse_candidate(&skill_file, SourceKind::AgentsProject, &config);
        assert!(result.is_err());
    }

    #[test]
    fn parse_candidate_oversized_file() {
        let dir = TempDir::new().unwrap();
        let skill_file = dir.path().join("SKILL.md");
        let content = format!(
            "---\nname: big\ndescription: big\n---\n{}",
            "x".repeat(300_000)
        );
        fs::write(&skill_file, content).unwrap();

        let config = test_config();
        let result = parse_candidate(&skill_file, SourceKind::AgentsProject, &config);
        assert!(result.is_err());
    }

    #[test]
    fn parse_candidate_malformed_yaml() {
        let dir = TempDir::new().unwrap();
        let skill_file = dir.path().join("SKILL.md");
        fs::write(&skill_file, "---\nname: [{bad yaml\n---\nBody").unwrap();

        let config = test_config();
        let result = parse_candidate(&skill_file, SourceKind::AgentsProject, &config);
        assert!(result.is_err());
    }

    #[test]
    fn parse_candidate_resources_inventoried() {
        let dir = TempDir::new().unwrap();
        let skill_dir = dir.path().join("myskill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: rsrc\ndescription: with resources\n---\nBody",
        )
        .unwrap();
        fs::write(skill_dir.join("helper.sh"), "#!/bin/bash\necho hi").unwrap();
        fs::write(skill_dir.join("data.txt"), "some data").unwrap();

        let config = test_config();
        let candidate = parse_candidate(
            &skill_dir.join("SKILL.md"),
            SourceKind::AgentsProject,
            &config,
        )
        .unwrap();
        assert_eq!(candidate.resources.len(), 2);
        assert!(candidate.resources.iter().any(|r| r.name == "helper.sh"));
        assert!(candidate.resources.iter().any(|r| r.name == "data.txt"));
    }

    #[test]
    fn parse_candidate_allowed_tools_preserved_as_metadata() {
        let dir = TempDir::new().unwrap();
        let skill_file = dir.path().join("SKILL.md");
        fs::write(
            &skill_file,
            "---\nname: tool-user\ndescription: uses tools\nallowed-tools:\n  - bash\n  - read\n---\nBody",
        )
        .unwrap();

        let config = test_config();
        let candidate = parse_candidate(&skill_file, SourceKind::AgentsProject, &config).unwrap();
        assert!(candidate.metadata.contains_key("allowed-tools"));
        assert!(
            !candidate.diagnostics.is_empty(),
            "should warn about allowed-tools"
        );
    }
}
