use serde::Deserialize;

use super::types::{PythonRiskAssessment, PythonRiskLevel, PythonRiskScanner};

/// Result from the Python AST scanner.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
#[allow(dead_code)]
struct AstScanResult {
    imports: Vec<String>,
    from_imports: Vec<String>,
    calls: Vec<String>,
    dynamic_execution: bool,
    subprocess: bool,
    network: bool,
    destructive_fs: bool,
    dependency_install: bool,
    file_read: bool,
    file_write: bool,
    parse_error: bool,
    fallback: bool,
}

/// Inline Python script that parses source via `ast.parse()` and walks the
/// tree to extract risk-relevant features. Output is JSON to stdout.
const AST_SCANNER_SCRIPT: &str = r#"
import ast
import json
import sys

def main():
    source = sys.stdin.read()
    try:
        tree = ast.parse(source)
    except SyntaxError:
        json.dump({"parse_error": True, "fallback": False}, sys.stdout)
        return

    imports = []
    from_imports = []
    calls = []
    dynamic_exec = False
    subprocess = False
    network = False
    destructive_fs = False
    dep_install = False
    file_read = False
    file_write = False

    suspicious_imports = {
        "subprocess", "os", "socket", "urllib", "requests", "httpx",
        "ctypes", "pickle", "marshal", "pty", "shutil",
    }
    network_imports = {"requests", "urllib", "http.client", "socket", "httpx"}
    destructive_names = {
        "os.remove", "os.unlink", "os.rmdir", "shutil.rmtree",
        "Path.unlink", "Path.rmdir", "chmod", "chown",
    }
    subprocess_names = {"subprocess.run", "subprocess.Popen", "subprocess.call",
                        "subprocess.check_call", "subprocess.check_output",
                        "os.system", "os.popen", "os.exec",
                        "os.execv", "os.execve", "os.execl", "os.execle"}

    def dotted_name(node):
        if isinstance(node, ast.Name):
            return node.id
        if isinstance(node, ast.Attribute):
            parent = dotted_name(node.value)
            if parent:
                return f"{parent}.{node.attr}"
        if isinstance(node, ast.Call):
            return dotted_name(node.func)
        return ""

    for node in ast.walk(tree):
        # Imports
        if isinstance(node, ast.Import):
            for alias in node.names:
                mod = alias.name.split(".")[0]
                imports.append(mod)
        elif isinstance(node, ast.ImportFrom):
            if node.module:
                mod = node.module.split(".")[0]
                from_imports.append(mod)

        # Function / method calls
        elif isinstance(node, ast.Call):
            name = dotted_name(node.func)
            if name:
                calls.append(name)

    # Classify imports
    all_modules = set(imports) | set(from_imports)
    for mod in all_modules:
        if mod in suspicious_imports:
            subprocess = subprocess or (mod == "subprocess")
            network = network or (mod in network_imports)

    # Classify calls
    for c in calls:
        # Dynamic execution
        if c in ("eval", "exec", "compile", "__import__"):
            dynamic_exec = True

        # Subprocess
        if c in subprocess_names or c.startswith("subprocess.") or c.startswith("os.system") or c.startswith("os.popen") or c.startswith("os.exec"):
            subprocess = True

        # Network
        if c.startswith("requests.") or c.startswith("urllib.") or c.startswith("http.client.") or c.startswith("socket.") or c.startswith("httpx."):
            network = True

        # Destructive filesystem
        if c in destructive_names or c.endswith(".remove") or c.endswith(".unlink") or c.endswith(".rmdir") or c.endswith(".rmtree") or c.endswith(".chmod") or c.endswith(".chown"):
            destructive_fs = True

        # File read/write
        if c == "open":
            file_read = True
        if c.endswith(".write") or c.endswith(".write_text") or c.endswith(".writelines"):
            file_write = True
        if c.endswith(".read") or c.endswith(".read_text") or c.endswith(".readlines"):
            file_read = True

        # Dependency install
        if "pip" in c and "install" in c or "conda" in c and "install" in c:
            dep_install = True

    # Check from-imports for suspicious modules
    for mod in from_imports:
        if mod in suspicious_imports:
            subprocess = subprocess or (mod == "subprocess")
            network = network or (mod in network_imports)

    # Also check string literals for pip/conda install
    for node in ast.walk(tree):
        if isinstance(node, ast.Constant) and isinstance(node.value, str):
            val = node.value
            if "pip install" in val or "pip3 install" in val or "conda install" in val:
                dep_install = True

    result = {
        "imports": imports,
        "from_imports": from_imports,
        "calls": calls,
        "dynamic_execution": dynamic_exec,
        "subprocess": subprocess,
        "network": network,
        "destructive_fs": destructive_fs,
        "dependency_install": dep_install,
        "file_read": file_read,
        "file_write": file_write,
        "parse_error": False,
        "fallback": False,
    }
    json.dump(result, sys.stdout)

