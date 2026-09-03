//! Explicit user-authorized publication of validated skill proposals.
//!
//! This is deliberately a host/TUI service.  The model-facing proposal tool
//! can create and validate a proposal, but it cannot construct a
//! [`SkillPublicationRequest`] or call this module.  Targets are derived from
//! the closed [`SkillTargetScope`] enum and never accepted as filesystem
//! paths.

use super::promotion::{
    publication_restriction_diagnostics, PublishedSkillRef, SkillPromotionStore, SkillProposal,
    SkillProposalId, SkillProposalStatus, SkillTargetScope,
};
use super::{AssetDiscoveryConfig, AssetRegistry};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

const SKILL_FILE: &str = "SKILL.md";
const MAX_READ_BYTES: u64 = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillPublicationRequest {
    pub proposal_id: SkillProposalId,
    pub expected_revision: u64,
    pub expected_content_digest: String,
    pub target_scope: SkillTargetScope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillPublicationResult {
    pub proposal: SkillProposal,
    pub relative_path: String,
    pub content_digest: String,
    pub idempotent: bool,
    pub reconciled: bool,
    pub shadowed_by: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum SkillPublicationError {
    #[error("skill proposal is stale; preview and approve the current revision again")]
    StaleApproval,
    #[error("skill proposal is not validated")]
    ProposalNotValidated,
    #[error("skill proposal content is invalid: {0}")]
    InvalidProposal(String),
    #[error("skill publication target is unsafe: {0}")]
    UnsafeTarget(String),
    #[error("skill publication conflicts with existing content at {0}")]
    SkillAlreadyExists(String),
    #[error("skill publication precedence changed since preview: {0}")]
    PrecedenceChangedSincePreview(String),
    #[error("skill publication metadata could not be persisted after the file was written: {0}")]
    MetadataPersistence(String),
    #[error("skill publication failed: {0}")]
    Io(#[from] io::Error),
}

pub struct SkillPublicationService {
    store: SkillPromotionStore,
}

impl SkillPublicationService {
    pub fn new(store: SkillPromotionStore) -> Self {
        Self { store }
    }

    pub fn with_default_store() -> Result<Self, SkillPublicationError> {
        Ok(Self::new(SkillPromotionStore::new()?))
    }

    /// Publish one exact proposal revision.  `project_root` and
    /// `global_config_dir` are explicit host-resolved context values; the
    /// request itself contains only a closed scope enum.
    pub fn publish(
        &self,
        project_identity: &str,
        project_root: &Path,
        global_config_dir: &Path,
        request: SkillPublicationRequest,
        now: i64,
    ) -> Result<SkillPublicationResult, SkillPublicationError> {
        let proposal = self
            .store
            .get_proposal(project_identity, &request.proposal_id)?
            .ok_or(SkillPublicationError::StaleApproval)?;
        self.validate_approval(&proposal, &request)?;
        let document = validate_current_proposal(&proposal)?;
        let root =
            self.resolve_owned_root(request.target_scope, project_root, global_config_dir)?;
        let normalized_name = document.normalized_name.clone();
        let package = safe_package_path(&root, &normalized_name)?;
        let destination = package.join(SKILL_FILE);
        let relative_path = format!("{normalized_name}/{SKILL_FILE}");
        let lock_path = root.join(".codegg-skill-publish.lock");
        let lock = OpenOptions::new()
            .create(true)
            .write(true)
            .open(&lock_path)?;
        flock_lock(&lock)?;

        let result = self.publish_locked(
            project_identity,
            project_root,
            global_config_dir,
            request,
            &root,
            &package,
            &destination,
            &relative_path,
            now,
        );
        let _ = flock_unlock(&lock);
        result
    }

    /// Reconcile a complete file left by a crash after rename and before
    /// proposal persistence.  This operation never rewrites the file; it
    /// only completes the exact revision/digest metadata transition.
    pub fn reconcile(
        &self,
        project_identity: &str,
        project_root: &Path,
        global_config_dir: &Path,
        request: SkillPublicationRequest,
        now: i64,
    ) -> Result<SkillPublicationResult, SkillPublicationError> {
        let proposal = self
            .store
            .get_proposal(project_identity, &request.proposal_id)?
            .ok_or(SkillPublicationError::StaleApproval)?;
        self.validate_approval(&proposal, &request)?;
        let document = validate_current_proposal(&proposal)?;
        let root =
            self.resolve_owned_root(request.target_scope, project_root, global_config_dir)?;
        let lock_path = root.join(".codegg-skill-publish.lock");
        let lock = OpenOptions::new()
            .create(true)
            .write(true)
            .open(&lock_path)?;
        flock_lock(&lock)?;
        let result = self.reconcile_locked(
            project_identity,
            proposal,
            request,
            root,
            document.normalized_name,
            now,
        );
        let _ = flock_unlock(&lock);
        result
    }

    fn reconcile_locked(
        &self,
        project_identity: &str,
        initial_proposal: SkillProposal,
        request: SkillPublicationRequest,
        root: PathBuf,
        normalized_name: String,
        now: i64,
    ) -> Result<SkillPublicationResult, SkillPublicationError> {
        let proposal = self
            .store
            .get_proposal(project_identity, &request.proposal_id)?
            .ok_or(SkillPublicationError::StaleApproval)?;
        self.validate_approval(&proposal, &request)?;
        let document = validate_current_proposal(&proposal)?;
        if proposal != initial_proposal || document.normalized_name != normalized_name {
            return Err(SkillPublicationError::StaleApproval);
        }
        let package = safe_package_path(&root, &normalized_name)?;
        let destination = package.join(SKILL_FILE);
        inspect_package(&package, &destination)?;
        let existing = read_existing_skill(&destination)?.ok_or_else(|| {
            SkillPublicationError::SkillAlreadyExists(destination.display().to_string())
        })?;
        if existing != request.expected_content_digest {
            return Err(SkillPublicationError::StaleApproval);
        }
        let publication = publication_ref(
            &proposal,
            request.target_scope,
            &normalized_name,
            &format!("{}/{}", normalized_name, SKILL_FILE),
            now,
        );
        let published = self
            .store
            .reconcile_published(
                project_identity,
                &proposal.id,
                request.expected_revision,
                &request.expected_content_digest,
                publication,
                now,
            )
            .map_err(|error| SkillPublicationError::MetadataPersistence(error.to_string()))?;
        self.mark_habit_promoted(project_identity, &published, now)?;
        Ok(SkillPublicationResult {
            proposal: published,
            relative_path: format!("{}/{}", normalized_name, SKILL_FILE),
            content_digest: existing,
            idempotent: false,
            reconciled: true,
            shadowed_by: None,
        })
    }

    fn publish_locked(
        &self,
        project_identity: &str,
        project_root: &Path,
        global_config_dir: &Path,
        request: SkillPublicationRequest,
        root: &Path,
        package: &Path,
        destination: &Path,
        relative_path: &str,
        now: i64,
    ) -> Result<SkillPublicationResult, SkillPublicationError> {
        // Reload under the destination lock so a stale preview cannot race a
        // revision change or another publisher.
        let proposal = self
            .store
            .get_proposal(project_identity, &request.proposal_id)?
            .ok_or(SkillPublicationError::StaleApproval)?;
        self.validate_approval(&proposal, &request)?;
        let document = validate_current_proposal(&proposal)?;

        inspect_package(package, destination)?;
        let shadowed_by = self.check_precedence(
            project_identity,
            project_root,
            global_config_dir,
            &request,
            &proposal,
            root,
        )?;

        if let Some(existing_digest) = read_existing_skill(destination)? {
            let current = self
                .store
                .get_proposal(project_identity, &request.proposal_id)?
                .ok_or(SkillPublicationError::StaleApproval)?;
            let expected_publication = publication_ref(
                &current,
                request.target_scope,
                &document.normalized_name,
                relative_path,
                now,
            );
            if current.status == SkillProposalStatus::Published
                && current.publication.as_ref().is_some_and(|publication| {
                    publication.proposal_id == expected_publication.proposal_id
                        && publication.target_scope == expected_publication.target_scope
                        && publication.normalized_name == expected_publication.normalized_name
                        && publication.relative_path == expected_publication.relative_path
                        && publication.content_digest == expected_publication.content_digest
                })
                && existing_digest == request.expected_content_digest
            {
                self.mark_habit_promoted(project_identity, &current, now)?;
                return Ok(SkillPublicationResult {
                    proposal: current,
                    relative_path: relative_path.to_string(),
                    content_digest: existing_digest,
                    idempotent: true,
                    reconciled: false,
                    shadowed_by,
                });
            }
            return Err(SkillPublicationError::SkillAlreadyExists(
                relative_path.to_string(),
            ));
        }

        ensure_directory(package)?;
        let temp = package.join(format!(".SKILL.md.codegg-tmp-{}", uuid::Uuid::new_v4()));
        let bytes = current_skill_bytes(project_identity, &request, &self.store)?;
        let write_result = (|| -> Result<(), SkillPublicationError> {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temp)?;
            restrict_permissions(&file)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            // Recheck the destination and package immediately before commit.
            inspect_package(package, destination)?;
            if destination.exists() {
                return Err(SkillPublicationError::SkillAlreadyExists(
                    relative_path.to_string(),
                ));
            }
            // The proposal store has its own lock. Revalidate immediately
            // before rename so a concurrent diagnostic/rejection cannot cause
            // bytes from an obsolete approval to become durable.
            let latest = self
                .store
                .get_proposal(project_identity, &request.proposal_id)?
                .ok_or(SkillPublicationError::StaleApproval)?;
            self.validate_approval(&latest, &request)?;
            let latest_document = validate_current_proposal(&latest)?;
            if latest_document.normalized_name != document.normalized_name
                || latest.skill_markdown.as_bytes() != bytes.as_slice()
            {
                return Err(SkillPublicationError::StaleApproval);
            }
            fs::rename(&temp, destination)?;
            sync_directory(package)?;
            Ok(())
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temp);
            return write_result.map(|_| unreachable!());
        }

        let current = self
            .store
            .get_proposal(project_identity, &request.proposal_id)?
            .ok_or(SkillPublicationError::StaleApproval)?;
        let publication = publication_ref(
            &current,
            request.target_scope,
            &document.normalized_name,
            relative_path,
            now,
        );
        let published = self
            .store
            .mark_published(
                project_identity,
                &current.id,
                request.expected_revision,
                &request.expected_content_digest,
                publication,
                now,
            )
            .map_err(|error| SkillPublicationError::MetadataPersistence(error.to_string()))?;
        self.mark_habit_promoted(project_identity, &published, now)?;
        Ok(SkillPublicationResult {
            proposal: published,
            relative_path: relative_path.to_string(),
            content_digest: request.expected_content_digest,
            idempotent: false,
            reconciled: false,
            shadowed_by,
        })
    }

    fn validate_approval(
        &self,
        proposal: &SkillProposal,
        request: &SkillPublicationRequest,
    ) -> Result<(), SkillPublicationError> {
        if proposal.revision != request.expected_revision
            || proposal.content_digest != request.expected_content_digest
        {
            return Err(SkillPublicationError::StaleApproval);
        }
        if proposal.status == SkillProposalStatus::Published {
            if proposal
                .publication
                .as_ref()
                .is_some_and(|publication| publication.target_scope == request.target_scope)
            {
                return Ok(());
            }
            return Err(SkillPublicationError::StaleApproval);
        }
        if proposal.status != SkillProposalStatus::Validated {
            return Err(SkillPublicationError::ProposalNotValidated);
        }
        Ok(())
    }

    fn resolve_owned_root(
        &self,
        scope: SkillTargetScope,
        project_root: &Path,
        global_config_dir: &Path,
    ) -> Result<PathBuf, SkillPublicationError> {
        let base = match scope {
            SkillTargetScope::Project => canonical_directory(project_root)?,
            SkillTargetScope::Global => canonical_directory(global_config_dir)?,
        };
        let owned_root = match scope {
            SkillTargetScope::Project => base.join(".codegg").join("skills"),
            SkillTargetScope::Global => base.join("codegg").join("skills"),
        };
        ensure_owned_root(&owned_root)?;
        let canonical = owned_root.canonicalize()?;
        if !canonical.starts_with(&base) {
            return Err(SkillPublicationError::UnsafeTarget(
                "owned skill root escapes its explicit context root".to_string(),
            ));
        }
        Ok(canonical)
    }

    fn check_precedence(
        &self,
        _project_identity: &str,
        project_root: &Path,
        global_config_dir: &Path,
        request: &SkillPublicationRequest,
        proposal: &SkillProposal,
        destination_root: &Path,
    ) -> Result<Option<String>, SkillPublicationError> {
        let registry = AssetRegistry::build(
            &AssetDiscoveryConfig::default(),
            project_root,
            &[global_config_dir.to_path_buf()],
        );
        let Some(skill) = registry.get(&proposal.name) else {
            return Ok(None);
        };
        if skill.source_path
            == destination_root
                .join(&proposal.name.trim().to_lowercase())
                .join(SKILL_FILE)
        {
            return Ok(None);
        }
        if proposal.previewed_revision != Some(request.expected_revision) {
            return Err(SkillPublicationError::PrecedenceChangedSincePreview(
                format!("{} from {:?}", proposal.name, skill.source_kind),
            ));
        }
        Ok(Some(format!(
            "{} from {:?}",
            proposal.name, skill.source_kind
        )))
    }

    fn mark_habit_promoted(
        &self,
        project_identity: &str,
        proposal: &SkillProposal,
        _now: i64,
    ) -> Result<(), SkillPublicationError> {
        let habits = codegg_core::memory::habit::HabitStore::with_root(
            self.store.habit_root_for_publication(),
        )?;
        let marked = habits.mark_promoted(
            project_identity,
            &proposal.habit_id,
            codegg_core::memory::habit::PublishedSkillRef {
                id: proposal.id.as_str().to_string(),
            },
        )?;
        if !marked {
            let already_promoted = habits
                .load(project_identity)?
                .into_iter()
                .find(|candidate| candidate.id == proposal.habit_id)
                .is_some_and(|candidate| {
                    candidate.status == codegg_core::memory::habit::HabitCandidateStatus::Promoted
                        && candidate
                            .promoted_skill
                            .as_ref()
                            .is_some_and(|skill| skill.id == proposal.id.as_str())
                });
            if !already_promoted {
                return Err(SkillPublicationError::MetadataPersistence(
                    "habit candidate was not ready for promotion".to_string(),
                ));
            }
        }
        Ok(())
    }
}

fn validate_current_proposal(
    proposal: &SkillProposal,
) -> Result<super::parser::ValidatedSkillDocument, SkillPublicationError> {
    if proposal.status != SkillProposalStatus::Validated
        && proposal.status != SkillProposalStatus::Published
    {
        return Err(SkillPublicationError::ProposalNotValidated);
    }
    let document = super::parser::validate_portable_document(
        &proposal.skill_markdown,
        &AssetDiscoveryConfig::default(),
    )
    .map_err(|diagnostic| SkillPublicationError::InvalidProposal(diagnostic.reason))?;
    let mut diagnostics = publication_restriction_diagnostics(&proposal.skill_markdown, &document);
    if proposal.name.trim().to_lowercase() != document.normalized_name
        || proposal.description.trim() != document.description.trim()
    {
        diagnostics.push(super::promotion::BoundedDiagnostic::error(
            "proposal provenance does not match its current frontmatter",
        ));
    }
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == super::Severity::Error)
    {
        return Err(SkillPublicationError::InvalidProposal(
            diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.reason)
                .take(8)
                .collect::<Vec<_>>()
                .join("; "),
        ));
    }
    if super::promotion::compute_content_digest(&proposal.skill_markdown) != proposal.content_digest
    {
        return Err(SkillPublicationError::StaleApproval);
    }
    Ok(document)
}

