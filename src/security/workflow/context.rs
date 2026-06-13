use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::types::*;

// ---------------------------------------------------------------------------
// SecurityContext executor boundary
// ---------------------------------------------------------------------------

/// Trait for executing bounded, read-only `securityContext` LSP requests.
///
/// Implementations must be `Send + Sync` so enrichment can run concurrently.
/// Errors are captured as notes, never panics.
#[async_trait::async_trait]
pub trait SecurityContextExecutor: Send + Sync {
    async fn security_context(
        &self,
        request: serde_json::Value,
    ) -> Result<serde_json::Value, String>;
}

/// No-op executor for deterministic tests and when no LSP is available.
pub struct NoopSecurityContextExecutor;

#[async_trait::async_trait]
impl SecurityContextExecutor for NoopSecurityContextExecutor {
    async fn security_context(
        &self,
        _request: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        Err("no securityContext executor available".to_string())
    }
}

/// Fixture executor for unit tests. Returns pre-configured responses or
/// failures keyed by file path.
pub struct FixtureSecurityContextExecutor {
    pub responses: HashMap<PathBuf, serde_json::Value>,
    pub failures: HashMap<PathBuf, String>,
    /// Track which requests were made (file_path -> request).
    pub requests: Mutex<Vec<serde_json::Value>>,
}

impl Default for FixtureSecurityContextExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl FixtureSecurityContextExecutor {
    pub fn new() -> Self {
        Self {
            responses: HashMap::new(),
            failures: HashMap::new(),
            requests: Mutex::new(Vec::new()),
        }
    }

    pub fn with_response(path: PathBuf, response: serde_json::Value) -> Self {
        let mut responses = HashMap::new();
        responses.insert(path, response);
        Self {
            responses,
            failures: HashMap::new(),
            requests: Mutex::new(Vec::new()),
        }
    }

