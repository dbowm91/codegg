//! Phase 3: read-only project-scoped fact reads for frontends.
//!
//! Two request families live here:
//!
//! - `git_read` — read-only git facts (status, diff summary, branch list)
//!   for the repository a project is bound to.
//! - `skill_catalogue_list` — the skills a project can discover.
//!
//! Both resolve their subject **server-side**, from the daemon's own
//! durable project bindings. A request carries a `project_id` and nothing
//! else, so there is no client-supplied path to traverse. Neither read has
//! a write path: the git facts come from the read-only git fact layer
//! (`egggit`), which never reaches the checkout writer fence, and the
//! catalogue comes from the canonical skill registry, so the daemon never
//! enumerates a skills tree itself.
//!
//! Neither read emits a `CoreEvent`: a read answers the caller that asked
//! and changes nothing.

use std::path::{Path, PathBuf};

use codegg_protocol::core::{CoreRequest, CoreResponse};
use codegg_protocol::git::{
    GitBranchDto, GitBranchListDto, GitChangeKindDto, GitChangedFileDto, GitDiffSummaryDto,
    GitReadConsistencyDto, GitReadOperationDto, GitReadResultDto, GitRepoStatusDto,
    MAX_GIT_READ_ROWS,
};
use codegg_protocol::skill::{
    SkillCatalogueDto, SkillSourceDto, SkillSummaryDto, MAX_SKILL_CATALOGUE_ENTRIES,
};

use crate::core::daemon::CoreDaemon;
use crate::error::AppError;

// The temporary-Git-repository fixture used by the tests below lives in a
// `tests/` directory: `src/core/daemon_reads.rs` is a canonical finite path,
// and the execution-ownership guard forbids a direct process spawn anywhere
// in it.
#[cfg(test)]
#[path = "tests/git_fixture.rs"]
mod git_fixture;

impl CoreDaemon {
    /// Phase 3: serve one read-only git read.
    ///
    /// The request names a project; the repository root comes from the
    /// daemon's durable binding. The read is served by `egggit`, the
    /// read-only fact layer, so there is no checkout write and no writer
    /// permit involved: this operation cannot mutate a checkout.
    pub(crate) async fn handle_git_request(
        &self,
        request: CoreRequest,
    ) -> Result<CoreResponse, AppError> {
        let CoreRequest::GitRead { request } = request else {
            return Ok(read_error(
                "git_read_unavailable",
                "unsupported git read request",
            ));
        };
        let root = match self
            .resolve_project_repository_root(&request.project_id)
            .await
        {
            Ok(root) => root,
            Err(code) => return Ok(read_error(code, "project repository is unavailable")),
        };
        // A client may narrow the row bound; it can never widen it.
        let max_rows = request
            .max_rows
            .unwrap_or(MAX_GIT_READ_ROWS)
            .min(MAX_GIT_READ_ROWS);
        let result = match git_read_facts(&root, request.operation, max_rows).await {
            Ok(result) => result,
            Err(code) => return Ok(read_error(code, "git facts are unavailable")),
        };
        Ok(CoreResponse::GitRead { result })
    }

    /// Phase 3: serve the skill catalogue for one project.
    ///
    /// The catalogue is built by the canonical skill registry over the
    /// project's own root plus the configured global roots, so it matches
    /// what the agent's `skill` tool would discover. The response carries
    /// metadata only — never a skill body, never a filesystem path.
    pub(crate) async fn handle_skill_catalogue_request(
        &self,
        request: CoreRequest,
    ) -> Result<CoreResponse, AppError> {
        let CoreRequest::SkillCatalogueList { request } = request else {
            return Ok(read_error(
                "skill_catalogue_unavailable",
                "unsupported skill catalogue request",
            ));
        };
        let root = match self
            .resolve_project_workspace_root(&request.project_id)
            .await
        {
            Ok(root) => root,
            Err(code) => return Ok(read_error(code, "project workspace is unavailable")),
        };
        // A client may narrow the entry bound; it can never widen it.
        let max_entries = request
            .max_entries
            .unwrap_or(MAX_SKILL_CATALOGUE_ENTRIES)
            .min(MAX_SKILL_CATALOGUE_ENTRIES);
        let project_id = request.project_id.clone();
        let worker_project_id = project_id.clone();
        // Discovery walks the filesystem, so it runs on a blocking worker
        // rather than on the daemon's async runtime.
        let catalogue = tokio::task::spawn_blocking(move || {
            build_skill_catalogue(worker_project_id, root, max_entries)
        })
        .await
        .unwrap_or_else(|_| SkillCatalogueDto {
            project_id: project_id.clone(),
            skills: Vec::new(),
            total: 0,
            truncated: false,
        });
        Ok(CoreResponse::SkillCatalogueList { result: catalogue })
    }

