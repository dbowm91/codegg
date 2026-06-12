use std::path::Path;

use super::types::*;

// ---------------------------------------------------------------------------
// Preflight checks
// ---------------------------------------------------------------------------

const SECRET_PATTERNS: &[&str] = &[
    "API_KEY",
    "SECRET",
    "PASSWORD",
    "TOKEN",
    "PRIVATE_KEY",
    "api_key",
    "secret_key",
    "password",
    "private_key",
];

const UNSAFE_PATTERNS: &[&str] = &[
    "unsafe {",
    "unsafe fn",
    "unsafe impl",
    "transmute",
    "raw pointer",
];

/// Run deterministic preflight checks against target file paths.
///
/// These checks inspect **file names only**, not file contents.  Check names
/// and notes reflect this limitation explicitly.
pub fn run_preflight_checks(targets: &[SecurityReviewTarget]) -> Vec<SecurityPreflightResult> {
    let mut results = Vec::new();

    // Secret filename-hint scan — check file names for obvious indicators
    let secret_evidence: Vec<String> = targets
        .iter()
        .filter(|t| {
            let name = t
                .file_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_lowercase();
            SECRET_PATTERNS
                .iter()
                .any(|pat| name.contains(&pat.to_lowercase()))
        })
        .map(|t| format!("{}: file name matches secret hint", t.file_path.display()))
        .collect();

    let mut structured_secret_fn = Vec::new();
    if secret_evidence.is_empty() {
        results.push(SecurityPreflightResult {
            check_name: "secret_filename_hint_scan".to_string(),
            status: PreflightStatus::Pass,
            evidence: Vec::new(),
            structured_evidence: Vec::new(),
            notes: vec!["No secret filename hints detected in target file names".to_string()],
        });
    } else {
        for t in targets {
            let name = t
                .file_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_lowercase();
            if SECRET_PATTERNS
                .iter()
                .any(|pat| name.contains(&pat.to_lowercase()))
            {
                structured_secret_fn.push(SecurityPreflightEvidence {
                    file_path: t.file_path.clone(),
                    line: None,
                    summary: format!("{}: file name matches secret hint", t.file_path.display()),
                    detail: Some("filename/path hint only".to_string()),
                });
            }
        }
        results.push(SecurityPreflightResult {
            check_name: "secret_filename_hint_scan".to_string(),
            status: PreflightStatus::Fail,
            evidence: secret_evidence,
            structured_evidence: structured_secret_fn,
            notes: vec!["Secret-like filename hints found in target file names".to_string()],
        });
    }

    // Unsafe filename-hint scan — check file names for unsafe indicators
    let unsafe_evidence: Vec<String> = targets
        .iter()
        .filter(|t| {
            let name = t
                .file_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_lowercase();
            UNSAFE_PATTERNS
                .iter()
                .any(|pat| name.contains(&pat.to_lowercase()))
        })
        .map(|t| format!("{}: file name matches unsafe hint", t.file_path.display()))
        .collect();

    let mut structured_unsafe_fn = Vec::new();
    if unsafe_evidence.is_empty() {
        results.push(SecurityPreflightResult {
            check_name: "unsafe_filename_hint_scan".to_string(),
            status: PreflightStatus::Pass,
            evidence: Vec::new(),
            structured_evidence: Vec::new(),
            notes: vec!["No unsafe filename hints detected in target file names".to_string()],
        });
    } else {
        for t in targets {
            let name = t
                .file_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_lowercase();
            if UNSAFE_PATTERNS
                .iter()
                .any(|pat| name.contains(&pat.to_lowercase()))
            {
                structured_unsafe_fn.push(SecurityPreflightEvidence {
                    file_path: t.file_path.clone(),
                    line: None,
                    summary: format!("{}: file name matches unsafe hint", t.file_path.display()),
                    detail: Some("filename/path hint only".to_string()),
                });
            }
        }
        results.push(SecurityPreflightResult {
            check_name: "unsafe_filename_hint_scan".to_string(),
            status: PreflightStatus::Fail,
            evidence: unsafe_evidence,
            structured_evidence: structured_unsafe_fn,
            notes: vec!["Unsafe-like filename hints found in target file names".to_string()],
        });
    }

    results
}

// ---------------------------------------------------------------------------
// Content-aware preflight checks (full-file)
// ---------------------------------------------------------------------------

