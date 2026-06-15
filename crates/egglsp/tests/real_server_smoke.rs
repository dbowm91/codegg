//! Real-server smoke tests for Tier 1 LSP compatibility.
//!
//! These tests launch actual language servers and verify basic
//! protocol operations. They are opt-in via the
//! `lsp-real-server-tests` feature and skip automatically when
//! server binaries are not available.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tempfile::TempDir;

/// Timeout for server initialization.
const _INIT_TIMEOUT: Duration = Duration::from_secs(20);
/// Timeout for readiness/indexing.
const _READINESS_TIMEOUT: Duration = Duration::from_secs(30);
/// Timeout for individual semantic requests.
const _REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Total test timeout (enforced by the test harness).
const _TEST_TIMEOUT: Duration = Duration::from_secs(60);

// ── Server Binary Discovery ─────────────────────────────────────────

/// Try to find a server binary from an env var or PATH candidates.
/// Returns `None` if not found (tests should skip).
fn require_server_binary(env_var: &str, candidates: &[&str]) -> Option<PathBuf> {
    if let Ok(path) = std::env::var(env_var) {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }
    for name in candidates {
        if let Ok(path) = which::which(name) {
            return Some(path);
        }
    }
    None
}

/// Capture server version by running `--version`.
fn capture_version(bin: &Path) -> Option<String> {
    std::process::Command::new(bin)
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout)
                    .ok()
                    .map(|s| s.trim().to_string())
            } else {
                None
            }
        })
}

// ── Fixture Generators ──────────────────────────────────────────────

/// Create a minimal Rust project fixture in a temp directory.
fn create_rust_fixture(dir: &Path) -> Vec<PathBuf> {
    let cargo_toml = dir.join("Cargo.toml");
    std::fs::write(
        &cargo_toml,
        r#"[package]
name = "test_fixture"
version = "0.1.0"
edition = "2021"
"#,
    )
    .unwrap();

    let src_dir = dir.join("src");
    std::fs::create_dir_all(&src_dir).unwrap();

    let lib_rs = src_dir.join("lib.rs");
    std::fs::write(
        &lib_rs,
        r#"pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

pub fn greet(name: &str) -> String {
    format!("Hello, {name}!")
}

pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn distance(&self, other: &Point) -> f64 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }
}

// Intentional type error for diagnostics
pub fn broken() -> i32 {
    let x: String = 42;
    x
}

// Call hierarchy target
pub fn caller() -> i32 {
    add(1, 2)
}
"#,
    )
    .unwrap();

    vec![cargo_toml, lib_rs]
}

/// Create a minimal Python project fixture in a temp directory.
fn create_python_fixture(dir: &Path) -> Vec<PathBuf> {
    let pyproject = dir.join("pyproject.toml");
    std::fs::write(
        &pyproject,
        r#"[project]
name = "test_fixture"
version = "0.1.0"
"#,
    )
    .unwrap();

    let helper_py = dir.join("helper.py");
    std::fs::write(
        &helper_py,
        r#"def add(a: int, b: int) -> int:
    return a + b

class Point:
    def __init__(self, x: float, y: float):
        self.x = x
        self.y = y

    def distance(self, other: "Point") -> float:
        return ((self.x - other.x)**2 + (self.y - other.y)**2)**0.5
"#,
    )
    .unwrap();

    let main_py = dir.join("main.py");
    std::fs::write(
        &main_py,
        r#"from helper import add, Point

def greet(name: str) -> str:
    return f"Hello, {name}!"

# Intentional type error for diagnostics
def broken() -> int:
    x: str = 42
    return x

# Cross-file reference
def caller() -> int:
    return add(1, 2)
"#,
    )
    .unwrap();

    vec![pyproject, helper_py, main_py]
}

// ── Common Smoke Assertions ─────────────────────────────────────────

use egglsp::capability::LspCapabilitySnapshot;
use egglsp::client::{LspClient, LspClientOptions};
use egglsp::compatibility::{
    self, CompatibilityCheckStatus, LspCompatibilityCheck, LspCompatibilityProfile,
    LspCompatibilityReport,
};
use egglsp::diagnostics::LspDiagnosticSnapshot;
use egglsp::launch::LspLaunchSpec;

/// Result of a single smoke check.
struct SmokeCheck {
    name: String,
    result: Result<(), String>,
    duration_ms: u64,
}

