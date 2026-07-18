//! Inert DTOs for exchanging a bounded runtime-asset manifest.
//!
//! These types describe asset metadata only.  In particular, they do not
//! carry paths, executable commands, permissions, or transport state.  A
//! future conversion layer may map them to local runtime-asset types after it
//! has applied its own policy.

use std::cmp::Ordering;
use std::fmt;

use serde::{Deserialize, Serialize};

pub const RUNTIME_ASSET_MANIFEST_SCHEMA_VERSION: u16 = 1;
pub const MAX_RUNTIME_ASSET_MANIFEST_ASSETS: usize = 256;
pub const MAX_RUNTIME_ASSET_MANIFEST_DIAGNOSTICS: usize = 64;
pub const MAX_RUNTIME_ASSET_MANIFEST_STRING_LENGTH: usize = 256;
pub const MAX_RUNTIME_ASSET_MANIFEST_DIGEST_LENGTH: usize = 128;
pub const MAX_RUNTIME_ASSET_SIZE_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_RUNTIME_ASSET_MANIFEST_TOTAL_SIZE_BYTES: u64 = 1024 * 1024 * 1024;
pub const MAX_RUNTIME_ASSET_MANIFEST_SERIALIZED_BYTES: usize = 256 * 1024;

/// The project/workspace scope to which a manifest belongs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeAssetManifestScopeDto {
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub workspace_id: String,
}

/// Stable identity and generation metadata for one manifest publication.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeAssetManifestIdentityDto {
    #[serde(default)]
    pub manifest_id: String,
    #[serde(default)]
    pub generation: u64,
    #[serde(default)]
    pub fingerprint: Option<String>,
}

/// A remote workspace runtime-asset manifest containing metadata, not asset
/// bodies or instructions for executing them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeAssetManifestDto {
    #[serde(default = "default_manifest_schema_version")]
    pub schema_version: u16,
    #[serde(default)]
    pub identity: RuntimeAssetManifestIdentityDto,
    #[serde(default)]
    pub scope: RuntimeAssetManifestScopeDto,
    #[serde(default)]
    pub assets: Vec<RuntimeAssetEntryDto>,
    #[serde(default)]
    pub diagnostics: Vec<RuntimeAssetCompatibilityDiagnosticDto>,
}

