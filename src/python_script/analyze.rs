use super::types::{PythonRiskAssessment, PythonRiskLevel};

/// Analyze Python code for static risk indicators.
///
/// This is NOT a proof of safety. It feeds the capability envelope and
/// permission prompts. Runtime sandbox/snapshot checks remain required.
pub fn analyze_python_risk(code: &str) -> PythonRiskAssessment {
    let mut reasons = Vec::new();
    let mut imports = Vec::new();

    // --- Import detection ---
    for line in code.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("import ") || trimmed.starts_with("from ") {
            let module = if let Some(rest) = trimmed.strip_prefix("from ") {
                rest.split_whitespace().next().unwrap_or("")
            } else {
                trimmed
                    .strip_prefix("import ")
                    .unwrap_or("")
                    .split([',', ' '])
                    .next()
                    .unwrap_or("")
            };
            if !module.is_empty() {
                imports.push(module.to_string());
            }
        }
    }

    // --- Suspicious import detection ---
    let suspicious_imports: &[&str] = &[
        "subprocess",
        "os",
        "socket",
        "urllib",
        "requests",
        "httpx",
        "ctypes",
        "pickle",
        "marshal",
        "pty",
        "shutil",
    ];
    let mut found_suspicious = Vec::new();
    for imp in &imports {
        for &susp in suspicious_imports {
            if imp == susp {
                found_suspicious.push(imp.clone());
            }
        }
    }

    // --- Dynamic execution detection ---
    let has_dynamic_execution = code.contains("eval(")
        || code.contains("exec(")
        || code.contains("compile(")
        || code.contains("__import__(");

    if has_dynamic_execution {
        reasons.push("dynamic code execution detected".to_string());
    }

    // --- File I/O detection ---
    let has_file_io = code.contains("open(")
        || code.contains(".write(")
        || code.contains(".read(")
        || code.contains("os.remove")
        || code.contains("os.unlink");

    if has_file_io {
        reasons.push("file I/O operations detected".to_string());
    }

    // --- Subprocess detection ---
    let has_subprocess = code.contains("subprocess.")
        || code.contains("os.system")
        || code.contains("os.popen")
        || code.contains("os.exec");

    if has_subprocess {
        reasons.push("subprocess calls detected".to_string());
    }

    // --- Network detection ---
    let has_network = code.contains("requests.")
        || code.contains("urllib")
        || code.contains("http.client")
        || code.contains("socket.")
        || code.contains("httpx.");

    if has_network {
        reasons.push("network access detected".to_string());
    }

    // --- Destructive operations detection ---
    let has_destructive_ops = code.contains("shutil.rmtree")
        || code.contains("os.unlink")
        || code.contains("os.rmdir")
        || code.contains("os.remove")
        || code.contains("chmod")
        || code.contains("chown")
        || code.contains("Path.unlink");

    if has_destructive_ops {
        reasons.push("destructive file operations detected".to_string());
    }

    // --- Dependency install detection ---
    let has_dependency_install = code.contains("pip install")
        || code.contains("pip3 install")
        || code.contains("conda install");

    if has_dependency_install {
        reasons.push("dependency installation detected".to_string());
    }

    // --- Risk level calculation ---
    let level = if has_destructive_ops {
        PythonRiskLevel::High
    } else if has_subprocess || has_network {
        PythonRiskLevel::Medium
    } else if has_file_io
        || has_dynamic_execution
        || has_dependency_install
        || !found_suspicious.is_empty()
    {
        PythonRiskLevel::Low
    } else {
        PythonRiskLevel::Safe
    };

    PythonRiskAssessment {
        level,
        reasons,
        has_file_io,
        has_subprocess,
        has_network,
        has_destructive_ops,
        has_dynamic_execution,
        imports,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_code_returns_safe() {
        let result = analyze_python_risk("print('hello')");
        assert_eq!(result.level, PythonRiskLevel::Safe);
        assert!(result.reasons.is_empty());
        assert!(!result.has_file_io);
        assert!(!result.has_subprocess);
        assert!(!result.has_network);
        assert!(!result.has_destructive_ops);
        assert!(!result.has_dynamic_execution);
    }

    #[test]
    fn file_io_returns_low() {
        let result = analyze_python_risk("f = open('x', 'w')");
        assert_eq!(result.level, PythonRiskLevel::Low);
        assert!(result.has_file_io);
    }

    #[test]
    fn subprocess_returns_medium() {
        let result = analyze_python_risk("import subprocess\nsubprocess.run(['ls'])");
        assert_eq!(result.level, PythonRiskLevel::Medium);
        assert!(result.has_subprocess);
    }

    #[test]
    fn shutil_rmtree_returns_high() {
        let result = analyze_python_risk("import shutil\nshutil.rmtree('/tmp/dir')");
        assert_eq!(result.level, PythonRiskLevel::High);
        assert!(result.has_destructive_ops);
    }

    #[test]
    fn requests_returns_medium() {
        let result = analyze_python_risk("import requests\nrequests.get('http://example.com')");
        assert_eq!(result.level, PythonRiskLevel::Medium);
        assert!(result.has_network);
    }

    #[test]
    fn eval_sets_dynamic_execution() {
        let result = analyze_python_risk("eval('1+1')");
        assert!(result.has_dynamic_execution);
    }

    #[test]
    fn import_detection_works() {
        let result = analyze_python_risk("import os\nimport sys\nfrom pathlib import Path");
        assert!(result.imports.contains(&"os".to_string()));
        assert!(result.imports.contains(&"sys".to_string()));
        assert!(result.imports.contains(&"pathlib".to_string()));
    }

    #[test]
    fn multiple_risks_prioritize_high() {
        let result = analyze_python_risk(
            "import shutil\nimport requests\nshutil.rmtree('x')\nrequests.get('y')",
        );
        assert_eq!(result.level, PythonRiskLevel::High);
        assert!(result.has_destructive_ops);
        assert!(result.has_network);
    }

    #[test]
    fn dynamic_execution_only_returns_low() {
        let result = analyze_python_risk("result = eval('1 + 1')");
        assert_eq!(result.level, PythonRiskLevel::Low);
        assert!(result.has_dynamic_execution);
        assert!(!result.has_file_io);
        assert!(!result.has_subprocess);
    }

    #[test]
    fn destructive_file_ops_detected() {
        let result = analyze_python_risk("os.remove('file.txt')");
        assert_eq!(result.level, PythonRiskLevel::High);
        assert!(result.has_destructive_ops);
    }

    #[test]
    fn chmod_detected_as_destructive() {
        let result = analyze_python_risk("os.chmod('file.txt', 0o777)");
        assert_eq!(result.level, PythonRiskLevel::High);
        assert!(result.has_destructive_ops);
    }
}
