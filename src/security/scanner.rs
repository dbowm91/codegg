use crate::error::ToolError;
use crate::security::finding::{
    Confidence, FindingMode, FindingSource, SecurityCategory, SecurityFinding, Severity,
};
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

static AWS_KEY_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"AKIA[0-9A-Z]{16}").unwrap());

static GITHUB_TOKEN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(ghp_[A-Za-z0-9]{36,}|gho_[A-Za-z0-9]{36,}|github_pat_[A-Za-z0-9_]{82,})").unwrap()
});

static OPENAI_KEY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"sk-[A-Za-z0-9]{20,}").unwrap());

static PRIVATE_KEY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"-----BEGIN\s+.*PRIVATE\s+KEY-----").unwrap());

static PASSWORD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)password\s*=\s*"[^"]{4,}""#).unwrap());

static API_KEY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)api[_-]?key\s*=\s*"[^"]{8,}""#).unwrap());

static SECRET_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)secret\s*=\s*"[^"]{8,}""#).unwrap());

static UNSAFE_BLOCK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"unsafe\s*\{").unwrap());

static UNSAFE_FN_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"unsafe\s+fn\s+").unwrap());

static DANGER_ACCEPT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"danger_accept_invalid_certs\s*\(\s*true\s*\)").unwrap());

static COMMAND_NEW_SH_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"Command::new\s*\(\s*"(sh|bash)"\s*\)"#).unwrap());

static CORS_WILDCARD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"allow_origin\s*\(\s*Any\s*\)|cors\s*=\s*\[\s*"\*"\s*\]"#).unwrap()
});

static BIND_ALL_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"0\.0\.0\.0"#).unwrap());

const DEPENDENCY_FILENAMES: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "package.json",
    "pnpm-lock.yaml",
    "requirements.txt",
    "pyproject.toml",
    "uv.lock",
    "Dockerfile",
];

const GITHUB_WORKFLOWS_PREFIX: &str = ".github/workflows/";

#[derive(Debug, Clone)]
struct Rule {
    re: Regex,
    category: SecurityCategory,
    severity: Severity,
    confidence: Confidence,
    recommendation: String,
}

static SECRET_RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        Rule {
            re: AWS_KEY_RE.clone(),
            category: SecurityCategory::SecretExposure,
            severity: Severity::Critical,
            confidence: Confidence::High,
            recommendation: "Remove AWS access key and rotate credentials immediately".into(),
        },
        Rule {
            re: GITHUB_TOKEN_RE.clone(),
            category: SecurityCategory::SecretExposure,
            severity: Severity::Critical,
            confidence: Confidence::High,
            recommendation: "Remove GitHub token and revoke it in GitHub settings".into(),
        },
        Rule {
            re: OPENAI_KEY_RE.clone(),
            category: SecurityCategory::SecretExposure,
            severity: Severity::Critical,
            confidence: Confidence::Medium,
            recommendation: "Remove OpenAI API key and regenerate in dashboard".into(),
        },
        Rule {
            re: PRIVATE_KEY_RE.clone(),
            category: SecurityCategory::SecretExposure,
            severity: Severity::Critical,
            confidence: Confidence::High,
            recommendation: "Remove private key from source code and rotate".into(),
        },
        Rule {
            re: PASSWORD_RE.clone(),
            category: SecurityCategory::SecretExposure,
            severity: Severity::High,
            confidence: Confidence::Medium,
            recommendation: "Move password to environment variable or secrets manager".into(),
        },
        Rule {
            re: API_KEY_RE.clone(),
            category: SecurityCategory::SecretExposure,
            severity: Severity::High,
            confidence: Confidence::Medium,
            recommendation: "Move API key to environment variable or secrets manager".into(),
        },
        Rule {
            re: SECRET_RE.clone(),
            category: SecurityCategory::SecretExposure,
            severity: Severity::High,
            confidence: Confidence::Medium,
            recommendation: "Move secret to environment variable or secrets manager".into(),
        },
    ]
});