impl Default for RuntimeAssetManifestDto {
    fn default() -> Self {
        Self {
            schema_version: RUNTIME_ASSET_MANIFEST_SCHEMA_VERSION,
            identity: RuntimeAssetManifestIdentityDto::default(),
            scope: RuntimeAssetManifestScopeDto::default(),
            assets: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}

impl RuntimeAssetManifestDto {
    /// Validate and deterministically normalize a manifest.
    ///
    /// Invalid identity and scope fields reject the manifest.  Invalid asset
    /// entries are omitted with a bounded diagnostic, while oversized asset
    /// and diagnostic collections are truncated with diagnostics.  The
    /// returned manifest is safe to serialize within the published caps.
    pub fn validate_and_normalize(mut self) -> Result<Self, RuntimeAssetManifestValidationError> {
        validate_manifest_identifier("identity.manifest_id", &self.identity.manifest_id)?;
        validate_manifest_identifier("scope.project_id", &self.scope.project_id)?;
        validate_manifest_identifier("scope.workspace_id", &self.scope.workspace_id)?;
        if self.schema_version == 0 {
            return Err(RuntimeAssetManifestValidationError::InvalidField {
                field: "schema_version".into(),
                reason: "must be greater than zero".into(),
            });
        }
        if let Some(fingerprint) = self.identity.fingerprint.as_mut() {
            normalize_bounded_opaque(fingerprint, MAX_RUNTIME_ASSET_MANIFEST_DIGEST_LENGTH)
                .map_err(|reason| RuntimeAssetManifestValidationError::InvalidField {
                    field: "identity.fingerprint".into(),
                    reason,
                })?;
        }

        let mut generated_diagnostics = Vec::new();
        if self.schema_version > RUNTIME_ASSET_MANIFEST_SCHEMA_VERSION {
            generated_diagnostics.push(RuntimeAssetCompatibilityDiagnosticDto::warning(
                RuntimeAssetDiagnosticCodeDto::UnsupportedSchema,
                format!(
                    "schema version {} is newer than supported version {}",
                    self.schema_version, RUNTIME_ASSET_MANIFEST_SCHEMA_VERSION
                ),
                None,
            ));
        }
        let mut assets =
            Vec::with_capacity(self.assets.len().min(MAX_RUNTIME_ASSET_MANIFEST_ASSETS));
        for asset in self.assets {
            match normalize_asset(asset) {
                Ok(asset) => assets.push(asset),
                Err(reason) => {
                    generated_diagnostics.push(RuntimeAssetCompatibilityDiagnosticDto::error(
                        RuntimeAssetDiagnosticCodeDto::InvalidAsset,
                        reason,
                        None,
                    ))
                }
            }
        }

        assets.sort_by(asset_ordering);
        let mut unique_assets = Vec::with_capacity(assets.len());
        for asset in assets {
            if unique_assets
                .last()
                .is_some_and(|previous: &RuntimeAssetEntryDto| {
                    previous.kind == asset.kind && previous.name == asset.name
                })
            {
                generated_diagnostics.push(RuntimeAssetCompatibilityDiagnosticDto::warning(
                    RuntimeAssetDiagnosticCodeDto::DuplicateAsset,
                    format!("duplicate asset '{}' was omitted", asset.name),
                    Some(asset.name),
                ));
            } else {
                unique_assets.push(asset);
            }
        }

        let original_asset_count = unique_assets.len();
        if original_asset_count > MAX_RUNTIME_ASSET_MANIFEST_ASSETS {
            unique_assets.truncate(MAX_RUNTIME_ASSET_MANIFEST_ASSETS);
            generated_diagnostics.push(RuntimeAssetCompatibilityDiagnosticDto::warning(
                RuntimeAssetDiagnosticCodeDto::AssetsTruncated,
                format!(
                    "asset list truncated to {} entries",
                    MAX_RUNTIME_ASSET_MANIFEST_ASSETS
                ),
                None,
            ));
        }

        let mut bounded_size_bytes = 0u64;
        let mut bounded_assets = Vec::with_capacity(unique_assets.len());
        let mut size_was_truncated = false;
        for asset in unique_assets {
            let next_size = bounded_size_bytes.saturating_add(asset.size_bytes);
            if next_size > MAX_RUNTIME_ASSET_MANIFEST_TOTAL_SIZE_BYTES {
                size_was_truncated = true;
            } else {
                bounded_size_bytes = next_size;
                bounded_assets.push(asset);
            }
        }
        if size_was_truncated {
            generated_diagnostics.push(RuntimeAssetCompatibilityDiagnosticDto::warning(
                RuntimeAssetDiagnosticCodeDto::ManifestSizeExceeded,
                format!(
                    "asset list truncated at {} declared bytes",
                    MAX_RUNTIME_ASSET_MANIFEST_TOTAL_SIZE_BYTES
                ),
                None,
            ));
        }

        self.assets = bounded_assets;
        self.diagnostics.extend(generated_diagnostics);
        self.diagnostics = normalize_diagnostics(std::mem::take(&mut self.diagnostics));

        if self.serialized_size()? > MAX_RUNTIME_ASSET_MANIFEST_SERIALIZED_BYTES {
            return Err(
                RuntimeAssetManifestValidationError::SerializedSizeExceeded {
                    limit: MAX_RUNTIME_ASSET_MANIFEST_SERIALIZED_BYTES,
                },
            );
        }
        Ok(self)
    }

    /// Alias emphasizing that this method is the boundary for untrusted
    /// manifest data.
    pub fn normalize(self) -> Result<Self, RuntimeAssetManifestValidationError> {
        self.validate_and_normalize()
    }

    pub fn serialized_size(&self) -> Result<usize, RuntimeAssetManifestValidationError> {
        serde_json::to_vec(self)
            .map(|bytes| bytes.len())
            .map_err(|error| RuntimeAssetManifestValidationError::Serialization(error.to_string()))
    }
}

/// The semantic category of an asset.  Unknown is retained for additive
/// compatibility and has no execution meaning.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeAssetKindDto {
    Agent,
    Skill,
    ProjectInstruction,
    PromptFragment,
    Resource,
    #[default]
    Unknown,
}

/// Metadata identifying where an asset came from without exposing a local
/// path or granting any authority.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeAssetProvenanceDto {
    #[serde(default)]
    pub source_kind: RuntimeAssetSourceKindDto,
    #[serde(default)]
    pub precedence: Option<u32>,
    #[serde(default)]
    pub source_revision: Option<String>,
}