main()
"#;

/// Run the AST scanner by spawning `python3 -I` with the script piped via stdin.
/// Returns a fallback result if Python is unavailable or parsing fails.
fn ast_scan_python(code: &str) -> AstScanResult {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = match Command::new("python3")
        .arg("-I")
        .arg("-c")
        .arg(AST_SCANNER_SCRIPT)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => {
            return AstScanResult {
                fallback: true,
                ..Default::default()
            }
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(code.as_bytes());
    }

    let output = match child.wait_with_output() {
        Ok(o) => o,
        Err(_) => {
            return AstScanResult {
                fallback: true,
                ..Default::default()
            }
        }
    };

    if !output.status.success() {
        return AstScanResult {
            fallback: true,
            ..Default::default()
        };
    }

    match serde_json::from_slice(&output.stdout) {
        Ok(result) => result,
        Err(_) => AstScanResult {
            fallback: true,
            ..Default::default()
        },
    }
}

/// Analyze Python code for static risk indicators.
///
/// This is NOT a proof of safety. It feeds the capability envelope and
/// permission prompts. Runtime sandbox/snapshot checks remain required.
pub fn analyze_python_risk(code: &str) -> PythonRiskAssessment {
    let ast_result = ast_scan_python(code);

    if !ast_result.fallback {
        return build_risk_from_ast(code, &ast_result);
    }

    // Fallback to string scanning
    build_risk_from_string(code)
}

/// Build a risk assessment from AST scan results.
fn build_risk_from_ast(code: &str, ast: &AstScanResult) -> PythonRiskAssessment {
    let mut reasons = Vec::new();

    // Merge imports for the public field
    let mut imports = ast.imports.clone();
    for imp in &ast.from_imports {
        if !imports.contains(imp) {
            imports.push(imp.clone());
        }
    }

    if ast.dynamic_execution {
        reasons.push("dynamic code execution detected".to_string());
    }
    if ast.file_read || ast.file_write {
        reasons.push("file I/O operations detected".to_string());
    }
    if ast.subprocess {
        reasons.push("subprocess calls detected".to_string());
    }
    if ast.network {
        reasons.push("network access detected".to_string());
    }
    if ast.destructive_fs {
        reasons.push("destructive file operations detected".to_string());
    }
    if ast.dependency_install {
        reasons.push("dependency installation detected".to_string());
    }

    let has_file_io = ast.file_read || ast.file_write;

    // For parse errors, fall back to string scanning for risk classification
    // but mark as AST-scanned
    if ast.parse_error {
        let mut fallback = build_risk_from_string(code);
        fallback.scanner = PythonRiskScanner::Ast;
        fallback.reasons.push("parse error in source".to_string());
        return fallback;
    }

    let level = if ast.destructive_fs {
        PythonRiskLevel::High
    } else if ast.subprocess || ast.network {
        PythonRiskLevel::Medium
    } else if has_file_io || ast.dynamic_execution || ast.dependency_install || !imports.is_empty()
    {
        PythonRiskLevel::Low
    } else {
        PythonRiskLevel::Safe
    };

    PythonRiskAssessment {
        level,
        reasons,
        has_file_io,
        has_file_read: ast.file_read,
        has_file_write: ast.file_write,
        has_subprocess: ast.subprocess,
        has_network: ast.network,
        has_destructive_ops: ast.destructive_fs,
        has_dynamic_execution: ast.dynamic_execution,
        imports,
        scanner: PythonRiskScanner::Ast,
    }
}