fn safe_package_path(root: &Path, name: &str) -> Result<PathBuf, SkillPublicationError> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
        return Err(SkillPublicationError::UnsafeTarget(
            "skill name must be one safe path component".to_string(),
        ));
    }
    let path = Path::new(name);
    let mut components = path.components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(SkillPublicationError::UnsafeTarget(
            "skill name must be one normal path component".to_string(),
        ));
    }
    Ok(root.join(path))
}

fn ensure_owned_root(root: &Path) -> Result<(), SkillPublicationError> {
    let Some(parent) = root.parent() else {
        return Err(SkillPublicationError::UnsafeTarget(
            "owned root has no parent".to_string(),
        ));
    };
    ensure_directory(parent)?;
    ensure_directory(root)?;
    Ok(())
}

fn ensure_directory(path: &Path) -> Result<(), SkillPublicationError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(SkillPublicationError::UnsafeTarget(format!(
                    "{} is not a non-symlink directory",
                    path.display()
                )));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(path)?;
            let metadata = fs::symlink_metadata(path)?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(SkillPublicationError::UnsafeTarget(format!(
                    "{} was replaced by an unsafe directory",
                    path.display()
                )));
            }
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn canonical_directory(path: &Path) -> Result<PathBuf, SkillPublicationError> {
    let canonical = path.canonicalize()?;
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(SkillPublicationError::UnsafeTarget(format!(
            "{} is not an explicit directory",
            path.display()
        )));
    }
    Ok(canonical)
}

