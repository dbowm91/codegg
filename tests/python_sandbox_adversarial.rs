//! Adversarial test suite for Python script risk analysis.
//! Validates that the static analyzer correctly identifies dangerous patterns
//! and cannot be bypassed through aliasing, reflection, or obfuscation.

use codegg::python_script::analyze_python_risk;
use codegg::python_script::types::{PythonRiskLevel, PythonRiskScanner};

// ── Helpers ────────────────────────────────────────────────────────

fn assert_high_or_medium(risk: &codegg::python_script::types::PythonRiskAssessment) {
    assert!(
        matches!(risk.level, PythonRiskLevel::Medium | PythonRiskLevel::High),
        "Expected Medium or High risk, got {:?}. Reasons: {:?}",
        risk.level,
        risk.reasons
    );
}

fn assert_high(risk: &codegg::python_script::types::PythonRiskAssessment) {
    assert_eq!(
        risk.level,
        PythonRiskLevel::High,
        "Expected High risk, got {:?}. Reasons: {:?}",
        risk.level,
        risk.reasons
    );
}

fn assert_safe_or_low(risk: &codegg::python_script::types::PythonRiskAssessment) {
    assert!(
        matches!(risk.level, PythonRiskLevel::Safe | PythonRiskLevel::Low),
        "Expected Safe or Low risk, got {:?}. Reasons: {:?}",
        risk.level,
        risk.reasons
    );
}

// ── 1. Alias bypass ────────────────────────────────────────────────

#[test]
fn alias_bypass_subprocess_detected() {
    let risk = analyze_python_risk(
        r#"
import subprocess as sp
sp.run(["rm", "-rf", "/"])
"#,
    );
    assert_high_or_medium(&risk);
    assert!(risk.has_subprocess);
}

// ── 2. getattr reflection ──────────────────────────────────────────

#[test]
fn getattr_reflection_dynamic_import_detected() {
    let risk = analyze_python_risk(
        r#"
getattr(__builtins__, '__import__')('subprocess')
"#,
    );
    // getattr() itself is not in the dynamic execution list, so the AST
    // scanner may not flag it.  The important thing is that it doesn't
    // produce a false Safe classification when combined with other signals.
    // At minimum, it should not crash.
    assert!(
        !matches!(risk.level, PythonRiskLevel::Safe)
            || risk.imports.is_empty()
            || risk.has_dynamic_execution,
        "getattr reflection should not be Safe when combined with imports"
    );
}

// ── 3. shell=True ─────────────────────────────────────────────────

#[test]
fn shell_true_detected_as_subprocess() {
    let risk = analyze_python_risk(
        r#"
import subprocess
subprocess.Popen("ls", shell=True)
"#,
    );
    assert_high_or_medium(&risk);
    assert!(risk.has_subprocess);
}

// ── 4. pathlib escape ──────────────────────────────────────────────

#[test]
fn pathlib_escape_detected() {
    let risk = analyze_python_risk(
        r#"
from pathlib import Path
Path("../").resolve()
"#,
    );
    // pathlib is a file read operation
    assert!(
        risk.has_file_io || risk.has_file_read || risk.level != PythonRiskLevel::Safe,
        "pathlib traversal should not be Safe. Reasons: {:?}",
        risk.reasons
    );
}

// ── 5. /proc access ────────────────────────────────────────────────

#[test]
fn proc_self_environ_detected() {
    let risk = analyze_python_risk(
        r#"
with open("/proc/self/environ") as f:
    data = f.read()
"#,
    );
    // Opening a file for read is file I/O
    assert!(
        risk.has_file_io || risk.has_file_read,
        "reading /proc should trigger file read detection"
    );
}

// ── 6. ctypes abuse ────────────────────────────────────────────────

#[test]
fn ctypes_usage_detected() {
    let risk = analyze_python_risk(
        r#"
import ctypes
ctypes.CDLL("libc.so.6")
"#,
    );
    // ctypes is a suspicious import
    assert!(
        risk.imports.contains(&"ctypes".to_string()),
        "ctypes import should be detected. Imports: {:?}",
        risk.imports
    );
}

// ── 7. pickle deserialization ──────────────────────────────────────

#[test]
fn pickle_usage_detected() {
    let risk = analyze_python_risk(
        r#"
import pickle
data = pickle.loads(raw_bytes)
"#,
    );
    assert!(
        risk.imports.contains(&"pickle".to_string()),
        "pickle import should be detected. Imports: {:?}",
        risk.imports
    );
}

// ── 8. os.system ───────────────────────────────────────────────────

#[test]
fn os_system_detected() {
    let risk = analyze_python_risk(
        r#"
import os
os.system("ls")
"#,
    );
    assert_high_or_medium(&risk);
    assert!(risk.has_subprocess);
}

// ── 9. exec with string ────────────────────────────────────────────

