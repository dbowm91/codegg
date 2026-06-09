use crate::dependency::{detect_dependency_file, recommended_audit_commands};
use crate::finding::{
    Confidence, FindingMode, FindingSource, SecurityCategory, SecurityFinding, SecurityReport,
    Severity,
};
use crate::scanner::inspect_file;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityProfile {
    Ambient,
    DependencyDelta,
    PreCommit,
    SecurityReview,
}

impl SecurityProfile {
    pub fn as_str(&self) -> &'static str {
        match self {
            SecurityProfile::Ambient => "ambient",
            SecurityProfile::DependencyDelta => "dependency_delta",
            SecurityProfile::PreCommit => "pre_commit",
            SecurityProfile::SecurityReview => "security_review",
        }
    }
}

/// Crate-local placeholder for the subset of `codegg::SecurityConfig` that
/// `ProfileRunner` needs. Codegg converts its own config into this type at
/// the boundary so that `eggsentry` does not depend on Codegg config types.
#[derive(Debug, Clone, Default)]
pub struct ProfileConfig {
    pub max_bytes: Option<usize>,
}

pub struct ProfileRunner {
    #[allow(dead_code)]
    config: ProfileConfig,
}

impl ProfileRunner {
    pub fn new(config: ProfileConfig) -> Self {
        Self { config }
    }

    pub async fn inspect_paths(
        &self,
        profile: SecurityProfile,
        paths: &[PathBuf],
    ) -> SecurityReport {
        let mut report = SecurityReport {
            profile: Some(profile.as_str().to_string()),
            findings: Vec::new(),
            summary: String::new(),
        };

        match profile {
            SecurityProfile::Ambient => {
                self.run_ambient(&mut report, paths).await;
            }
            SecurityProfile::DependencyDelta => {
                self.run_dependency_delta(&mut report, paths).await;
            }
            SecurityProfile::PreCommit => {
                self.run_pre_commit(&mut report, paths).await;
            }
            SecurityProfile::SecurityReview => {
                self.run_security_review(&mut report, paths).await;
            }
        }

        report.summarize();
        report
    }

    async fn run_ambient(&self, report: &mut SecurityReport, paths: &[PathBuf]) {
        let max_bytes = self.config.max_bytes.unwrap_or(1_048_576);
        for path in paths {
            match inspect_file(path, max_bytes).await {
                Ok(findings) => report.findings.extend(findings),
                Err(_) => {
                    // File too large or unreadable - add info finding
                    let id = crate::finding::SecurityFinding::deterministic_id(
                        "file",
                        &crate::finding::SecurityCategory::Unknown,
                        &path.display().to_string(),
                        0,
                    );
                    report.findings.push(crate::finding::SecurityFinding {
                        id,
                        severity: Severity::Info,
                        confidence: Confidence::High,
                        category: SecurityCategory::Unknown,
                        source: FindingSource::BuiltinHeuristic,
                        mode: FindingMode::Deterministic,
                        file: Some(path.clone()),
                        line_range: None,
                        evidence: "file too large or unreadable for security scan".into(),
                        recommendation: "Reduce file size or adjust scan limits".into(),
                    });
                }
            }
        }
    }

    async fn run_dependency_delta(&self, report: &mut SecurityReport, paths: &[PathBuf]) {
        for path in paths {
            if let Some(eco) = detect_dependency_file(path) {
                let id = SecurityFinding::deterministic_id(
                    "file",
                    &SecurityCategory::SupplyChainRisk,
                    &path.display().to_string(),
                    1,
                );
                report.findings.push(SecurityFinding {
                    id,
                    severity: Severity::Info,
                    confidence: Confidence::High,
                    category: SecurityCategory::SupplyChainRisk,
                    source: FindingSource::DependencyInspector,
                    mode: FindingMode::Deterministic,
                    file: Some(path.clone()),
                    line_range: None,
                    evidence: format!("dependency file: {:?}", eco),
                    recommendation: "Run recommended audit commands".into(),
                });

                let cmds = recommended_audit_commands(eco);
                if !cmds.is_empty() {
                    let id = SecurityFinding::deterministic_id(
                        "file",
                        &SecurityCategory::SupplyChainRisk,
                        &path.display().to_string(),
                        2,
                    );
                    report.findings.push(SecurityFinding {
                        id,
                        severity: Severity::Info,
                        confidence: Confidence::High,
                        category: SecurityCategory::SupplyChainRisk,
                        source: FindingSource::DependencyInspector,
                        mode: FindingMode::Deterministic,
                        file: Some(path.clone()),
                        line_range: None,
                        evidence: format!("recommended commands: {}", cmds.join(", ")),
                        recommendation: "Execute audit commands".into(),
                    });
                }
            }
        }
    }

    async fn run_pre_commit(&self, report: &mut SecurityReport, paths: &[PathBuf]) {
        let max_bytes = 1_048_576;
        for path in paths {
            match inspect_file(path, max_bytes).await {
                Ok(findings) => report.findings.extend(findings),
                Err(e) => {
                    tracing::warn!("pre-commit scan failed for {}: {}", path.display(), e);
                }
            }
        }
    }