fn inspect_package(package: &Path, destination: &Path) -> Result<(), SkillPublicationError> {
    match fs::symlink_metadata(package) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(SkillPublicationError::UnsafeTarget(format!(
                    "skill package {} is not a directory",
                    package.display()
                )));
            }
            let canonical = package.canonicalize()?;
            let root = package
                .parent()
                .and_then(|parent| parent.canonicalize().ok())
                .ok_or_else(|| {
                    SkillPublicationError::UnsafeTarget("package root unavailable".to_string())
                })?;
            if !canonical.starts_with(root) {
                return Err(SkillPublicationError::UnsafeTarget(
                    "skill package escapes CodeGG root".to_string(),
                ));
            }
            for entry in fs::read_dir(package)? {
                let entry = entry?;
                let name = entry.file_name();
                if name != SKILL_FILE
                    && !name.to_string_lossy().starts_with(".SKILL.md.codegg-tmp-")
                {
                    return Err(SkillPublicationError::SkillAlreadyExists(
                        package.display().to_string(),
                    ));
                }
                let entry_meta = fs::symlink_metadata(entry.path())?;
                if entry_meta.file_type().is_symlink()
                    || (name == SKILL_FILE && !entry_meta.is_file())
                {
                    return Err(SkillPublicationError::UnsafeTarget(format!(
                        "unexpected package entry {}",
                        entry.path().display()
                    )));
                }
            }
            if destination.exists() {
                let metadata = fs::symlink_metadata(destination)?;
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(SkillPublicationError::UnsafeTarget(
                        "destination is not a regular file".to_string(),
                    ));
                }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn read_existing_skill(path: &Path) -> Result<Option<String>, SkillPublicationError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(SkillPublicationError::UnsafeTarget(format!(
            "destination {} is not a regular file",
            path.display()
        )));
    }
    if metadata.len() > MAX_READ_BYTES {
        return Err(SkillPublicationError::UnsafeTarget(
            "existing SKILL.md exceeds the parser bound".to_string(),
        ));
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_READ_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_READ_BYTES {
        return Err(SkillPublicationError::UnsafeTarget(
            "existing SKILL.md exceeds the parser bound".to_string(),
        ));
    }
    let text = String::from_utf8(bytes).map_err(|_| {
        SkillPublicationError::UnsafeTarget("existing SKILL.md is not UTF-8".to_string())
    })?;
    Ok(Some(super::promotion::compute_content_digest(&text)))
}