/// Source namespace used for provenance.  The names intentionally describe
/// a harness/source kind rather than a filesystem location.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeAssetSourceKindDto {
    CodeggProject,
    AgentsProject,
    OpencodeProject,
    ClaudeProject,
    CodeggGlobal,
    AgentsGlobal,
    OpencodeGlobal,
    ClaudeGlobal,
    CodeggNativeCompatibility,
    Remote,
    #[default]
    Unknown,
}

/// A bounded metadata entry in a runtime-asset manifest.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeAssetEntryDto {
    #[serde(default)]
    pub kind: RuntimeAssetKindDto,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub digest: String,
    #[serde(default, alias = "size")]
    pub size_bytes: u64,
    #[serde(default)]
    pub provenance: RuntimeAssetProvenanceDto,
}

/// Severity of a compatibility or normalization diagnostic.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeAssetDiagnosticSeverityDto {
    Info,
    #[default]
    Warning,
    Error,
}

/// Stable machine-readable reasons for compatibility diagnostics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeAssetDiagnosticCodeDto {
    InvalidAsset,
    DuplicateAsset,
    AssetsTruncated,
    DiagnosticsTruncated,
    ManifestSizeExceeded,
    UnsupportedSchema,
    UnsupportedAssetKind,
    UnsupportedSourceKind,
    #[default]
    Other,
}

/// A bounded, data-only compatibility diagnostic.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeAssetCompatibilityDiagnosticDto {
    #[serde(default)]
    pub severity: RuntimeAssetDiagnosticSeverityDto,
    #[serde(default)]
    pub code: RuntimeAssetDiagnosticCodeDto,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub asset_name: Option<String>,
    #[serde(default)]
    pub source_kind: Option<RuntimeAssetSourceKindDto>,
}

impl RuntimeAssetCompatibilityDiagnosticDto {
    fn warning(
        code: RuntimeAssetDiagnosticCodeDto,
        message: impl Into<String>,
        asset_name: Option<String>,
    ) -> Self {
        Self {
            severity: RuntimeAssetDiagnosticSeverityDto::Warning,
            code,
            message: message.into(),
            asset_name,
            source_kind: None,
        }
    }

    fn error(
        code: RuntimeAssetDiagnosticCodeDto,
        message: impl Into<String>,
        asset_name: Option<String>,
    ) -> Self {
        Self {
            severity: RuntimeAssetDiagnosticSeverityDto::Error,
            code,
            message: message.into(),
            asset_name,
            source_kind: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeAssetManifestValidationError {
    InvalidField { field: String, reason: String },
    SerializedSizeExceeded { limit: usize },
    Serialization(String),
}

impl fmt::Display for RuntimeAssetManifestValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidField { field, reason } => write!(formatter, "{field}: {reason}"),
            Self::SerializedSizeExceeded { limit } => {
                write!(formatter, "serialized manifest exceeds {limit} bytes")
            }
            Self::Serialization(error) => {
                write!(formatter, "manifest serialization failed: {error}")
            }
        }
    }
}

impl std::error::Error for RuntimeAssetManifestValidationError {}

fn default_manifest_schema_version() -> u16 {
    RUNTIME_ASSET_MANIFEST_SCHEMA_VERSION
}