    /// Resolve the repository root the daemon owns for one project.
    ///
    /// The canonical answer is the root of the durable
    /// `workspace_project_binding` row that names a repository: the catalog
    /// recorded it when it reconciled the workspace, and
    /// `project_repository` returns only a binding that actually names one.
    ///
    /// A workspace whose remote lineage is not uniquely identifiable has no
    /// repository binding at all, so the project's own workspace root is the
    /// fallback. Either way the root comes from the daemon's durable
    /// bindings; nothing in the request can influence the answer — the
    /// request carries a project identity, not a path.
    async fn resolve_project_repository_root(
        &self,
        project_id: &str,
    ) -> Result<PathBuf, &'static str> {
        let Some(pool) = &self.pool else {
            return Err("git_read_unavailable");
        };
        let Ok(project_id) = codegg_core::identity::ProjectId::parse(project_id) else {
            return Err("git_read_not_found");
        };
        let storage = codegg_core::project_storage::ProjectStorage::new(pool.clone());
        let resolved = storage
            .project_repository(&project_id)
            .await
            .map_err(|_| "git_read_unavailable")?;
        if let Some((_workspace_id, _repository_id, root)) = resolved {
            if !root.as_os_str().is_empty() {
                return Ok(root);
            }
        }
        match self.catalog_workspace_root(pool, &project_id).await {
            Some(root) => Ok(root),
            None => Err("git_read_not_found"),
        }
    }

    /// Resolve the workspace root the daemon owns for one project.
    ///
    /// The root comes from the durable `workspace` table, reached through
    /// the binding that names the project. The catalog's own order is
    /// deterministic, so one project always resolves to the same root.
    async fn resolve_project_workspace_root(
        &self,
        project_id: &str,
    ) -> Result<PathBuf, &'static str> {
        let Some(pool) = &self.pool else {
            return Err("skill_catalogue_unavailable");
        };
        let Ok(project_id) = codegg_core::identity::ProjectId::parse(project_id) else {
            return Err("skill_catalogue_not_found");
        };
        match self.catalog_workspace_root(pool, &project_id).await {
            Some(root) => Ok(root),
            None => Err("skill_catalogue_not_found"),
        }
    }

    /// The project's first bound workspace root, read from the durable
    /// catalog.
    async fn catalog_workspace_root(
        &self,
        pool: &sqlx::SqlitePool,
        project_id: &codegg_core::identity::ProjectId,
    ) -> Option<PathBuf> {
        let catalog = codegg_core::project_catalog::ProjectCatalog::new(pool.clone());
        let workspaces = catalog.list_workspaces_for_project(project_id).await.ok()?;
        let workspace = workspaces.into_iter().next()?;
        if workspace.canonical_root.as_os_str().is_empty() {
            return None;
        }
        Some(workspace.canonical_root)
    }
}