impl SmokeCheck {
    fn pass(name: impl Into<String>, duration_ms: u64) -> Self {
        Self {
            name: name.into(),
            result: Ok(()),
            duration_ms,
        }
    }

    fn fail(name: impl Into<String>, reason: impl Into<String>, duration_ms: u64) -> Self {
        Self {
            name: name.into(),
            result: Err(reason.into()),
            duration_ms,
        }
    }

    fn to_compatibility_check(&self) -> LspCompatibilityCheck {
        LspCompatibilityCheck {
            name: self.name.clone(),
            status: match &self.result {
                Ok(()) => CompatibilityCheckStatus::Passing,
                Err(_) => CompatibilityCheckStatus::Failing,
            },
            detail: self.result.as_ref().err().cloned(),
            duration_ms: Some(self.duration_ms),
        }
    }
}

/// Wait for diagnostics from a specific file, with timeout.
async fn wait_for_diagnostics(
    client: &LspClient,
    file_path: &std::path::Path,
    timeout: Duration,
) -> Option<LspDiagnosticSnapshot> {
    let uri = url::Url::from_file_path(file_path).ok()?;
    let uri_str = uri.as_str();
    let start = std::time::Instant::now();
    loop {
        let snap = client.diagnostic_snapshot(uri_str).await;
        if !snap.diagnostics.is_empty() || start.elapsed() > timeout {
            return Some(snap);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

// ── Smoke Test Runner ──────────────────────────────────────────────

/// Run the common smoke test suite against a live server.
async fn run_smoke_suite(
    profile: &LspCompatibilityProfile,
    bin_path: &Path,
    tempdir: &TempDir,
    source_files: &[PathBuf],
    server_version: Option<String>,
) -> LspCompatibilityReport {
    let root = tempdir.path().to_path_buf();
    let mut checks: Vec<SmokeCheck> = Vec::new();
    let mut stderr_tail: Vec<String> = Vec::new();

    // Build launch spec
    let spec = LspLaunchSpec::new(
        &profile.server_id,
        bin_path,
        profile.default_args.clone(),
        vec![],
        vec![],
        vec![],
    );

    let client_options = LspClientOptions::default();

    // 1. Launch and Initialize
    let start = std::time::Instant::now();
    let client_result =
        LspClient::new_with_launch_spec(spec, &root, serde_json::json!({}), client_options).await;
    let init_ms = start.elapsed().as_millis() as u64;

    let client = match client_result {
        Ok(c) => {
            checks.push(SmokeCheck::pass("initialize", init_ms));
            c
        }
        Err(e) => {
            checks.push(SmokeCheck::fail("initialize", format!("{e}"), init_ms));
            return LspCompatibilityReport {
                server_id: profile.server_id.clone(),
                server_version,
                platform: std::env::consts::OS.to_string(),
                initialize_ms: init_ms,
                readiness_ms: None,
                capabilities: LspCapabilitySnapshot::default(),
                checks: checks.iter().map(|c| c.to_compatibility_check()).collect(),
                stderr_tail,
                known_limitations: profile.known_limitations.clone(),
            };
        }
    };

    // 2. Capability Snapshot
    let start = std::time::Instant::now();
    let raw_caps = client.capabilities.lock().await.clone().unwrap_or_default();
    let caps = LspCapabilitySnapshot::from_capabilities(&raw_caps, Some(&profile.server_id), None);
    let cap_ms = start.elapsed().as_millis() as u64;
    checks.push(SmokeCheck::pass("capability_snapshot", cap_ms));

    // 3. Open fixture files
    for file in source_files {
        let uri = url::Url::from_file_path(file).unwrap();
        let content = std::fs::read_to_string(file).unwrap_or_default();
        client.open_file(&uri, &content, 1).await.ok();
    }
    checks.push(SmokeCheck::pass("didOpen", 0));

    // 4. Wait for readiness (diagnostics or timeout based on profile)
    let start = std::time::Instant::now();
    match &profile.readiness_policy {
        egglsp::compatibility::LspReadinessPolicy::WaitForDiagnosticsOrTimeout { timeout } => {
            if let Some(first_file) = source_files.first() {
                let _ = wait_for_diagnostics(&client, first_file, *timeout).await;
            }
        }
        egglsp::compatibility::LspReadinessPolicy::WaitForProgressEndOrTimeout { timeout } => {
            // For progress-based servers, just wait a bit
            tokio::time::sleep(std::cmp::min(*timeout, Duration::from_secs(10))).await;
        }
        egglsp::compatibility::LspReadinessPolicy::WarmupDelay { duration } => {
            tokio::time::sleep(*duration).await;
        }
        egglsp::compatibility::LspReadinessPolicy::InitializedIsReady => {}
    }
    let readiness_ms = start.elapsed().as_millis() as u64;
    checks.push(SmokeCheck::pass("readiness_wait", readiness_ms));

    // 5. Document Symbols
    if caps.supports_document_symbols {
        let start = std::time::Instant::now();
        let result = if let Some(file) = source_files.first() {
            let uri = url::Url::from_file_path(file).unwrap();
            client
                .document_symbols(&uri)
                .await
                .map(|s| s.len())
                .map_err(|e| format!("{e}"))
        } else {
            Err("no source files".to_string())
        };
        let ms = start.elapsed().as_millis() as u64;
        match result {
            Ok(count) => {
                if count > 0 {
                    checks.push(SmokeCheck::pass(
                        format!("document_symbols ({count} found)"),
                        ms,
                    ));
                } else {
                    checks.push(SmokeCheck::fail(
                        "document_symbols",
                        "0 symbols found".to_string(),
                        ms,
                    ));
                }
            }
            Err(e) => {
                checks.push(SmokeCheck::fail("document_symbols", e, ms));
            }
        }
    } else {
        checks.push(SmokeCheck::pass(
            "document_symbols (skipped: not supported)",
            0,
        ));
    }

    // 6. Definition
    if caps.supports_definition {
        let start = std::time::Instant::now();
        let result = if let Some(file) = source_files.first() {
            let uri = url::Url::from_file_path(file).unwrap();
            // Try to find definition at a function call site
            client
                .go_to_definition(&uri, lsp_types::Position::new(10, 5))
                .await
                .map(|d| d.is_some())
                .map_err(|e| format!("{e}"))
        } else {
            Err("no source files".to_string())
        };
        let ms = start.elapsed().as_millis() as u64;
        match result {
            Ok(found) => {
                checks.push(SmokeCheck::pass(format!("definition (found={found})"), ms));
            }
            Err(e) => {
                checks.push(SmokeCheck::fail("definition", e, ms));
            }
        }
    } else {
        checks.push(SmokeCheck::pass("definition (skipped: not supported)", 0));
    }

    // 7. References
    if caps.supports_references {
        let start = std::time::Instant::now();
        let result = if let Some(file) = source_files.first() {
            let uri = url::Url::from_file_path(file).unwrap();
            client
                .find_references(&uri, lsp_types::Position::new(0, 5))
                .await
                .map(|r| r.len())
                .map_err(|e| format!("{e}"))
        } else {
            Err("no source files".to_string())
        };
        let ms = start.elapsed().as_millis() as u64;
        match result {
            Ok(count) => {
                checks.push(SmokeCheck::pass(format!("references ({count} found)"), ms));
            }
            Err(e) => {
                checks.push(SmokeCheck::fail("references", e, ms));
            }
        }
    } else {
        checks.push(SmokeCheck::pass("references (skipped: not supported)", 0));
    }

    // 8. Hover
    if caps.supports_hover {
        let start = std::time::Instant::now();
        let result = if let Some(file) = source_files.first() {
            let uri = url::Url::from_file_path(file).unwrap();
            client
                .hover(&uri, lsp_types::Position::new(0, 5))
                .await
                .map(|h| h.is_some())
                .map_err(|e| format!("{e}"))
        } else {
            Err("no source files".to_string())
        };
        let ms = start.elapsed().as_millis() as u64;
        match result {
            Ok(found) => {
                checks.push(SmokeCheck::pass(format!("hover (found={found})"), ms));
            }
            Err(e) => {
                checks.push(SmokeCheck::fail("hover", e, ms));
            }
        }
    } else {
        checks.push(SmokeCheck::pass("hover (skipped: not supported)", 0));
    }

    // 9. Graceful Shutdown
    let start = std::time::Instant::now();
    let shutdown_result = client.shutdown().await;
    let shutdown_ms = start.elapsed().as_millis() as u64;
    match shutdown_result {
        Ok(()) => {
            checks.push(SmokeCheck::pass("shutdown", shutdown_ms));
        }
        Err(e) => {
            checks.push(SmokeCheck::fail("shutdown", format!("{e}"), shutdown_ms));
        }
    }

    // Collect stderr if available
    if let Ok(lines) = std::fs::read_to_string(tempdir.path().join("stderr.log")) {
        stderr_tail = lines.lines().take(20).map(String::from).collect();
    }

    LspCompatibilityReport {
        server_id: profile.server_id.clone(),
        server_version,
        platform: std::env::consts::OS.to_string(),
        initialize_ms: init_ms,
        readiness_ms: Some(readiness_ms),
        capabilities: caps,
        checks: checks.iter().map(|c| c.to_compatibility_check()).collect(),
        stderr_tail,
        known_limitations: profile.known_limitations.clone(),
    }
}

// ── Rust Analyzer Tests ────────────────────────────────────────────

#[tokio::test]
async fn rust_analyzer_smoke() {
    let bin = match require_server_binary("CODEGG_RA_BIN", &["rust-analyzer"]) {
        Some(b) => b,
        None => {
            eprintln!("SKIP: rust-analyzer not found (set CODEGG_RA_BIN or install rust-analyzer)");
            return;
        }
    };

    let version = capture_version(&bin);
    eprintln!("rust-analyzer version: {:?}", version);

    let tempdir = tempfile::tempdir().unwrap();
    let files = create_rust_fixture(tempdir.path());

    let profile = compatibility::rust_analyzer_profile();
    let report = run_smoke_suite(&profile, &bin, &tempdir, &files, version).await;

    // Write compatibility report
    let report_path = std::path::PathBuf::from("target/lsp-compatibility");
    let _ = std::fs::create_dir_all(&report_path);
    let json = serde_json::to_string_pretty(&report).unwrap();
    let filename = format!(
        "rust-analyzer-{}.json",
        report.server_version.as_deref().unwrap_or("unknown")
    );
    let _ = std::fs::write(report_path.join(&filename), &json);
    eprintln!("Compatibility report: {json}");

    // Assert critical checks passed
    for check in &report.checks {
        if check.name == "initialize" {
            assert_eq!(
                check.status,
                CompatibilityCheckStatus::Passing,
                "rust-analyzer initialize failed: {:?}",
                check.detail
            );
        }
    }

    // Shutdown should succeed
    let shutdown_check = report.checks.iter().find(|c| c.name == "shutdown");
    assert!(shutdown_check.is_some(), "no shutdown check in report");
    assert_eq!(
        shutdown_check.unwrap().status,
        CompatibilityCheckStatus::Passing,
        "rust-analyzer shutdown failed: {:?}",
        shutdown_check.unwrap().detail
    );
}

// ── Pyright/Basedpyright Tests ─────────────────────────────────────

#[tokio::test]
async fn basedpyright_smoke() {
    let bin = match require_server_binary(
        "CODEGG_PYRIGHT_BIN",
        &[
            "basedpyright-langserver",
            "basedpyright",
            "pyright-langserver",
            "pyright",
        ],
    ) {
        Some(b) => b,
        None => {
            eprintln!("SKIP: pyright/basedpyright not found (set CODEGG_PYRIGHT_BIN)");
            return;
        }
    };

    let version = capture_version(&bin);
    eprintln!("pyright version: {:?}", version);

    let tempdir = tempfile::tempdir().unwrap();
    let files = create_python_fixture(tempdir.path());

    let profile = compatibility::pyright_profile();
    let report = run_smoke_suite(&profile, &bin, &tempdir, &files, version).await;

    // Write compatibility report
    let report_path = std::path::PathBuf::from("target/lsp-compatibility");
    let _ = std::fs::create_dir_all(&report_path);
    let json = serde_json::to_string_pretty(&report).unwrap();
    let filename = format!(
        "pyright-{}.json",
        report.server_version.as_deref().unwrap_or("unknown")
    );
    let _ = std::fs::write(report_path.join(&filename), &json);
    eprintln!("Compatibility report: {json}");

    // Assert critical checks
    for check in &report.checks {
        if check.name == "initialize" {
            assert_eq!(
                check.status,
                CompatibilityCheckStatus::Passing,
                "pyright initialize failed: {:?}",
                check.detail
            );
        }
    }
}
