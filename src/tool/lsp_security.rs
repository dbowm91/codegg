use std::collections::HashSet;

use super::lsp::{DiagnosticSummary, SourceExcerpt, SymbolSummary};

pub(crate) const SECURITY_NEARBY_LINE_RADIUS: u32 = 20;

pub(crate) struct RiskPattern {
    pub category: &'static str,
    pub label: &'static str,
    pub needles: &'static [&'static str],
    pub rationale: &'static str,
}

#[derive(Serialize)]
pub(crate) struct SecurityRiskMarker {
    pub category: String,
    pub label: String,
    pub line: u32,
    pub column: u32,
    pub matched_text: String,
    pub rationale: String,
}

pub(crate) struct RiskScanResult {
    pub markers: Vec<SecurityRiskMarker>,
    pub truncated: bool,
}

pub(crate) const MAX_RISK_MATCHED_TEXT: usize = 120;

pub(crate) fn cap_vec<T>(items: Vec<T>, max: usize) -> (Vec<T>, bool) {
    let truncated = items.len() > max;
    (items.into_iter().take(max).collect(), truncated)
}

pub(crate) fn security_terms() -> &'static [&'static str] {
    &[
        "auth",
        "login",
        "token",
        "secret",
        "password",
        "session",
        "cookie",
        "jwt",
        "permission",
        "role",
        "admin",
        "encrypt",
        "decrypt",
        "sign",
        "verify",
        "parse",
        "deserialize",
        "upload",
        "download",
        "path",
        "file",
        "exec",
        "command",
        "shell",
        "unsafe",
        "crypt",
        "hash",
        "verify",
    ]
}

pub(crate) fn is_security_relevant_symbol(
    sym: &SymbolSummary,
    risk_markers: &[SecurityRiskMarker],
    target_line: Option<u32>,
) -> bool {
    if target_line.is_some_and(|t| sym.start_line <= t && sym.end_line >= t) {
        return true;
    }
    let name_lower = sym.name.to_lowercase();
    if security_terms()
        .iter()
        .any(|term| name_lower.contains(term))
    {
        return true;
    }
    risk_markers.iter().any(|m| {
        m.line >= sym.start_line.saturating_sub(SECURITY_NEARBY_LINE_RADIUS)
            && m.line <= sym.end_line.saturating_add(SECURITY_NEARBY_LINE_RADIUS)
    })
}

pub(crate) fn is_security_relevant_diagnostic(
    diag: &DiagnosticSummary,
    risk_markers: &[SecurityRiskMarker],
) -> bool {
    if diag.severity == "error" || diag.severity == "warning" {
        return true;
    }
    risk_markers.iter().any(|m| {
        m.line >= diag.line.saturating_sub(SECURITY_NEARBY_LINE_RADIUS)
            && m.line <= diag.line.saturating_add(SECURITY_NEARBY_LINE_RADIUS)
    })
}

