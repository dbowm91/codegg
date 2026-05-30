use std::path::PathBuf;

use chrono::Utc;

use crate::research::claims::build_claims;
use crate::research::error::{ResearchError, Result};
use crate::research::extract::extract_evidence;
use crate::research::sources::{
    advisory::AdvisorySource, crates_io::CratesIoSource, docs_rs::DocsRsSource,
    github::GitHubSource, local_repo::LocalRepoSource, search_provider::SearchProviderSource,
    search_provider::SearchProvider, url::UrlSource, ResearchSourceAdapter,
};
use crate::research::store::ResearchStore;
use crate::research::synthesis;
use crate::research::types::*;
use crate::research::verify;

pub struct ResearchCoordinator {
    store: ResearchStore,
    source_adapters: Vec<Box<dyn ResearchSourceAdapter>>,
}

impl ResearchCoordinator {
    pub fn new(project_root: PathBuf, artifact_root: PathBuf) -> Self {
        let store = ResearchStore::new(artifact_root);
        let source_adapters: Vec<Box<dyn ResearchSourceAdapter>> = vec![
            Box::new(LocalRepoSource::new(project_root.clone())),
            Box::new(UrlSource::new()),
            Box::new(CratesIoSource::new()),
            Box::new(GitHubSource::new()),
            Box::new(DocsRsSource::new()),
        ];
        Self {
            store,
            source_adapters,
        }
    }

    pub fn with_search_provider(
        project_root: PathBuf,
        artifact_root: PathBuf,
        provider: SearchProvider,
        api_key: Option<String>,
    ) -> Self {
        let store = ResearchStore::new(artifact_root);
        let source_adapters: Vec<Box<dyn ResearchSourceAdapter>> = vec![
            Box::new(LocalRepoSource::new(project_root.clone())),
            Box::new(UrlSource::new()),
            Box::new(AdvisorySource::new()),
            Box::new(SearchProviderSource::new(provider, api_key)),
        ];
        Self {
            store,
            source_adapters,
        }
    }

    pub fn store(&self) -> &ResearchStore {
        &self.store
    }

    pub async fn run(&self, request: ResearchRequest) -> Result<ResearchRunResult> {
        let run_id = request.id.clone();

        // Phase 0: Create run
        self.store.create_run(&request).await?;

        // Phase 1: Planning
        let plan = self.plan(&request);
        self.store.write_plan(&run_id, &plan).await?;
        let mut status = self.store.load_run_status(&run_id).await?;
        status.status = RunStatus::Collecting;
        self.store.update_run_status(&run_id, status).await?;

        // Phase 2: Source collection
        let mut sources = self.collect_sources(&request, &plan).await?;
        for source in &mut sources {
            source.run_id = run_id.clone();
            self.store.append_source(source).await?;
        }
        let mut status = self.store.load_run_status(&run_id).await?;
        status.status = RunStatus::Extracting;
        status.counts.sources = sources.len();
        self.store.update_run_status(&run_id, status).await?;

        // Phase 3: Evidence extraction (deterministic chunking)
        let source_contents = self.read_source_contents(&sources).await?;
        let evidence = extract_evidence(
            &run_id,
            &sources,
            &source_contents,
            &request.budget,
        );
        for ev in &evidence {
            self.store.append_evidence(ev).await?;
        }
        let mut status = self.store.load_run_status(&run_id).await?;
        status.status = RunStatus::Claiming;
        status.counts.evidence_spans = evidence.len();
        self.store.update_run_status(&run_id, status).await?;

        // Phase 4: Claim construction (deterministic fallback)
        let claims = build_claims(&run_id, &evidence, &sources, false);
        for claim in &claims {
            self.store.append_claim(claim).await?;
        }
        let mut status = self.store.load_run_status(&run_id).await?;
        status.status = RunStatus::Contradicting;
        status.counts.claims = claims.len();
        self.store.update_run_status(&run_id, status).await?;

        // Phase 5: Contradiction/gap pass (deterministic)
        let contradictions = self.check_contradictions(&run_id, &claims);
        for contra in &contradictions {
            self.store.append_contradiction(contra).await?;
        }
        let mut status = self.store.load_run_status(&run_id).await?;
        status.status = RunStatus::Synthesizing;
        status.counts.contradictions = contradictions.len();
        self.store.update_run_status(&run_id, status).await?;

        // Phase 6: Synthesis - render all requested output profiles
        let mut outputs = Vec::new();
        for profile in &request.output_profiles {
            let text = self.render_profile(
                &request,
                &plan,
                &sources,
                &evidence,
                &claims,
                &contradictions,
                profile,
            );
            let path = self.store.write_report(&run_id, profile, &text).await?;
            outputs.push(ResearchArtifactRef {
                run_id: run_id.clone(),
                profile: profile.clone(),
                path,
            });
        }

        // Phase 7: Verification
        let verification = verify::verify_structural(
            &request,
            &sources,
            &evidence,
            &claims,
            &contradictions,
        );
        if !verification.passed {
            let err_msg = verification.errors.join("; ");
            let mut status = self.store.load_run_status(&run_id).await?;
            status.status = RunStatus::Failed;
            status.error = Some(err_msg.clone());
            self.store.update_run_status(&run_id, status).await?;
            return Err(ResearchError::VerificationFailed(err_msg));
        }

        // Write verification warnings if any
        if !verification.warnings.is_empty() {
            let warnings_text = verification.warnings.join("\n");
            let warn_path = self
                .store
                .write_report(
                    &run_id,
                    &ResearchOutputProfile::EvidenceBundle,
                    &warnings_text,
                )
                .await;
            // Best effort - don't fail on warning write errors
            let _ = warn_path;
        }

        // Finalize
        let artifact_dir = self.store.root().join(&run_id);
        let mut status = self.store.load_run_status(&run_id).await?;
        status.status = RunStatus::Completed;
        status.finished_at = Some(Utc::now());
        self.store.update_run_status(&run_id, status.clone()).await?;

        Ok(ResearchRunResult {
            run_id,
            status: status.status,
            artifact_dir,
            outputs,
            counts: status.counts,
        })
    }

