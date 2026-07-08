use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::eggsact::adapter::{
    truncate_utf8_safe, EggsactCallResult, EggsactConfig, EggsactRuntime,
};
use crate::error::ToolError;

/// Severity level for a preflight finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreflightSeverity {
    /// Deterministic violation that would make the operation incorrect or unsafe.
    Block,
    /// Likely issue that should be surfaced but may not block.
    Warn,
    /// Informational finding for logs/provenance only.
    Annotate,
}

/// Location reference within a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightLocation {
    pub file: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

/// A structured finding from a preflight check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightFinding {
    pub severity: PreflightSeverity,
    pub machine_code: Option<String>,
    pub message: String,
    pub location: Option<PreflightLocation>,
    pub source_tool: String,
}

/// The preflight service's decision after running checks.
#[derive(Debug, Clone)]
pub enum PreflightDecision {
    Allow { findings: Vec<PreflightFinding> },
    Warn { findings: Vec<PreflightFinding> },
    Block { findings: Vec<PreflightFinding> },
}

impl PreflightDecision {
    /// Returns true if this decision blocks the operation.
    pub fn is_blocked(&self) -> bool {
        matches!(self, PreflightDecision::Block { .. })
    }

    /// Returns true if this decision has any warnings.
    pub fn has_warnings(&self) -> bool {
        matches!(self, PreflightDecision::Warn { .. })
    }

    /// Collect all findings regardless of decision variant.
    pub fn findings(&self) -> &[PreflightFinding] {
        match self {
            PreflightDecision::Allow { findings }
            | PreflightDecision::Warn { findings }
            | PreflightDecision::Block { findings } => findings,
        }
    }

    /// Format findings as a human-readable summary for tool output enrichment.
    pub fn summary(&self) -> String {
        let findings = self.findings();
        if findings.is_empty() {
            return String::new();
        }
        let mut parts = Vec::new();
        for f in findings {
            let tag = match f.severity {
                PreflightSeverity::Block => "BLOCK",
                PreflightSeverity::Warn => "WARN",
                PreflightSeverity::Annotate => "INFO",
            };
            parts.push(format!("[{tag}] {}: {}", f.source_tool, f.message));
        }
        parts.join("\n")
    }
}

/// Policy controlling preflight behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightPolicy {
    /// Master switch.
    pub enabled: bool,
    /// Global mode: off, observe, warn, block_on_definite.
    pub mode: PreflightMode,
    /// Enable patch/edit preflights.
    pub patch: bool,
    /// Enable config write preflights.
    pub config: bool,
    /// Enable shell command preflights.
    pub shell: bool,
    /// Enable unicode/identifier safety checks.
    pub unicode: bool,
    /// Log findings to tracing.
    pub log_findings: bool,
    /// Include findings in model-visible tool output.
    pub model_visible_findings: bool,
}

/// Preflight operating mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreflightMode {
    /// No preflight checks.
    Off,
    /// Log findings but never alter behavior.
    Observe,
    /// Surface warnings but never block.
    Warn,
    /// Block on deterministic correctness failures; warn on likely issues.
    BlockOnDefinite,
}

impl Default for PreflightPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: PreflightMode::Warn,
            patch: true,
            config: true,
            shell: true,
            unicode: true,
            log_findings: true,
            model_visible_findings: true,
        }
    }
}

impl From<crate::config::schema::PreflightMode> for PreflightMode {
    fn from(m: crate::config::schema::PreflightMode) -> Self {
        match m {
            crate::config::schema::PreflightMode::Off => PreflightMode::Off,
            crate::config::schema::PreflightMode::Observe => PreflightMode::Observe,
            crate::config::schema::PreflightMode::Warn => PreflightMode::Warn,
            crate::config::schema::PreflightMode::BlockOnDefinite => PreflightMode::BlockOnDefinite,
        }
    }
}