    async fn run_security_review(&self, report: &mut SecurityReport, paths: &[PathBuf]) {
        // Same as pre-commit: include all findings regardless of confidence
        let max_bytes = 1_048_576;
        for path in paths {
            match inspect_file(path, max_bytes).await {
                Ok(findings) => report.findings.extend(findings),
                Err(e) => {
                    tracing::warn!("security review scan failed for {}: {}", path.display(), e);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn default_config() -> ProfileConfig {
        ProfileConfig::default()
    }

    #[tokio::test]
    async fn ambient_inspects_supplied_files() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("main.rs");
        std::fs::write(&file, "fn main() { unsafe { } }").unwrap();

        let runner = ProfileRunner::new(default_config());
        let report = runner
            .inspect_paths(SecurityProfile::Ambient, std::slice::from_ref(&file))
            .await;

        let unsafe_findings: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.category == SecurityCategory::UnsafeCode)
            .collect();
        assert_eq!(unsafe_findings.len(), 1);
        assert_eq!(unsafe_findings[0].file, Some(file));
    }

    #[tokio::test]
    async fn ambient_skips_missing_files() {
        let runner = ProfileRunner::new(default_config());
        let report = runner
            .inspect_paths(
                SecurityProfile::Ambient,
                &[PathBuf::from("/nonexistent/file.rs")],
            )
            .await;

        // Missing files produce an info finding about being unreadable
        let info_findings: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.severity == Severity::Info)
            .collect();
        assert_eq!(info_findings.len(), 1);
        assert!(info_findings[0]
            .evidence
            .contains("too large or unreadable"));
    }

    #[tokio::test]
    async fn dependency_delta_detects_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let cargo = dir.path().join("Cargo.toml");
        std::fs::write(&cargo, "[package]\nname = \"test\"").unwrap();

        let runner = ProfileRunner::new(default_config());
        let report = runner
            .inspect_paths(
                SecurityProfile::DependencyDelta,
                std::slice::from_ref(&cargo),
            )
            .await;

        let supply_findings: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.category == SecurityCategory::SupplyChainRisk)
            .collect();
        assert_eq!(supply_findings.len(), 2); // file detection + recommended commands
        assert_eq!(supply_findings[0].file, Some(cargo));
    }

    #[tokio::test]
    async fn dependency_delta_empty_for_non_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("main.rs");
        std::fs::write(&file, "fn main() {}").unwrap();

        let runner = ProfileRunner::new(default_config());
        let report = runner
            .inspect_paths(SecurityProfile::DependencyDelta, &[file])
            .await;

        assert!(report.findings.is_empty());
    }

    #[tokio::test]
    async fn pre_commit_scans_files() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("secret.txt");
        std::fs::write(&file, r#"password = "supersecret123456""#).unwrap();

        let runner = ProfileRunner::new(default_config());
        let report = runner
            .inspect_paths(SecurityProfile::PreCommit, &[file])
            .await;

        let secret_findings: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.category == SecurityCategory::SecretExposure)
            .collect();
        assert_eq!(secret_findings.len(), 1);
    }

    #[tokio::test]
    async fn security_review_scans_files() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("tls.rs");
        std::fs::write(&file, "builder.danger_accept_invalid_certs(true)").unwrap();

        let runner = ProfileRunner::new(default_config());
        let report = runner
            .inspect_paths(SecurityProfile::SecurityReview, &[file])
            .await;

        let tls_findings: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.category == SecurityCategory::InsecureTls)
            .collect();
        assert_eq!(tls_findings.len(), 1);
    }

    #[tokio::test]
    async fn report_summary_is_populated() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("main.rs");
        std::fs::write(&file, "fn main() { unsafe { } }").unwrap();

        let runner = ProfileRunner::new(default_config());
        let report = runner
            .inspect_paths(SecurityProfile::Ambient, &[file])
            .await;

        assert!(!report.summary.is_empty());
        assert!(report.summary.contains("total"));
    }

    #[tokio::test]
    async fn profile_serialization_roundtrip() {
        let profiles = [
            SecurityProfile::Ambient,
            SecurityProfile::DependencyDelta,
            SecurityProfile::PreCommit,
            SecurityProfile::SecurityReview,
        ];
        for profile in &profiles {
            let json = serde_json::to_string(profile).unwrap();
            let deserialized: SecurityProfile = serde_json::from_str(&json).unwrap();
            assert_eq!(*profile, deserialized);
        }
    }

    #[tokio::test]
    async fn ambient_ignores_max_file_size() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("big.rs");
        // 2MB file
        let content = "x".repeat(2_000_000);
        std::fs::write(&file, &content).unwrap();

        let runner = ProfileRunner::new(default_config());
        let report = runner
            .inspect_paths(SecurityProfile::Ambient, &[file])
            .await;

        // Should return an error finding (file too large) but not crash
        assert!(!report.findings.is_empty());
    }
}