    fn plan(&self, request: &ResearchRequest) -> ResearchPlan {
        // Deterministic plan generation from request parameters
        let scope = match request.mode {
            ResearchMode::Landscape => {
                format!(
                    "Landscape survey of: {}. Examining available options, ecosystem state, and key characteristics.",
                    request.question
                )
            }
            ResearchMode::ArchitectureDecision => {
                format!(
                    "Architecture decision analysis for: {}. Comparing options against project constraints and requirements.",
                    request.question
                )
            }
            ResearchMode::LibraryEvaluation => {
                format!(
                    "Library evaluation of: {}. Assessing fitness, maturity, and compatibility.",
                    request.question
                )
            }
            ResearchMode::ApiInvestigation => {
                format!(
                    "API investigation: {}. Examining interfaces, contracts, and integration patterns.",
                    request.question
                )
            }
            ResearchMode::DebuggingInvestigation => {
                format!(
                    "Debugging investigation: {}. Tracing root cause and examining failure modes.",
                    request.question
                )
            }
            ResearchMode::SecurityReview => {
                format!(
                    "Security review: {}. Checking for vulnerabilities, attack surfaces, and hardening.",
                    request.question
                )
            }
            ResearchMode::SpecDigest => {
                format!(
                    "Specification digest: {}. Summarizing requirements, constraints, and behaviors.",
                    request.question
                )
            }
            ResearchMode::NarrowAnswer => {
                format!(
                    "Narrow answer to: {}. Providing direct, focused response.",
                    request.question
                )
            }
        };

        let comparison_axes = match request.mode {
            ResearchMode::ArchitectureDecision | ResearchMode::LibraryEvaluation => vec![
                "maintenance health".to_string(),
                "release cadence".to_string(),
                "API ergonomics".to_string(),
                "ecosystem gravity".to_string(),
                "runtime compatibility".to_string(),
                "performance model".to_string(),
                "license/governance".to_string(),
                "migration/lock-in risk".to_string(),
            ],
            ResearchMode::SecurityReview => vec![
                "attack surface".to_string(),
                "dependency risk".to_string(),
                "input validation".to_string(),
                "authentication/authorization".to_string(),
            ],
            _ => vec![],
        };

        let source_classes = request
            .sources
            .iter()
            .map(|s| format!("{:?}", s.spec_type))
            .collect();

        ResearchPlan {
            scope,
            comparison_axes,
            source_classes,
            exclusion_criteria: vec![
                "Exclude vendored/generated code unless explicitly requested".to_string(),
                "Exclude test fixtures unless relevant to question".to_string(),
            ],
            stopping_conditions: vec![
                format!("Budget limits reached (max {} sources)", request.budget.max_sources),
                "All requested comparison axes covered".to_string(),
            ],
            expected_outputs: request
                .output_profiles
                .iter()
                .map(|p| format!("{:?}", p))
                .collect(),
        }
    }