    pub fn with_failure(path: PathBuf, error: String) -> Self {
        let mut failures = HashMap::new();
        failures.insert(path, error);
        Self {
            responses: HashMap::new(),
            failures,
            requests: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait::async_trait]
impl SecurityContextExecutor for FixtureSecurityContextExecutor {
    async fn security_context(
        &self,
        request: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        if let Ok(mut reqs) = self.requests.lock() {
            reqs.push(request.clone());
        }

        let file_path = request
            .get("file_path")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .unwrap_or_default();

        if let Some(err) = self.failures.get(&file_path) {
            return Err(err.clone());
        }

        self.responses
            .get(&file_path)
            .cloned()
            .ok_or_else(|| format!("no fixture response for {}", file_path.display()))
    }
}

// ---------------------------------------------------------------------------
// HunkSourceContext executor boundary
// ---------------------------------------------------------------------------

/// Executor boundary for `hunkSourceContext` LSP requests in the
/// security review workflow. Mirrors [`SecurityContextExecutor`].
#[async_trait::async_trait]
pub trait HunkSourceContextExecutor: Send + Sync {
    /// Execute a hunk source navigation request and return the response.
    async fn execute_hunk_source_context(
        &self,
        request: egglsp::hunk_context::HunkSourceNavigationRequest,
    ) -> Result<egglsp::hunk_context::HunkSourceNavigationResponse, String>;
}

/// No-op executor that always returns an error. Used in tests and
/// as a type parameter when no executor is available.
pub struct NoopHunkSourceContextExecutor;

#[async_trait::async_trait]
impl HunkSourceContextExecutor for NoopHunkSourceContextExecutor {
    async fn execute_hunk_source_context(
        &self,
        _request: egglsp::hunk_context::HunkSourceNavigationRequest,
    ) -> Result<egglsp::hunk_context::HunkSourceNavigationResponse, String> {
        Err("no hunkSourceContext executor available".to_string())
    }
}

// ---------------------------------------------------------------------------
// Build securityContext request payloads
// ---------------------------------------------------------------------------

/// Build a `securityContext` request payload for a security review target.
///
/// Returns a JSON value matching the `securityContext` operation input
/// schema.  `call_depth` is always 0 in this vertical slice.
pub fn build_security_context_request(target: &SecurityReviewTarget) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    map.insert(
        "operation".to_string(),
        serde_json::Value::String("securityContext".to_string()),
    );
    map.insert(
        "file_path".to_string(),
        serde_json::Value::String(target.file_path.to_string_lossy().to_string()),
    );
    map.insert(
        "security_preset".to_string(),
        serde_json::Value::String(target.preset.clone()),
    );
    map.insert("max_risk_markers".to_string(), serde_json::json!(80));
    map.insert("call_depth".to_string(), serde_json::json!(0));

    if let Some(line) = target.line {
        map.insert("line".to_string(), serde_json::json!(line));
    }
    if let Some(column) = target.column {
        map.insert("column".to_string(), serde_json::json!(column));
    }

    serde_json::Value::Object(map)
}

// ---------------------------------------------------------------------------
// Convert securityContext packets to review prompts
// ---------------------------------------------------------------------------

/// Convert parsed `securityContext` JSON output into review prompts.
///
/// Each risk marker becomes a [`SecurityReviewPrompt`].  No markers are
/// turned into findings in this vertical slice.  Malformed or missing
/// fields fail soft with empty prompts or notes.
pub fn prompts_from_security_context(
    target: &SecurityReviewTarget,
    context_json: &serde_json::Value,
) -> Vec<SecurityReviewPrompt> {
    let mut prompts = Vec::new();

    let risk_markers = match context_json.get("risk_markers") {
        Some(serde_json::Value::Array(arr)) => arr,
        _ => return prompts,
    };

    let truncated = context_json
        .get("truncated")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    for marker in risk_markers {
        let category = marker
            .get("category")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let label = marker
            .get("label")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let marker_line = marker
            .get("line")
            .and_then(|v| v.as_u64())
            .map(|l| l as u32);
        let matched_text = marker
            .get("matched_text")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let rationale = marker
            .get("rationale")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let file_path = marker
            .get("file")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .unwrap_or_else(|| target.file_path.clone());

        let line = marker_line.or(target.line);

        let title = format!(
            "Review {}: {}",
            category.as_deref().unwrap_or("unknown"),
            label
        );

        let mut evidence = vec![
            "source: securityContext.risk_marker".to_string(),
            category.unwrap_or_default(),
            matched_text.to_string(),
            rationale.to_string(),
            format!("target reason: {:?}", target.reason),
            format!("preset: {}", target.preset),
        ];

        if truncated {
            evidence.push("context was truncated".to_string());
        }

        prompts.push(SecurityReviewPrompt {
            file_path,
            line,
            preset: target.preset.clone(),
            category: marker
                .get("category")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            title,
            rationale: rationale.to_string(),
            evidence,
        });
    }

    prompts
}

// ---------------------------------------------------------------------------
// Finding synthesis (produces only prompts in the vertical slice)
// ---------------------------------------------------------------------------

/// Synthesize review prompts from targets, risk markers, and preflight
/// results.
///
/// In the vertical slice, risk markers **always** become
/// [`SecurityReviewPrompt`]s — never findings.  The returned
/// `Vec<SecurityReviewFinding>` is always empty by design.  Preflight
/// failures are also surfaced as prompts.
pub fn synthesize_review_prompts_only(
    _targets: &[SecurityReviewTarget],
    risk_markers: &[SecurityRiskMarkerFromWorkflow],
    preflight: &[SecurityPreflightResult],
) -> (Vec<SecurityReviewFinding>, Vec<SecurityReviewPrompt>) {
    let mut prompts = Vec::new();

    // Process risk markers — always prompts, never findings
    for marker in risk_markers {
        prompts.push(SecurityReviewPrompt {
            file_path: marker.file_path.clone(),
            line: Some(marker.line),
            preset: String::new(),
            category: Some(marker.category.clone()),
            title: format!("Review {}: {}", marker.category, marker.label),
            rationale: marker.rationale.clone(),
            evidence: vec![
                marker.category.clone(),
                marker.matched_text.clone(),
                marker.rationale.clone(),
            ],
        });
    }

    // Process preflight failures — also prompts
    for result in preflight {
        if result.status == PreflightStatus::Fail {
            for evidence_str in &result.evidence {
                prompts.push(SecurityReviewPrompt {
                    file_path: PathBuf::new(),
                    line: None,
                    preset: String::new(),
                    category: Some(result.check_name.clone()),
                    title: format!("Preflight check failed: {}", result.check_name),
                    rationale: result.notes.join("; "),
                    evidence: vec![evidence_str.clone()],
                });
            }
        }
    }

    // Findings are always empty in the vertical slice
    (Vec::new(), prompts)
}

/// Deprecated: use [`synthesize_review_prompts_only`] or
/// [`super::evidence::synthesize_evidence_based_findings`].
pub fn synthesize_findings(
    targets: &[SecurityReviewTarget],
    risk_markers: &[SecurityRiskMarkerFromWorkflow],
    preflight: &[SecurityPreflightResult],
) -> (Vec<SecurityReviewFinding>, Vec<SecurityReviewPrompt>) {
    synthesize_review_prompts_only(targets, risk_markers, preflight)
}

// ---------------------------------------------------------------------------
// Security context escalation
// ---------------------------------------------------------------------------

/// Escalation level for securityContext requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SecurityContextEscalationLevel {
    /// Do not request securityContext.
    None,
    /// Basic context with call_depth=0.
    Basic,
    /// Call expansion with depth=1.
    CallDepth1,
    /// Call expansion with depth=2.
    CallDepth2,
}