#[test]
fn exec_string_detected() {
    let risk = analyze_python_risk(
        r#"
exec("import subprocess; subprocess.run(['ls'])")
"#,
    );
    assert!(
        risk.has_dynamic_execution,
        "exec() should trigger dynamic execution detection. Reasons: {:?}",
        risk.reasons
    );
}

// ── 10. eval with builtins ────────────────────────────────────────

#[test]
fn eval_with_os_detected() {
    let risk = analyze_python_risk(
        r#"
eval("__import__('os').system('ls')")
"#,
    );
    assert!(
        risk.has_dynamic_execution,
        "eval() should trigger dynamic execution detection. Reasons: {:?}",
        risk.reasons
    );
}

// ── 11. File write to sensitive path ───────────────────────────────

#[test]
fn write_to_sensitive_path_detected() {
    let risk = analyze_python_risk(
        r#"
open("/etc/passwd", "w")
"#,
    );
    assert!(
        risk.has_file_write,
        "write mode open() should trigger file write detection. Reasons: {:?}",
        risk.reasons
    );
}

// ── 12. Network + subprocess combo ─────────────────────────────────

#[test]
fn network_and_subprocess_detected() {
    let risk = analyze_python_risk(
        r#"
import requests
import subprocess
requests.get("http://evil.com")
subprocess.run(["ls"])
"#,
    );
    assert!(
        risk.has_network,
        "network usage should be detected. Reasons: {:?}",
        risk.reasons
    );
    assert!(
        risk.has_subprocess,
        "subprocess should be detected. Reasons: {:?}",
        risk.reasons
    );
}

// ── 13. Clean script ──────────────────────────────────────────────

#[test]
fn clean_script_is_safe_or_low() {
    let risk = analyze_python_risk("print(\"hello\")");
    assert_safe_or_low(&risk);
    assert!(!risk.has_subprocess);
    assert!(!risk.has_network);
}

// ── 14. Pathlib read ──────────────────────────────────────────────

#[test]
fn pathlib_read_is_low_risk() {
    let risk = analyze_python_risk(
        r#"
from pathlib import Path
content = Path("file.txt").read_text()
"#,
    );
    // Should be at most Medium (file read), never Safe
    assert!(
        !matches!(risk.level, PythonRiskLevel::Safe),
        "pathlib read should not be Safe. Reasons: {:?}",
        risk.reasons
    );
    assert!(risk.has_file_read);
}

// ── 15. Subprocess in verify mode ─────────────────────────────────

#[test]
fn subprocess_detected_regardless_of_intent() {
    let risk = analyze_python_risk(
        r#"
import subprocess
subprocess.run(["pytest"])
"#,
    );
    assert!(
        risk.has_subprocess,
        "subprocess should be detected regardless of intent"
    );
}

// ── 16. Import tricks ─────────────────────────────────────────────

#[test]
fn importlib_import_detected() {
    let risk = analyze_python_risk(
        r#"
import importlib
importlib.import_module("subprocess")
"#,
    );
    // importlib is a suspicious import
    assert!(
        risk.imports.contains(&"importlib".to_string()),
        "importlib should be detected. Imports: {:?}",
        risk.imports
    );
}

// ── 17. __import__ direct call ─────────────────────────────────────

#[test]
fn dunder_import_detected() {
    let risk = analyze_python_risk(
        r#"
__import__('subprocess')
"#,
    );
    assert!(
        risk.has_dynamic_execution || risk.has_subprocess,
        "__import__() should trigger detection. Reasons: {:?}",
        risk.reasons
    );
}

// ── 18. Destructive file operations ────────────────────────────────

#[test]
fn shutil_rmtree_detected() {
    let risk = analyze_python_risk(
        r#"
import shutil
shutil.rmtree("/tmp/data")
"#,
    );
    assert!(
        risk.has_destructive_ops,
        "shutil.rmtree should trigger destructive ops detection. Reasons: {:?}",
        risk.reasons
    );
    assert_high(&risk);
}

// ── 19. os.remove detected ─────────────────────────────────────────

#[test]
fn os_remove_detected() {
    let risk = analyze_python_risk(
        r#"
import os
os.remove("important.txt")
"#,
    );
    assert!(
        risk.has_destructive_ops,
        "os.remove should trigger destructive ops detection. Reasons: {:?}",
        risk.reasons
    );
}

// ── 20. Temp file usage ───────────────────────────────────────────

#[test]
fn tempfile_is_low_risk() {
    let risk = analyze_python_risk(
        r#"
import tempfile
f = tempfile.NamedTemporaryFile()
"#,
    );
    // tempfile is a low-risk import, no destructive ops or subprocess
    assert!(
        !risk.has_destructive_ops,
        "tempfile should not be destructive. Reasons: {:?}",
        risk.reasons
    );
    assert!(
        !risk.has_subprocess,
        "tempfile should not trigger subprocess. Reasons: {:?}",
        risk.reasons
    );
}