fn validate_manifest_identifier(
    field: &str,
    value: &str,
) -> Result<(), RuntimeAssetManifestValidationError> {
    if value.trim().is_empty() {
        return Err(RuntimeAssetManifestValidationError::InvalidField {
            field: field.into(),
            reason: "must not be empty".into(),
        });
    }
    if value.len() > MAX_RUNTIME_ASSET_MANIFEST_STRING_LENGTH {
        return Err(RuntimeAssetManifestValidationError::InvalidField {
            field: field.into(),
            reason: format!("must be at most {MAX_RUNTIME_ASSET_MANIFEST_STRING_LENGTH} bytes"),
        });
    }
    if value
        .chars()
        .any(|character| character.is_control() || matches!(character, '/' | '\\'))
    {
        return Err(RuntimeAssetManifestValidationError::InvalidField {
            field: field.into(),
            reason: "must be an opaque identifier, not a local path".into(),
        });
    }
    Ok(())
}

fn normalize_asset(mut asset: RuntimeAssetEntryDto) -> Result<RuntimeAssetEntryDto, String> {
    normalize_name(&mut asset.name).map_err(|reason| format!("asset name: {reason}"))?;
    normalize_bounded_opaque(&mut asset.digest, MAX_RUNTIME_ASSET_MANIFEST_DIGEST_LENGTH)
        .map_err(|reason| format!("asset '{}', digest: {reason}", asset.name))?;
    if asset.size_bytes > MAX_RUNTIME_ASSET_SIZE_BYTES {
        return Err(format!(
            "asset '{}' declares {} bytes, above the {} byte limit",
            asset.name, asset.size_bytes, MAX_RUNTIME_ASSET_SIZE_BYTES
        ));
    }
    if let Some(source_revision) = asset.provenance.source_revision.as_mut() {
        normalize_bounded_opaque(source_revision, MAX_RUNTIME_ASSET_MANIFEST_STRING_LENGTH)
            .map_err(|reason| format!("asset '{}', source revision: {reason}", asset.name))?;
    }
    Ok(asset)
}

fn normalize_name(name: &mut String) -> Result<(), String> {
    let normalized = name.trim().to_owned();
    if normalized.is_empty() {
        return Err("must not be empty".into());
    }
    if normalized.len() > MAX_RUNTIME_ASSET_MANIFEST_STRING_LENGTH {
        return Err(format!(
            "must be at most {} bytes",
            MAX_RUNTIME_ASSET_MANIFEST_STRING_LENGTH
        ));
    }
    if normalized
        .chars()
        .any(|character| character.is_control() || matches!(character, '/' | '\\'))
    {
        return Err("must be a logical name, not a local path".into());
    }
    *name = normalized;
    Ok(())
}

fn normalize_bounded_opaque(value: &mut String, limit: usize) -> Result<(), String> {
    let normalized = value.trim().to_owned();
    if normalized.is_empty() {
        return Err("must not be empty".into());
    }
    if normalized.len() > limit {
        return Err(format!("must be at most {limit} bytes"));
    }
    if normalized.chars().any(char::is_control) {
        return Err("must not contain control characters".into());
    }
    *value = normalized;
    Ok(())
}

fn asset_ordering(left: &RuntimeAssetEntryDto, right: &RuntimeAssetEntryDto) -> Ordering {
    left.name
        .cmp(&right.name)
        .then_with(|| left.kind.cmp(&right.kind))
        .then_with(|| left.provenance.precedence.cmp(&right.provenance.precedence))
        .then_with(|| {
            left.provenance
                .source_kind
                .cmp(&right.provenance.source_kind)
        })
        .then_with(|| left.digest.cmp(&right.digest))
}