/// Content-aware preflight checks that scan file content for heuristic
/// security signals.  These are local, deterministic, and do not require
/// network access or external scanners.
pub fn run_content_preflight_checks(
    targets: &[SecurityReviewTarget],
    load_content: impl Fn(&Path) -> Option<String>,
) -> Vec<SecurityPreflightResult> {
    let mut results = Vec::new();

    // Hardcoded secret-like assignments
    let mut structured_secret = Vec::new();
    let secret_evidence: Vec<String> = targets
        .iter()
        .filter_map(|t| {
            let content = load_content(&t.file_path)?;
            let mut found_lines = Vec::new();
            for (idx, line) in content.lines().enumerate() {
                let lower = line.to_lowercase();
                if (lower.contains("api_key")
                    || lower.contains("secret")
                    || lower.contains("password")
                    || lower.contains("token")
                    || lower.contains("private_key"))
                    && (lower.contains("=") && !lower.contains("//"))
                {
                    found_lines.push(idx);
                    structured_secret.push(SecurityPreflightEvidence {
                        file_path: t.file_path.clone(),
                        line: Some((idx + 1) as u32),
                        summary: "hardcoded secret-like assignment in content".to_string(),
                        detail: Some("local heuristic content scan".to_string()),
                    });
                }
            }
            if found_lines.is_empty() {
                None
            } else {
                Some(format!(
                    "{}: hardcoded secret-like assignment in content",
                    t.file_path.display()
                ))
            }
        })
        .collect();

    if secret_evidence.is_empty() {
        results.push(SecurityPreflightResult {
            check_name: "secret_content_scan".to_string(),
            status: PreflightStatus::Pass,
            evidence: Vec::new(),
            structured_evidence: Vec::new(),
            notes: vec!["No hardcoded secret-like assignments found in content".to_string()],
        });
    } else {
        results.push(SecurityPreflightResult {
            check_name: "secret_content_scan".to_string(),
            status: PreflightStatus::Fail,
            evidence: secret_evidence,
            structured_evidence: structured_secret,
            notes: vec!["Hardcoded secret-like assignments found in content".to_string()],
        });
    }

    // Unsafe keyword usage
    let mut structured_unsafe = Vec::new();
    let unsafe_evidence: Vec<String> = targets
        .iter()
        .filter_map(|t| {
            let content = load_content(&t.file_path)?;
            let mut found_lines = Vec::new();
            for (idx, line) in content.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.contains("unsafe {")
                    || trimmed.starts_with("unsafe fn")
                    || trimmed.starts_with("unsafe impl")
                    || trimmed.contains("transmute")
                    || trimmed.contains("raw pointer")
                {
                    found_lines.push(idx);
                    structured_unsafe.push(SecurityPreflightEvidence {
                        file_path: t.file_path.clone(),
                        line: Some((idx + 1) as u32),
                        summary: "unsafe keyword usage in content".to_string(),
                        detail: Some("local heuristic content scan".to_string()),
                    });
                }
            }
            if found_lines.is_empty() {
                None
            } else {
                Some(format!(
                    "{}: unsafe keyword usage in content",
                    t.file_path.display()
                ))
            }
        })
        .collect();

    if unsafe_evidence.is_empty() {
        results.push(SecurityPreflightResult {
            check_name: "unsafe_content_scan".to_string(),
            status: PreflightStatus::Pass,
            evidence: Vec::new(),
            structured_evidence: Vec::new(),
            notes: vec!["No unsafe keyword usage found in content".to_string()],
        });
    } else {
        results.push(SecurityPreflightResult {
            check_name: "unsafe_content_scan".to_string(),
            status: PreflightStatus::Fail,
            evidence: unsafe_evidence,
            structured_evidence: structured_unsafe,
            notes: vec!["Unsafe keyword usage found in content".to_string()],
        });
    }

    // Process execution APIs
    let mut structured_process = Vec::new();
    let process_evidence: Vec<String> = targets
        .iter()
        .filter_map(|t| {
            let content = load_content(&t.file_path)?;
            let mut found_lines = Vec::new();
            for (idx, line) in content.lines().enumerate() {
                if line.contains("Command::new")
                    || line.contains("std::process::Command")
                    || line.contains("process::Command")
                    || line.contains("exec(")
                {
                    found_lines.push(idx);
                    structured_process.push(SecurityPreflightEvidence {
                        file_path: t.file_path.clone(),
                        line: Some((idx + 1) as u32),
                        summary: "process execution API in content".to_string(),
                        detail: Some("local heuristic content scan".to_string()),
                    });
                }
            }
            if found_lines.is_empty() {
                None
            } else {
                Some(format!(
                    "{}: process execution API in content",
                    t.file_path.display()
                ))
            }
        })
        .collect();

    if process_evidence.is_empty() {
        results.push(SecurityPreflightResult {
            check_name: "process_exec_scan".to_string(),
            status: PreflightStatus::Pass,
            evidence: Vec::new(),
            structured_evidence: Vec::new(),
            notes: vec!["No process execution APIs found in content".to_string()],
        });
    } else {
        results.push(SecurityPreflightResult {
            check_name: "process_exec_scan".to_string(),
            status: PreflightStatus::Fail,
            evidence: process_evidence,
            structured_evidence: structured_process,
            notes: vec!["Process execution APIs found in content".to_string()],
        });
    }

    // SQL string construction with format/interpolation
    let mut structured_sql = Vec::new();
    let sql_evidence: Vec<String> = targets
        .iter()
        .filter_map(|t| {
            let content = load_content(&t.file_path)?;
            let mut found_lines = Vec::new();
            for (idx, line) in content.lines().enumerate() {
                let lower = line.to_lowercase();
                if (lower.contains("format!") || lower.contains("format!("))
                    && (lower.contains("select")
                        || lower.contains("insert")
                        || lower.contains("update")
                        || lower.contains("delete"))
                {
                    found_lines.push(idx);
                    structured_sql.push(SecurityPreflightEvidence {
                        file_path: t.file_path.clone(),
                        line: Some((idx + 1) as u32),
                        summary: "SQL string construction with format interpolation".to_string(),
                        detail: Some("local heuristic content scan".to_string()),
                    });
                }
            }
            if found_lines.is_empty() {
                None
            } else {
                Some(format!(
                    "{}: SQL string construction with format interpolation",
                    t.file_path.display()
                ))
            }
        })
        .collect();

    if sql_evidence.is_empty() {
        results.push(SecurityPreflightResult {
            check_name: "sql_injection_scan".to_string(),
            status: PreflightStatus::Pass,
            evidence: Vec::new(),
            structured_evidence: Vec::new(),
            notes: vec!["No SQL string construction with interpolation found".to_string()],
        });
    } else {
        results.push(SecurityPreflightResult {
            check_name: "sql_injection_scan".to_string(),
            status: PreflightStatus::Fail,
            evidence: sql_evidence,
            structured_evidence: structured_sql,
            notes: vec!["SQL string construction with format interpolation found".to_string()],
        });
    }

    // Weak crypto names
    let mut structured_crypto = Vec::new();
    let crypto_evidence: Vec<String> = targets
        .iter()
        .filter_map(|t| {
            let content = load_content(&t.file_path)?;
            let mut found_lines = Vec::new();
            for (idx, line) in content.lines().enumerate() {
                let lower = line.to_lowercase();
                if lower.contains("md5")
                    || lower.contains("sha1")
                    || lower.contains("des")
                    || lower.contains("ecb")
                {
                    found_lines.push(idx);
                    structured_crypto.push(SecurityPreflightEvidence {
                        file_path: t.file_path.clone(),
                        line: Some((idx + 1) as u32),
                        summary: "weak crypto primitive in content".to_string(),
                        detail: Some("local heuristic content scan".to_string()),
                    });
                }
            }
            if found_lines.is_empty() {
                None
            } else {
                Some(format!(
                    "{}: weak crypto primitive in content",
                    t.file_path.display()
                ))
            }
        })
        .collect();

    if crypto_evidence.is_empty() {
        results.push(SecurityPreflightResult {
            check_name: "weak_crypto_scan".to_string(),
            status: PreflightStatus::Pass,
            evidence: Vec::new(),
            structured_evidence: Vec::new(),
            notes: vec!["No weak crypto primitives found in content".to_string()],
        });
    } else {
        results.push(SecurityPreflightResult {
            check_name: "weak_crypto_scan".to_string(),
            status: PreflightStatus::Fail,
            evidence: crypto_evidence,
            structured_evidence: structured_crypto,
            notes: vec!["Weak crypto primitives found in content".to_string()],
        });
    }

    results
}