/// Build a risk assessment using the original string/line scanning fallback.
fn build_risk_from_string(code: &str) -> PythonRiskAssessment {
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

    // Distinguish read vs write file operations
    let has_file_read = code.contains(".read(")
        || code.contains("open(") && (code.contains("'r'") || code.contains("\"r\""));
    let has_file_write = code.contains(".write(")
        || code.contains("open(") && (code.contains("'w'") || code.contains("\"w\""))
        || code.contains("open(") && (code.contains("'a'") || code.contains("\"a\""))
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
        has_file_read,
        has_file_write,
        has_subprocess,
        has_network,
        has_destructive_ops,
        has_dynamic_execution,
        imports,
        scanner: PythonRiskScanner::Fallback,
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

    // ── AST scanner: alias and bypass detection ──────────────────────────

    #[test]
    fn ast_from_import_subprocess_detected() {
        let result = analyze_python_risk("from subprocess import run\nrun(['ls'])");
        assert_eq!(result.level, PythonRiskLevel::Medium);
        assert!(result.has_subprocess);
        assert_eq!(result.scanner, PythonRiskScanner::Ast);
    }

    #[test]
    fn ast_import_alias_subprocess_detected() {
        let result = analyze_python_risk("import subprocess as sp\nsp.run(['ls'])");
        assert_eq!(result.level, PythonRiskLevel::Medium);
        assert!(result.has_subprocess);
        assert_eq!(result.scanner, PythonRiskScanner::Ast);
    }

    #[test]
    fn ast_pathlib_write_detected() {
        let result = analyze_python_risk("from pathlib import Path\nPath('x').write_text('y')");
        assert_eq!(result.level, PythonRiskLevel::Low);
        assert!(result.has_file_write);
        assert!(result.has_file_io);
        assert_eq!(result.scanner, PythonRiskScanner::Ast);
    }

    #[test]
    fn ast_os_alias_remove_detected() {
        let result = analyze_python_risk("import os as o\no.remove('x')");
        assert_eq!(result.level, PythonRiskLevel::High);
        assert!(result.has_destructive_ops);
        assert_eq!(result.scanner, PythonRiskScanner::Ast);
    }

    #[test]
    fn ast_eval_in_string_not_detected() {
        // The AST scanner should NOT flag eval inside a string literal
        let result = analyze_python_risk("x = 'eval(1+1)'");
        assert!(!result.has_dynamic_execution);
        assert_eq!(result.scanner, PythonRiskScanner::Ast);
    }

    #[test]
    fn ast_comment_subprocess_not_detected() {
        // The AST scanner should NOT flag subprocess.run in a comment
        let result = analyze_python_risk("# subprocess.run(['ls'])\nx = 1");
        assert!(!result.has_subprocess);
        assert_eq!(result.scanner, PythonRiskScanner::Ast);
    }

    #[test]
    fn ast_syntax_error_produces_assessment() {
        // Syntax errors should still produce a risk assessment
        let result = analyze_python_risk("def foo(");
        // The AST scanner detects parse error and falls back to string scanning
        assert_eq!(result.scanner, PythonRiskScanner::Ast);
    }

    #[test]
    fn ast_os_system_detected() {
        let result = analyze_python_risk("import os\nos.system('ls')");
        assert_eq!(result.level, PythonRiskLevel::Medium);
        assert!(result.has_subprocess);
    }

    #[test]
    fn ast_socket_detected() {
        let result = analyze_python_risk("import socket\nsocket.socket()");
        assert_eq!(result.level, PythonRiskLevel::Medium);
        assert!(result.has_network);
    }

    #[test]
    fn ast_requests_detected() {
        let result = analyze_python_risk("import requests\nrequests.get('http://x')");
        assert_eq!(result.level, PythonRiskLevel::Medium);
        assert!(result.has_network);
    }

    #[test]
    fn ast_compile_detected() {
        let result = analyze_python_risk("compile('1+1', '<string>', 'eval')");
        assert!(result.has_dynamic_execution);
        assert_eq!(result.level, PythonRiskLevel::Low);
    }

    #[test]
    fn ast_shutil_rmtree_detected() {
        let result = analyze_python_risk("import shutil\nshutil.rmtree('/tmp/dir')");
        assert_eq!(result.level, PythonRiskLevel::High);
        assert!(result.has_destructive_ops);
    }

    #[test]
    fn ast_pip_install_in_string_detected() {
        let result = analyze_python_risk("x = 'pip install numpy'");
        // The AST scanner checks string constants for dependency install patterns
        assert!(
            result.has_dynamic_execution
                || result
                    .reasons
                    .iter()
                    .any(|r| r.contains("dependency installation"))
        );
    }
}
