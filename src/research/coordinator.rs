use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;

use crate::provider::Provider;
use crate::research::claims::{build_claims, build_claims_with_model};
use crate::research::error::{ResearchError, Result};
use crate::research::extract::{extract_evidence, extract_evidence_with_model};
use crate::research::sources::{
    advisory::AdvisorySource, crates_io::CratesIoSource, docs_rs::DocsRsSource,
    github::GitHubSource, local_repo::LocalRepoSource, search_provider::SearchProviderSource,
    search_provider::SearchProvider, url::UrlSource, ResearchSourceAdapter,
};
use crate::research::store::ResearchStore;
use crate::research::synthesis;
use crate::research::types::*;
use crate::research::verify;

/// Diff between old and new research run claims after a rerun.
#[derive(Debug, Clone)]
pub struct ClaimDiff {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub unchanged: Vec<String>,
}

pub struct ResearchCoordinator {
    store: ResearchStore,
    source_adapters: Vec<Box<dyn ResearchSourceAdapter>>,
    provider: Option<Arc<dyn Provider>>,
    model: Option<String>,
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
            provider: None,
            model: None,
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
            provider: None,
            model: None,
        }
    }

    /// Set an LLM provider for model-backed evidence extraction and claim construction.
    pub fn with_provider(mut self, provider: Arc<dyn Provider>, model: String) -> Self {
        self.provider = Some(provider);
        self.model = Some(model);
        self
    }

    pub fn store(&self) -> &ResearchStore {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut ResearchStore {
        &mut self.store
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

        // Phase 3: Evidence extraction (model-backed when provider available)
        let source_contents = self.read_source_contents(&sources).await?;
        let evidence = if let (Some(provider), Some(model)) = (&self.provider, &self.model) {
            extract_evidence_with_model(
                &run_id,
                &sources,
                &source_contents,
                &request.budget,
                Some(provider.as_ref()),
                Some(model.as_str()),
                &request.question,
            )
            .await
        } else {
            extract_evidence(&run_id, &sources, &source_contents, &request.budget)
        };
        for ev in &evidence {
            self.store.append_evidence(ev).await?;
        }
        let mut status = self.store.load_run_status(&run_id).await?;
        status.status = RunStatus::Claiming;
        status.counts.evidence_spans = evidence.len();
        self.store.update_run_status(&run_id, status).await?;

        // Phase 4: Claim construction (model-backed when provider available)
        let claims = if let (Some(provider), Some(model)) = (&self.provider, &self.model) {
            build_claims_with_model(
                &run_id,
                &evidence,
                &sources,
                provider.as_ref(),
                model,
                &request.question,
            )
            .await
        } else {
            build_claims(&run_id, &evidence, &sources, false)
        };
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

        // Phase 7: Verification (structural + optional semantic)
        let verification = verify::verify_structural(
            &request,
            &sources,
            &evidence,
            &claims,
            &contradictions,
        );

        // Phase 7b: Semantic verification when model available
        if let (Some(provider), Some(model)) = (&self.provider, &self.model) {
            let semantic_results = verify::verify_semantic(
                provider.as_ref(),
                model,
                &request.question,
                &claims,
                &evidence,
                &sources,
            )
            .await;

            // Add semantic warnings for unsupported claims
            for result in &semantic_results {
                if result.support_status == "unsupported" {
                    let mut status = self.store.load_run_status(&run_id).await?;
                    status.status = RunStatus::Verifying;
                    self.store.update_run_status(&run_id, status).await?;

                    return Err(ResearchError::VerificationFailed(format!(
                        "Claim {} is unsupported by cited evidence: {}",
                        result.claim_id, result.explanation
                    )));
                }
                if result.support_status == "partially_supported" {
                    // Write as warning (non-blocking)
                    let warn_text = format!(
                        "Claim {} is only partially supported: {}",
                        result.claim_id, result.explanation
                    );
                    let _ = self
                        .store
                        .write_report(
                            &run_id,
                            &ResearchOutputProfile::EvidenceBundle,
                            &warn_text,
                        )
                        .await;
                }
            }
        }

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

    /// Rerun a research run: re-collect sources, re-extract evidence, re-construct claims,
    /// and diff the new claims against the old ones.
    ///
    /// Returns the new run result and a diff showing what changed.
    pub async fn rerun(
        &self,
        original_run_id: &str,
    ) -> Result<(ResearchRunResult, ClaimDiff)> {
        // Load the original run bundle
        let original = self.store.load_run_bundle(original_run_id).await?;

        // Create a new request based on the original
        let new_request = ResearchRequest {
            id: uuid::Uuid::new_v4().to_string(),
            question: original.request.question.clone(),
            mode: original.request.mode.clone(),
            audience: original.request.audience.clone(),
            depth: original.request.depth.clone(),
            output_profiles: original.request.output_profiles.clone(),
            constraints: original.request.constraints.clone(),
            sources: original.request.sources.clone(),
            existing_context_refs: original.request.existing_context_refs.clone(),
            budget: original.request.budget.clone(),
            created_at: Utc::now(),
        };

        // Run the full pipeline with the new request
        let new_result = self.run(new_request).await?;

        // Load the new bundle to get claims for diffing
        let new_bundle = self.store.load_run_bundle(&new_result.run_id).await?;

        // Compute claim diff
        let diff = compute_claim_diff(&original.claims, &new_bundle.claims);

        Ok((new_result, diff))
    }

    /// Re-synthesize output profiles from an existing run's evidence and claims.
    ///
    /// Does NOT re-collect sources or re-extract evidence. Only re-renders
    /// the requested output profiles from the existing claim graph.
    pub async fn resynthesize(
        &self,
        run_id: &str,
        profiles: &[ResearchOutputProfile],
    ) -> Result<Vec<ResearchArtifactRef>> {
        let bundle = self.store.load_run_bundle(run_id).await?;

        let plan = bundle.plan.unwrap_or_else(|| self.plan(&bundle.request));

        let mut outputs = Vec::new();
        for profile in profiles {
            let text = self.render_profile(
                &bundle.request,
                &plan,
                &bundle.sources,
                &bundle.evidence,
                &bundle.claims,
                &bundle.contradictions,
                profile,
            );
            let path = self
                .store
                .write_report(run_id, profile, &text)
                .await?;
            outputs.push(ResearchArtifactRef {
                run_id: run_id.to_string(),
                profile: profile.clone(),
                path,
            });
        }

        Ok(outputs)
    }
}

fn truncate_short(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}

/// Compute a diff between old and new claim sets based on claim text.
fn compute_claim_diff(old_claims: &[ClaimRecord], new_claims: &[ClaimRecord]) -> ClaimDiff {
    let old_texts: std::collections::HashSet<&str> =
        old_claims.iter().map(|c| c.text.as_str()).collect();
    let new_texts: std::collections::HashSet<&str> =
        new_claims.iter().map(|c| c.text.as_str()).collect();

    let added: Vec<String> = new_claims
        .iter()
        .filter(|c| !old_texts.contains(c.text.as_str()))
        .map(|c| c.text.clone())
        .collect();

    let removed: Vec<String> = old_claims
        .iter()
        .filter(|c| !new_texts.contains(c.text.as_str()))
        .map(|c| c.text.clone())
        .collect();

    let unchanged: Vec<String> = new_claims
        .iter()
        .filter(|c| old_texts.contains(c.text.as_str()))
        .map(|c| c.text.clone())
        .collect();

    ClaimDiff {
        added,
        removed,
        unchanged,
    }
}
