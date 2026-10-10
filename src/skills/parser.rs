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
    #[serde(rename = "allowed-tools")]
    pub allowed_tools: Option<serde_json::Value>,
    #[serde(flatten)]
    pub extension_fields: HashMap<String, serde_json::Value>,
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
            let portable_input = if source_kind == SourceKind::ClaudeProject
                || source_kind == SourceKind::ClaudeGlobal
            {
                claude_name_fallback(&raw_content, skill_file)
                    .unwrap_or_else(|| raw_content.clone())
            } else {
                raw_content.clone()
            };
            let parsed = validate_portable_document_inner(
                &portable_input,
                config,
                matches!(
                    source_kind,
                    SourceKind::CodeGGProject | SourceKind::CodeGGNativeCompat
                ),
            )?;
            diagnostics.extend(parsed.diagnostics.clone());
            (parsed.name, parsed.description, parsed.metadata)
        }
    };

    let normalized_name = normalize_name(&name, config)?;
    if source_kind.is_foreign() {
        let directory_name = skill_file
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if directory_name.to_lowercase() != normalized_name {
            return Err(Diagnostic::error(
                "portable skill name must match its package directory",
                location,
            ));
        }
    }

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

    let content_digest = compute_package_digest(&frontmatter_str, &body, &resources);

    Ok(SkillCandidate {
        name,
        normalized_name,
        description,
        source_kind,
        precedence_rank: source_kind.precedence_rank(),
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

fn claude_name_fallback(source: &str, skill_file: &Path) -> Option<String> {
    let (frontmatter, body) = parse_frontmatter(source)?;
    let raw: RawFrontmatter =
        codegg_config::parse_yaml("skill frontmatter", frontmatter.as_bytes()).ok()?;
    if raw.inner.contains_key("name") || !raw.inner.contains_key("description") {
        return None;
    }
    let folder = skill_file.parent()?.file_name()?.to_string_lossy();
    let quoted = serde_json::to_string(folder.as_ref()).ok()?;
    Some(format!("---\nname: {quoted}\n{frontmatter}\n---{body}"))
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
    validate_portable_document_inner(source, config, false)
}

fn validate_portable_document_inner(
    source: &str,
    config: &AssetDiscoveryConfig,
    allow_empty_description: bool,
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
        .ok_or_else(|| Diagnostic::error("missing required field: name", location.clone()))?;
    validate_portable_name(&name).map_err(|reason| Diagnostic::error(reason, location.clone()))?;
    let description = fm.description.ok_or_else(|| {
        Diagnostic::error("missing required field: description", location.clone())
    })?;
    if !allow_empty_description && description.trim().is_empty() {
        return Err(Diagnostic::error(
            "portable skill description must not be empty",
            location.clone(),
        ));
    }
    let normalized_name = normalize_name(&name, config)?;
    let mut metadata = fm.metadata;
    let mut diagnostics = Vec::new();
    let mut extension_fields = fm.extension_fields.into_iter().collect::<Vec<_>>();
    extension_fields.sort_by(|a, b| a.0.cmp(&b.0));
    for (key, value) in extension_fields.into_iter().take(32) {
        diagnostics.push(Diagnostic::warning(
            format!("unrecognized vendor metadata '{key}' is retained as inert data"),
            location.clone(),
        ));
        metadata.insert(key, value);
    }
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

fn validate_portable_name(name: &str) -> Result<(), &'static str> {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 {
        return Err("portable skill name must be 1 to 64 characters");
    }
    if !bytes[0].is_ascii_lowercase() && !bytes[0].is_ascii_digit() {
        return Err("portable skill name must start with a lowercase letter or digit");
    }
    if !bytes[bytes.len() - 1].is_ascii_lowercase() && !bytes[bytes.len() - 1].is_ascii_digit() {
        return Err("portable skill name must end with a lowercase letter or digit");
    }
    if bytes
        .iter()
        .any(|b| !b.is_ascii_lowercase() && !b.is_ascii_digit() && *b != b'-')
    {
        return Err("portable skill name may contain only lowercase letters, digits, and hyphens");
    }
    if name.contains("--") {
        return Err("portable skill name must not contain consecutive hyphens");
    }
    Ok(())
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

    let mut pending = vec![(package_root.to_path_buf(), 0usize)];
    let mut visited_entries = 0usize;
    const MAX_DEPTH: usize = 8;
    const MAX_ENTRIES: usize = 1024;
    while let Some((directory, depth)) = pending.pop() {
        let entries = std::fs::read_dir(&directory).map_err(|e| {
            Diagnostic::warning(
                format!("failed to read resource directory: {e}"),
                location.to_string(),
            )
        })?;
        let mut entries = entries.filter_map(Result::ok).collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            visited_entries += 1;
            if visited_entries > MAX_ENTRIES || resources.len() >= config.max_resources_per_skill {
                diagnostics.push(Diagnostic::warning(
                    "resource inventory reached its entry limit",
                    location.to_string(),
                ));
                pending.clear();
                break;
            }
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                if depth >= MAX_DEPTH {
                    diagnostics.push(Diagnostic::warning(
                        "resource inventory depth limit reached",
                        location.to_string(),
                    ));
                } else {
                    pending.push((path, depth + 1));
                }
                continue;
            }
            if !file_type.is_file() || path.file_name().is_some_and(|name| name == "SKILL.md") {
                continue;
            }
            let name = match path.strip_prefix(package_root).ok().and_then(Path::to_str) {
                Some(n) => n.replace('\\', "/"),
                None => continue,
            };
            let metadata = entry.metadata().ok();
            let size = metadata.as_ref().map(|meta| meta.len()).unwrap_or(0);
            let modified_unix_nanos = metadata
                .and_then(|meta| meta.modified().ok())
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_nanos())
                .unwrap_or(0);
            resources.push(ResourceDescriptor {
                name,
                relative_path: path
                    .strip_prefix(package_root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/"),
                size,
                modified_unix_nanos,
            });
        }
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

fn compute_package_digest(
    frontmatter: &str,
    body: &str,
    resources: &[ResourceDescriptor],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(compute_digest(frontmatter, body).as_bytes());
    for resource in resources {
        hasher.update(resource.relative_path.as_bytes());
        hasher.update([0]);
        hasher.update(resource.size.to_le_bytes());
        hasher.update(resource.modified_unix_nanos.to_le_bytes());
    }
    hex::encode(hasher.finalize())
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
    fn package_digest_tracks_resource_inventory_changes() {
        let one = vec![ResourceDescriptor {
            name: "a.md".into(),
            relative_path: "references/a.md".into(),
            size: 10,
            modified_unix_nanos: 1,
        }];
        let changed = vec![ResourceDescriptor {
            name: "a.md".into(),
            relative_path: "references/a.md".into(),
            size: 10,
            modified_unix_nanos: 2,
        }];
        assert_ne!(
            compute_package_digest("name: x", "Body", &one),
            compute_package_digest("name: x", "Body", &changed)
        );
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
        let skill_dir = dir.path().join("portable-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        let skill_file = skill_dir.join("SKILL.md");
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
    fn portable_validation_rejects_nonconforming_names_but_native_remains_compatible() {
        let config = test_config();
        for name in ["Upper", "bad_name", "-edge", "double--dash", "a/../b"] {
            let source = format!("---\nname: {name:?}\ndescription: portable\n---\nBody");
            assert!(
                validate_portable_document(&source, &config).is_err(),
                "{name}"
            );
        }
        let source = "---\nname: Upper_Native\nversion: 1\n---\nBody";
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("SKILL.md");
        fs::write(&path, source).unwrap();
        assert!(parse_candidate(&path, SourceKind::CodeGGNativeCompat, &config).is_ok());
    }

    #[test]
    fn portable_description_must_not_be_empty_but_codegg_remains_compatible() {
        let config = test_config();
        assert!(validate_portable_document(
            "---\nname: example\ndescription: '  '\n---\nBody",
            &config
        )
        .is_err());

        let dir = TempDir::new().unwrap();
        let skill_dir = dir.path().join("legacy");
        fs::create_dir_all(&skill_dir).unwrap();
        let skill_file = skill_dir.join("SKILL.md");
        fs::write(
            &skill_file,
            "---\nname: legacy\ndescription: ''\nversion: 1\n---\nBody",
        )
        .unwrap();
        let candidate = parse_candidate(&skill_file, SourceKind::CodeGGProject, &config).unwrap();
        assert!(candidate.description.is_empty());
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
        let skill_dir = dir.path().join("rsrc");
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
    fn nested_resources_are_inventoried_and_vendor_hints_stay_data() {
        let dir = TempDir::new().unwrap();
        let skill_dir = dir.path().join("nested");
        fs::create_dir_all(skill_dir.join("references")).unwrap();
        fs::create_dir_all(skill_dir.join("scripts")).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: nested\ndescription: Nested\ncustom-mode: execute\n---\nBody",
        )
        .unwrap();
        fs::write(skill_dir.join("references/API.md"), "Reference").unwrap();
        fs::write(skill_dir.join("scripts/run.sh"), "echo inert").unwrap();
        let candidate = parse_candidate(
            &skill_dir.join("SKILL.md"),
            SourceKind::AgentsProject,
            &test_config(),
        )
        .unwrap();
        assert!(candidate
            .resources
            .iter()
            .any(|resource| resource.relative_path == "references/API.md"));
        assert!(candidate
            .resources
            .iter()
            .any(|resource| resource.relative_path == "scripts/run.sh"));
        assert_eq!(
            candidate
                .metadata
                .get("custom-mode")
                .and_then(serde_json::Value::as_str),
            Some("execute")
        );
        assert!(candidate.diagnostics.iter().any(|diagnostic| {
            diagnostic.reason.contains("custom-mode") && diagnostic.reason.contains("inert")
        }));
    }

    #[test]
    fn claude_may_derive_name_from_its_package_directory() {
        let dir = TempDir::new().unwrap();
        let skill_dir = dir.path().join("claude-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        let path = skill_dir.join("SKILL.md");
        fs::write(&path, "---\ndescription: Claude skill\n---\nBody").unwrap();
        let candidate = parse_candidate(&path, SourceKind::ClaudeProject, &test_config()).unwrap();
        assert_eq!(candidate.name, "claude-skill");
        assert!(parse_candidate(&path, SourceKind::AgentsProject, &test_config()).is_err());
    }

    #[test]
    fn parse_candidate_allowed_tools_preserved_as_metadata() {
        let dir = TempDir::new().unwrap();
        let skill_dir = dir.path().join("tool-user");
        fs::create_dir_all(&skill_dir).unwrap();
        let skill_file = skill_dir.join("SKILL.md");
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