// ---------------------------------------------------------------------------
// Locality-aware content preflight checks (hunk-local)
// ---------------------------------------------------------------------------

/// Locality-aware content preflight checks that scan only a window around
/// positioned targets.  For unpositioned targets, the full file is scanned
/// but evidence is marked as file-level.
pub fn run_content_preflight_checks_for_targets(
    targets: &[SecurityReviewTarget],
    load_content: impl Fn(&Path) -> Option<String>,
) -> Vec<SecurityPreflightResult> {
    const WINDOW_RADIUS: u32 = 10;
    let mut results = Vec::new();

    fn scan_lines_window(
        content: &str,
        target_line: Option<u32>,
        radius: u32,
    ) -> Vec<(usize, &str)> {
        match target_line {
            Some(line) => {
                let start = if line > radius {
                    (line - radius) as usize
                } else {
                    0
                };
                let end = (line + radius) as usize;
                content
                    .lines()
                    .enumerate()
                    .filter(|(i, _)| *i >= start && *i <= end)
                    .collect()
            }
            None => content.lines().enumerate().collect(),
        }
    }

    // secret_content_scan
    let mut structured_secret = Vec::new();
    let secret_evidence: Vec<String> = targets
        .iter()
        .filter_map(|t| {
            let content = load_content(&t.file_path)?;
            let lines = scan_lines_window(&content, t.line, WINDOW_RADIUS);
            let mut found = false;
            for (idx, line) in lines {
                let lower = line.to_lowercase();
                if (lower.contains("api_key")
                    || lower.contains("secret")
                    || lower.contains("password")
                    || lower.contains("token")
                    || lower.contains("private_key"))
                    && (lower.contains("=") && !lower.contains("//"))
                {
                    found = true;
                    structured_secret.push(SecurityPreflightEvidence {
                        file_path: t.file_path.clone(),
                        line: Some((idx + 1) as u32),
                        summary: "hardcoded secret-like assignment in content".to_string(),
                        detail: Some("local heuristic content scan (hunk-local)".to_string()),
                    });
                }
            }
            if found {
                Some(format!(
                    "{}: hardcoded secret-like assignment in content",
                    t.file_path.display()
                ))
            } else {
                None
            }
        })
        .collect();

    if secret_evidence.is_empty() {
        results.push(SecurityPreflightResult {
            check_name: "secret_content_scan".to_string(),
            status: PreflightStatus::Pass,
            evidence: Vec::new(),
            structured_evidence: Vec::new(),
            notes: vec!["No hardcoded secret-like assignments found in content".to_string()],
        });
    } else {
        results.push(SecurityPreflightResult {
            check_name: "secret_content_scan".to_string(),
            status: PreflightStatus::Fail,
            evidence: secret_evidence,
            structured_evidence: structured_secret,
            notes: vec!["Hardcoded secret-like assignments found in content".to_string()],
        });
    }

    // unsafe_content_scan
    let mut structured_unsafe = Vec::new();
    let unsafe_evidence: Vec<String> = targets
        .iter()
        .filter_map(|t| {
            let content = load_content(&t.file_path)?;
            let lines = scan_lines_window(&content, t.line, WINDOW_RADIUS);
            let mut found = false;
            for (idx, line) in lines {
                let trimmed = line.trim();
                if trimmed.contains("unsafe {")
                    || trimmed.starts_with("unsafe fn")
                    || trimmed.starts_with("unsafe impl")
                    || trimmed.contains("transmute")
                    || trimmed.contains("raw pointer")
                {
                    found = true;
                    structured_unsafe.push(SecurityPreflightEvidence {
                        file_path: t.file_path.clone(),
                        line: Some((idx + 1) as u32),
                        summary: "unsafe keyword usage in content".to_string(),
                        detail: Some("local heuristic content scan (hunk-local)".to_string()),
                    });
                }
            }
            if found {
                Some(format!(
                    "{}: unsafe keyword usage in content",
                    t.file_path.display()
                ))
            } else {
                None
            }
        })
        .collect();

    if unsafe_evidence.is_empty() {
        results.push(SecurityPreflightResult {
            check_name: "unsafe_content_scan".to_string(),
            status: PreflightStatus::Pass,
            evidence: Vec::new(),
            structured_evidence: Vec::new(),
            notes: vec!["No unsafe keyword usage found in content".to_string()],
        });
    } else {
        results.push(SecurityPreflightResult {
            check_name: "unsafe_content_scan".to_string(),
            status: PreflightStatus::Fail,
            evidence: unsafe_evidence,
            structured_evidence: structured_unsafe,
            notes: vec!["Unsafe keyword usage found in content".to_string()],
        });
    }

    // process_exec_scan
    let mut structured_process = Vec::new();
    let process_evidence: Vec<String> = targets
        .iter()
        .filter_map(|t| {
            let content = load_content(&t.file_path)?;
            let lines = scan_lines_window(&content, t.line, WINDOW_RADIUS);
            let mut found = false;
            for (idx, line) in lines {
                if line.contains("Command::new")
                    || line.contains("std::process::Command")
                    || line.contains("process::Command")
                    || line.contains("exec(")
                {
                    found = true;
                    structured_process.push(SecurityPreflightEvidence {
                        file_path: t.file_path.clone(),
                        line: Some((idx + 1) as u32),
                        summary: "process execution API in content".to_string(),
                        detail: Some("local heuristic content scan (hunk-local)".to_string()),
                    });
                }
            }
            if found {
                Some(format!(
                    "{}: process execution API in content",
                    t.file_path.display()
                ))
            } else {
                None
            }
        })
        .collect();

    if process_evidence.is_empty() {
        results.push(SecurityPreflightResult {
            check_name: "process_exec_scan".to_string(),
            status: PreflightStatus::Pass,
            evidence: Vec::new(),
            structured_evidence: Vec::new(),
            notes: vec!["No process execution APIs found in content".to_string()],
        });
    } else {
        results.push(SecurityPreflightResult {
            check_name: "process_exec_scan".to_string(),
            status: PreflightStatus::Fail,
            evidence: process_evidence,
            structured_evidence: structured_process,
            notes: vec!["Process execution APIs found in content".to_string()],
        });
    }

    // sql_injection_scan
    let mut structured_sql = Vec::new();
    let sql_evidence: Vec<String> = targets
        .iter()
        .filter_map(|t| {
            let content = load_content(&t.file_path)?;
            let lines = scan_lines_window(&content, t.line, WINDOW_RADIUS);
            let mut found = false;
            for (idx, line) in lines {
                let lower = line.to_lowercase();
                if (lower.contains("format!") || lower.contains("format!("))
                    && (lower.contains("select")
                        || lower.contains("insert")
                        || lower.contains("update")
                        || lower.contains("delete"))
                {
                    found = true;
                    structured_sql.push(SecurityPreflightEvidence {
                        file_path: t.file_path.clone(),
                        line: Some((idx + 1) as u32),
                        summary: "SQL string construction with format interpolation".to_string(),
                        detail: Some("local heuristic content scan (hunk-local)".to_string()),
                    });
                }
            }
            if found {
                Some(format!(
                    "{}: SQL string construction with format interpolation",
                    t.file_path.display()
                ))
            } else {
                None
            }
        })
        .collect();

    if sql_evidence.is_empty() {
        results.push(SecurityPreflightResult {
            check_name: "sql_injection_scan".to_string(),
            status: PreflightStatus::Pass,
            evidence: Vec::new(),
            structured_evidence: Vec::new(),
            notes: vec!["No SQL string construction with interpolation found".to_string()],
        });
    } else {
        results.push(SecurityPreflightResult {
            check_name: "sql_injection_scan".to_string(),
            status: PreflightStatus::Fail,
            evidence: sql_evidence,
            structured_evidence: structured_sql,
            notes: vec!["SQL string construction with format interpolation found".to_string()],
        });
    }

    // weak_crypto_scan
    let mut structured_crypto = Vec::new();
    let crypto_evidence: Vec<String> = targets
        .iter()
        .filter_map(|t| {
            let content = load_content(&t.file_path)?;
            let lines = scan_lines_window(&content, t.line, WINDOW_RADIUS);
            let mut found = false;
            for (idx, line) in lines {
                let lower = line.to_lowercase();
                if lower.contains("md5")
                    || lower.contains("sha1")
                    || lower.contains("des")
                    || lower.contains("ecb")
                {
                    found = true;
                    structured_crypto.push(SecurityPreflightEvidence {
                        file_path: t.file_path.clone(),
                        line: Some((idx + 1) as u32),
                        summary: "weak crypto primitive in content".to_string(),
                        detail: Some("local heuristic content scan (hunk-local)".to_string()),
                    });
                }
            }
            if found {
                Some(format!(
                    "{}: weak crypto primitive in content",
                    t.file_path.display()
                ))
            } else {
                None
            }
        })
        .collect();

    if crypto_evidence.is_empty() {
        results.push(SecurityPreflightResult {
            check_name: "weak_crypto_scan".to_string(),
            status: PreflightStatus::Pass,
            evidence: Vec::new(),
            structured_evidence: Vec::new(),
            notes: vec!["No weak crypto primitives found in content".to_string()],
        });
    } else {
        results.push(SecurityPreflightResult {
            check_name: "weak_crypto_scan".to_string(),
            status: PreflightStatus::Fail,
            evidence: crypto_evidence,
            structured_evidence: structured_crypto,
            notes: vec!["Weak crypto primitives found in content".to_string()],
        });
    }

    results
}
