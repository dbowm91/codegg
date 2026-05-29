use std::collections::HashMap;
use std::fmt::Write as _;

use chrono::Utc;

use crate::research::types::*;

/// Render a human-facing full report from claims, evidence, and sources.
pub fn render_human_full_report(
    request: &ResearchRequest,
    plan: &ResearchPlan,
    sources: &[SourceRecord],
    evidence: &[EvidenceSpan],
    claims: &[ClaimRecord],
    contradictions: &[ContradictionRecord],
) -> String {
    let mut out = String::with_capacity(4096);
    let ts = Utc::now().format("%Y-%m-%d %H:%M UTC");

    writeln!(out, "# Research Report").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "**Question:** {}", request.question).unwrap();
    writeln!(out, "**Mode:** {:?}", request.mode).unwrap();
    writeln!(out, "**Generated:** {ts}").unwrap();
    writeln!(out).unwrap();

    // Executive conclusion
    let top_recs: Vec<&ClaimRecord> = claims
        .iter()
        .filter(|c| c.claim_type == ClaimType::Recommendation)
        .collect();
    if let Some(rec) = top_recs.first() {
        writeln!(out, "## Executive Conclusion").unwrap();
        writeln!(out).unwrap();
        writeln!(out, "{}", rec.text).unwrap();
        writeln!(out, "\n**Confidence:** {:?}", rec.confidence).unwrap();
        if !rec.caveats.is_empty() {
            writeln!(out).unwrap();
            writeln!(out, "**Caveats:**").unwrap();
            for c in &rec.caveats {
                writeln!(out, "- {c}").unwrap();
            }
        }
        writeln!(out).unwrap();
    }

    // Scope
    writeln!(out, "## Scope").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "{}", plan.scope).unwrap();
    writeln!(out).unwrap();

    // Method
    writeln!(out, "## Method").unwrap();
    writeln!(out).unwrap();
    let source_count = sources.len();
    let evidence_count = evidence.len();
    writeln!(
        out,
        "Reviewed {source_count} source(s), extracted {evidence_count} evidence span(s)."
    )
    .unwrap();
    writeln!(out).unwrap();

    // Sources reviewed
    if !sources.is_empty() {
        writeln!(out, "## Sources Reviewed").unwrap();
        writeln!(out).unwrap();
        for (i, src) in sources.iter().enumerate() {
            let num = i + 1;
            let title = src.title.as_deref().unwrap_or("Untitled");
            let quality = format!("{:?}", src.source_quality);
            writeln!(out, "{num}. **{title}** — quality: {quality}").unwrap();
            writeln!(out, "   URI: `{}`", src.uri).unwrap();
        }
        writeln!(out).unwrap();
    }

    // Decision matrix / comparison axes
    if !plan.comparison_axes.is_empty() {
        writeln!(out, "## Comparison Axes").unwrap();
        writeln!(out).unwrap();
        for axis in &plan.comparison_axes {
            writeln!(out, "- {axis}").unwrap();
        }
        writeln!(out).unwrap();
    }

    // Findings grouped by claim type
    let grouped = group_claims_by_type(claims);
    if !claims.is_empty() {
        writeln!(out, "## Findings").unwrap();
        writeln!(out).unwrap();

        let type_order = [
            ClaimType::Fact,
            ClaimType::Comparison,
            ClaimType::Inference,
            ClaimType::Recommendation,
        ];

        for ct in type_order {
            if let Some(group) = grouped.get(&ct) {
                let heading = match ct {
                    ClaimType::Fact => "Facts",
                    ClaimType::Comparison => "Comparisons",
                    ClaimType::Inference => "Inferences",
                    ClaimType::Recommendation => "Recommendations",
                    _ => continue,
                };
                writeln!(out, "### {heading}").unwrap();
                writeln!(out).unwrap();
                for claim in group {
                    let sid = short_id(&claim.id);
                    let conf = format!("{:?}", claim.confidence);
                    writeln!(out, "- **[{sid}]** {} (confidence: {conf})", claim.text)
                        .unwrap();
                    if !claim.evidence_ids.is_empty() {
                        let refs: Vec<String> = claim
                            .evidence_ids
                            .iter()
                            .map(|id| format!("[{}]", short_id(id)))
                            .collect();
                        writeln!(out, "  Evidence: {}", refs.join(", ")).unwrap();
                    }
                    if !claim.caveats.is_empty() {
                        for c in &claim.caveats {
                            writeln!(out, "  - Caveat: {c}").unwrap();
                        }
                    }
                }
                writeln!(out).unwrap();
            }
        }
    }

    // Risks and caveats
    let risks: Vec<&ClaimRecord> = claims
        .iter()
        .filter(|c| matches!(c.claim_type, ClaimType::Risk | ClaimType::Caveat))
        .collect();
    if !risks.is_empty() {
        writeln!(out, "## Risks and Caveats").unwrap();
        writeln!(out).unwrap();
        for claim in &risks {
            let sid = short_id(&claim.id);
            writeln!(out, "- **[{sid}]** {} ({})", claim.text, claim.claim_type.as_str())
                .unwrap();
        }
        writeln!(out).unwrap();
    }

    // Open questions
    let open: Vec<&ClaimRecord> = claims
        .iter()
        .filter(|c| c.claim_type == ClaimType::OpenQuestion)
        .collect();
    if !open.is_empty() {
        writeln!(out, "## Open Questions").unwrap();
        writeln!(out).unwrap();
        for claim in &open {
            let sid = short_id(&claim.id);
            writeln!(out, "- **[{sid}]** {}", claim.text).unwrap();
        }
        writeln!(out).unwrap();
    }

    // Contradictions
    if !contradictions.is_empty() {
        writeln!(out, "## Contradictions").unwrap();
        writeln!(out).unwrap();
        for contra in contradictions {
            let refs: Vec<String> = contra
                .claim_ids
                .iter()
                .map(|id| format!("[{}]", short_id(id)))
                .collect();
            writeln!(
                out,
                "- **[{}]** {} — claims: {} (severity: {:?})",
                short_id(&contra.id),
                contra.description,
                refs.join(", "),
                contra.severity
            )
            .unwrap();
        }
        writeln!(out).unwrap();
    }

    // Recommended validation work
    writeln!(out, "## Recommended Validation").unwrap();
    writeln!(out).unwrap();
    for open_claim in &open {
        let sid = short_id(&open_claim.id);
        writeln!(
            out,
            "- Resolve open question [{}]: {}",
            sid, open_claim.text
        )
        .unwrap();
    }
    if let Some(cond) = plan.stopping_conditions.first() {
        writeln!(out, "- Validation criterion: {cond}").unwrap();
    }
    writeln!(out).unwrap();

    // Bibliography
    if !sources.is_empty() {
        writeln!(out, "## Bibliography").unwrap();
        writeln!(out).unwrap();
        for (i, src) in sources.iter().enumerate() {
            let num = i + 1;
            let title = src.title.as_deref().unwrap_or("Untitled");
            writeln!(out, "{num}. {title}").unwrap();
            writeln!(out, "   `{}`", src.uri).unwrap();
        }
    }

    out
}