/// Read one git fact set from the read-only git fact layer.
///
/// The fact layer is `egggit`: it never mutates, and it is the layer the
/// repo's own git tooling reads through. Every collection is capped here at
/// the daemon boundary, so a repository with ten thousand changed files
/// still answers with a bounded response.
async fn git_read_facts(
    root: &Path,
    operation: GitReadOperationDto,
    max_rows: usize,
) -> Result<GitReadResultDto, &'static str> {
    match operation {
        GitReadOperationDto::Status => {
            let status_fact = egggit::status::repo_status(root)
                .await
                .map_err(|_| "git_read_failed")?;
            let consistency = status_fact.class();
            let status = status_fact.into_value();
            Ok(GitReadResultDto::Status(GitRepoStatusDto {
                branch: status.branch,
                is_dirty: status.is_dirty,
                commit_hash: status.commit_hash,
                stash_count: status.stash_count,
                consistency: consistency_dto(consistency),
            }))
        }
        GitReadOperationDto::Diff => {
            // `diff_summary` reads the numstat of the working tree against
            // `HEAD`: a count-and-name view that never carries patch text,
            // so the response stays bounded whatever the diff size. It is
            // the tracked-file slice — `git diff` does not list untracked
            // files — so the file list is a subset of the working tree's
            // changes by construction.
            let diff_fact = egggit::diff::diff_summary(root, None)
                .await
                .map_err(|_| "git_read_failed")?;
            // This read is a *display* read, not a generation-bound
            // consumer: it publishes nothing, so it does not claim one
            // stable object generation. It therefore propagates the class
            // it observed rather than admitting it — the class travels
            // with the facts to the renderer, who can say what the
            // observation is worth. Dropping it here would make the class
            // unfalsifiable from the client's side.
            let consistency = diff_fact.class();
            let summary = diff_fact.value();
            let total_files = summary.files.len();
            let files = summary
                .files
                .iter()
                .take(max_rows)
                .map(|file| GitChangedFileDto {
                    path: file.path.clone(),
                    kind: change_kind(file.kind),
                })
                .collect();
            Ok(GitReadResultDto::Diff(GitDiffSummaryDto {
                files_changed: summary.files_changed,
                insertions: summary.insertions,
                deletions: summary.deletions,
                files,
                total_files,
                truncated: total_files > max_rows,
                consistency: consistency_dto(consistency),
            }))
        }
        GitReadOperationDto::Branches => {
            let branch_fact = egggit::refs::list_branches(root)
                .await
                .map_err(|_| "git_read_failed")?;
            let consistency = branch_fact.class();
            let branches = branch_fact.into_value();
            let total = branches.len();
            let rows = branches
                .into_iter()
                .take(max_rows)
                .map(|branch| GitBranchDto {
                    name: branch.name,
                    is_current: branch.is_current,
                    is_detached: branch.is_detached,
                    upstream: branch.upstream,
                    ahead: branch.ahead,
                    behind: branch.behind,
                    head: branch.head,
                })
                .collect();
            Ok(GitReadResultDto::Branches(GitBranchListDto {
                branches: rows,
                total,
                truncated: total > max_rows,
                consistency: consistency_dto(consistency),
            }))
        }
    }
}

/// Map the read-only fact layer's consistency class onto the wire taxonomy.
///
/// The wire form and the fact layer's form share their names deliberately:
/// a client that reads `"captured_not_current"` is reading the same class
/// the fact layer observed, not a paraphrase of it.
fn consistency_dto(class: egggit::consistency::GitReadConsistency) -> GitReadConsistencyDto {
    match class {
        egggit::consistency::GitReadConsistency::ImmutableGeneration => {
            GitReadConsistencyDto::ImmutableGeneration
        }
        egggit::consistency::GitReadConsistency::CapturedNotCurrent => {
            GitReadConsistencyDto::CapturedNotCurrent
        }
        egggit::consistency::GitReadConsistency::LiveMutable => GitReadConsistencyDto::LiveMutable,
        egggit::consistency::GitReadConsistency::NoObjectGeneration => {
            GitReadConsistencyDto::NoObjectGeneration
        }
    }
}

/// Map the read-only fact layer's change taxonomy onto the wire taxonomy.
fn change_kind(kind: egggit::diff::ChangeKind) -> GitChangeKindDto {
    match kind {
        egggit::diff::ChangeKind::Added => GitChangeKindDto::Added,
        egggit::diff::ChangeKind::Modified => GitChangeKindDto::Modified,
        egggit::diff::ChangeKind::Deleted => GitChangeKindDto::Deleted,
        egggit::diff::ChangeKind::Renamed => GitChangeKindDto::Renamed,
        egggit::diff::ChangeKind::Copied => GitChangeKindDto::Copied,
        egggit::diff::ChangeKind::Other => GitChangeKindDto::Other,
    }
}