    async fn collect_sources(
        &self,
        request: &ResearchRequest,
        plan: &ResearchPlan,
    ) -> Result<Vec<SourceRecord>> {
        let mut all_sources = Vec::new();
        for adapter in &self.source_adapters {
            match adapter.collect(request, plan).await {
                Ok(sources) => all_sources.extend(sources),
                Err(ResearchError::NetworkNotAllowed) => {
                    // Skip network adapters when network not allowed
                }
                Err(e) => {
                    eprintln!("Warning: source adapter '{}' failed: {}", adapter.name(), e);
                }
            }
        }
        // Deduplicate by URI
        let mut seen = std::collections::HashSet::new();
        all_sources.retain(|s| seen.insert(s.uri.clone()));
        // Apply budget
        let max = request.budget.max_sources.min(all_sources.len());
        all_sources.truncate(max);
        Ok(all_sources)
    }

    async fn read_source_contents(
        &self,
        sources: &[SourceRecord],
    ) -> Result<Vec<(String, String)>> {
        let mut contents = Vec::new();
        for source in sources {
            let content = match &source.source_type {
                SourceType::LocalFile | SourceType::LocalSearchResult => {
                    let path = std::path::Path::new(&source.uri);
                    match tokio::fs::read_to_string(path).await {
                        Ok(c) => c,
                        Err(e) => {
                            eprintln!(
                                "Warning: failed to read {}: {}",
                                source.uri, e
                            );
                            continue;
                        }
                    }
                }
                SourceType::Url | SourceType::HtmlPage | SourceType::MarkdownPage => {
                    // For URL sources, we already fetched the content during collection.
                    // Re-fetch would be wasteful; store content in notes or skip.
                    // For MVP, skip re-fetching URLs.
                    eprintln!(
                        "Note: URL source {} content not available for extraction (MVP limitation)",
                        source.uri
                    );
                    continue;
                }
                _ => continue,
            };
            contents.push((source.id.clone(), content));
        }
        Ok(contents)
    }

    fn check_contradictions(
        &self,
        run_id: &str,
        claims: &[ClaimRecord],
    ) -> Vec<ContradictionRecord> {
        let mut contradictions = Vec::new();

        // Group claims by applies_to
        let mut by_target: std::collections::HashMap<String, Vec<&ClaimRecord>> =
            std::collections::HashMap::new();
        for claim in claims {
            for target in &claim.applies_to {
                by_target.entry(target.clone()).or_default().push(claim);
            }
        }

        // Flag conflicting recommendations
        for (_target, target_claims) in &by_target {
            let recs: Vec<&&ClaimRecord> = target_claims
                .iter()
                .filter(|c| c.claim_type == ClaimType::Recommendation)
                .collect();
            if recs.len() > 1 {
                contradictions.push(ContradictionRecord {
                    id: uuid::Uuid::new_v4().to_string(),
                    run_id: run_id.to_string(),
                    description: format!(
                        "Multiple conflicting recommendations for target '{}' with different conclusions",
                        _target
                    ),
                    claim_ids: recs.iter().map(|c| c.id.clone()).collect(),
                    severity: ContradictionSeverity::Medium,
                });
            }
        }

        // Flag low-confidence claims that appear important
        for claim in claims {
            if claim.confidence == Confidence::Low
                && matches!(
                    claim.claim_type,
                    ClaimType::Fact | ClaimType::Comparison | ClaimType::Recommendation
                )
            {
                contradictions.push(ContradictionRecord {
                    id: uuid::Uuid::new_v4().to_string(),
                    run_id: run_id.to_string(),
                    description: format!(
                        "Low-confidence {} claim may need additional evidence: {}",
                        claim.claim_type.as_str(),
                        truncate_short(&claim.text, 80)
                    ),
                    claim_ids: vec![claim.id.clone()],
                    severity: ContradictionSeverity::Low,
                });
            }
        }

        contradictions
    }

    fn render_profile(
        &self,
        request: &ResearchRequest,
        plan: &ResearchPlan,
        sources: &[SourceRecord],
        evidence: &[EvidenceSpan],
        claims: &[ClaimRecord],
        contradictions: &[ContradictionRecord],
        profile: &ResearchOutputProfile,
    ) -> String {
        match profile {
            ResearchOutputProfile::HumanFullReport => {
                synthesis::render_human_full_report(request, plan, sources, evidence, claims, contradictions)
            }
            ResearchOutputProfile::HumanBrief => {
                synthesis::render_human_brief(request, claims, contradictions)
            }
            ResearchOutputProfile::AgentAnswer => {
                synthesis::render_agent_answer(request, claims, contradictions, sources, evidence)
            }
            ResearchOutputProfile::AgentHandoff => {
                let artifact_dir = self.store.root().join(&request.id);
                synthesis::render_agent_handoff(request, plan, claims, contradictions, &artifact_dir)
            }
            ResearchOutputProfile::EvidenceBundle => {
                synthesis::render_evidence_bundle(sources, evidence, claims, contradictions)
            }
        }
    }
}

fn truncate_short(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}