/// Render a human-facing brief memo.
pub fn render_human_brief(
    request: &ResearchRequest,
    claims: &[ClaimRecord],
    contradictions: &[ContradictionRecord],
) -> String {
    let mut out = String::with_capacity(1024);

    writeln!(out, "# Research Memo").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "**Question:** {}", request.question).unwrap();
    writeln!(out).unwrap();

    // Top recommendation
    if let Some(rec) = claims
        .iter()
        .find(|c| c.claim_type == ClaimType::Recommendation)
    {
        writeln!(out, "## Recommendation").unwrap();
        writeln!(out).unwrap();
        writeln!(out, "{}", rec.text).unwrap();
        writeln!(out, "\n**Confidence:** {:?}", rec.confidence).unwrap();
    } else {
        writeln!(out, "## Recommendation").unwrap();
        writeln!(out).unwrap();
        writeln!(out, "_No recommendation derived._").unwrap();
    }
    writeln!(out).unwrap();

    // Key caveats
    let caveats: Vec<&ClaimRecord> = claims
        .iter()
        .filter(|c| matches!(c.claim_type, ClaimType::Risk | ClaimType::Caveat))
        .collect();
    if !caveats.is_empty() {
        writeln!(out, "## Key Caveats").unwrap();
        writeln!(out).unwrap();
        for c in &caveats {
            writeln!(out, "- {}", c.text).unwrap();
        }
        writeln!(out).unwrap();
    }

    // Contradiction count
    if !contradictions.is_empty() {
        let count = contradictions.len();
        writeln!(out, "**{count} contradiction(s) detected** — see full report.")
            .unwrap();
    }

    out
}