/// Build the catalogue of skills discoverable for one project.
///
/// Discovery goes through the canonical skill registry, so the daemon never
/// enumerates a skills tree itself and the catalogue matches what the
/// agent's `skill` tool would see. The global root is the *parent*
/// directory the registry appends `<vendor>/skills` to — never an
/// already-joined path, which would silently discover nothing.
fn build_skill_catalogue(
    project_id: String,
    project_root: PathBuf,
    max_entries: usize,
) -> SkillCatalogueDto {
    let app_config = crate::core::load_config_or_default();
    let discovery = crate::agent::asset_context::asset_discovery_config_from(&app_config);
    let global_roots: Vec<PathBuf> = crate::agent::asset_context::default_global_discovery_root()
        .into_iter()
        .collect();
    let registry = crate::skills::AssetRegistry::build(&discovery, &project_root, &global_roots);
    let total = registry.list().len();
    let skills = registry
        .list()
        .iter()
        .take(max_entries)
        .map(|skill| SkillSummaryDto {
            name: skill.name.clone(),
            description: skill.description.clone(),
            source: skill_source(skill.source_kind),
            content_digest: skill.content_digest.clone(),
            resource_count: skill.resources.len(),
        })
        .collect();
    SkillCatalogueDto {
        project_id,
        skills,
        total,
        truncated: total > max_entries,
    }
}

/// Map the discovery layer's vendor taxonomy onto the wire's namespaces.
fn skill_source(kind: crate::skills::source::SourceKind) -> SkillSourceDto {
    use crate::skills::source::SourceKind;
    match kind {
        SourceKind::Plugin => SkillSourceDto::Plugin,
        SourceKind::Configured => SkillSourceDto::Configured,
        SourceKind::CodeGGNativeCompat => SkillSourceDto::Native,
        other if other.is_global() => SkillSourceDto::Global,
        _ => SkillSourceDto::Project,
    }
}