static RUST_RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        Rule {
            re: UNSAFE_BLOCK_RE.clone(),
            category: SecurityCategory::UnsafeCode,
            severity: Severity::Medium,
            confidence: Confidence::Medium,
            recommendation: "Review unsafe block for soundness; consider safe alternatives".into(),
        },
        Rule {
            re: UNSAFE_FN_RE.clone(),
            category: SecurityCategory::UnsafeCode,
            severity: Severity::Medium,
            confidence: Confidence::Medium,
            recommendation: "Review unsafe fn for soundness; document safety invariants".into(),
        },
        Rule {
            re: DANGER_ACCEPT_RE.clone(),
            category: SecurityCategory::InsecureTls,
            severity: Severity::High,
            confidence: Confidence::High,
            recommendation: "Remove danger_accept_invalid_certs; validate certificates properly"
                .into(),
        },
        Rule {
            re: COMMAND_NEW_SH_RE.clone(),
            category: SecurityCategory::RemoteCodeExecution,
            severity: Severity::Medium,
            confidence: Confidence::High,
            recommendation: "Avoid spawning shell commands directly; use structured APIs".into(),
        },
    ]
});

static WEB_RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        Rule {
            re: CORS_WILDCARD_RE.clone(),
            category: SecurityCategory::ConfigRisk,
            severity: Severity::Medium,
            confidence: Confidence::High,
            recommendation: "Replace CORS wildcard with explicit allowed origins".into(),
        },
        Rule {
            re: BIND_ALL_RE.clone(),
            category: SecurityCategory::ConfigRisk,
            severity: Severity::Low,
            confidence: Confidence::Low,
            recommendation: "Bind to localhost or specific interface instead of 0.0.0.0".into(),
        },
    ]
});

fn is_dependency_file(path: &Path) -> bool {
    let file_name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return false,
    };
    if DEPENDENCY_FILENAMES.contains(&file_name) {
        return true;
    }
    // Check .github/workflows/*.yml
    if path.to_string_lossy().starts_with(GITHUB_WORKFLOWS_PREFIX) {
        return true;
    }
    false
}

fn make_finding(path: Option<&Path>, line: usize, rule: &Rule, evidence: &str) -> SecurityFinding {
    let context = path
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<text>".to_string());
    let id = SecurityFinding::deterministic_id("file", &rule.category, &context, line);
    SecurityFinding {
        id,
        severity: rule.severity,
        confidence: rule.confidence,
        category: rule.category.clone(),
        source: FindingSource::BuiltinHeuristic,
        mode: FindingMode::Deterministic,
        file: path.map(|p| p.to_path_buf()),
        line_range: Some((line, line)),
        evidence: evidence.to_string(),
        recommendation: rule.recommendation.clone(),
    }
}

fn inspect_lines(path: Option<&Path>, text: &str) -> Vec<SecurityFinding> {
    let mut findings = Vec::new();
    let mut supply_chain_added = false;

    for (idx, line) in text.lines().enumerate() {
        let line_num = idx + 1;

        // Supply chain risk for dependency files
        if let Some(p) = path {
            if is_dependency_file(p) && !supply_chain_added {
                supply_chain_added = true;
                let id = SecurityFinding::deterministic_id(
                    "file",
                    &SecurityCategory::SupplyChainRisk,
                    &p.display().to_string(),
                    1,
                );
                findings.push(SecurityFinding {
                    id,
                    severity: Severity::Info,
                    confidence: Confidence::High,
                    category: SecurityCategory::SupplyChainRisk,
                    source: FindingSource::DependencyInspector,
                    mode: FindingMode::Deterministic,
                    file: Some(p.to_path_buf()),
                    line_range: None,
                    evidence: "dependency or config file detected".into(),
                    recommendation: "Run dependency audit for this ecosystem".into(),
                });
            }
        }

        // Secret rules
        for rule in SECRET_RULES.iter() {
            if rule.re.is_match(line) {
                findings.push(make_finding(path, line_num, rule, line.trim()));
            }
        }

        // Rust security rules (only for .rs files)
        if path.and_then(|p| p.extension()).and_then(|e| e.to_str()) == Some("rs") {
            for rule in RUST_RULES.iter() {
                if rule.re.is_match(line) {
                    findings.push(make_finding(path, line_num, rule, line.trim()));
                }
            }
        }

        // Web/config rules
        for rule in WEB_RULES.iter() {
            if rule.re.is_match(line) {
                findings.push(make_finding(path, line_num, rule, line.trim()));
            }
        }
    }

    findings
}

pub fn inspect_text(path: Option<&std::path::Path>, text: &str) -> Vec<SecurityFinding> {
    inspect_lines(path, text)
}