impl PreflightPolicy {
    /// Build a policy from the config schema type.
    pub fn from_config(config: &crate::config::schema::PreflightConfig) -> Self {
        Self {
            enabled: config.enabled.unwrap_or(true),
            mode: config
                .mode
                .map(PreflightMode::from)
                .unwrap_or(PreflightMode::Warn),
            patch: config.patch.unwrap_or(true),
            config: config.config.unwrap_or(true),
            shell: config.shell.unwrap_or(true),
            unicode: config.unicode.unwrap_or(true),
            log_findings: config.log_findings.unwrap_or(true),
            model_visible_findings: config.model_visible_findings.unwrap_or(true),
        }
    }

    /// Whether the given severity should block given this policy.
    pub fn should_block(&self, severity: PreflightSeverity) -> bool {
        self.enabled
            && self.mode == PreflightMode::BlockOnDefinite
            && severity == PreflightSeverity::Block
    }

    /// Whether findings should be surfaced in tool output.
    pub fn should_surface(&self) -> bool {
        self.enabled && self.model_visible_findings
    }
}

/// Harness-side preflight service wrapping eggsact runtime.
pub struct PreflightService {
    runtime: Arc<EggsactRuntime>,
    policy: PreflightPolicy,
}

impl PreflightService {
    /// Create a new preflight service with harness audience.
    pub fn new(policy: PreflightPolicy) -> Result<Self, ToolError> {
        let config = EggsactConfig {
            profile: "codegg_core".to_string(),
            audience: "harness".to_string(),
            max_output_chars: 8_000,
        };
        let runtime = Arc::new(EggsactRuntime::new(config)?);
        Ok(Self { runtime, policy })
    }

    /// Create a preflight service with an existing runtime (for testing or shared use).
    pub fn with_runtime(runtime: Arc<EggsactRuntime>, policy: PreflightPolicy) -> Self {
        Self { runtime, policy }
    }

    /// Get the current policy.
    pub fn policy(&self) -> &PreflightPolicy {
        &self.policy
    }

    /// Run `text_replace_check` to verify a replacement will apply cleanly.
    pub async fn check_text_replace(&self, text: &str, old: &str, new: &str) -> PreflightDecision {
        if !self.policy.enabled || !self.policy.patch {
            return PreflightDecision::Allow { findings: vec![] };
        }

        let args = json!({
            "text": text,
            "old": old,
            "new": new,
        });

        match self.runtime.call_json("text_replace_check", args) {
            Ok(result) => self.parse_replace_check_result(result),
            Err(e) => {
                tracing::debug!(error = %e, "preflight text_replace_check failed");
                PreflightDecision::Allow { findings: vec![] }
            }
        }
    }

    /// Parse a pre-computed `EggsactCallResult` from `text_replace_check` into a decision.
    /// Public for testing with synthetic results.
    pub fn parse_replace_check_result(&self, result: EggsactCallResult) -> PreflightDecision {
        let mut findings = Vec::new();

        // Extract match count and ambiguity from structured result first,
        // then fall back to string parsing.
        let (match_count, ambiguity) = if let Some(ref structured) = result.result {
            let mc = structured
                .get("match_count")
                .or_else(|| structured.get("matches"))
                .and_then(|v| v.as_u64())
                .map(|n| n as usize)
                .unwrap_or_else(|| parse_match_count(&result.output));
            let amb = structured
                .get("ambiguous")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
                || structured
                    .get("multiple_matches")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
            (mc, amb)
        } else {
            (parse_match_count(&result.output), false)
        };

        if match_count == 0 {
            findings.push(PreflightFinding {
                severity: PreflightSeverity::Block,
                machine_code: result.machine_code.clone(),
                message: "Replacement text not found in source — edit would have no effect"
                    .to_string(),
                location: None,
                source_tool: "text_replace_check".to_string(),
            });
        } else if ambiguity && match_count > 1 {
            findings.push(PreflightFinding {
                severity: PreflightSeverity::Block,
                machine_code: result.machine_code.clone(),
                message: format!(
                    "Replacement has {match_count} candidate matches — disambiguation required"
                ),
                location: None,
                source_tool: "text_replace_check".to_string(),
            });
        } else if match_count > 1 {
            findings.push(PreflightFinding {
                severity: PreflightSeverity::Warn,
                machine_code: result.machine_code.clone(),
                message: format!("Replacement matches {match_count} locations"),
                location: None,
                source_tool: "text_replace_check".to_string(),
            });
        }

        self.decide_from_findings(findings)
    }