/// A refusal that names no daemon path and no repository detail.
fn read_error(code: &str, message: &str) -> CoreResponse {
    CoreResponse::Error {
        code: code.to_string(),
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use codegg_protocol::core::CoreResponse;
    use codegg_protocol::git::{GitReadOperationDto, GitReadRequestDto};
    use codegg_protocol::skill::{SkillCatalogueRequestDto, SkillSourceDto};
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use sqlx::SqlitePool;
    use std::str::FromStr;

    /// An in-memory database, migrated once. `migrate` runs inside the
    /// helper, so no test adds its own.
    async fn isolated_pool() -> SqlitePool {
        let name = format!("daemon_reads_{}", uuid::Uuid::new_v4().simple());
        let url = format!("file:{name}?mode=memory&cache=shared");
        let opts = SqliteConnectOptions::from_str(&url)
            .expect("valid sqlite options")
            .create_if_missing(true)
            .busy_timeout(std::time::Duration::from_secs(5))
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .expect("connect in-memory sqlite");
        codegg_core::session::schema::migrate(&pool)
            .await
            .expect("migrations run inside the helper");
        pool
    }

    /// Reconcile one workspace bound to `root` and return the project the
    /// catalog bound it to.
    ///
    /// This is the daemon's own catalog path, not a hand-written row: the
    /// project identity the read later resolves comes from the catalog's
    /// durable binding.
    async fn register_project(pool: &SqlitePool, root: &Path) -> String {
        let storage = codegg_core::project_storage::ProjectStorage::new(pool.clone());
        let workspace = codegg_core::workspace::WorkspaceRecord {
            id: codegg_core::workspace::WorkspaceId::new(),
            canonical_root: root.to_path_buf(),
            display_name: "fixture".to_string(),
            created_at: Utc::now(),
            last_opened_at: Utc::now(),
            archived_at: None,
        };
        let reconciliation = storage
            .reconcile_workspace_path(&workspace, "test")
            .await
            .expect("the workspace reconciles against the catalog");
        reconciliation.binding.project_id.as_str().to_string()
    }

    /// Ask the daemon for one git fact set and return the status facts.
    async fn git_status(daemon: &CoreDaemon, project_id: &str) -> GitRepoStatusDto {
        let response = daemon
            .handle_request(crate::core::new_request(
                "git-status".to_string(),
                CoreRequest::GitRead {
                    request: GitReadRequestDto {
                        project_id: project_id.to_string(),
                        operation: GitReadOperationDto::Status,
                        max_rows: None,
                    },
                },
            ))
            .await
            .expect("the daemon answers the git read");
        let CoreResponse::GitRead { result } = response else {
            panic!("a git read answers with git facts");
        };
        let GitReadResultDto::Status(status) = result else {
            panic!("the status operation answers with status facts");
        };
        status
    }

    #[tokio::test]
    async fn git_read_returns_facts_for_the_bound_repository() {
        let (_dir, root) = git_fixture::temp_git_repo("feature-alpha");
        let pool = isolated_pool().await;
        let project_id = register_project(&pool, &root).await;
        let daemon = CoreDaemon::new(Some(pool), None, None);

        let status = git_status(&daemon, &project_id).await;

        assert_eq!(
            status.branch, "feature-alpha",
            "the read reports the branch of the repository the project is bound to"
        );
        assert!(
            status.is_dirty,
            "the read reports the fixture's staged change"
        );
        assert!(
            status.commit_hash.is_some(),
            "the read reports the fixture's commit"
        );
    }

    #[tokio::test]
    async fn git_read_ignores_a_client_supplied_path() {
        let (_bound_dir, bound_root) = git_fixture::temp_git_repo("bound-branch");
        let (_other_dir, other_root) = git_fixture::temp_git_repo("other-branch");
        let pool = isolated_pool().await;
        let project_id = register_project(&pool, &bound_root).await;
        let daemon = CoreDaemon::new(Some(pool), None, None);

        // The request shape has no path field at all, so a client that
        // sends one is ignored rather than honoured. The read is still
        // answered from the project's own bound repository.
        let request: CoreRequest = serde_json::from_value(serde_json::json!({
            "type": "git_read",
            "request": {
                "project_id": project_id,
                "operation": "status",
                "path": other_root.to_string_lossy(),
                "repository_root": other_root.to_string_lossy(),
            }
        }))
        .expect("the request decodes");

        let response = daemon
            .handle_request(crate::core::new_request(
                "git-redirect".to_string(),
                request,
            ))
            .await
            .expect("the daemon answers the git read");
        let CoreResponse::GitRead { result } = response else {
            panic!("a git read answers with git facts");
        };
        let GitReadResultDto::Status(status) = result else {
            panic!("the status operation answers with status facts");
        };
        assert_eq!(
            status.branch, "bound-branch",
            "the read follows the project's own binding, not the client's path"
        );
    }

    /// Ask the daemon for one git fact set and return the consistency class
    /// it reported.
    async fn git_read_class(
        daemon: &CoreDaemon,
        project_id: &str,
        operation: GitReadOperationDto,
    ) -> GitReadConsistencyDto {
        let response = daemon
            .handle_request(crate::core::new_request(
                "git-consistency".to_string(),
                CoreRequest::GitRead {
                    request: GitReadRequestDto {
                        project_id: project_id.to_string(),
                        operation,
                        max_rows: None,
                    },
                },
            ))
            .await
            .expect("the daemon answers the git read");
        let CoreResponse::GitRead { result } = response else {
            panic!("a git read answers with git facts");
        };
        match result {
            GitReadResultDto::Status(status) => status.consistency,
            GitReadResultDto::Diff(diff) => diff.consistency,
            GitReadResultDto::Branches(branches) => branches.consistency,
        }
    }

    #[tokio::test]
    async fn every_git_read_carries_the_class_it_observed() {
        // A read that dropped its class on the way to the wire would make
        // the class unfalsifiable from the client's side: the client could
        // not tell a one-generation observation from a live one. Every
        // operation therefore reports the class it observed, and the class
        // is *specific* — a daemon that reported one constant for every
        // read would satisfy a weaker check and mislead the client about
        // the two reads that differ.
        let (_dir, root) = git_fixture::temp_git_repo("feature-consistency");
        let pool = isolated_pool().await;
        let project_id = register_project(&pool, &root).await;
        let daemon = CoreDaemon::new(Some(pool), None, None);

        let status = git_status(&daemon, &project_id).await;
        // The status read consults refs and the index, not the object
        // store, so it honestly reports that it observed no generation.
        assert_eq!(
            status.consistency,
            GitReadConsistencyDto::NoObjectGeneration,
            "a read of refs and the index observed no object generation"
        );

        let diff_class = git_read_class(&daemon, &project_id, GitReadOperationDto::Diff).await;
        // The diff read compares the working tree against `HEAD`, so it
        // reads the object store: it observed a generation. Which of the
        // two generation-bearing classes it is depends on whether the
        // object view came from the capture cache or from a fresh
        // snapshot, and both are honest answers.
        assert!(
            matches!(
                diff_class,
                GitReadConsistencyDto::ImmutableGeneration
                    | GitReadConsistencyDto::CapturedNotCurrent
            ),
            "a read of HEAD's tree observed a generation, so it must not \
             report that it observed none: {diff_class:?}"
        );

        let branch_class =
            git_read_class(&daemon, &project_id, GitReadOperationDto::Branches).await;
        // The branch list reads refs, not the object store.
        assert_eq!(
            branch_class,
            GitReadConsistencyDto::NoObjectGeneration,
            "a read of refs observed no object generation"
        );
    }

    #[tokio::test]
    async fn git_read_caps_the_changed_file_list() {
        let (_dir, root) = git_fixture::temp_git_repo("feature-caps");
        let pool = isolated_pool().await;
        let project_id = register_project(&pool, &root).await;
        let daemon = CoreDaemon::new(Some(pool), None, None);

        let response = daemon
            .handle_request(crate::core::new_request(
                "git-diff".to_string(),
                CoreRequest::GitRead {
                    request: GitReadRequestDto {
                        project_id,
                        operation: GitReadOperationDto::Diff,
                        max_rows: Some(2),
                    },
                },
            ))
            .await
            .expect("the daemon answers the git read");
        let CoreResponse::GitRead { result } = response else {
            panic!("a git read answers with git facts");
        };
        let GitReadResultDto::Diff(diff) = result else {
            panic!("the diff operation answers with diff facts");
        };
        assert_eq!(diff.total_files, 3, "the fixture stages three files");
        assert_eq!(
            diff.files.len(),
            2,
            "the response carries only the rows the caller asked for"
        );
        assert!(
            diff.truncated,
            "a shortened list says so, so a renderer can tell it is partial"
        );
        assert_eq!(
            diff.files_changed, 3,
            "the counts describe the whole change, not the capped list"
        );
    }

    #[tokio::test]
    async fn git_read_refuses_a_project_without_a_workspace() {
        let pool = isolated_pool().await;
        let daemon = CoreDaemon::new(Some(pool), None, None);

        let response = daemon
            .handle_request(crate::core::new_request(
                "git-missing".to_string(),
                CoreRequest::GitRead {
                    request: GitReadRequestDto {
                        project_id: "project-absent".to_string(),
                        operation: GitReadOperationDto::Status,
                        max_rows: None,
                    },
                },
            ))
            .await
            .expect("the daemon answers the git read");
        let CoreResponse::Error { code, .. } = response else {
            panic!("a project with no bound workspace is refused, not answered");
        };
        assert_eq!(
            code, "git_read_not_found",
            "the refusal names the missing repository, not a path"
        );
    }

    #[tokio::test]
    async fn git_read_answers_for_a_repository_without_a_remote() {
        // A workspace whose remote lineage is not uniquely identifiable has
        // no repository binding, so the read falls back to the project's own
        // workspace root rather than refusing a perfectly readable checkout.
        let (_dir, root) = git_fixture::temp_git_repo_without_remote("local-branch");
        let pool = isolated_pool().await;
        let project_id = register_project(&pool, &root).await;
        let daemon = CoreDaemon::new(Some(pool), None, None);

        let status = git_status(&daemon, &project_id).await;

        assert_eq!(
            status.branch, "local-branch",
            "a local-only repository still answers the read"
        );
    }

    #[tokio::test]
    async fn a_read_publishes_no_core_event() {
        let (_dir, root) = git_fixture::temp_git_repo("feature-quiet");
        let pool = isolated_pool().await;
        let project_id = register_project(&pool, &root).await;
        let daemon = CoreDaemon::new(Some(pool), None, None);
        let before = daemon.event_log.current_seq();

        let status = git_status(&daemon, &project_id).await;

        assert_eq!(status.branch, "feature-quiet");
        assert_eq!(
            daemon.event_log.current_seq(),
            before,
            "a read answers the caller and publishes no event"
        );
    }

    #[tokio::test]
    async fn skill_catalogue_lists_the_projects_skills() {
        let (_dir, root) = git_fixture::temp_git_repo("feature-skills");
        git_fixture::write_project_skill(&root, "fixture-skill", "A fixture skill.");
        let pool = isolated_pool().await;
        let project_id = register_project(&pool, &root).await;
        let daemon = CoreDaemon::new(Some(pool), None, None);

        let response = daemon
            .handle_request(crate::core::new_request(
                "skills".to_string(),
                CoreRequest::SkillCatalogueList {
                    request: SkillCatalogueRequestDto {
                        project_id: project_id.clone(),
                        max_entries: None,
                    },
                },
            ))
            .await
            .expect("the daemon answers the skill catalogue");
        let CoreResponse::SkillCatalogueList { result } = response else {
            panic!("a skill catalogue request answers with a catalogue");
        };
        assert_eq!(result.project_id, project_id);
        let skill = result
            .skills
            .iter()
            .find(|skill| skill.name == "fixture-skill")
            .expect("the project's own skill is in the catalogue");
        assert_eq!(skill.description, "A fixture skill.");
        assert_eq!(skill.source, SkillSourceDto::Project);
        assert!(
            !skill.content_digest.is_empty(),
            "the catalogue carries the content digest"
        );
        assert_eq!(
            result.total,
            result.skills.len(),
            "nothing was dropped, so the list is not truncated"
        );
        assert!(!result.truncated);
    }

    #[tokio::test]
    async fn skill_catalogue_does_not_enumerate_arbitrary_directories() {
        let (_dir, root) = git_fixture::temp_git_repo("feature-scoped");
        git_fixture::write_project_skill(&root, "fixture-skill", "A fixture skill.");
        // A `SKILL.md` that is not under a discovery root must never be
        // catalogued: the daemon does not walk the project tree itself.
        let stray = root.join("notes");
        std::fs::create_dir_all(&stray).expect("create stray directory");
        std::fs::write(
            stray.join("SKILL.md"),
            "---\nname: stray-skill\ndescription: Not discoverable.\n---\n\nBody.\n",
        )
        .expect("write stray document");
        let pool = isolated_pool().await;
        let project_id = register_project(&pool, &root).await;
        let daemon = CoreDaemon::new(Some(pool), None, None);

        let response = daemon
            .handle_request(crate::core::new_request(
                "skills-scoped".to_string(),
                CoreRequest::SkillCatalogueList {
                    request: SkillCatalogueRequestDto {
                        project_id,
                        max_entries: None,
                    },
                },
            ))
            .await
            .expect("the daemon answers the skill catalogue");
        let CoreResponse::SkillCatalogueList { result } = response else {
            panic!("a skill catalogue request answers with a catalogue");
        };
        assert!(
            result
                .skills
                .iter()
                .all(|skill| skill.name != "stray-skill"),
            "a document outside the discovery roots is not a skill"
        );
    }
}