impl std::fmt::Display for SecurityContextEscalationLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "none"),
            Self::Basic => write!(f, "basic"),
            Self::CallDepth1 => write!(f, "call_depth_1"),
            Self::CallDepth2 => write!(f, "call_depth_2"),
        }
    }
}

/// Decide the escalation level for a securityContext request based on
/// the target, any preliminary finding, and any review prompt.
///
/// Rules:
/// - `None`: low-risk changed hunk with no prompt/finding.
/// - `Basic`: marker prompt with no finding, or dependency review target.
/// - `CallDepth1`: eligible finding with Medium+ severity, or target reason
///   is AuthOrSecretHandling, ProcessExecution, NetworkBoundary, or UnsafeCode.
/// - `CallDepth2`: only for High severity with Medium+ confidence and category
///   auth, process, unsafe, secret, or sql.
pub fn choose_security_context_escalation(
    target: &SecurityReviewTarget,
    finding: Option<&SecurityReviewFinding>,
    prompt: Option<&SecurityReviewPrompt>,
) -> SecurityContextEscalationLevel {
    // CallDepth2: highest severity finding with strong confidence
    if let Some(f) = finding {
        if f.severity >= SecuritySeverity::High && f.confidence >= SecurityConfidence::Medium {
            let eligible_category = matches!(
                f.category.as_deref(),
                Some("auth" | "process" | "unsafe" | "secret" | "sql")
            );
            if eligible_category {
                return SecurityContextEscalationLevel::CallDepth2;
            }
        }
    }

    // CallDepth1: Medium+ severity finding, or high-risk target reason
    if let Some(f) = finding {
        if f.severity >= SecuritySeverity::Medium {
            return SecurityContextEscalationLevel::CallDepth1;
        }
    }

    let high_risk_reason = matches!(
        target.reason,
        SecurityTargetReason::AuthOrSecretHandling
            | SecurityTargetReason::ProcessExecution
            | SecurityTargetReason::NetworkBoundary
            | SecurityTargetReason::UnsafeCode
    );
    if high_risk_reason {
        return SecurityContextEscalationLevel::CallDepth1;
    }

    // Basic: marker prompt present, or dependency review target
    if prompt.is_some() {
        return SecurityContextEscalationLevel::Basic;
    }
    if target.reason == SecurityTargetReason::DependencyMetadata {
        return SecurityContextEscalationLevel::Basic;
    }

    // None: low-risk with no signal
    SecurityContextEscalationLevel::None
}