fn normalize_diagnostics(
    mut diagnostics: Vec<RuntimeAssetCompatibilityDiagnosticDto>,
) -> Vec<RuntimeAssetCompatibilityDiagnosticDto> {
    for diagnostic in &mut diagnostics {
        diagnostic.message = truncate_text(
            &diagnostic.message,
            MAX_RUNTIME_ASSET_MANIFEST_STRING_LENGTH,
        );
        if let Some(asset_name) = diagnostic.asset_name.as_mut() {
            *asset_name = truncate_text(asset_name, MAX_RUNTIME_ASSET_MANIFEST_STRING_LENGTH);
        }
    }
    diagnostics.sort_by(|left, right| {
        left.severity
            .cmp(&right.severity)
            .then_with(|| left.code.cmp(&right.code))
            .then_with(|| left.asset_name.cmp(&right.asset_name))
            .then_with(|| left.message.cmp(&right.message))
    });
    diagnostics.dedup();

    if diagnostics.len() <= MAX_RUNTIME_ASSET_MANIFEST_DIAGNOSTICS {
        return diagnostics;
    }
    diagnostics.truncate(MAX_RUNTIME_ASSET_MANIFEST_DIAGNOSTICS - 1);
    diagnostics.push(RuntimeAssetCompatibilityDiagnosticDto::warning(
        RuntimeAssetDiagnosticCodeDto::DiagnosticsTruncated,
        format!(
            "diagnostics truncated to {} entries",
            MAX_RUNTIME_ASSET_MANIFEST_DIAGNOSTICS
        ),
        None,
    ));
    diagnostics
}