pub(crate) fn scan_risk_markers(
    excerpt: &SourceExcerpt,
    categories: &Option<Vec<String>>,
    max_markers: usize,
) -> RiskScanResult {
    static PATTERNS: &[RiskPattern] = &[
        RiskPattern {
            category: "auth",
            label: "authentication/authorization",
            needles: &[
                "password",
                "Password",
                "PASSWORD",
                "login",
                "Login",
                "authenticate",
                "authorize",
                "jwt",
                "JWT",
                "bearer",
                "Bearer",
                "session",
                "cookie",
                "Cookie",
                "auth",
                "Auth",
            ],
            rationale: "authentication and authorization code controls access to resources",
        },
        RiskPattern {
            category: "crypto",
            label: "cryptographic operation",
            needles: &[
                "encrypt",
                "decrypt",
                "sign",
                "verify",
                "hash",
                "sha256",
                "sha512",
                "md5",
                "hmac",
                "aes",
                "rsa",
                "rand::random",
                "OsRng",
                "CryptoRng",
            ],
            rationale: "cryptographic operations must use correct algorithms and key management",
        },
        RiskPattern {
            category: "filesystem",
            label: "filesystem access",
            needles: &[
                "std::fs::",
                "tokio::fs::",
                "File::open",
                "File::create",
                "OpenOptions",
                "read_to_string",
                "write(",
                "create_dir",
            ],
            rationale: "filesystem access may need path validation and permission review",
        },
        RiskPattern {
            category: "network",
            label: "network boundary",
            needles: &[
                "TcpListener",
                "TcpStream",
                "UdpSocket",
                "axum::",
                "hyper::",
                "reqwest::",
                "hyper::Client",
                "bind(",
                "connect(",
            ],
            rationale: "network-facing code often processes untrusted input",
        },
        RiskPattern {
            category: "process",
            label: "process execution",
            needles: &[
                "Command::new",
                "std::process::Command",
                "tokio::process::Command",
                "exec(",
                "spawn(",
            ],
            rationale: "process execution can cross a trust boundary and requires input validation",
        },
        RiskPattern {
            category: "unsafe",
            label: "unsafe Rust",
            needles: &["unsafe {", "unsafe fn", "unsafe impl"],
            rationale: "unsafe blocks bypass compiler memory-safety guarantees and deserve review",
        },
        RiskPattern {
            category: "serialization",
            label: "serialization/deserialization",
            needles: &[
                "serde_json::from",
                "toml::from",
                "bincode::",
                "deserialize",
                "from_str(",
                "from_slice(",
            ],
            rationale: "deserialization can expand trust boundaries and parser attack surface",
        },
        RiskPattern {
            category: "sql",
            label: "database query",
            needles: &[
                "sqlx::query",
                "rusqlite",
                "SELECT ",
                "INSERT ",
                "UPDATE ",
                "DELETE ",
                "execute(",
                "prepare(",
            ],
            rationale: "database access should be reviewed for parameterization and authorization",
        },
        RiskPattern {
            category: "secrets",
            label: "secret material",
            needles: &[
                "API_KEY",
                "SECRET",
                "TOKEN",
                "PASSWORD",
                "Authorization",
                "credential",
                "private_key",
            ],
            rationale: "secret-bearing code should avoid logging and accidental exposure",
        },
        RiskPattern {
            category: "path_traversal",
            label: "path traversal risk",
            needles: &["../", "..\\", "path::join", "push(", "components()"],
            rationale: "path construction should be validated against traversal attacks",
        },
        RiskPattern {
            category: "concurrency",
            label: "concurrency primitive",
            needles: &[
                "unsafe {",
                "UnsafeCell",
                "transmute",
                "raw pointer",
                "AtomicPtr",
            ],
            rationale: "concurrency primitives require careful synchronization review",
        },
    ];

    let lines: Vec<&str> = excerpt.text.lines().collect();
    let mut markers = Vec::new();
    let category_filter: Option<HashSet<&str>> = categories
        .as_ref()
        .map(|cats| cats.iter().map(|s| s.as_str()).collect());

    for (line_offset, line_text) in lines.iter().enumerate() {
        let line_number = excerpt.start_line + line_offset as u32;
        for pattern in PATTERNS {
            if let Some(ref filter) = category_filter {
                if !filter.contains(pattern.category) {
                    continue;
                }
            }
            for &needle in pattern.needles {
                if let Some(col) = line_text.find(needle) {
                    let col_1indexed = col as u32 + 1;
                    let matched_text: String = line_text
                        .chars()
                        .skip(col)
                        .take(MAX_RISK_MATCHED_TEXT)
                        .collect();
                    markers.push(SecurityRiskMarker {
                        category: pattern.category.to_string(),
                        label: pattern.label.to_string(),
                        line: line_number,
                        column: col_1indexed,
                        matched_text,
                        rationale: pattern.rationale.to_string(),
                    });
                    break;
                }
            }
        }
    }

    markers.sort_by(|a, b| {
        a.line
            .cmp(&b.line)
            .then_with(|| a.category.cmp(&b.category))
    });
    let truncated = markers.len() > max_markers;
    markers.truncate(max_markers);
    RiskScanResult { markers, truncated }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SecurityPreset {
    RustServer,
    RustCli,
    WebBackend,
    DependencyReview,
    UnsafeReview,
}

pub(crate) fn parse_security_preset(input: Option<&str>) -> Result<Option<SecurityPreset>, String> {
    match input {
        None => Ok(None),
        Some("rust_server") => Ok(Some(SecurityPreset::RustServer)),
        Some("rust_cli") => Ok(Some(SecurityPreset::RustCli)),
        Some("web_backend") => Ok(Some(SecurityPreset::WebBackend)),
        Some("dependency_review") => Ok(Some(SecurityPreset::DependencyReview)),
        Some("unsafe_review") => Ok(Some(SecurityPreset::UnsafeReview)),
        Some(other) => Err(format!(
            "unknown security_preset \"{other}\": supported values are rust_server, rust_cli, web_backend, dependency_review, unsafe_review"
        )),
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SecurityPresetDefaults {
    pub categories: Vec<String>,
    pub radius: u32,
    pub max_risk_markers: usize,
    pub include_call_hierarchy: bool,
    pub note: &'static str,
}

pub(crate) fn preset_defaults(preset: SecurityPreset) -> SecurityPresetDefaults {
    match preset {
        SecurityPreset::RustServer => SecurityPresetDefaults {
            categories: vec![
                "auth",
                "network",
                "serialization",
                "filesystem",
                "process",
                "secrets",
                "sql",
                "path_traversal",
                "crypto",
                "unsafe",
                "concurrency",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            radius: 120,
            max_risk_markers: 120,
            include_call_hierarchy: true,
            note: "security preset rust_server applied: tuned for Rust network service review",
        },
        SecurityPreset::RustCli => SecurityPresetDefaults {
            categories: vec![
                "process",
                "filesystem",
                "secrets",
                "path_traversal",
                "serialization",
                "crypto",
                "unsafe",
                "concurrency",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            radius: 100,
            max_risk_markers: 100,
            include_call_hierarchy: true,
            note: "security preset rust_cli applied: tuned for CLI and local automation review",
        },
        SecurityPreset::WebBackend => SecurityPresetDefaults {
            categories: vec![
                "auth",
                "network",
                "serialization",
                "sql",
                "secrets",
                "filesystem",
                "path_traversal",
                "crypto",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            radius: 120,
            max_risk_markers: 120,
            include_call_hierarchy: true,
            note: "security preset web_backend applied: tuned for web handler and database review",
        },
        SecurityPreset::DependencyReview => SecurityPresetDefaults {
            categories: vec![
                "secrets",
                "filesystem",
                "process",
                "network",
                "serialization",
                "crypto",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            radius: 80,
            max_risk_markers: 80,
            include_call_hierarchy: false,
            note: "security preset dependency_review applied: tuned for dependency-sensitive files",
        },
        SecurityPreset::UnsafeReview => SecurityPresetDefaults {
            categories: vec!["unsafe", "concurrency", "filesystem", "process"]
                .into_iter()
                .map(String::from)
                .collect(),
            radius: 160,
            max_risk_markers: 120,
            include_call_hierarchy: true,
            note: "security preset unsafe_review applied: focused on unsafe and concurrency review",
        },
    }
}

pub(crate) fn security_preset_name(preset: SecurityPreset) -> &'static str {
    match preset {
        SecurityPreset::RustServer => "rust_server",
        SecurityPreset::RustCli => "rust_cli",
        SecurityPreset::WebBackend => "web_backend",
        SecurityPreset::DependencyReview => "dependency_review",
        SecurityPreset::UnsafeReview => "unsafe_review",
    }
}

use serde::Serialize;

#[cfg(test)]
mod tests {
    use super::*;

    fn make_excerpt(text: &str, start_line: u32) -> SourceExcerpt {
        let lines: Vec<&str> = text.lines().collect();
        SourceExcerpt {
            start_line,
            end_line: start_line + lines.len() as u32 - 1,
            text: text.to_string(),
        }
    }

    #[test]
    fn scanner_exact_cap_not_truncated() {
        let mut lines = Vec::new();
        for i in 0..5 {
            lines.push(format!("Command::new(\"{i}\");"));
        }
        let excerpt = make_excerpt(&lines.join("\n"), 1);
        let result = scan_risk_markers(&excerpt, &None, 5);
        assert_eq!(result.markers.len(), 5);
        assert!(!result.truncated);
    }

    #[test]
    fn scanner_over_cap_truncated() {
        let mut lines = Vec::new();
        for i in 0..200 {
            lines.push(format!("Command::new(\"{i}\");"));
        }
        let excerpt = make_excerpt(&lines.join("\n"), 1);
        let result = scan_risk_markers(&excerpt, &None, 3);
        assert!(result.markers.len() <= 3);
        assert!(result.truncated);
    }

    #[test]
    fn scanner_filters_categories() {
        let excerpt = make_excerpt(
            "use std::process::Command;\nuse std::fs::File;\nfn main() {}",
            1,
        );
        let result = scan_risk_markers(&excerpt, &Some(vec!["process".to_string()]), 80);
        assert!(result.markers.iter().all(|m| m.category == "process"));
    }

    #[test]
    fn scanner_preserves_line_numbers() {
        let excerpt = make_excerpt("fn main() {}\nunsafe { }\nfn foo() {}", 10);
        let result = scan_risk_markers(&excerpt, &None, 80);
        let unsafe_marker = result.markers.iter().find(|m| m.category == "unsafe");
        assert!(unsafe_marker.is_some());
        assert_eq!(unsafe_marker.unwrap().line, 11);
    }

    #[test]
    fn scanner_caps_matched_text() {
        let long_ident = "x".repeat(200);
        let excerpt = make_excerpt(&format!("let {long_ident} = unsafe {{ 1 }};"), 1);
        let result = scan_risk_markers(&excerpt, &None, 80);
        let unsafe_marker = result.markers.iter().find(|m| m.category == "unsafe");
        assert!(unsafe_marker.is_some());
        assert!(unsafe_marker.unwrap().matched_text.len() <= MAX_RISK_MATCHED_TEXT);
    }

    #[test]
    fn diagnostics_exact_cap_not_truncated() {
        let markers = vec![];
        let diags: Vec<DiagnosticSummary> = (0..80)
            .map(|i| DiagnosticSummary {
                file: "test.rs".to_string(),
                line: i + 1,
                column: 1,
                severity: "error".to_string(),
                source: None,
                code: None,
                message: format!("err {i}"),
            })
            .collect();
        let relevant: Vec<_> = diags
            .into_iter()
            .filter(|d| is_security_relevant_diagnostic(d, &markers))
            .collect();
        let (capped, truncated) = cap_vec(relevant, 80);
        assert_eq!(capped.len(), 80);
        assert!(!truncated);
    }

    #[test]
    fn diagnostics_over_cap_truncated() {
        let markers = vec![];
        let diags: Vec<DiagnosticSummary> = (0..85)
            .map(|i| DiagnosticSummary {
                file: "test.rs".to_string(),
                line: i + 1,
                column: 1,
                severity: "error".to_string(),
                source: None,
                code: None,
                message: format!("err {i}"),
            })
            .collect();
        let relevant: Vec<_> = diags
            .into_iter()
            .filter(|d| is_security_relevant_diagnostic(d, &markers))
            .collect();
        let (capped, truncated) = cap_vec(relevant, 80);
        assert_eq!(capped.len(), 80);
        assert!(truncated);
    }

    #[test]
    fn diagnostics_filter_before_cap_keeps_late_relevant() {
        let markers = vec![SecurityRiskMarker {
            category: "auth".to_string(),
            label: "test".to_string(),
            line: 200,
            column: 1,
            matched_text: "auth".to_string(),
            rationale: "test".to_string(),
        }];
        let mut diags: Vec<DiagnosticSummary> = Vec::new();
        for i in 0..80 {
            diags.push(DiagnosticSummary {
                file: "test.rs".to_string(),
                line: i + 1,
                column: 1,
                severity: "info".to_string(),
                source: None,
                code: None,
                message: format!("info {i}"),
            });
        }
        diags.push(DiagnosticSummary {
            file: "test.rs".to_string(),
            line: 200,
            column: 1,
            severity: "info".to_string(),
            source: None,
            code: None,
            message: "auth-related".to_string(),
        });
        let relevant: Vec<_> = diags
            .into_iter()
            .filter(|d| is_security_relevant_diagnostic(d, &markers))
            .collect();
        let (capped, truncated) = cap_vec(relevant, 80);
        assert_eq!(capped.len(), 1);
        assert!(!truncated);
        assert_eq!(capped[0].message, "auth-related");
    }

    #[test]
    fn symbols_exact_cap_not_truncated() {
        let markers = vec![];
        let syms: Vec<SymbolSummary> = (0..80)
            .map(|i| SymbolSummary {
                name: format!("exec_{i}"),
                kind: "function".to_string(),
                file: "test.rs".to_string(),
                start_line: i as u32 + 1,
                start_column: 1,
                end_line: i as u32 + 1,
                end_column: 10,
            })
            .collect();
        let relevant: Vec<_> = syms
            .into_iter()
            .filter(|s| is_security_relevant_symbol(s, &markers, None))
            .collect();
        let (capped, truncated) = cap_vec(relevant, 80);
        assert_eq!(capped.len(), 80);
        assert!(!truncated);
    }

    #[test]
    fn symbols_over_cap_truncated() {
        let markers = vec![];
        let syms: Vec<SymbolSummary> = (0..85)
            .map(|i| SymbolSummary {
                name: format!("exec_{i}"),
                kind: "function".to_string(),
                file: "test.rs".to_string(),
                start_line: i as u32 + 1,
                start_column: 1,
                end_line: i as u32 + 1,
                end_column: 10,
            })
            .collect();
        let relevant: Vec<_> = syms
            .into_iter()
            .filter(|s| is_security_relevant_symbol(s, &markers, None))
            .collect();
        let (capped, truncated) = cap_vec(relevant, 80);
        assert_eq!(capped.len(), 80);
        assert!(truncated);
    }

    #[test]
    fn symbols_target_line_included() {
        let markers = vec![];
        let syms = [SymbolSummary {
            name: "my_func".to_string(),
            kind: "function".to_string(),
            file: "test.rs".to_string(),
            start_line: 10,
            start_column: 1,
            end_line: 15,
            end_column: 1,
        }];
        assert!(is_security_relevant_symbol(&syms[0], &markers, Some(12)));
    }

    #[test]
    fn symbols_keyword_included() {
        let markers = vec![];
        let syms = [SymbolSummary {
            name: "handle_auth".to_string(),
            kind: "function".to_string(),
            file: "test.rs".to_string(),
            start_line: 1,
            start_column: 1,
            end_line: 1,
            end_column: 20,
        }];
        assert!(is_security_relevant_symbol(&syms[0], &markers, None));
    }

    #[test]
    fn symbols_near_marker_included() {
        let markers = vec![SecurityRiskMarker {
            category: "network".to_string(),
            label: "test".to_string(),
            line: 25,
            column: 1,
            matched_text: "connect".to_string(),
            rationale: "test".to_string(),
        }];
        let syms = [SymbolSummary {
            name: "my_func".to_string(),
            kind: "function".to_string(),
            file: "test.rs".to_string(),
            start_line: 30,
            start_column: 1,
            end_line: 35,
            end_column: 1,
        }];
        assert!(is_security_relevant_symbol(&syms[0], &markers, None));
    }

    #[test]
    fn security_preset_parse_accepts_all_known_presets() {
        assert_eq!(parse_security_preset(None).unwrap(), None);
        assert_eq!(
            parse_security_preset(Some("rust_server")).unwrap(),
            Some(SecurityPreset::RustServer)
        );
        assert_eq!(
            parse_security_preset(Some("rust_cli")).unwrap(),
            Some(SecurityPreset::RustCli)
        );
        assert_eq!(
            parse_security_preset(Some("web_backend")).unwrap(),
            Some(SecurityPreset::WebBackend)
        );
        assert_eq!(
            parse_security_preset(Some("dependency_review")).unwrap(),
            Some(SecurityPreset::DependencyReview)
        );
        assert_eq!(
            parse_security_preset(Some("unsafe_review")).unwrap(),
            Some(SecurityPreset::UnsafeReview)
        );
    }

    #[test]
    fn security_preset_parse_rejects_unknown() {
        let err = parse_security_preset(Some("bogus")).unwrap_err();
        assert!(err.contains("unknown security_preset"));
        assert!(err.contains("bogus"));
    }

    #[test]
    fn rust_server_preset_defaults_match_expected() {
        let d = preset_defaults(SecurityPreset::RustServer);
        assert_eq!(d.radius, 120);
        assert_eq!(d.max_risk_markers, 120);
        assert!(d.include_call_hierarchy);
        assert!(d.categories.contains(&"auth".to_string()));
        assert!(d.categories.contains(&"network".to_string()));
        assert!(d.categories.contains(&"sql".to_string()));
        assert_eq!(d.categories.len(), 11);
    }

    #[test]
    fn rust_cli_preset_defaults_match_expected() {
        let d = preset_defaults(SecurityPreset::RustCli);
        assert_eq!(d.radius, 100);
        assert_eq!(d.max_risk_markers, 100);
        assert!(d.include_call_hierarchy);
        assert!(d.categories.contains(&"process".to_string()));
        assert!(!d.categories.contains(&"auth".to_string()));
        assert_eq!(d.categories.len(), 8);
    }

    #[test]
    fn web_backend_preset_defaults_match_expected() {
        let d = preset_defaults(SecurityPreset::WebBackend);
        assert_eq!(d.radius, 120);
        assert_eq!(d.max_risk_markers, 120);
        assert!(d.include_call_hierarchy);
        assert!(d.categories.contains(&"auth".to_string()));
        assert!(d.categories.contains(&"sql".to_string()));
        assert_eq!(d.categories.len(), 8);
    }

    #[test]
    fn dependency_review_preset_disables_call_hierarchy_by_default() {
        let d = preset_defaults(SecurityPreset::DependencyReview);
        assert!(!d.include_call_hierarchy);
        assert_eq!(d.radius, 80);
        assert_eq!(d.max_risk_markers, 80);
        assert!(d.categories.contains(&"secrets".to_string()));
        assert_eq!(d.categories.len(), 6);
    }

    #[test]
    fn unsafe_review_preset_focuses_unsafe_and_concurrency() {
        let d = preset_defaults(SecurityPreset::UnsafeReview);
        assert!(d.categories.contains(&"unsafe".to_string()));
        assert!(d.categories.contains(&"concurrency".to_string()));
        assert_eq!(d.radius, 160);
        assert_eq!(d.max_risk_markers, 120);
        assert!(d.include_call_hierarchy);
        assert_eq!(d.categories.len(), 4);
    }
}