/// Build a securityContext request JSON payload for an escalated target.
///
/// Maps escalation level to LSP parameters:
/// - `None`: returns empty object (caller should skip the request)
/// - `Basic`: call_depth=0, default caps
/// - `CallDepth1`: call_depth=1, max_call_nodes=32
/// - `CallDepth2`: call_depth=2, max_call_nodes=64
pub fn build_escalated_security_context_request(
    target: &SecurityReviewTarget,
    level: SecurityContextEscalationLevel,
) -> serde_json::Value {
    let mut request = serde_json::json!({
        "file_path": target.file_path,
        "security_preset": target.preset,
    });

    if let Some(line) = target.line {
        request["line"] = serde_json::json!(line);
    }
    if let Some(col) = target.column {
        request["column"] = serde_json::json!(col);
    }

    match level {
        SecurityContextEscalationLevel::None => {}
        SecurityContextEscalationLevel::Basic => {
            request["call_depth"] = serde_json::json!(0);
            request["max_risk_markers"] = serde_json::json!(80);
        }
        SecurityContextEscalationLevel::CallDepth1 => {
            request["call_depth"] = serde_json::json!(1);
            request["max_risk_markers"] = serde_json::json!(80);
            request["max_call_nodes"] = serde_json::json!(32);
        }
        SecurityContextEscalationLevel::CallDepth2 => {
            request["call_depth"] = serde_json::json!(2);
            request["max_risk_markers"] = serde_json::json!(80);
            request["max_call_nodes"] = serde_json::json!(64);
        }
    }

    request
}

// ---------------------------------------------------------------------------
// Escalation planning
// ---------------------------------------------------------------------------

/// An escalation plan for a single security review target.
///
/// This is a policy output — it recommends whether and how to call
/// `securityContext` LSP for a target, but does NOT execute it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityContextEscalationPlan {
    /// The target being evaluated.
    pub target: SecurityReviewTarget,
    /// Recommended escalation level.
    pub level: SecurityContextEscalationLevel,
    /// Pre-built securityContext request payload (None when level is None).
    pub request: Option<serde_json::Value>,
    /// Human-readable reason for the recommended level.
    pub reason: String,
}

/// Generate escalation plans for all targets in a security review output.
///
/// Maps findings and prompts back to targets by file path, then uses
/// `choose_security_context_escalation` to determine the level.
/// The request payload is pre-built but NOT executed.
pub fn plan_security_context_escalations(
    output: &SecurityReviewOutput,
) -> Vec<SecurityContextEscalationPlan> {
    let mut plans = Vec::new();

    for target in &output.targets {
        let best_finding = output
            .findings
            .iter()
            .filter(|f| f.file_path == target.file_path)
            .max_by_key(|f| &f.severity);

        let best_prompt = output
            .review_prompts
            .iter()
            .find(|p| p.file_path == target.file_path);

        let level = choose_security_context_escalation(target, best_finding, best_prompt);

        let reason = match level {
            SecurityContextEscalationLevel::None => {
                "low-risk target with no findings or prompts".to_string()
            }
            SecurityContextEscalationLevel::Basic => {
                if best_prompt.is_some() {
                    "review prompt present for target".to_string()
                } else {
                    "dependency review target".to_string()
                }
            }
            SecurityContextEscalationLevel::CallDepth1 => {
                if let Some(f) = best_finding {
                    format!("finding with {} severity", f.severity)
                } else {
                    "high-risk target reason".to_string()
                }
            }
            SecurityContextEscalationLevel::CallDepth2 => {
                if let Some(f) = best_finding {
                    format!(
                        "high-severity finding with {} confidence in {} category",
                        f.confidence,
                        f.category.as_deref().unwrap_or("unknown")
                    )
                } else {
                    "high-risk target".to_string()
                }
            }
        };

        let request = if level == SecurityContextEscalationLevel::None {
            None
        } else {
            Some(build_escalated_security_context_request(target, level))
        };

        plans.push(SecurityContextEscalationPlan {
            target: target.clone(),
            level,
            request,
            reason,
        });
    }

    plans
}

// ---------------------------------------------------------------------------
// Executor provider abstraction
// ---------------------------------------------------------------------------

/// Provider abstraction for obtaining a security context executor at
/// command execution time.  This avoids hardwiring `NoopSecurityContextExecutor`
/// inside the command handler.
pub trait SecurityContextExecutorProvider {
    fn security_context_executor(&self) -> Option<Arc<dyn SecurityContextExecutor>>;
}
