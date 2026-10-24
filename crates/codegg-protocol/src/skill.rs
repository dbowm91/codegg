//! The discoverable skill catalogue exposed to frontends.
//!
//! The catalogue is **metadata only**: a name, a description, the namespace
//! the skill was discovered in, and the content digest. It never carries a
//! skill body, a resource body, or a filesystem path, so a renderer can draw
//! the list without learning where the daemon's files live — and the
//! response stays bounded whatever the skill packages contain.

use serde::{Deserialize, Serialize};

/// Maximum number of skills one catalogue response returns.
///
/// The daemon clamps a client-requested `max_entries` to this value, so a
/// client can narrow the bound but never widen it.
pub const MAX_SKILL_CATALOGUE_ENTRIES: usize = 256;

/// One request for a project's skill catalogue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillCatalogueRequestDto {
    /// The project whose roots are searched for skills.
    ///
    /// The daemon resolves the project's root from its own durable project
    /// binding; this request carries no filesystem path.
    pub project_id: String,
    /// An optional client-requested entry cap.
    ///
    /// The daemon clamps this to [`MAX_SKILL_CATALOGUE_ENTRIES`]. `None`
    /// means "the daemon's cap".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_entries: Option<usize>,
}

/// The skills the daemon discovered for one project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillCatalogueDto {
    /// The project the catalogue was built for.
    pub project_id: String,
    /// The discovered skills, capped at the daemon's entry bound.
    pub skills: Vec<SkillSummaryDto>,
    /// How many skills were discovered before the cap was applied.
    pub total: usize,
    /// Whether `skills` was shortened by the cap.
    pub truncated: bool,
}

/// One discovered skill, described without its body or its location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillSummaryDto {
    /// The skill's name.
    pub name: String,
    /// The skill's one-line description.
    pub description: String,
    /// The namespace the skill was discovered in.
    pub source: SkillSourceDto,
    /// The digest of the skill's content.
    pub content_digest: String,
    /// How many additional resources the skill package declares.
    pub resource_count: usize,
}

/// The namespace a skill was discovered in.
///
/// The wire shape is deliberately coarser than the discovery layer's own
/// vendor taxonomy: a frontend renders a namespace, not a vendor matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillSourceDto {
    /// A skill found under the project's own roots.
    Project,
    /// A skill found under a user-level (global) root.
    Global,
    /// A skill contributed by an installed plugin.
    Plugin,
    /// A skill found under a user-configured extra root.
    Configured,
    /// A compatibility skill shipped with CodeGG itself.
    Native,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_catalogue_request_ignores_a_client_supplied_path() {
        let json = serde_json::json!({
            "project_id": "project-1",
            "path": "/etc",
            "skills_root": "/etc",
        });
        let request: SkillCatalogueRequestDto =
            serde_json::from_value(json).expect("the request decodes without a path field");
        assert_eq!(request.project_id, "project-1");
        assert_eq!(request.max_entries, None);
        let rendered = serde_json::to_value(&request).expect("the request re-renders");
        assert!(
            rendered.get("path").is_none(),
            "a client-supplied path must not survive into the request"
        );
        assert!(rendered.get("skills_root").is_none());
    }

    #[test]
    fn skill_catalogue_carries_metadata_but_no_body_or_location() {
        let catalogue = SkillCatalogueDto {
            project_id: "project-1".into(),
            skills: vec![SkillSummaryDto {
                name: "testing-ci".into(),
                description: "Select focused verification.".into(),
                source: SkillSourceDto::Project,
                content_digest: "sha256:abc".into(),
                resource_count: 2,
            }],
            total: 1,
            truncated: false,
        };
        let json = serde_json::to_value(&catalogue).expect("the catalogue renders");
        let entry = json
            .get("skills")
            .and_then(|skills| skills.get(0))
            .expect("the first skill is present");
        assert_eq!(
            entry.get("name").and_then(|value| value.as_str()),
            Some("testing-ci")
        );
        assert_eq!(
            entry.get("source").and_then(|value| value.as_str()),
            Some("project")
        );
        assert!(
            entry.get("body").is_none(),
            "the catalogue must not carry a skill body"
        );
        assert!(
            entry.get("source_path").is_none(),
            "the catalogue must not carry a filesystem path"
        );
        let decoded: SkillCatalogueDto =
            serde_json::from_value(json).expect("the catalogue decodes back");
        assert_eq!(decoded, catalogue);
    }
}
