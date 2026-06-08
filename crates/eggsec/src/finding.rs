use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityCategory {
    SecretExposure,
    DangerousCommand,
    DestructiveFilesystem,
    NetworkExfiltration,
    RemoteCodeExecution,
    DependencyVulnerability,
    DependencyRisk,
    UnsafeCode,
    PathTraversal,
    InsecureTls,
    SsrfRisk,
    AuthzRisk,
    SandboxEscapeRisk,
    SupplyChainRisk,
    ConfigRisk,
    Unknown,
}

impl SecurityCategory {
    pub fn label(&self) -> &'static str {
        match self {
            SecurityCategory::SecretExposure => "secret_exposure",
            SecurityCategory::DangerousCommand => "dangerous_command",
            SecurityCategory::DestructiveFilesystem => "destructive_filesystem",
            SecurityCategory::NetworkExfiltration => "network_exfiltration",
            SecurityCategory::RemoteCodeExecution => "remote_code_execution",
            SecurityCategory::DependencyVulnerability => "dependency_vulnerability",
            SecurityCategory::DependencyRisk => "dependency_risk",
            SecurityCategory::UnsafeCode => "unsafe_code",
            SecurityCategory::PathTraversal => "path_traversal",
            SecurityCategory::InsecureTls => "insecure_tls",
            SecurityCategory::SsrfRisk => "ssrf_risk",
            SecurityCategory::AuthzRisk => "authz_risk",
            SecurityCategory::SandboxEscapeRisk => "sandbox_escape_risk",
            SecurityCategory::SupplyChainRisk => "supply_chain_risk",
            SecurityCategory::ConfigRisk => "config_risk",
            SecurityCategory::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingSource {
    BuiltinHeuristic,
    CommandClassifier,
    DependencyInspector,
    ExternalTool,
    AgentReview,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingMode {
    Deterministic,
    Agentic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityFinding {
    pub id: String,
    pub severity: Severity,
    pub confidence: Confidence,
    pub category: SecurityCategory,
    pub source: FindingSource,
    pub mode: FindingMode,
    pub file: Option<PathBuf>,
    pub line_range: Option<(usize, usize)>,
    pub evidence: String,
    pub recommendation: String,
}

pub fn make_finding_id(prefix: &str, category: &SecurityCategory, evidence: &str) -> String {
    let cat_str = serde_json::to_string(category).unwrap_or_default();
    let input = format!("{}:{}:{}", prefix, cat_str, evidence);
    let hash = Sha256::digest(input.as_bytes());
    let short: String = hash.iter().take(8).map(|b| format!("{:02x}", b)).collect();
    format!("{}:{}:{}", prefix, cat_str.trim_matches('"'), short)
}

impl SecurityFinding {
    pub fn is_high_signal(&self) -> bool {
        matches!(
            (self.severity, self.confidence),
            (
                Severity::High | Severity::Critical,
                Confidence::Medium | Confidence::High
            ) | (Severity::Medium, Confidence::High)
        )
    }

    pub fn compact_summary(&self) -> String {
        let file_info = self
            .file
            .as_ref()
            .map(|f| f.display().to_string())
            .unwrap_or_else(|| "<text>".to_string());
        let line_info = self
            .line_range
            .map(|(start, end)| {
                if start == end {
                    format!(":{}", start)
                } else {
                    format!(":{}-{}", start, end)
                }
            })
            .unwrap_or_default();
        format!(
            "[{:?} {:?}] {}{}: {}",
            self.severity, self.confidence, file_info, line_info, self.evidence
        )
    }

    pub fn deterministic_id(
        prefix: &str,
        category: &SecurityCategory,
        context: &str,
        line: usize,
    ) -> String {
        let cat_str = serde_json::to_string(category).unwrap_or_default();
        let input = format!("{}:{}:{}:{}", prefix, cat_str, context, line);
        let hash = Sha256::digest(input.as_bytes());
        let short: String = hash.iter().take(8).map(|b| format!("{:02x}", b)).collect();
        format!("file:{}:{}:{}", cat_str.trim_matches('"'), short, line)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SecurityReport {
    pub profile: Option<String>,
    pub findings: Vec<SecurityFinding>,
    pub summary: String,
}

impl SecurityReport {
    pub fn summarize(&mut self) {
        let mut counts = [0u32; 5];
        for f in &self.findings {
            match f.severity {
                Severity::Info => counts[0] += 1,
                Severity::Low => counts[1] += 1,
                Severity::Medium => counts[2] += 1,
                Severity::High => counts[3] += 1,
                Severity::Critical => counts[4] += 1,
            }
        }
        let total = self.findings.len();
        self.summary = format!(
            "findings: {} total ({} info, {} low, {} medium, {} high, {} critical)",
            total, counts[0], counts[1], counts[2], counts[3], counts[4]
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_ordering() {
        assert!(Severity::Info < Severity::Low);
        assert!(Severity::Low < Severity::Medium);
        assert!(Severity::Medium < Severity::High);
        assert!(Severity::High < Severity::Critical);
    }

    #[test]
    fn is_high_signal_cases() {
        let high = SecurityFinding {
            id: "test".into(),
            severity: Severity::High,
            confidence: Confidence::Medium,
            category: SecurityCategory::SecretExposure,
            source: FindingSource::BuiltinHeuristic,
            mode: FindingMode::Deterministic,
            file: None,
            line_range: None,
            evidence: "test".into(),
            recommendation: "test".into(),
        };
        assert!(high.is_high_signal());

        let low = SecurityFinding {
            id: "test".into(),
            severity: Severity::Info,
            confidence: Confidence::Low,
            category: SecurityCategory::SecretExposure,
            source: FindingSource::BuiltinHeuristic,
            mode: FindingMode::Deterministic,
            file: None,
            line_range: None,
            evidence: "test".into(),
            recommendation: "test".into(),
        };
        assert!(!low.is_high_signal());

        let med_high = SecurityFinding {
            id: "test".into(),
            severity: Severity::Medium,
            confidence: Confidence::High,
            category: SecurityCategory::SecretExposure,
            source: FindingSource::BuiltinHeuristic,
            mode: FindingMode::Deterministic,
            file: None,
            line_range: None,
            evidence: "test".into(),
            recommendation: "test".into(),
        };
        assert!(med_high.is_high_signal());
    }

    #[test]
    fn compact_summary_no_file() {
        let f = SecurityFinding {
            id: "test".into(),
            severity: Severity::High,
            confidence: Confidence::High,
            category: SecurityCategory::SecretExposure,
            source: FindingSource::BuiltinHeuristic,
            mode: FindingMode::Deterministic,
            file: None,
            line_range: None,
            evidence: "found token".into(),
            recommendation: "remove".into(),
        };
        let s = f.compact_summary();
        assert!(s.contains("<text>"));
        assert!(s.contains("found token"));
    }

    #[test]
    fn compact_summary_with_file_and_line() {
        let f = SecurityFinding {
            id: "test".into(),
            severity: Severity::Medium,
            confidence: Confidence::Medium,
            category: SecurityCategory::UnsafeCode,
            source: FindingSource::BuiltinHeuristic,
            mode: FindingMode::Deterministic,
            file: Some(PathBuf::from("src/main.rs")),
            line_range: Some((10, 10)),
            evidence: "unsafe block".into(),
            recommendation: "remove unsafe".into(),
        };
        let s = f.compact_summary();
        assert!(s.contains("src/main.rs"));
        assert!(s.contains(":10"));
    }

    #[test]
    fn compact_summary_with_line_range() {
        let f = SecurityFinding {
            id: "test".into(),
            severity: Severity::Low,
            confidence: Confidence::Low,
            category: SecurityCategory::ConfigRisk,
            source: FindingSource::BuiltinHeuristic,
            mode: FindingMode::Deterministic,
            file: Some(PathBuf::from("config.toml")),
            line_range: Some((5, 8)),
            evidence: "wildcard".into(),
            recommendation: "restrict".into(),
        };
        let s = f.compact_summary();
        assert!(s.contains(":5-8"));
    }

    #[test]
    fn deterministic_id_is_stable() {
        let id1 = SecurityFinding::deterministic_id(
            "file",
            &SecurityCategory::SecretExposure,
            "src/main.rs",
            10,
        );
        let id2 = SecurityFinding::deterministic_id(
            "file",
            &SecurityCategory::SecretExposure,
            "src/main.rs",
            10,
        );
        assert_eq!(id1, id2);
    }

    #[test]
    fn deterministic_id_differs_by_line() {
        let id1 = SecurityFinding::deterministic_id(
            "file",
            &SecurityCategory::SecretExposure,
            "src/main.rs",
            10,
        );
        let id2 = SecurityFinding::deterministic_id(
            "file",
            &SecurityCategory::SecretExposure,
            "src/main.rs",
            20,
        );
        assert_ne!(id1, id2);
    }

    #[test]
    fn deterministic_id_differs_by_category() {
        let id1 = SecurityFinding::deterministic_id(
            "file",
            &SecurityCategory::SecretExposure,
            "src/main.rs",
            10,
        );
        let id2 = SecurityFinding::deterministic_id(
            "file",
            &SecurityCategory::UnsafeCode,
            "src/main.rs",
            10,
        );
        assert_ne!(id1, id2);
    }

    #[test]
    fn security_report_summarize() {
        let mut report = SecurityReport {
            profile: Some("test".into()),
            findings: vec![
                SecurityFinding {
                    id: "1".into(),
                    severity: Severity::Info,
                    confidence: Confidence::Low,
                    category: SecurityCategory::SupplyChainRisk,
                    source: FindingSource::BuiltinHeuristic,
                    mode: FindingMode::Deterministic,
                    file: None,
                    line_range: None,
                    evidence: "dep file".into(),
                    recommendation: "check deps".into(),
                },
                SecurityFinding {
                    id: "2".into(),
                    severity: Severity::High,
                    confidence: Confidence::High,
                    category: SecurityCategory::SecretExposure,
                    source: FindingSource::BuiltinHeuristic,
                    mode: FindingMode::Deterministic,
                    file: None,
                    line_range: None,
                    evidence: "key found".into(),
                    recommendation: "rotate".into(),
                },
                SecurityFinding {
                    id: "3".into(),
                    severity: Severity::Critical,
                    confidence: Confidence::High,
                    category: SecurityCategory::RemoteCodeExecution,
                    source: FindingSource::BuiltinHeuristic,
                    mode: FindingMode::Deterministic,
                    file: None,
                    line_range: None,
                    evidence: "rce".into(),
                    recommendation: "fix".into(),
                },
            ],
            summary: String::new(),
        };
        report.summarize();
        assert!(report.summary.contains("3 total"));
        assert!(report.summary.contains("1 info"));
        assert!(report.summary.contains("1 high"));
        assert!(report.summary.contains("1 critical"));
    }
}