/// Render an agent-facing compact answer.
pub fn render_agent_answer(
    _request: &ResearchRequest,
    claims: &[ClaimRecord],
    contradictions: &[ContradictionRecord],
    sources: &[SourceRecord],
    evidence: &[EvidenceSpan],
) -> String {
    let mut out = String::with_capacity(2048);

    writeln!(out, "# Research Answer").unwrap();
    writeln!(out).unwrap();

    // Direct answer from recommendation
    if let Some(rec) = claims
        .iter()
        .find(|c| c.claim_type == ClaimType::Recommendation)
    {
        writeln!(out, "## Answer").unwrap();
        writeln!(out).unwrap();
        writeln!(out, "{}", rec.text).unwrap();
        writeln!(out, "\n**Confidence:** {:?}", rec.confidence).unwrap();
    } else if let Some(fact) = claims.iter().find(|c| c.claim_type == ClaimType::Fact) {
        writeln!(out, "## Answer").unwrap();
        writeln!(out).unwrap();
        writeln!(out, "{}", fact.text).unwrap();
        writeln!(out, "\n**Confidence:** {:?}", fact.confidence).unwrap();
    }
    writeln!(out).unwrap();

    // Rationale with claim IDs
    let rationale: Vec<&ClaimRecord> = claims
        .iter()
        .filter(|c| matches!(c.claim_type, ClaimType::Fact | ClaimType::Inference))
        .collect();
    if !rationale.is_empty() {
        writeln!(out, "## Rationale").unwrap();
        writeln!(out).unwrap();
        for c in &rationale {
            let sid = short_id(&c.id);
            writeln!(out, "- **[{sid}]** {}", c.text).unwrap();
        }
        writeln!(out).unwrap();
    }

    // Caveats
    let caveats: Vec<&ClaimRecord> = claims
        .iter()
        .filter(|c| matches!(c.claim_type, ClaimType::Risk | ClaimType::Caveat))
        .collect();
    if !caveats.is_empty() {
        writeln!(out, "## Caveats").unwrap();
        writeln!(out).unwrap();
        for c in &caveats {
            let sid = short_id(&c.id);
            writeln!(out, "- **[{sid}]** {}", c.text).unwrap();
        }
        writeln!(out).unwrap();
    }

    // Contradictions / gaps
    if !contradictions.is_empty() {
        writeln!(out, "## Contradictions / Gaps").unwrap();
        writeln!(out).unwrap();
        for contra in contradictions {
            let refs: Vec<String> = contra
                .claim_ids
                .iter()
                .map(|id| format!("[{}]", short_id(id)))
                .collect();
            writeln!(
                out,
                "- **[{}]** {} — affecting: {}",
                short_id(&contra.id),
                contra.description,
                refs.join(", ")
            )
            .unwrap();
        }
        writeln!(out).unwrap();
    }

    // Do-not-assume list
    let open: Vec<&ClaimRecord> = claims
        .iter()
        .filter(|c| c.claim_type == ClaimType::OpenQuestion)
        .collect();
    if !open.is_empty() {
        writeln!(out, "## Do Not Assume").unwrap();
        writeln!(out).unwrap();
        for c in &open {
            writeln!(out, "- {}", c.text).unwrap();
        }
        writeln!(out).unwrap();
    }

    // Validation tasks
    writeln!(out, "## Validation Tasks").unwrap();
    writeln!(out).unwrap();
    for c in &open {
        let sid = short_id(&c.id);
        writeln!(out, "- [{}] Verify: {}", sid, c.text).unwrap();
    }
    if !contradictions.is_empty() {
        writeln!(out, "- Resolve all contradictions listed above").unwrap();
    }
    writeln!(out).unwrap();

    // Evidence pointers
    if !evidence.is_empty() {
        writeln!(out, "## Evidence").unwrap();
        writeln!(out).unwrap();
        for e in evidence {
            let sid = short_id(&e.id);
            let source = sources
                .iter()
                .find(|s| s.id == e.source_id)
                .map(|s| format!("{} ({})", short_id(&s.id), s.uri.as_str()))
                .unwrap_or_else(|| "unknown".to_string());
            let preview = truncate_str(&e.text, 120);
            writeln!(out, "- **[{sid}]** {preview} — source: {source}").unwrap();
        }
    }

    out
}