fn truncate_text(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_owned();
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

// Friendly aliases for conversion layers that use the remote-workspace name.
pub type RemoteWorkspaceRuntimeAssetManifestDto = RuntimeAssetManifestDto;
pub type RemoteRuntimeAssetManifestDto = RuntimeAssetManifestDto;
pub type RuntimeAssetManifestEntryDto = RuntimeAssetEntryDto;

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> RuntimeAssetManifestDto {
        RuntimeAssetManifestDto {
            identity: RuntimeAssetManifestIdentityDto {
                manifest_id: "manifest-1".into(),
                generation: 4,
                fingerprint: Some("fingerprint-1".into()),
            },
            scope: RuntimeAssetManifestScopeDto {
                project_id: "project-1".into(),
                workspace_id: "workspace-1".into(),
            },
            assets: vec![RuntimeAssetEntryDto {
                kind: RuntimeAssetKindDto::Skill,
                name: "review".into(),
                digest: "sha256:abc".into(),
                size_bytes: 128,
                provenance: RuntimeAssetProvenanceDto {
                    source_kind: RuntimeAssetSourceKindDto::CodeggProject,
                    precedence: Some(0),
                    source_revision: Some("rev-1".into()),
                },
            }],
            ..Default::default()
        }
    }

    #[test]
    fn manifest_round_trips_and_has_no_execution_fields() {
        let normalized = manifest().normalize().unwrap();
        let json = serde_json::to_string(&normalized).unwrap();
        let decoded: RuntimeAssetManifestDto = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, normalized);
        assert!(!json.contains("path"));
        assert!(!json.contains("command"));
        assert!(!json.contains("permission"));
        assert!(
            normalized.serialized_size().unwrap() <= MAX_RUNTIME_ASSET_MANIFEST_SERIALIZED_BYTES
        );
    }

    #[test]
    fn legacy_missing_optional_fields_use_safe_defaults() {
        let decoded: RuntimeAssetManifestDto = serde_json::from_str(
            r#"{
                "identity": {"manifest_id":"manifest-1"},
                "scope": {"project_id":"project-1","workspace_id":"workspace-1"},
                "assets": [{"name":"review","digest":"d","size":3}]
            }"#,
        )
        .unwrap();
        assert_eq!(
            decoded.schema_version,
            RUNTIME_ASSET_MANIFEST_SCHEMA_VERSION
        );
        assert_eq!(decoded.assets[0].size_bytes, 3);
        assert_eq!(decoded.assets[0].kind, RuntimeAssetKindDto::Unknown);
        assert_eq!(
            decoded.assets[0].provenance.source_kind,
            RuntimeAssetSourceKindDto::Unknown
        );
        assert!(decoded.normalize().is_ok());
    }

    #[test]
    fn assets_are_sorted_deduplicated_and_capped_with_diagnostics() {
        let mut input = manifest();
        input.assets = (0..MAX_RUNTIME_ASSET_MANIFEST_ASSETS + 2)
            .map(|index| RuntimeAssetEntryDto {
                kind: RuntimeAssetKindDto::Skill,
                name: format!("skill-{index:03}"),
                digest: format!("digest-{index}"),
                ..Default::default()
            })
            .collect();
        input.assets.push(RuntimeAssetEntryDto {
            kind: RuntimeAssetKindDto::Skill,
            name: "skill-000".into(),
            digest: "duplicate".into(),
            ..Default::default()
        });
        let normalized = input.normalize().unwrap();
        assert_eq!(normalized.assets.len(), MAX_RUNTIME_ASSET_MANIFEST_ASSETS);
        assert!(normalized
            .assets
            .windows(2)
            .all(|window| asset_ordering(&window[0], &window[1]) != Ordering::Greater));
        assert!(normalized
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == RuntimeAssetDiagnosticCodeDto::AssetsTruncated));
        assert!(normalized
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == RuntimeAssetDiagnosticCodeDto::DuplicateAsset));
    }

    #[test]
    fn malformed_identity_is_rejected_and_bad_assets_are_diagnosed() {
        let mut invalid_manifest = manifest();
        invalid_manifest.scope.workspace_id = "/tmp/workspace".into();
        assert!(matches!(
            invalid_manifest.normalize(),
            Err(RuntimeAssetManifestValidationError::InvalidField { .. })
        ));

        let mut input = manifest();
        input.assets.push(RuntimeAssetEntryDto {
            name: "../escape".into(),
            digest: "digest".into(),
            ..Default::default()
        });
        let normalized = input.normalize().unwrap();
        assert_eq!(normalized.assets.len(), 1);
        assert!(normalized
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == RuntimeAssetDiagnosticCodeDto::InvalidAsset));
    }

    #[test]
    fn oversized_fields_are_rejected_or_truncated_with_diagnostics() {
        let mut input = manifest();
        input.assets.push(RuntimeAssetEntryDto {
            name: "valid".into(),
            digest: "x".repeat(MAX_RUNTIME_ASSET_MANIFEST_DIGEST_LENGTH + 1),
            ..Default::default()
        });
        input.diagnostics = (0..MAX_RUNTIME_ASSET_MANIFEST_DIAGNOSTICS + 4)
            .map(|index| RuntimeAssetCompatibilityDiagnosticDto {
                message: format!("diagnostic-{index}-{}", "x".repeat(400)),
                ..Default::default()
            })
            .collect();
        let normalized = input.normalize().unwrap();
        assert!(normalized.assets.iter().all(|asset| asset.name != "valid"));
        assert!(normalized.diagnostics.len() <= MAX_RUNTIME_ASSET_MANIFEST_DIAGNOSTICS);
        assert!(normalized.diagnostics.iter().all(|diagnostic| {
            diagnostic.message.len() <= MAX_RUNTIME_ASSET_MANIFEST_STRING_LENGTH
        }));
        assert!(normalized.diagnostics.iter().any(
            |diagnostic| diagnostic.code == RuntimeAssetDiagnosticCodeDto::DiagnosticsTruncated
        ));
    }

    #[test]
    fn declared_total_size_is_bounded_with_a_diagnostic() {
        let mut input = manifest();
        input.assets = (0..5)
            .map(|index| RuntimeAssetEntryDto {
                kind: RuntimeAssetKindDto::Resource,
                name: format!("resource-{index}"),
                digest: format!("digest-{index}"),
                size_bytes: MAX_RUNTIME_ASSET_SIZE_BYTES,
                ..Default::default()
            })
            .collect();

        let normalized = input.normalize().unwrap();
        assert!(normalized.assets.len() < 5);
        assert!(
            normalized
                .assets
                .iter()
                .map(|asset| asset.size_bytes)
                .sum::<u64>()
                <= MAX_RUNTIME_ASSET_MANIFEST_TOTAL_SIZE_BYTES
        );
        assert!(normalized.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == RuntimeAssetDiagnosticCodeDto::ManifestSizeExceeded
        }));
    }
}