fn current_skill_bytes(
    project_identity: &str,
    request: &SkillPublicationRequest,
    store: &SkillPromotionStore,
) -> Result<Vec<u8>, SkillPublicationError> {
    let proposal = store
        .get_proposal(project_identity, &request.proposal_id)?
        .ok_or(SkillPublicationError::StaleApproval)?;
    if proposal.revision != request.expected_revision
        || proposal.content_digest != request.expected_content_digest
    {
        return Err(SkillPublicationError::StaleApproval);
    }
    Ok(proposal.skill_markdown.into_bytes())
}

fn publication_ref(
    proposal: &SkillProposal,
    target_scope: SkillTargetScope,
    normalized_name: &str,
    relative_path: &str,
    published_at: i64,
) -> PublishedSkillRef {
    PublishedSkillRef {
        proposal_id: proposal.id.clone(),
        target_scope,
        normalized_name: normalized_name.to_string(),
        relative_path: relative_path.to_string(),
        content_digest: proposal.content_digest.clone(),
        published_at,
    }
}

fn restrict_permissions(file: &File) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(path)?.sync_all()?;
    }
    Ok(())
}

#[cfg(unix)]
fn flock_lock(file: &File) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    #[allow(unsafe_code)]
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(unix)]
fn flock_unlock(file: &File) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    #[allow(unsafe_code)]
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(windows)]
fn flock_lock(_file: &File) -> io::Result<()> {
    Ok(())
}

#[cfg(windows)]
fn flock_unlock(_file: &File) -> io::Result<()> {
    Ok(())
}