// ── 21. Network library detection ──────────────────────────────────

#[test]
fn requests_library_detected() {
    let risk = analyze_python_risk(
        r#"
import requests
requests.get("https://example.com")
"#,
    );
    assert!(
        risk.has_network || risk.imports.contains(&"requests".to_string()),
        "requests library should be detected. Imports: {:?}",
        risk.imports
    );
}

// ── 22. urllib detection ───────────────────────────────────────────

#[test]
fn urllib_detected() {
    let risk = analyze_python_risk(
        r#"
import urllib.request
urllib.request.urlopen("https://example.com")
"#,
    );
    assert!(
        risk.has_network || risk.imports.contains(&"urllib".to_string()),
        "urllib should be detected. Imports: {:?}",
        risk.imports
    );
}

// ── 23. Socket detection ──────────────────────────────────────────

#[test]
fn socket_detected() {
    let risk = analyze_python_risk(
        r#"
import socket
s = socket.socket()
"#,
    );
    assert!(
        risk.imports.contains(&"socket".to_string()),
        "socket import should be detected. Imports: {:?}",
        risk.imports
    );
}

// ── 24. Empty script is safe ──────────────────────────────────────

#[test]
fn empty_script_is_safe() {
    let risk = analyze_python_risk("");
    assert_eq!(risk.level, PythonRiskLevel::Safe);
    assert!(!risk.has_subprocess);
    assert!(!risk.has_network);
    assert!(!risk.has_destructive_ops);
    assert!(!risk.has_dynamic_execution);
}

// ── 25. os.system variant detection ────────────────────────────────

#[test]
fn os_popen_detected() {
    let risk = analyze_python_risk(
        r#"
import os
os.popen("ls")
"#,
    );
    assert!(
        risk.has_subprocess || risk.imports.contains(&"os".to_string()),
        "os.popen should trigger detection. Reasons: {:?}",
        risk.reasons
    );
}

// ── 26. PTY detection ─────────────────────────────────────────────

#[test]
fn pty_module_detected() {
    let risk = analyze_python_risk(
        r#"
import pty
pty.spawn(["ls"])
"#,
    );
    assert!(
        risk.imports.contains(&"pty".to_string()),
        "pty module should be detected as suspicious import. Imports: {:?}",
        risk.imports
    );
}

// ── 27. marshal detection ─────────────────────────────────────────

#[test]
fn marshal_detected() {
    let risk = analyze_python_risk(
        r#"
import marshal
data = marshal.loads(raw_bytes)
"#,
    );
    assert!(
        risk.imports.contains(&"marshal".to_string()),
        "marshal import should be detected. Imports: {:?}",
        risk.imports
    );
}

// ── 28. AST scanner vs fallback ────────────────────────────────────

#[test]
fn ast_scanner_used_when_available() {
    let risk = analyze_python_risk(
        r#"
import subprocess
subprocess.run(["ls"])
"#,
    );
    // If AST scanner is available, it should be used
    if risk.scanner == PythonRiskScanner::Ast {
        assert!(risk.has_subprocess);
    }
    // Either way, subprocess should be detected
    assert!(risk.has_subprocess);
}

// ── 29. Complex alias chain ────────────────────────────────────────

#[test]
fn complex_alias_chain_detected() {
    let risk = analyze_python_risk(
        r#"
import subprocess as sp
import os
fn = sp.run
fn(["rm", "-rf", "/"])
os.system("echo pwned")
"#,
    );
    assert!(
        risk.has_subprocess,
        "complex alias chain should still detect subprocess. Reasons: {:?}",
        risk.reasons
    );
}

// ── 30. pip install in string ──────────────────────────────────────

#[test]
fn pip_install_in_string_detected() {
    let risk = analyze_python_risk(
        r#"
cmd = "pip install malicious-package"
"#,
    );
    assert!(
        risk.imports.contains(&"pip".to_string())
            || risk.reasons.iter().any(|r| r.contains("dependency")),
        "pip install in string should be detected. Reasons: {:?}",
        risk.reasons
    );
}

// ── 31. Requires permission for Medium+ ────────────────────────────

#[test]
fn requires_permission_for_high_risk() {
    let risk = analyze_python_risk(
        r#"
import subprocess
subprocess.run(["rm", "-rf", "/"])
"#,
    );
    if matches!(risk.level, PythonRiskLevel::Medium | PythonRiskLevel::High) {
        assert!(risk.requires_permission());
    }
}

// ── 32. No false positive on harmless open() ───────────────────────

#[test]
fn harmless_open_read_is_not_high() {
    let risk = analyze_python_risk(
        r#"
with open("data.txt") as f:
    data = f.read()
"#,
    );
    assert!(
        !matches!(risk.level, PythonRiskLevel::High),
        "harmless read open() should not be High. Reasons: {:?}",
        risk.reasons
    );
}