pub async fn inspect_file(
    path: &std::path::Path,
    max_bytes: usize,
) -> Result<Vec<SecurityFinding>, ToolError> {
    let path_buf = path.to_path_buf();
    let max = max_bytes;

    let text = tokio::task::spawn_blocking(move || -> Result<String, ToolError> {
        let meta = std::fs::metadata(&path_buf)
            .map_err(|e| ToolError::Io(format!("failed to read {}: {}", path_buf.display(), e)))?;
        if meta.len() as usize > max {
            return Err(ToolError::Execution(format!(
                "file too large: {} ({} bytes, max {})",
                path_buf.display(),
                meta.len(),
                max
            )));
        }
        std::fs::read_to_string(&path_buf)
            .map_err(|e| ToolError::Io(format!("failed to read {}: {}", path_buf.display(), e)))
    })
    .await
    .map_err(|e| ToolError::Execution(format!("task join error: {}", e)))??;

    Ok(inspect_lines(Some(path), &text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn no_findings_for_harmless_code() {
        let code = r#"
fn main() {
    let x = 42;
    println!("hello world");
}
"#;
        let findings = inspect_text(None, code);
        assert!(
            findings.is_empty(),
            "expected no findings, got: {:?}",
            findings
        );
    }

    #[test]
    fn detect_aws_key() {
        let text = "aws_key = \"AKIAIOSFODNN7EXAMPLE\"";
        let findings = inspect_text(None, text);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, SecurityCategory::SecretExposure);
        assert_eq!(findings[0].severity, Severity::Critical);
        assert_eq!(findings[0].confidence, Confidence::High);
        assert!(findings[0].evidence.contains("AKIA"));
    }

    #[test]
    fn detect_github_token() {
        let text = "token: ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnop";
        let findings = inspect_text(None, text);
        assert!(!findings.is_empty());
        let secret_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.category == SecurityCategory::SecretExposure)
            .collect();
        assert_eq!(secret_findings.len(), 1);
        assert!(secret_findings[0].evidence.contains("ghp_"));
    }

    #[test]
    fn detect_gho_token() {
        let text = "GITHUB_TOKEN=gho_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnop";
        let findings = inspect_text(None, text);
        let secret_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.category == SecurityCategory::SecretExposure)
            .collect();
        assert!(!secret_findings.is_empty());
    }

    #[test]
    fn detect_github_pat() {
        // github_pat_ prefix + 82 alphanumeric/underscore chars
        let token = format!("github_pat_{}", "A".repeat(82));
        let text = format!("token = \"{}\"", token);
        let findings = inspect_text(None, &text);
        let secret_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.category == SecurityCategory::SecretExposure)
            .collect();
        assert!(!secret_findings.is_empty());
        assert!(secret_findings[0].evidence.contains("github_pat_"));
    }

    #[test]
    fn detect_openai_key() {
        let text = "api_key = \"sk-proj12345678901234567890\"";
        let findings = inspect_text(None, text);
        let secret_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.category == SecurityCategory::SecretExposure)
            .collect();
        assert_eq!(secret_findings.len(), 2);
        let has_sk = secret_findings.iter().any(|f| f.evidence.contains("sk-"));
        assert!(has_sk);
    }

    #[test]
    fn detect_private_key_block() {
        let text = "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA...";
        let findings = inspect_text(None, text);
        let secret_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.category == SecurityCategory::SecretExposure)
            .collect();
        assert_eq!(secret_findings.len(), 1);
        assert_eq!(secret_findings[0].severity, Severity::Critical);
        assert_eq!(secret_findings[0].confidence, Confidence::High);
    }

    #[test]
    fn detect_password_literal() {
        let text = r#"password = "supersecretpassword""#;
        let findings = inspect_text(None, text);
        let secret_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.category == SecurityCategory::SecretExposure)
            .collect();
        assert_eq!(secret_findings.len(), 1);
        assert_eq!(secret_findings[0].severity, Severity::High);
    }

    #[test]
    fn detect_api_key_literal() {
        let text = r#"api_key = "my_long_api_key_value_12345""#;
        let findings = inspect_text(None, text);
        let secret_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.category == SecurityCategory::SecretExposure)
            .collect();
        assert_eq!(secret_findings.len(), 1);
    }

    #[test]
    fn detect_secret_literal() {
        let text = r#"secret = "my_long_secret_value_abcde""#;
        let findings = inspect_text(None, text);
        let secret_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.category == SecurityCategory::SecretExposure)
            .collect();
        assert_eq!(secret_findings.len(), 1);
    }

    #[test]
    fn detect_unsafe_block() {
        let text = r#"fn main() { unsafe { ptr.write(42); } }"#;
        let findings = inspect_text(Some(Path::new("main.rs")), text);
        let unsafe_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.category == SecurityCategory::UnsafeCode)
            .collect();
        assert_eq!(unsafe_findings.len(), 1);
        assert_eq!(unsafe_findings[0].severity, Severity::Medium);
    }

    #[test]
    fn detect_unsafe_fn() {
        let text = "unsafe fn dangerous() {}";
        let findings = inspect_text(Some(Path::new("lib.rs")), text);
        let unsafe_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.category == SecurityCategory::UnsafeCode)
            .collect();
        assert_eq!(unsafe_findings.len(), 1);
    }

    #[test]
    fn unsafe_not_detected_in_non_rust() {
        let text = "unsafe { some_js_code() }";
        let findings = inspect_text(Some(Path::new("main.js")), text);
        let unsafe_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.category == SecurityCategory::UnsafeCode)
            .collect();
        assert!(unsafe_findings.is_empty());
    }

    #[test]
    fn detect_danger_accept_invalid_certs() {
        let text = "builder.danger_accept_invalid_certs(true)";
        let findings = inspect_text(Some(Path::new("tls.rs")), text);
        let tls_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.category == SecurityCategory::InsecureTls)
            .collect();
        assert_eq!(tls_findings.len(), 1);
        assert_eq!(tls_findings[0].severity, Severity::High);
        assert_eq!(tls_findings[0].confidence, Confidence::High);
    }

    #[test]
    fn detect_command_new_sh() {
        let text = r#"let cmd = Command::new("sh");"#;
        let findings = inspect_text(Some(Path::new("exec.rs")), text);
        let rce_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.category == SecurityCategory::RemoteCodeExecution)
            .collect();
        assert_eq!(rce_findings.len(), 1);
    }

    #[test]
    fn detect_command_new_bash() {
        let text = r#"Command::new("bash")"#;
        let findings = inspect_text(Some(Path::new("exec.rs")), text);
        let rce_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.category == SecurityCategory::RemoteCodeExecution)
            .collect();
        assert_eq!(rce_findings.len(), 1);
    }

    #[test]
    fn detect_cors_wildcard() {
        let text = "cors::Cors::default().allow_origin(Any)";
        let findings = inspect_text(None, text);
        let config_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.category == SecurityCategory::ConfigRisk)
            .collect();
        assert_eq!(config_findings.len(), 1);
    }

    #[test]
    fn detect_cors_toml() {
        let text = r#"cors = ["*"]"#;
        let findings = inspect_text(None, text);
        let config_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.category == SecurityCategory::ConfigRisk)
            .collect();
        assert_eq!(config_findings.len(), 1);
    }

    #[test]
    fn detect_bind_all() {
        let text = r#"addr = "0.0.0.0:8080""#;
        let findings = inspect_text(None, text);
        let config_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.category == SecurityCategory::ConfigRisk)
            .collect();
        assert_eq!(config_findings.len(), 1);
        assert_eq!(config_findings[0].severity, Severity::Low);
    }

    #[test]
    fn supply_chain_risk_for_cargo_toml() {
        let text = "[package]\nname = \"foo\"\nversion = \"0.1.0\"";
        let findings = inspect_text(Some(Path::new("Cargo.toml")), text);
        let supply_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.category == SecurityCategory::SupplyChainRisk)
            .collect();
        assert_eq!(supply_findings.len(), 1);
        assert_eq!(supply_findings[0].severity, Severity::Info);
    }

    #[test]
    fn supply_chain_risk_for_package_json() {
        let text = r#"{ "name": "foo", "version": "1.0.0" }"#;
        let findings = inspect_text(Some(Path::new("package.json")), text);
        let supply_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.category == SecurityCategory::SupplyChainRisk)
            .collect();
        assert_eq!(supply_findings.len(), 1);
    }

    #[test]
    fn supply_chain_risk_for_github_workflow() {
        let text = "name: CI\non: push";
        let findings = inspect_text(Some(Path::new(".github/workflows/ci.yml")), text);
        let supply_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.category == SecurityCategory::SupplyChainRisk)
            .collect();
        assert_eq!(supply_findings.len(), 1);
    }

    #[test]
    fn no_supply_chain_risk_for_regular_file() {
        let text = "fn main() {}";
        let findings = inspect_text(Some(Path::new("src/main.rs")), text);
        let supply_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.category == SecurityCategory::SupplyChainRisk)
            .collect();
        assert!(supply_findings.is_empty());
    }

    #[test]
    fn line_numbers_are_correct() {
        let text = "line1\nline2\nAKIAIOSFODNN7EXAMPLE12\nline4";
        let findings = inspect_text(None, text);
        let secret: Vec<_> = findings
            .iter()
            .filter(|f| f.category == SecurityCategory::SecretExposure)
            .collect();
        assert_eq!(secret.len(), 1);
        assert_eq!(secret[0].line_range, Some((3, 3)));
    }

    #[test]
    fn multiple_findings_in_one_line() {
        let text = r#"password = "secret12345678"; api_key = "another_secret_12345""#;
        let findings = inspect_text(None, text);
        let secret_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.category == SecurityCategory::SecretExposure)
            .collect();
        assert!(secret_findings.len() >= 2);
    }

    #[test]
    fn deterministic_ids_are_stable() {
        let text = "AKIAIOSFODNN7EXAMPLE12345";
        let f1 = inspect_text(None, text);
        let f2 = inspect_text(None, text);
        assert_eq!(f1.len(), f2.len());
        if let (Some(a), Some(b)) = (f1.first(), f2.first()) {
            assert_eq!(a.id, b.id);
        }
    }

    #[test]
    fn test_make_finding_id_format() {
        let f = make_finding(
            Some(Path::new("src/main.rs")),
            42,
            &Rule {
                re: Regex::new("test").unwrap(),
                category: SecurityCategory::SecretExposure,
                severity: Severity::High,
                confidence: Confidence::High,
                recommendation: "test".into(),
            },
            "evidence",
        );
        assert!(f.id.starts_with("file:"));
        assert!(f.id.contains(":42"));
    }

    #[test]
    fn secret_case_insensitive() {
        let text = r#"PASSWORD = "my_long_password_value""#;
        let findings = inspect_text(None, text);
        let secret_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.category == SecurityCategory::SecretExposure)
            .collect();
        assert_eq!(secret_findings.len(), 1);
    }

    #[test]
    fn empty_text() {
        let findings = inspect_text(None, "");
        assert!(findings.is_empty());
    }

    #[test]
    fn short_password_not_detected() {
        let text = r#"password = "abc""#;
        let findings = inspect_text(None, text);
        let secret_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.category == SecurityCategory::SecretExposure)
            .collect();
        assert!(secret_findings.is_empty());
    }

    #[tokio::test]
    async fn inspect_file_success() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.rs");
        std::fs::write(&file_path, "fn main() { unsafe { } }").unwrap();
        let findings = inspect_file(&file_path, 1_000_000).await.unwrap();
        let unsafe_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.category == SecurityCategory::UnsafeCode)
            .collect();
        assert_eq!(unsafe_findings.len(), 1);
        assert_eq!(unsafe_findings[0].file, Some(file_path));
    }

    #[tokio::test]
    async fn inspect_file_too_large() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("big.txt");
        std::fs::write(&file_path, "x".repeat(2000)).unwrap();
        let result = inspect_file(&file_path, 100).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn inspect_file_not_found() {
        let path = PathBuf::from("/nonexistent/file.txt");
        let result = inspect_file(&path, 1_000_000).await;
        assert!(result.is_err());
    }

    #[test]
    fn openai_key_not_too_short() {
        let text = "key = sk-short";
        let findings = inspect_text(None, text);
        let secret_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.category == SecurityCategory::SecretExposure)
            .collect();
        assert!(secret_findings.is_empty());
    }

    #[test]
    fn mixed_findings() {
        let text = r#"
# config.toml example
server_addr = "0.0.0.0:8080"
password = "hunter2_production_key"
-----BEGIN EC PRIVATE KEY-----
MIGTAgEAMBMGByqGSM49AgEGCCqGSM49AwEHA..."#;
        let findings = inspect_text(Some(Path::new("config.toml")), text);
        assert!(findings.len() >= 2);
        let categories: Vec<_> = findings.iter().map(|f| &f.category).collect();
        assert!(categories.contains(&&SecurityCategory::ConfigRisk));
        assert!(categories.contains(&&SecurityCategory::SecretExposure));
    }
}