    /// Run `validate_json` on config text.
    pub async fn check_json_valid(&self, text: &str) -> PreflightDecision {
        if !self.policy.enabled || !self.policy.config {
            return PreflightDecision::Allow { findings: vec![] };
        }

        let args = json!({ "text": text });
        match self.runtime.call_json("validate_json", args) {
            Ok(result) => {
                let mut findings = Vec::new();
                if !result.success {
                    findings.push(PreflightFinding {
                        severity: PreflightSeverity::Block,
                        machine_code: result.machine_code.clone(),
                        message: format!("Invalid JSON: {}", truncate(&result.output, 200)),
                        location: None,
                        source_tool: "validate_json".to_string(),
                    });
                }
                self.decide_from_findings(findings)
            }
            Err(e) => {
                tracing::debug!(error = %e, "preflight validate_json failed");
                PreflightDecision::Allow { findings: vec![] }
            }
        }
    }

    /// Run `validate_toml` on config text.
    pub async fn check_toml_valid(&self, text: &str) -> PreflightDecision {
        if !self.policy.enabled || !self.policy.config {
            return PreflightDecision::Allow { findings: vec![] };
        }

        let args = json!({ "text": text });
        match self.runtime.call_json("validate_toml", args) {
            Ok(result) => {
                let mut findings = Vec::new();
                if !result.success {
                    findings.push(PreflightFinding {
                        severity: PreflightSeverity::Block,
                        machine_code: result.machine_code.clone(),
                        message: format!("Invalid TOML: {}", truncate(&result.output, 200)),
                        location: None,
                        source_tool: "validate_toml".to_string(),
                    });
                }
                self.decide_from_findings(findings)
            }
            Err(e) => {
                tracing::debug!(error = %e, "preflight validate_toml failed");
                PreflightDecision::Allow { findings: vec![] }
            }
        }
    }

    /// Run `config_preflight` for auto-detected config format validation.
    pub async fn check_config(&self, text: &str) -> PreflightDecision {
        if !self.policy.enabled || !self.policy.config {
            return PreflightDecision::Allow { findings: vec![] };
        }

        let args = json!({ "text": text });
        match self.runtime.call_json("config_preflight", args) {
            Ok(result) => {
                let mut findings = Vec::new();
                if !result.success {
                    findings.push(PreflightFinding {
                        severity: PreflightSeverity::Block,
                        machine_code: result.machine_code.clone(),
                        message: format!(
                            "Config validation failed: {}",
                            truncate(&result.output, 200)
                        ),
                        location: None,
                        source_tool: "config_preflight".to_string(),
                    });
                }
                self.decide_from_findings(findings)
            }
            Err(e) => {
                tracing::debug!(error = %e, "preflight config_preflight failed");
                PreflightDecision::Allow { findings: vec![] }
            }
        }
    }

    /// Run `command_preflight` on a shell command.
    pub async fn check_command(&self, command: &str) -> PreflightDecision {
        if !self.policy.enabled || !self.policy.shell {
            return PreflightDecision::Allow { findings: vec![] };
        }

        let args = json!({ "command": command });
        match self.runtime.call_json("command_preflight", args) {
            Ok(result) => self.parse_command_result(result),
            Err(e) => {
                tracing::debug!(error = %e, "preflight command_preflight failed");
                PreflightDecision::Allow { findings: vec![] }
            }
        }
    }

