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
    pub metadata: HashMap<String, serde_json::Value>,
    #[serde(flatten)]
    pub additional: HashMap<String, serde_json::Value>,
    #[serde(rename = "allowed-tools")]
    pub allowed_tools: Option<serde_json::Value>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct NativeFrontmatter {
    pub name: Option<String>,
    pub description: Option<String>,
    pub version: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// The bounded, in-memory result of parsing one portable `SKILL.md`.
/// Proposal validation uses this same result as ordinary discovery, without
/// creating a temporary file or enumerating package resources.
#[derive(Debug, Clone)]
pub struct ValidatedSkillDocument {
    pub name: String,
    pub normalized_name: String,
    pub description: String,
    pub frontmatter_raw: String,
    pub body: String,
    pub metadata: HashMap<String, serde_json::Value>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct RawFrontmatter {
    #[serde(flatten)]
    inner: HashMap<String, serde_json::Value>,
}

pub fn parse_candidate(
    skill_file: &Path,
    source_kind: SourceKind,
    config: &AssetDiscoveryConfig,
) -> Result<SkillCandidate, Diagnostic> {
    let location = skill_file.display().to_string();

    // Sync I/O is intentional: startup-only skill discovery scan.
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

    let portable = has_portable_fields(&frontmatter_str);
    let (name, description, metadata) = match source_kind {
        SourceKind::CodeGGNativeCompat | SourceKind::CodeGGProject
            if !has_portable_fields(&frontmatter_str) =>
        {
            let fm: NativeFrontmatter =
                codegg_config::parse_yaml(location.clone(), frontmatter_str.as_bytes()).map_err(
                    |e| {
                        Diagnostic::error(
                            format!("failed to parse frontmatter: {e}"),
                            location.clone(),
                        )
                    },
                )?;
            let name = fm.name.unwrap_or_else(|| {
                skill_file
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default()
            });
            let description = fm.description.unwrap_or_default();
            let mut meta = HashMap::new();
            if let Some(v) = fm.version {
                meta.insert("version".to_string(), serde_json::Value::String(v));
            }
            if !fm.tags.is_empty() {
                meta.insert(
                    "tags".to_string(),
                    serde_json::Value::Array(
                        fm.tags.into_iter().map(serde_json::Value::String).collect(),
                    ),
                );
            }
            (name, description, meta)
        }
        _ => {
            let parsed = parse_portable_document(
                &raw_content,
                config,
                source_kind != SourceKind::CodeGGProject
                    && source_kind != SourceKind::CodeGGNativeCompat,
                matches!(
                    source_kind,
                    SourceKind::ClaudeProject | SourceKind::ClaudeGlobal
                )
                .then(|| skill_file.parent().and_then(Path::file_name))
                .flatten()
                .and_then(|name| name.to_str()),
            )?;
            diagnostics.extend(parsed.diagnostics.clone());
            (parsed.name, parsed.description, parsed.metadata)
        }
    };

    if portable
        && source_kind != SourceKind::CodeGGNativeCompat
        && source_kind != SourceKind::CodeGGProject
        && skill_file
            .parent()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
            .is_some_and(|directory| directory != name)
    {
        return Err(Diagnostic::error(
            "portable skill directory name must match its frontmatter name",
            location.clone(),
        ));
    }

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
        workspace_depth: 0,
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

/// Parse and validate a portable skill document in memory.
///
/// This is the single portable frontmatter/body validation seam shared by
/// filesystem discovery and user-triggered proposals. It deliberately does
/// not inspect a package directory or perform any filesystem write.
pub fn validate_portable_document(
    source: &str,
    config: &AssetDiscoveryConfig,
) -> Result<ValidatedSkillDocument, Diagnostic> {
    parse_portable_document(source, config, true, None)
}

fn parse_portable_document(
    source: &str,
    config: &AssetDiscoveryConfig,
    enforce_portable_name: bool,
    fallback_name: Option<&str>,
) -> Result<ValidatedSkillDocument, Diagnostic> {
    let location = "skill proposal".to_string();
    if source.len() as u64 > config.max_skill_file_size {
        return Err(Diagnostic::error(
            format!(
                "skill file exceeds maximum size ({} bytes)",
                config.max_skill_file_size
            ),
            location.clone(),
        ));
    }
    let (frontmatter_raw, body) = parse_frontmatter(source).ok_or_else(|| {
        Diagnostic::error(
            "missing or malformed YAML frontmatter (expected --- delimiters)",
            location.clone(),
        )
    })?;
    if frontmatter_raw.len() > config.max_frontmatter_size {
        return Err(Diagnostic::error(
            format!(
                "frontmatter exceeds maximum size ({})",
                config.max_frontmatter_size
            ),
            location.clone(),
        ));
    }
    let fm: PortableFrontmatter =
        codegg_config::parse_yaml(location.clone(), frontmatter_raw.as_bytes()).map_err(|e| {
            Diagnostic::error(
                format!("failed to parse frontmatter: {e}"),
                location.clone(),
            )
        })?;
    let name = fm
        .name
        .or_else(|| fallback_name.map(str::to_string))
        .ok_or_else(|| Diagnostic::error("missing required field: name", location.clone()))?;
    let description = fm.description.ok_or_else(|| {
        Diagnostic::error("missing required field: description", location.clone())
    })?;
    if enforce_portable_name {
        validate_portable_name(&name)?;
    }
    let normalized_name = normalize_name(&name, config)?;
    let mut metadata = fm.metadata;
    for (key, value) in fm.additional {
        metadata.entry(key).or_insert(value);
    }
    let mut diagnostics = Vec::new();
    if let Some(license) = fm.license {
        metadata.insert("license".to_string(), serde_json::Value::String(license));
    }
    if let Some(compatibility) = fm.compatibility {
        metadata.insert(
            "compatibility".to_string(),
            serde_json::Value::String(compatibility),
        );
    }
    if let Some(allowed_tools) = fm.allowed_tools {
        metadata.insert("allowed-tools".to_string(), allowed_tools);
        diagnostics.push(Diagnostic::warning(
            "allowed-tools is preserved as metadata only; it does not grant permissions",
            location.clone(),
        ));
    }
    if description.len() > config.max_description_length {
        diagnostics.push(Diagnostic::warning(
            format!(
                "description exceeds recommended maximum ({} chars)",
                config.max_description_length
            ),
            location,
        ));
    }
    Ok(ValidatedSkillDocument {
        name,
        normalized_name,
        description,
        frontmatter_raw,
        body,
        metadata,
        diagnostics,
    })
}

fn has_portable_fields(frontmatter: &str) -> bool {
    if let Ok(raw) =
        codegg_config::parse_yaml::<RawFrontmatter>("skill frontmatter", frontmatter.as_bytes())
    {
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
    // The portable Agent Skills name subset is intentionally narrower than
    // CodeGG's historical native names. Apply it only to portable documents;
    // native compatibility packages retain their established syntax.
    Ok(trimmed.to_lowercase())
}

fn validate_portable_name(name: &str) -> Result<(), Diagnostic> {
    let valid = !name.is_empty()
        && name.len() <= 64
        && name.as_bytes()[0].is_ascii_lowercase()
        && (name.as_bytes()[name.len() - 1].is_ascii_lowercase()
            || name.as_bytes()[name.len() - 1].is_ascii_digit())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !name.contains("--");
    if valid {
        Ok(())
    } else {
        Err(Diagnostic::error(
            "portable skill name must be lowercase letters/digits with single hyphens, start with a letter, end with a letter or digit, and be at most 64 characters",
            "frontmatter",
        ))
    }
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

    let root = match package_root.canonicalize() {
        Ok(root) => root,
        Err(error) => {
            diagnostics.push(Diagnostic::warning(
                format!("resource inventory unavailable: {error}"),
                location.to_string(),
            ));
            return Ok(resources);
        }
    };
    let mut pending = vec![(root.clone(), 0usize)];
    let mut truncated = false;
    let mut depth_limited = false;
    while let Some((directory, depth)) = pending.pop() {
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                diagnostics.push(Diagnostic::warning(
                    format!("resource directory could not be read: {error}"),
                    location.to_string(),
                ));
                continue;
            }
        };
        let mut paths = Vec::new();
        for entry in entries {
            match entry {
                Ok(entry) => paths.push(entry.path()),
                Err(error) => diagnostics.push(Diagnostic::warning(
                    format!("resource entry could not be read: {error}"),
                    location.to_string(),
                )),
            }
        }
        paths.sort();
        for path in paths {
            let file_type = match std::fs::symlink_metadata(&path) {
                Ok(meta) => meta.file_type(),
                Err(_) => continue,
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                if depth < 7 {
                    pending.push((path, depth + 1));
                } else {
                    depth_limited = true;
                }
                continue;
            }
            if !file_type.is_file() || path.file_name().is_some_and(|n| n == "SKILL.md") {
                continue;
            }
            if resources.len() >= config.max_resources_per_skill {
                truncated = true;
                break;
            }
            let canonical = match path.canonicalize() {
                Ok(path) if path.starts_with(&root) => path,
                _ => continue,
            };
            let size = std::fs::metadata(&canonical).map(|m| m.len()).unwrap_or(0);
            let relative_path = canonical
                .strip_prefix(&root)
                .unwrap_or(&canonical)
                .to_string_lossy()
                .to_string();
            let name = relative_path.clone();
            resources.push(ResourceDescriptor {
                name,
                relative_path,
                size,
            });
        }
        if truncated {
            break;
        }
    }
    if truncated {
        diagnostics.push(Diagnostic::warning(
            format!(
                "resource inventory truncated at {} items",
                config.max_resources_per_skill
            ),
            location.to_string(),
        ));
    }
    if depth_limited {
        diagnostics.push(Diagnostic::warning(
            "resource inventory skipped directories beyond depth 8",
            location.to_string(),
        ));
    }
    resources.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
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
    fn portable_names_follow_portable_syntax_without_changing_native_names() {
        for invalid in [
            "Upper",
            "-leading",
            "trailing-",
            "two--hyphens",
            "has space",
        ] {
            assert!(validate_portable_name(invalid).is_err(), "{invalid}");
        }
        assert!(validate_portable_name("valid-123").is_ok());
        assert!(normalize_name("Native_Name", &test_config()).is_ok());
    }

    #[test]
    fn native_codegg_package_keeps_legacy_name_compatibility() {
        let dir = TempDir::new().unwrap();
        let package = dir.path().join("native-folder");
        fs::create_dir_all(&package).unwrap();
        let skill_file = package.join("SKILL.md");
        fs::write(
            &skill_file,
            "---\nname: Native_Name\ndescription: native compatibility\n---\nBody",
        )
        .unwrap();
        let candidate =
            parse_candidate(&skill_file, SourceKind::CodeGGProject, &test_config()).unwrap();
        assert_eq!(candidate.name, "Native_Name");
    }

    #[test]
    fn direct_markdown_compat_keeps_legacy_portable_shaped_names() {
        let dir = TempDir::new().unwrap();
        let skill_file = dir.path().join("Native_Name.md");
        fs::write(
            &skill_file,
            "---\nname: Native_Name\ndescription: native markdown compatibility\n---\nBody",
        )
        .unwrap();
        let candidate =
            parse_candidate(&skill_file, SourceKind::CodeGGNativeCompat, &test_config()).unwrap();
        assert_eq!(candidate.name, "Native_Name");
    }

    #[test]
    fn portable_package_directory_must_match_its_name() {
        let dir = TempDir::new().unwrap();
        let package = dir.path().join("wrong-folder");
        fs::create_dir_all(&package).unwrap();
        let skill_file = package.join("SKILL.md");
        fs::write(
            &skill_file,
            "---\nname: correct-name\ndescription: portable fixture\n---\nBody",
        )
        .unwrap();
        assert!(parse_candidate(&skill_file, SourceKind::AgentsProject, &test_config()).is_err());
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