/// Render an agent-facing handoff context package.
pub fn render_agent_handoff(
    request: &ResearchRequest,
    plan: &ResearchPlan,
    claims: &[ClaimRecord],
    contradictions: &[ContradictionRecord],
    artifact_dir: &std::path::Path,
) -> String {
    let mut out = String::with_capacity(2048);

    writeln!(out, "# Research Handoff").unwrap();
    writeln!(out).unwrap();

    // Decision / framing
    writeln!(out, "## Decision Context").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "**Question:** {}", request.question).unwrap();
    writeln!(out, "**Mode:** {:?}", request.mode).unwrap();
    writeln!(out, "**Audience:** {:?}", request.audience).unwrap();
    writeln!(out, "\n**Scope:** {}", plan.scope).unwrap();
    writeln!(out).unwrap();

    // Operational guidance
    writeln!(out, "## Operational Guidance").unwrap();
    writeln!(out).unwrap();
    if let Some(rec) = claims
        .iter()
        .find(|c| c.claim_type == ClaimType::Recommendation)
    {
        writeln!(out, "Recommend: {}", rec.text).unwrap();
        writeln!(out, "Confidence: {:?}", rec.confidence).unwrap();
    } else {
        writeln!(out, "No recommendation available.").unwrap();
    }
    writeln!(out).unwrap();

    // Constraints
    if !request.constraints.is_empty() {
        writeln!(out, "## Constraints").unwrap();
        writeln!(out).unwrap();
        for c in &request.constraints {
            writeln!(out, "- {c}").unwrap();
        }
        writeln!(out).unwrap();
    }

    // Relevant claims (facts, inferences, risks)
    let relevant: Vec<&ClaimRecord> = claims
        .iter()
        .filter(|c| {
            matches!(
                c.claim_type,
                ClaimType::Fact | ClaimType::Inference | ClaimType::Risk
            )
        })
        .collect();
    if !relevant.is_empty() {
        writeln!(out, "## Relevant Claims").unwrap();
        writeln!(out).unwrap();
        for c in &relevant {
            let sid = short_id(&c.id);
            writeln!(
                out,
                "- **[{sid}]** ({}) {}",
                c.claim_type.as_str(),
                c.text
            )
            .unwrap();
        }
        writeln!(out).unwrap();
    }

    // Caveats
    let caveats: Vec<&ClaimRecord> = claims
        .iter()
        .filter(|c| matches!(c.claim_type, ClaimType::Caveat | ClaimType::Risk))
        .collect();
    if !caveats.is_empty() {
        writeln!(out, "## Caveats").unwrap();
        writeln!(out).unwrap();
        for c in &caveats {
            let sid = short_id(&c.id);
            writeln!(out, "- **[{sid}]** {}", c.text).unwrap();
        }
        writeln!(out).unwrap();
    }

    // Contradictions
    if !contradictions.is_empty() {
        writeln!(out, "## Contradictions").unwrap();
        writeln!(out).unwrap();
        for contra in contradictions {
            let refs: Vec<String> = contra
                .claim_ids
                .iter()
                .map(|id| format!("[{}]", short_id(id)))
                .collect();
            writeln!(
                out,
                "- [{}] {} (severity: {:?}) — claims: {}",
                short_id(&contra.id),
                contra.description,
                contra.severity,
                refs.join(", ")
            )
            .unwrap();
        }
        writeln!(out).unwrap();
    }

    // Suggested next actions
    let open: Vec<&ClaimRecord> = claims
        .iter()
        .filter(|c| c.claim_type == ClaimType::OpenQuestion)
        .collect();
    if !open.is_empty() {
        writeln!(out, "## Suggested Next Actions").unwrap();
        writeln!(out).unwrap();
        for c in &open {
            let sid = short_id(&c.id);
            writeln!(out, "- [{}] {}", sid, c.text).unwrap();
        }
        writeln!(out).unwrap();
    }

    // Artifact refs
    writeln!(out, "## Artifacts").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "`{}`", artifact_dir.display()).unwrap();
    if plan.comparison_axes.is_empty() {
        writeln!(out).unwrap();
    } else {
        writeln!(out).unwrap();
        writeln!(out, "## Comparison Axes").unwrap();
        writeln!(out).unwrap();
        for axis in &plan.comparison_axes {
            writeln!(out, "- {axis}").unwrap();
        }
    }

    out
}

/// Render evidence bundle as JSON string.
pub fn render_evidence_bundle(
    sources: &[SourceRecord],
    evidence: &[EvidenceSpan],
    claims: &[ClaimRecord],
    contradictions: &[ContradictionRecord],
) -> String {
    let bundle = serde_json::json!({
        "sources": sources,
        "evidence": evidence,
        "claims": claims,
        "contradictions": contradictions,
    });
    serde_json::to_string_pretty(&bundle).unwrap_or_else(|_| "{}".to_string())
}

// -- Internal helpers --

fn group_claims_by_type(claims: &[ClaimRecord]) -> HashMap<ClaimType, Vec<&ClaimRecord>> {
    let mut map: HashMap<ClaimType, Vec<&ClaimRecord>> = HashMap::new();
    for claim in claims {
        map.entry(claim.claim_type.clone()).or_default().push(claim);
    }
    map
}

fn short_id(id: &str) -> &str {
    if id.len() >= 8 {
        &id[..8]
    } else {
        id
    }
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        s.to_string()
    } else {
        format!("{}…", &s[..max_chars])
    }
}