    /// Parse a pre-computed `EggsactCallResult` from `command_preflight` into a decision.
    /// Public for testing with synthetic results.
    pub fn parse_command_result(&self, result: EggsactCallResult) -> PreflightDecision {
        let mut findings = Vec::new();

        // Extract verdict/risk from structured result first,
        // then fall back to string parsing.
        let (verdict, risk) = if let Some(ref structured) = result.result {
            let v = structured
                .get("verdict")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let r = structured
                .get("risk_level")
                .or_else(|| structured.get("risk"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            (v, r)
        } else {
            (None, None)
        };

        let output = &result.output;
        let is_block = verdict.as_deref() == Some("block")
            || risk.as_deref() == Some("high")
            || output.contains("risk: high")
            || output.contains("verdict: block");
        let is_warn = verdict.as_deref() == Some("warn")
            || risk.as_deref() == Some("medium")
            || output.contains("risk: medium")
            || output.contains("verdict: warn");

        if is_block {
            findings.push(PreflightFinding {
                severity: PreflightSeverity::Block,
                machine_code: result.machine_code.clone(),
                message: format!("Command preflight: {}", truncate(output, 200)),
                location: None,
                source_tool: "command_preflight".to_string(),
            });
        } else if is_warn {
            findings.push(PreflightFinding {
                severity: PreflightSeverity::Warn,
                machine_code: result.machine_code.clone(),
                message: format!("Command preflight: {}", truncate(output, 200)),
                location: None,
                source_tool: "command_preflight".to_string(),
            });
        }

        self.decide_from_findings(findings)
    }

    /// Run `text_security_inspect` on text for unicode/confusable issues.
    pub async fn check_text_security(&self, text: &str) -> PreflightDecision {
        if !self.policy.enabled || !self.policy.unicode {
            return PreflightDecision::Allow { findings: vec![] };
        }

        let args = json!({ "text": text });
        match self.runtime.call_json("text_security_inspect", args) {
            Ok(result) => self.parse_text_security_result(result),
            Err(e) => {
                tracing::debug!(error = %e, "preflight text_security_inspect failed");
                PreflightDecision::Allow { findings: vec![] }
            }
        }
    }

    /// Parse a pre-computed `EggsactCallResult` from `text_security_inspect` into a decision.
    /// Public for testing with synthetic results.
    pub fn parse_text_security_result(&self, result: EggsactCallResult) -> PreflightDecision {
        let mut findings = Vec::new();

        // Extract verdict from structured result first,
        // then fall back to string parsing.
        let verdict = result
            .result
            .as_ref()
            .and_then(|r| r.get("verdict"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Check findings for confusable entries in structured data
        let has_confusable_structured = result
            .findings
            .as_ref()
            .and_then(|f| f.as_array())
            .map(|arr| {
                arr.iter().any(|item| {
                    item.get("type")
                        .and_then(|v| v.as_str())
                        .map(|s| s == "confusable")
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);

        let output = &result.output;
        let is_block = verdict.as_deref() == Some("block") || output.contains("verdict: block");
        let is_review = verdict.as_deref() == Some("review") || output.contains("verdict: review");
        let has_confusable = has_confusable_structured || output.contains("confusable");

        if is_block {
            findings.push(PreflightFinding {
                severity: PreflightSeverity::Warn, // Unicode defaults to warn, not block
                machine_code: result.machine_code.clone(),
                message: format!("Text security: {}", truncate(output, 200)),
                location: None,
                source_tool: "text_security_inspect".to_string(),
            });
        } else if is_review || has_confusable {
            findings.push(PreflightFinding {
                severity: PreflightSeverity::Annotate,
                machine_code: result.machine_code.clone(),
                message: format!("Text security note: {}", truncate(output, 200)),
                location: None,
                source_tool: "text_security_inspect".to_string(),
            });
        }

        self.decide_from_findings(findings)
    }

    /// Evaluate a set of findings into a decision based on policy.
    fn decide_from_findings(&self, findings: Vec<PreflightFinding>) -> PreflightDecision {
        if findings.is_empty() {
            return PreflightDecision::Allow { findings };
        }

        if self.policy.log_findings {
            for f in &findings {
                match f.severity {
                    PreflightSeverity::Block => {
                        tracing::warn!(tool = %f.source_tool, message = %f.message, "preflight finding");
                    }
                    PreflightSeverity::Warn => {
                        tracing::info!(tool = %f.source_tool, message = %f.message, "preflight finding");
                    }
                    PreflightSeverity::Annotate => {
                        tracing::debug!(tool = %f.source_tool, message = %f.message, "preflight finding");
                    }
                }
            }
        }

        let has_block = findings
            .iter()
            .any(|f| f.severity == PreflightSeverity::Block);
        let has_warn = findings
            .iter()
            .any(|f| f.severity == PreflightSeverity::Warn);

        if has_block && self.policy.should_block(PreflightSeverity::Block) {
            PreflightDecision::Block { findings }
        } else if has_block || has_warn {
            PreflightDecision::Warn { findings }
        } else {
            PreflightDecision::Allow { findings }
        }
    }
}

/// Parse match count from text_replace_check output.
fn parse_match_count(output: &str) -> usize {
    // Look for patterns like "match_count: 1" or "matches: 2"
    for line in output.lines() {
        if let Some(rest) = line
            .strip_prefix("match_count:")
            .or_else(|| line.strip_prefix("matches:"))
        {
            if let Ok(n) = rest.trim().parse::<usize>() {
                return n;
            }
        }
    }
    // Fallback: if "ok: true" and no explicit count, assume 1 match
    if output.contains("ok: true") && !output.contains("match_count: 0") {
        return 1;
    }
    0
}

/// Truncate a string to max chars with ellipsis (UTF-8 safe).
fn truncate(s: &str, max: usize) -> String {
    truncate_utf8_safe(s, max, "...").text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_default_is_warn() {
        let policy = PreflightPolicy::default();
        assert_eq!(policy.mode, PreflightMode::Warn);
        assert!(policy.enabled);
        assert!(policy.patch);
        assert!(policy.config);
        assert!(policy.shell);
        assert!(policy.unicode);
    }

    #[test]
    fn should_block_only_in_block_on_definite() {
        let policy = PreflightPolicy {
            mode: PreflightMode::Warn,
            ..Default::default()
        };
        assert!(!policy.should_block(PreflightSeverity::Block));

        let policy = PreflightPolicy {
            mode: PreflightMode::BlockOnDefinite,
            ..Default::default()
        };
        assert!(policy.should_block(PreflightSeverity::Block));
        assert!(!policy.should_block(PreflightSeverity::Warn));
    }

    #[test]
    fn decision_is_blocked() {
        let d = PreflightDecision::Block { findings: vec![] };
        assert!(d.is_blocked());

        let d = PreflightDecision::Warn { findings: vec![] };
        assert!(!d.is_blocked());
    }

    #[test]
    fn decision_summary() {
        let d = PreflightDecision::Warn {
            findings: vec![
                PreflightFinding {
                    severity: PreflightSeverity::Warn,
                    machine_code: None,
                    message: "multiple matches".to_string(),
                    location: None,
                    source_tool: "text_replace_check".to_string(),
                },
                PreflightFinding {
                    severity: PreflightSeverity::Annotate,
                    machine_code: None,
                    message: "info note".to_string(),
                    location: None,
                    source_tool: "validate_json".to_string(),
                },
            ],
        };
        let summary = d.summary();
        assert!(summary.contains("[WARN]"));
        assert!(summary.contains("[INFO]"));
    }

    #[test]
    fn parse_match_count_works() {
        assert_eq!(parse_match_count("ok: true\nmatch_count: 3"), 3);
        assert_eq!(parse_match_count("ok: true\nmatches: 0"), 0);
        assert_eq!(parse_match_count("ok: true"), 1);
        assert_eq!(parse_match_count("ok: false"), 0);
    }

    #[test]
    fn truncate_works() {
        assert_eq!(truncate("hello", 10), "hello");
        // char-based: budget=5, marker=3 chars -> take 2 chars + "..."
        assert_eq!(truncate("hello world", 5), "he...");
    }
}
