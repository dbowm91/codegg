use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;

use crate::provider::Provider;
use crate::research::claims::{build_claims, build_claims_with_model};
use crate::research::error::{ResearchError, Result};
use crate::research::extract::{extract_evidence, extract_evidence_with_model};
use crate::research::sources::{
    crates_io::CratesIoSource, docs_rs::DocsRsSource, eggsearch::EggsearchSource,
    github::GitHubSource, local_repo::LocalRepoSource, url::UrlSource, ResearchSourceAdapter,
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
        let mut source_adapters: Vec<Box<dyn ResearchSourceAdapter>> = vec![
            Box::new(LocalRepoSource::new(project_root.clone())),
            Box::new(UrlSource::new()),
        ];
        // Sources that require TLS; gracefully skip if the platform
        // TLS stack is unavailable.
        if let Ok(s) = CratesIoSource::try_new() {
            source_adapters.push(Box::new(s));
        }
        if let Ok(s) = GitHubSource::try_new() {
            source_adapters.push(Box::new(s));
        }
        if let Ok(s) = DocsRsSource::try_new() {
            source_adapters.push(Box::new(s));
        }
        source_adapters.push(Box::new(EggsearchSource::new()));
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

    /// Attach an explicit runtime-owned search/MCP context (M005) to the
    /// eggsearch evidence adapter. The adapter is swapped (matched by its
    /// stable `"eggsearch"` adapter name) so deep-research evidence
    /// collection executes against this context instead of the deprecated
    /// process-global slots. Other adapters are untouched.
    pub fn with_search_runtime(
        mut self,
        runtime: crate::search_backend::SearchRuntimeContext,
    ) -> Self {
        self.source_adapters.retain(|a| a.name() != "eggsearch");
        self.source_adapters.push(Box::new(
            EggsearchSource::new().with_search_runtime(runtime),
        ));
        self
    }

    pub fn store(&self) -> &ResearchStore {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut ResearchStore {
        &mut self.store
    }

    /// Record a terminal failure for a run.
    ///
    /// Every error return from [`Self::run`] must pass through here so a
    /// failed run can never be persisted in an in-flight status (the
    /// `Verifying` trap this replaced).
    ///
    /// Best effort by design: if the run record cannot be loaded or
    /// written, the underlying verification failure must still reach the
    /// caller. Returning that bookkeeping error instead would hide the
    /// real reason the run failed, so it is logged and swallowed here.
    async fn fail_run(&self, run_id: &str, error: &str) {
        match self.store.load_run_status(run_id).await {
            Ok(mut status) => {
                status.status = RunStatus::Failed;
                status.error = Some(error.to_string());
                status.finished_at = Some(Utc::now());
                if let Err(e) = self.store.update_run_status(run_id, status).await {
                    tracing::error!(
                        run_id,
                        error = %e,
                        "failed to persist terminal Failed status for research run"
                    );
                }
            }
            Err(e) => {
                tracing::error!(
                    run_id,
                    error = %e,
                    "could not load research run status to record failure"
                );
            }
        }
    }

    pub async fn run(&self, request: ResearchRequest) -> Result<ResearchRunResult> {
        let run_id = request.id.clone();
        let provider_context = crate::research::llm::provider_context_for_run(&run_id);

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
                provider_context.clone(),
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
                provider_context.clone(),
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
        let verification =
            verify::verify_structural(&request, &sources, &evidence, &claims, &contradictions);

        // Enter the verifying state up front so a failure anywhere in this
        // phase has a terminal state to fall through to. Previously the
        // run was only ever set to `Verifying` on the semantic-failure
        // path, immediately before returning an error — which left every
        // semantic failure permanently recorded as `Verifying`.
        {
            let mut status = self.store.load_run_status(&run_id).await?;
            status.status = RunStatus::Verifying;
            self.store.update_run_status(&run_id, status).await?;
        }

        // Phase 7b: Semantic verification when model available
        if let (Some(provider), Some(model)) = (&self.provider, &self.model) {
            let semantic_results = verify::verify_semantic(
                provider.as_ref(),
                model,
                provider_context,
                &request.question,
                &claims,
                &evidence,
                &sources,
            )
            .await;

            // Add semantic warnings for unsupported claims
            for result in &semantic_results {
                if result.support_status == "unsupported" {
                    let err_msg = format!(
                        "Claim {} is unsupported by cited evidence: {}",
                        result.claim_id, result.explanation
                    );
                    self.fail_run(&run_id, &err_msg).await;
                    return Err(ResearchError::VerificationFailed(err_msg));
                }
                if result.support_status == "partially_supported" {
                    // Write as warning (non-blocking)
                    let warn_text = format!(
                        "Claim {} is only partially supported: {}",
                        result.claim_id, result.explanation
                    );
                    let _ = self
                        .store
                        .write_report(&run_id, &ResearchOutputProfile::EvidenceBundle, &warn_text)
                        .await;
                }
            }
        }

        if !verification.passed {
            let err_msg = verification.errors.join("; ");
            self.fail_run(&run_id, &err_msg).await;
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
        self.store
            .update_run_status(&run_id, status.clone())
            .await?;

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
                format!(
                    "Budget limits reached (max {} sources)",
                    request.budget.max_sources
                ),
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
                    // Expected whenever the run's budget disallows network
                    // collection. Logged rather than dropped silently: a
                    // run that silently collected zero web sources looks
                    // identical to a run that genuinely found nothing.
                    tracing::debug!(
                        source = adapter.name(),
                        "source adapter skipped: network not allowed for this run"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        source = adapter.name(),
                        error = %e,
                        "source adapter failed"
                    );
                }
            }
        }
        // Deduplicate by URI
        let mut seen = std::collections::HashSet::new();
        all_sources.retain(|s| seen.insert(s.uri.clone()));
        // Apply budget
        let max = request.budget.max_sources.min(all_sources.len());
        all_sources.truncate(max);

        if request.budget.allow_network && all_sources.is_empty() {
            // With network explicitly permitted, collecting nothing means
            // every network adapter failed or was skipped. Say so, rather
            // than letting the run proceed to an empty answer.
            tracing::warn!(
                "network collection was permitted but produced no sources; \
                 the answer will rest on local evidence only"
            );
        }

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
                            tracing::warn!(uri = %source.uri, error = %e, "failed to read source file");
                            continue;
                        }
                    }
                }
                SourceType::Url | SourceType::HtmlPage | SourceType::MarkdownPage => {
                    // Adapters already retrieved text during collection and
                    // stored it on the record (`snippet=`, or the title as a
                    // fallback). Re-fetching here would repeat the network
                    // round trip for content we already hold.
                    //
                    // These used to be dropped outright as an "MVP
                    // limitation", which meant a run that successfully
                    // collected web sources still produced zero evidence
                    // and an empty answer.
                    match source.retrieved_text() {
                        Some(text) if !text.trim().is_empty() => text,
                        _ => {
                            tracing::warn!(
                                uri = %source.uri,
                                "network source carried no retrieved text; skipping extraction"
                            );
                            continue;
                        }
                    }
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
            ResearchOutputProfile::HumanFullReport => synthesis::render_human_full_report(
                request,
                plan,
                sources,
                evidence,
                claims,
                contradictions,
            ),
            ResearchOutputProfile::HumanBrief => {
                synthesis::render_human_brief(request, claims, contradictions)
            }
            ResearchOutputProfile::AgentAnswer => {
                synthesis::render_agent_answer(request, claims, contradictions, sources, evidence)
            }
            ResearchOutputProfile::AgentHandoff => {
                let artifact_dir = self.store.root().join(&request.id);
                synthesis::render_agent_handoff(
                    request,
                    plan,
                    claims,
                    contradictions,
                    &artifact_dir,
                )
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
    pub async fn rerun(&self, original_run_id: &str) -> Result<(ResearchRunResult, ClaimDiff)> {
        // Load the original run bundle
        let original = self.store.load_run_bundle(original_run_id).await?;

        // Create a new request based on the original
        let new_request = crate::research::types::ResearchRequest {
            id: uuid::Uuid::new_v4().to_string(),
            created_at: Utc::now(),
            ..original.request.clone()
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
            let path = self.store.write_report(run_id, profile, &text).await?;
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
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &s[..end])
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

#[cfg(test)]
mod tests {
    use super::*;

    fn url_source(id: &str, uri: &str, notes: Vec<String>) -> SourceRecord {
        SourceRecord {
            id: id.to_string(),
            run_id: String::new(),
            uri: uri.to_string(),
            title: Some("A web page".to_string()),
            source_type: SourceType::Url,
            source_quality: SourceQuality::Secondary,
            retrieved_at: Utc::now(),
            published_at: None,
            content_hash: None,
            locator: SourceLocator::Url {
                url: uri.to_string(),
                heading: None,
            },
            notes,
        }
    }

    #[tokio::test]
    async fn network_source_content_is_extracted_instead_of_dropped() {
        // Regression: URL sources were skipped outright ("MVP limitation"),
        // so a run that collected web sources still produced zero evidence
        // and therefore an empty answer.
        let tmp = tempfile::tempdir().unwrap();
        let coordinator =
            ResearchCoordinator::new(tmp.path().to_path_buf(), tmp.path().join("artifacts"));
        let sources = vec![
            url_source(
                "s1",
                "https://example.com/1",
                vec![
                    "source=eggsearch".to_string(),
                    "snippet=The retrieved body".to_string(),
                ],
            ),
            url_source(
                "s2",
                "https://example.com/2",
                vec!["source=eggsearch".to_string()],
            ),
        ];

        let contents = coordinator.read_source_contents(&sources).await.unwrap();

        assert_eq!(contents.len(), 2, "both URL sources must yield content");
        assert_eq!(contents[0].0, "s1");
        assert_eq!(contents[0].1, "The retrieved body");
        // No snippet: falls back to the title rather than dropping the source.
        assert_eq!(contents[1].1, "A web page");
    }

    #[tokio::test]
    async fn url_source_content_flows_all_the_way_into_evidence() {
        // End-to-end shape of the fix: a collected network source must
        // produce an evidence span, which is what ultimately gives the
        // agent an answer with citations.
        let tmp = tempfile::tempdir().unwrap();
        let coordinator =
            ResearchCoordinator::new(tmp.path().to_path_buf(), tmp.path().join("artifacts"));
        let sources = vec![url_source(
            "s1",
            "https://example.com/1",
            vec!["snippet=Rust ownership rules".to_string()],
        )];
        let contents = coordinator.read_source_contents(&sources).await.unwrap();

        let budget = ResearchBudget {
            max_sources: 10,
            max_chunks_per_source: 20,
            max_evidence_spans: 200,
            max_model_calls: 0,
            max_output_tokens: None,
            allow_network: true,
        };
        let evidence =
            crate::research::extract::extract_evidence("run-1", &sources, &contents, &budget);

        assert!(
            !evidence.is_empty(),
            "a network source with retrieved text must yield evidence"
        );
        assert_eq!(evidence[0].source_id, "s1");
    }

    #[tokio::test]
    async fn fail_run_records_a_terminal_failed_state() {
        // Regression: a verification failure used to persist the run as
        // `Verifying`, so failed runs were indistinguishable from
        // in-flight ones in `list_runs`.
        let tmp = tempfile::tempdir().unwrap();
        let artifact_root = tmp.path().join("artifacts");
        let coordinator = ResearchCoordinator::new(tmp.path().to_path_buf(), artifact_root.clone());

        let request = ResearchRequest {
            id: "run-fail".to_string(),
            question: "q".to_string(),
            mode: ResearchMode::NarrowAnswer,
            audience: ResearchAudience::Human,
            depth: ResearchDepth::Low,
            output_profiles: vec![],
            constraints: vec![],
            sources: vec![],
            existing_context_refs: vec![],
            budget: ResearchBudget {
                max_sources: 1,
                max_chunks_per_source: 1,
                max_evidence_spans: 1,
                max_model_calls: 0,
                max_output_tokens: None,
                allow_network: false,
            },
            created_at: Utc::now(),
        };
        coordinator.store.create_run(&request).await.unwrap();

        coordinator.fail_run("run-fail", "boom").await;

        let status = coordinator.store.load_run_status("run-fail").await.unwrap();
        assert_eq!(status.status, RunStatus::Failed);
        assert_eq!(status.error.as_deref(), Some("boom"));
        assert!(
            status.finished_at.is_some(),
            "a failed run must record a finish time"
        );
    }

    #[test]
    fn truncate_short_ascii_within_limit() {
        assert_eq!(truncate_short("hello", 10), "hello");
    }

    #[test]
    fn truncate_short_ascii_at_limit() {
        assert_eq!(truncate_short("hello", 5), "hello");
    }

    #[test]
    fn truncate_short_ascii_over_limit() {
        assert_eq!(truncate_short("hello world", 5), "hello...");
    }

    #[test]
    fn truncate_short_multibyte_at_boundary() {
        let s = format!("{}{}", "a".repeat(79), "ñ");
        let result = truncate_short(&s, 80);
        assert!(result.ends_with("..."));
        assert!(
            !result.contains("ñ"),
            "truncated string must not split mid-codepoint"
        );
    }

    #[test]
    fn truncate_short_emoji() {
        let s = format!("{}{}", "a".repeat(78), "🦀");
        let result = truncate_short(&s, 80);
        assert!(result.ends_with("..."));
        assert!(
            !result.contains("🦀"),
            "emoji should be dropped at truncation boundary"
        );
    }

    #[test]
    fn truncate_short_cjk() {
        let s = format!("{}{}", "a".repeat(78), "語");
        let result = truncate_short(&s, 80);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn truncate_short_empty() {
        assert_eq!(truncate_short("", 10), "");
    }
}
