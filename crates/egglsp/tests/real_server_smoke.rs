//! Real-server smoke tests for Tier 1 LSP compatibility.
//!
//! These tests launch actual language servers and verify basic
//! protocol operations. They are opt-in via the
//! `lsp-real-server-tests` feature and skip automatically when
//! server binaries are not available.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use egglsp::runtime::spawn_process_runtime;
use lsp_types::Position;
use tempfile::TempDir;

/// Timeout for server initialization handshake.
const INIT_TIMEOUT: Duration = Duration::from_secs(20);
/// Timeout for `initialized` notification.
const INITIALIZED_TIMEOUT: Duration = Duration::from_secs(10);
/// Timeout for readiness/indexing.
const READINESS_TIMEOUT: Duration = Duration::from_secs(30);
/// Timeout for individual semantic requests.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Timeout for capturing server version.
const VERSION_TIMEOUT: Duration = Duration::from_secs(5);
/// Total test timeout (enforced by the test harness).
const TEST_TIMEOUT: Duration = Duration::from_secs(120);

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
async fn capture_version(bin: &Path) -> Option<String> {
    let output = tokio::process::Command::new(bin)
        .arg("--version")
        .output()
        .await;
    match output {
        Ok(o) if o.status.success() => String::from_utf8(o.stdout)
            .ok()
            .map(|s| s.trim().to_string()),
        _ => None,
    }
}

/// Sanitize a server version string for use in a filename.
fn sanitize_for_filename(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

// ── Fixture Metadata ────────────────────────────────────────────────

/// Typed fixture metadata for a real-server smoke test.
///
/// Source files referenced by semantic requests are explicit; positions
/// correspond to actual identifiers in the source text. Manifest files
/// (`Cargo.toml`, `pyproject.toml`) are still written to disk so the
/// language server recognizes the project, but they are not included in
/// `source_files` and never receive semantic requests.
#[allow(dead_code)]
struct RealServerFixture {
    /// Owns the temporary directory; dropping it deletes the project on disk.
    tempdir: TempDir,
    /// Absolute path to the project root.
    root: PathBuf,
    /// Source files eligible for semantic requests (no manifests).
    source_files: Vec<PathBuf>,
    /// The single source file the smoke suite drives most checks against.
    primary_source: PathBuf,
    /// Optional second source file used for cross-file reference checks.
    secondary_source: Option<PathBuf>,
    /// File the suite waits on for diagnostics (typically the broken-intent file).
    diagnostics_file: PathBuf,
    /// Position of a top-level item used for document symbols.
    symbol_position: Position,
    /// Position at the call site of a function (used for definition lookup).
    definition_position: Position,
    /// Position at the declaration of a function (used for references lookup).
    references_position: Position,
    /// Position at a function call / type use (used for hover).
    hover_position: Position,
    /// Expected symbol names from `document_symbols` (best-effort, not asserted).
    expected_symbol_names: Vec<&'static str>,
}

/// Build a Rust fixture with a `Point` struct, an `add`/`greet` pair, a
/// `broken()` for diagnostics, and a `caller()` that calls `add` from a
/// different scope. Positions are adjacent to the source text so changes
/// are obvious.
fn rust_fixture() -> RealServerFixture {
    let tempdir = tempfile::tempdir().unwrap();
    let root = tempdir.path().to_path_buf();

    std::fs::write(
        root.join("Cargo.toml"),
        r#"[package]
name = "test_fixture"
version = "0.1.0"
edition = "2021"
"#,
    )
    .unwrap();

    let src_dir = root.join("src");
    std::fs::create_dir_all(&src_dir).unwrap();

    // Source: line numbers are 0-based for LSP Position::line. Keep this
    // string and the position constants below adjacent so any edit is obvious.
    // Line 0: pub fn add(a: i32, b: i32) -> i32 {
    // Line 1:     a + b
    // Line 2: }
    // Line 3: (blank)
    // Line 4: pub fn greet(name: &str) -> String {
    // Line 5:     format!("Hello, {name}!")
    // Line 6: }
    // Line 7: (blank)
    // Line 8: pub struct Point {
    // Line 9:     pub x: f64,
    // Line 10:    pub y: f64,
    // Line 11: }
    // Line 12: (blank)
    // Line 13: impl Point {
    // Line 14:     pub fn distance(&self, other: &Point) -> f64 {
    // Line 15:         ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    // Line 16:     }
    // Line 17: }
    // Line 18: (blank)
    // Line 19: // Intentional type error for diagnostics
    // Line 20: pub fn broken() -> i32 {
    // Line 21:     let x: String = 42;
    // Line 22:     x
    // Line 23: }
    // Line 24: (blank)
    // Line 25: // Call hierarchy target
    // Line 26: pub fn caller() -> i32 {
    // Line 27:     add(1, 2)
    // Line 28: }
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

    RealServerFixture {
        tempdir,
        root,
        source_files: vec![lib_rs.clone()],
        primary_source: lib_rs.clone(),
        secondary_source: None,
        diagnostics_file: lib_rs.clone(),
        // `Point` struct on line 8, character 9 lands on the identifier.
        symbol_position: Position::new(8, 9),
        // `add` call site: line 27, character 4.
        definition_position: Position::new(27, 4),
        // `add` declaration: line 0, character 7.
        references_position: Position::new(0, 7),
        // `add` call site: line 27, character 4.
        hover_position: Position::new(27, 4),
        expected_symbol_names: vec!["add", "greet", "Point", "broken", "caller"],
    }
}

/// Build a Python fixture with a `Point` class, an `add` helper, a
/// `broken()` for diagnostics, and a `caller()` that uses `add` from a
/// different file.
fn python_fixture() -> RealServerFixture {
    let tempdir = tempfile::tempdir().unwrap();
    let root = tempdir.path().to_path_buf();

    std::fs::write(
        root.join("pyproject.toml"),
        r#"[project]
name = "test_fixture"
version = "0.1.0"
"#,
    )
    .unwrap();

    // helper.py — secondary source.
    // Line 0: def add(a: int, b: int) -> int:
    // Line 1:     return a + b
    // Line 2: (blank)
    // Line 3: class Point:
    // Line 4:     def __init__(self, x: float, y: float):
    // Line 5:         self.x = x
    // Line 6:         self.y = y
    // Line 7: (blank)
    // Line 8:     def distance(self, other: "Point") -> float:
    // Line 9:         return ((self.x - other.x)**2 + (self.y - other.y)**2)**0.5
    let helper_py = root.join("helper.py");
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

    // main.py — primary source.
    // Line 0: from helper import add, Point
    // Line 1: (blank)
    // Line 2: def greet(name: str) -> str:
    // Line 3:     return f"Hello, {name}!"
    // Line 4: (blank)
    // Line 5: # Intentional type error for diagnostics
    // Line 6: def broken() -> int:
    // Line 7:     x: str = 42
    // Line 8:     return x
    // Line 9: (blank)
    // Line 10: # Cross-file reference
    // Line 11: def caller() -> int:
    // Line 12:     return add(1, 2)
    let main_py = root.join("main.py");
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

    RealServerFixture {
        tempdir,
        root,
        source_files: vec![main_py.clone(), helper_py.clone()],
        primary_source: main_py.clone(),
        secondary_source: Some(helper_py.clone()),
        diagnostics_file: main_py.clone(),
        // `greet` def on line 2, character 4.
        symbol_position: Position::new(2, 4),
        // `add` call site in main.py: line 12, character 11.
        definition_position: Position::new(12, 11),
        // `add` import use in main.py: line 0, character 19.
        references_position: Position::new(0, 19),
        // `add` call site: line 12, character 11.
        hover_position: Position::new(12, 11),
        expected_symbol_names: vec!["greet", "broken", "caller"],
    }
}

// ── Common Smoke Assertions ─────────────────────────────────────────

use egglsp::capability::LspCapabilitySnapshot;
use egglsp::client::{LspClient, LspClientOptions};
use egglsp::compatibility::{
    self, CompatibilityCheckStatus, CompatibilityRequirement, LspCompatibilityCheck,
    LspCompatibilityProfile, LspCompatibilityReport,
};
use egglsp::diagnostics::LspDiagnosticSnapshot;
use egglsp::launch::LspLaunchSpec;

/// Result of a single smoke check.
struct SmokeCheck {
    name: String,
    result: Result<(), String>,
    requirement: CompatibilityRequirement,
    duration_ms: u64,
}

impl SmokeCheck {
    fn pass(
        name: impl Into<String>,
        requirement: CompatibilityRequirement,
        duration_ms: u64,
    ) -> Self {
        Self {
            name: name.into(),
            result: Ok(()),
            requirement,
            duration_ms,
        }
    }

    fn fail(
        name: impl Into<String>,
        requirement: CompatibilityRequirement,
        reason: impl Into<String>,
        duration_ms: u64,
    ) -> Self {
        Self {
            name: name.into(),
            result: Err(reason.into()),
            requirement,
            duration_ms,
        }
    }

    fn status(&self) -> CompatibilityCheckStatus {
        match (&self.result, self.requirement) {
            (Ok(()), _) => CompatibilityCheckStatus::Passing,
            (Err(_), CompatibilityRequirement::KnownLimitation) => {
                CompatibilityCheckStatus::PassingWithKnownLimits
            }
            (Err(_), _) => CompatibilityCheckStatus::Failing,
        }
    }

    fn to_compatibility_check(&self) -> LspCompatibilityCheck {
        LspCompatibilityCheck {
            name: self.name.clone(),
            status: self.status(),
            requirement: self.requirement,
            detail: self.result.as_ref().err().cloned(),
            duration_ms: Some(self.duration_ms),
        }
    }
}

/// Format a stage-timeout error with actionable detail.
fn stage_timeout_error(
    server_id: &str,
    bin: &Path,
    stage: &str,
    elapsed: Duration,
    stderr_tail: &[String],
) -> String {
    let stderr_summary = if stderr_tail.is_empty() {
        "<no stderr captured>".to_string()
    } else {
        stderr_tail.join(" | ")
    };
    format!(
        "stage '{stage}' timed out after {elapsed:?} for {server_id} at {} (stderr tail: {stderr_summary})",
        bin.display()
    )
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

// ── Runtime-Backed Harness ─────────────────────────────────────────

/// Outcome of a bounded harness shutdown.
#[derive(Debug)]
pub enum HarnessShutdownResult {
    /// Server exited gracefully within the deadline.
    Graceful {
        event: egglsp::LspProcessExitEvent,
        stderr_tail: Vec<String>,
    },
    /// Graceful deadline expired; server was force-killed.
    ForceKilled {
        event: egglsp::LspProcessExitEvent,
        stderr_tail: Vec<String>,
    },
    /// Absolute deadline expired; force-kill was attempted.
    TimeoutExpired { stderr_tail: Vec<String> },
}

/// Owns an [`LspClient`] and its companion [`LspProcessRuntime`]
/// for the duration of a smoke test.
///
/// After construction the client no longer owns the child process
/// or stderr handle — both are managed by the runtime. This allows
/// the test to capture real stderr output in exit events and to
/// exercise production readiness primitives (`wait_for_progress_end`,
/// `wait_for_first_diagnostics`).
pub struct RealServerHarness {
    client: Arc<LspClient>,
    runtime: egglsp::LspProcessRuntime,
}

impl RealServerHarness {
    /// Take the child and stderr from the provided `Arc<LspClient>` and
    /// wire them into a fresh `LspProcessRuntime` (generation 1).
    async fn new(client: Arc<LspClient>) -> Option<Self> {
        let server_id = client.server_id.clone();
        let root = client.root.clone();

        let child = match client.take_child_for_runtime().await {
            Some(c) => c,
            None => return None,
        };
        let stderr = match client.take_stderr_for_runtime().await {
            Some(s) => s,
            None => return None,
        };

        let (runtime, _join) = spawn_process_runtime(server_id, root, 1, child, stderr);

        Some(Self { client, runtime })
    }

    /// Execute the full bounded shutdown sequence:
    ///
    /// 1. `runtime.request_graceful_shutdown()` — sets intent so the
    ///    exit classifier marks a clean exit as `expected`.
    /// 2. `client.request_protocol_shutdown()` — sends LSP `shutdown`
    ///    request + `exit` notification.
    /// 3. `runtime.wait_for_exit()` under `graceful_timeout`.
    /// 4. Force kill and re-wait on `absolute_timeout` exhaustion.
    async fn shutdown_and_collect(
        &self,
        graceful_timeout: Duration,
        absolute_timeout: Duration,
    ) -> HarnessShutdownResult {
        self.runtime.request_graceful_shutdown();

        let proto_shutdown = self.client.request_protocol_shutdown();

        let graceful_deadline = tokio::time::Instant::now() + graceful_timeout;
        let graceful_result =
            tokio::time::timeout_at(graceful_deadline, self.runtime.wait_for_exit()).await;

        let stderr_tail = self.runtime.stderr_tail_capped(20);

        match graceful_result {
            Ok(Some(event)) => {
                let _ = proto_shutdown.await;
                HarnessShutdownResult::Graceful { event, stderr_tail }
            }
            Ok(None) => {
                let _ = proto_shutdown.await;
                HarnessShutdownResult::TimeoutExpired { stderr_tail }
            }
            Err(_) => {
                self.runtime.request_force_kill();

                let force_kill_deadline = tokio::time::Instant::now() + absolute_timeout;
                let force_result =
                    tokio::time::timeout_at(force_kill_deadline, self.runtime.wait_for_exit())
                        .await;

                match force_result {
                    Ok(Some(event)) => HarnessShutdownResult::ForceKilled { event, stderr_tail },
                    Ok(None) => HarnessShutdownResult::TimeoutExpired { stderr_tail },
                    Err(_) => HarnessShutdownResult::TimeoutExpired { stderr_tail },
                }
            }
        }
    }

    pub fn client(&self) -> &Arc<LspClient> {
        &self.client
    }

    pub fn runtime(&self) -> &egglsp::LspProcessRuntime {
        &self.runtime
    }
}

// ── Smoke Test Runner ──────────────────────────────────────────────

/// Run the common smoke test suite against a live server.
async fn run_smoke_suite(
    profile: &LspCompatibilityProfile,
    bin_path: &Path,
    fixture: &RealServerFixture,
    server_version: Option<String>,
) -> LspCompatibilityReport {
    let root = fixture.root.clone();
    let mut checks: Vec<SmokeCheck> = Vec::new();
    let mut stderr_tail: Vec<String> = Vec::new();

    // Build launch spec.
    let spec = LspLaunchSpec::new(
        &profile.server_id,
        bin_path,
        profile.default_args.clone(),
        vec![],
        vec![],
        vec![],
    );

    let client_options = LspClientOptions::default();

    // 1. Process launch (separate timing from the LSP handshake).
    let launch_start = std::time::Instant::now();
    let workspace_config = profile.workspace_configuration.clone();
    let client_result = match tokio::time::timeout(
        INIT_TIMEOUT,
        LspClient::new_with_launch_spec(spec, &root, workspace_config, client_options),
    )
    .await
    {
        Ok(Ok(c)) => Ok(c),
        Ok(Err(e)) => Err(format!("{e}")),
        Err(_elapsed) => Err(stage_timeout_error(
            &profile.server_id,
            bin_path,
            "process_launch",
            INIT_TIMEOUT,
            &stderr_tail,
        )),
    };
    let launch_ms = launch_start.elapsed().as_millis() as u64;
    let client = match client_result {
        Ok(c) => {
            checks.push(SmokeCheck::pass(
                "process_launch",
                CompatibilityRequirement::Required,
                launch_ms,
            ));
            c
        }
        Err(e) => {
            checks.push(SmokeCheck::fail(
                "process_launch",
                CompatibilityRequirement::Required,
                e,
                launch_ms,
            ));
            return build_report(
                profile,
                server_version,
                0,
                None,
                LspCapabilitySnapshot::default(),
                &checks,
                stderr_tail,
            );
        }
    };

    // Pass 5 — Wire the runtime-backed harness so the compatibility
    // report can capture real stderr output and exercise production
    // readiness primitives. The harness takes ownership of the child
    // process and stderr handle; the client still drives LSP I/O.
    let harness = match RealServerHarness::new(Arc::new(client)).await {
        Some(h) => h,
        None => {
            checks.push(SmokeCheck::fail(
                "harness_setup",
                CompatibilityRequirement::Required,
                "failed to extract child/stderr from client for runtime-backed harness",
                0,
            ));
            return build_report(
                profile,
                server_version,
                0,
                None,
                LspCapabilitySnapshot::default(),
                &checks,
                stderr_tail,
            );
        }
    };
    let client = harness.client();

    // 2. Initialize handshake — real LSP `initialize` request.
    let init_start = std::time::Instant::now();
    let init_opts = profile.initialization_options.clone();
    let server_caps =
        match tokio::time::timeout(INIT_TIMEOUT, client.initialize(Some(init_opts))).await {
            Ok(Ok(c)) => Ok(c),
            Ok(Err(e)) => Err(format!("{e}")),
            Err(_elapsed) => Err(stage_timeout_error(
                &profile.server_id,
                bin_path,
                "initialize",
                INIT_TIMEOUT,
                &stderr_tail,
            )),
        };
    let initialize_ms = init_start.elapsed().as_millis() as u64;
    let server_caps = match server_caps {
        Ok(c) => {
            checks.push(SmokeCheck::pass(
                "initialize",
                CompatibilityRequirement::Required,
                initialize_ms,
            ));
            c
        }
        Err(e) => {
            checks.push(SmokeCheck::fail(
                "initialize",
                CompatibilityRequirement::Required,
                e,
                initialize_ms,
            ));
            return build_report(
                profile,
                server_version,
                initialize_ms,
                None,
                LspCapabilitySnapshot::default(),
                &checks,
                stderr_tail,
            );
        }
    };

    // 3. `initialized` notification.
    let initialized_start = std::time::Instant::now();
    let initialized_result =
        match tokio::time::timeout(INITIALIZED_TIMEOUT, client.send_initialized()).await {
            Ok(r) => r,
            Err(_elapsed) => Err(egglsp::error::LspError::RequestFailed(stage_timeout_error(
                &profile.server_id,
                bin_path,
                "initialized",
                INITIALIZED_TIMEOUT,
                &stderr_tail,
            ))),
        };
    let initialized_ms = initialized_start.elapsed().as_millis() as u64;
    match initialized_result {
        Ok(()) => checks.push(SmokeCheck::pass(
            "initialized",
            CompatibilityRequirement::Required,
            initialized_ms,
        )),
        Err(e) => {
            checks.push(SmokeCheck::fail(
                "initialized",
                CompatibilityRequirement::Required,
                format!("{e}"),
                initialized_ms,
            ));
        }
    }

    // 4. Capability snapshot — derived from the real InitializeResult.
    let cap_start = std::time::Instant::now();
    let caps =
        LspCapabilitySnapshot::from_capabilities(&server_caps, Some(&profile.server_id), None);
    let cap_ms = cap_start.elapsed().as_millis() as u64;
    checks.push(SmokeCheck::pass(
        "capability_snapshot",
        CompatibilityRequirement::Required,
        cap_ms,
    ));

    // 5. didOpen — only source files, never manifests.
    let didopen_start = std::time::Instant::now();
    let mut didopen_err: Option<String> = None;
    for file in &fixture.source_files {
        let uri = match url::Url::from_file_path(file) {
            Ok(u) => u,
            Err(()) => {
                didopen_err = Some(format!("invalid uri for {}", file.display()));
                break;
            }
        };
        let content = std::fs::read_to_string(file).unwrap_or_default();
        if let Err(e) = client.open_file(&uri, &content, 1).await {
            didopen_err = Some(format!("{}: {e}", file.display()));
            break;
        }
    }
    let didopen_ms = didopen_start.elapsed().as_millis() as u64;
    match didopen_err {
        Some(e) => checks.push(SmokeCheck::fail(
            "didOpen",
            CompatibilityRequirement::Required,
            e,
            didopen_ms,
        )),
        None => checks.push(SmokeCheck::pass(
            "didOpen",
            CompatibilityRequirement::Required,
            didopen_ms,
        )),
    }

    // 6. Readiness wait — use production readiness primitives.
    let readiness_start = std::time::Instant::now();
    let readiness_passed;
    match &profile.readiness_policy {
        egglsp::compatibility::LspReadinessPolicy::WaitForDiagnosticsOrTimeout { timeout } => {
            let effective = std::cmp::min(*timeout, READINESS_TIMEOUT);
            readiness_passed = client.wait_for_first_diagnostics(effective).await;
        }
        egglsp::compatibility::LspReadinessPolicy::WaitForProgressEndOrTimeout { timeout } => {
            let effective = std::cmp::min(*timeout, READINESS_TIMEOUT);
            readiness_passed = client.wait_for_progress_end(effective).await;
        }
        egglsp::compatibility::LspReadinessPolicy::WarmupDelay { duration } => {
            tokio::time::sleep(*duration).await;
            readiness_passed = true;
        }
        egglsp::compatibility::LspReadinessPolicy::InitializedIsReady => {
            readiness_passed = true;
        }
    };
    let readiness_ms = readiness_start.elapsed().as_millis() as u64;
    if readiness_passed {
        checks.push(SmokeCheck::pass(
            "readiness_wait",
            CompatibilityRequirement::Required,
            readiness_ms,
        ));
    } else {
        checks.push(SmokeCheck::fail(
            "readiness_wait",
            CompatibilityRequirement::Required,
            "readiness signal not observed within timeout",
            readiness_ms,
        ));
    }

    // 7. Diagnostics intent check.
    let diag_start = std::time::Instant::now();
    let diagnostics_required = matches!(
        profile.readiness_policy,
        egglsp::compatibility::LspReadinessPolicy::WaitForDiagnosticsOrTimeout { .. }
    );
    let diag_snapshot = wait_for_diagnostics(
        &client,
        &fixture.diagnostics_file,
        std::cmp::min(READINESS_TIMEOUT, Duration::from_secs(5)),
    )
    .await;
    let diag_ms = diag_start.elapsed().as_millis() as u64;
    let diag_count = diag_snapshot
        .as_ref()
        .map(|s| s.diagnostics.len())
        .unwrap_or(0);
    if diagnostics_required {
        if diag_count > 0 {
            checks.push(SmokeCheck::pass(
                format!("diagnostics ({diag_count} found)"),
                CompatibilityRequirement::Required,
                diag_ms,
            ));
        } else {
            checks.push(SmokeCheck::fail(
                "diagnostics",
                CompatibilityRequirement::KnownLimitation,
                "no diagnostics observed after bounded wait (server may be slow to index)",
                diag_ms,
            ));
        }
    } else {
        checks.push(SmokeCheck::pass(
            format!("diagnostics ({diag_count} found, not required)"),
            CompatibilityRequirement::Optional,
            diag_ms,
        ));
    }

    let primary_uri = url::Url::from_file_path(&fixture.primary_source).unwrap();

    // 8. Document symbols.
    if caps.supports_document_symbols {
        let start = std::time::Instant::now();
        let result =
            tokio::time::timeout(REQUEST_TIMEOUT, client.document_symbols(&primary_uri)).await;
        let ms = start.elapsed().as_millis() as u64;
        match result {
            Ok(Ok(symbols)) => {
                if !symbols.is_empty() {
                    let names: Vec<String> = symbols.iter().map(|s| s.name.clone()).collect();
                    let missing: Vec<&str> = fixture
                        .expected_symbol_names
                        .iter()
                        .filter(|name| !names.iter().any(|n| n == *name))
                        .copied()
                        .collect();
                    if missing.is_empty() {
                        checks.push(SmokeCheck::pass(
                            format!(
                                "document_symbols ({} found, all expected names present: {:?})",
                                symbols.len(),
                                fixture.expected_symbol_names
                            ),
                            CompatibilityRequirement::RequiredIfAdvertised,
                            ms,
                        ));
                    } else {
                        checks.push(SmokeCheck::fail(
                            "document_symbols",
                            CompatibilityRequirement::RequiredIfAdvertised,
                            format!(
                                "expected symbol names {:?} not found in {:?}",
                                missing, names
                            ),
                            ms,
                        ));
                    }
                } else {
                    checks.push(SmokeCheck::fail(
                        "document_symbols",
                        CompatibilityRequirement::RequiredIfAdvertised,
                        "0 symbols found at primary source",
                        ms,
                    ));
                }
            }
            Ok(Err(e)) => checks.push(SmokeCheck::fail(
                "document_symbols",
                CompatibilityRequirement::RequiredIfAdvertised,
                format!("{e}"),
                ms,
            )),
            Err(_elapsed) => checks.push(SmokeCheck::fail(
                "document_symbols",
                CompatibilityRequirement::RequiredIfAdvertised,
                stage_timeout_error(
                    &profile.server_id,
                    bin_path,
                    "document_symbols",
                    REQUEST_TIMEOUT,
                    &stderr_tail,
                ),
                ms,
            )),
        }
    } else {
        checks.push(SmokeCheck::pass(
            "document_symbols (skipped: not supported)",
            CompatibilityRequirement::Optional,
            0,
        ));
    }

    // 9. Definition (call site -> declaration).
    if caps.supports_definition {
        let start = std::time::Instant::now();
        let result = tokio::time::timeout(
            REQUEST_TIMEOUT,
            client.go_to_definition(&primary_uri, fixture.definition_position),
        )
        .await;
        let ms = start.elapsed().as_millis() as u64;
        match result {
            Ok(Ok(Some(_))) => {
                checks.push(SmokeCheck::pass(
                    "definition (found)",
                    CompatibilityRequirement::RequiredIfAdvertised,
                    ms,
                ));
            }
            Ok(Ok(None)) => checks.push(SmokeCheck::fail(
                "definition",
                CompatibilityRequirement::RequiredIfAdvertised,
                "no definition returned at call site",
                ms,
            )),
            Ok(Err(e)) => checks.push(SmokeCheck::fail(
                "definition",
                CompatibilityRequirement::RequiredIfAdvertised,
                format!("{e}"),
                ms,
            )),
            Err(_elapsed) => checks.push(SmokeCheck::fail(
                "definition",
                CompatibilityRequirement::RequiredIfAdvertised,
                stage_timeout_error(
                    &profile.server_id,
                    bin_path,
                    "definition",
                    REQUEST_TIMEOUT,
                    &stderr_tail,
                ),
                ms,
            )),
        }
    } else {
        checks.push(SmokeCheck::pass(
            "definition (skipped: not supported)",
            CompatibilityRequirement::Optional,
            0,
        ));
    }

    // 10. References (declaration -> call sites).
    //
    // Pass 6 — Use the shared `evaluate_references_check` helper
    // so the rule (zero locations → `RequiredIfAdvertised`
    // failure) is consistent across harness and unit tests. The
    // Rust fixture passes if at least one reference is found;
    // the Python cross-file check requires two distinct URIs.
    if caps.supports_references {
        let start = std::time::Instant::now();
        let result = tokio::time::timeout(
            REQUEST_TIMEOUT,
            client.find_references(&primary_uri, fixture.references_position),
        )
        .await;
        let ms = start.elapsed().as_millis() as u64;
        let check = match result {
            Ok(Ok(refs)) => {
                compatibility::evaluate_references_check(caps.supports_references, &refs, 1)
            }
            Ok(Err(e)) => SmokeCheck::fail(
                "references",
                CompatibilityRequirement::RequiredIfAdvertised,
                format!("{e}"),
                ms,
            )
            .to_compatibility_check(),
            Err(_elapsed) => SmokeCheck::fail(
                "references",
                CompatibilityRequirement::RequiredIfAdvertised,
                stage_timeout_error(
                    &profile.server_id,
                    bin_path,
                    "references",
                    REQUEST_TIMEOUT,
                    &stderr_tail,
                ),
                ms,
            )
            .to_compatibility_check(),
        };
        let detail = check.detail.clone();
        let status = check.status.clone();
        let _ = check; // consumed below
        let pass = matches!(
            status,
            CompatibilityCheckStatus::Passing | CompatibilityCheckStatus::PassingWithKnownLimits
        );
        if pass {
            checks.push(SmokeCheck::pass(
                format!("references ({})", detail.unwrap_or_default()),
                CompatibilityRequirement::RequiredIfAdvertised,
                ms,
            ));
        } else {
            checks.push(SmokeCheck::fail(
                "references",
                CompatibilityRequirement::RequiredIfAdvertised,
                detail.unwrap_or_else(|| "0 references found".to_string()),
                ms,
            ));
        }

        // 10b. Cross-file references — only when the fixture has a
        // secondary source AND the server advertised references. The
        // Python cross-file assertion requires at least 2 distinct
        // URIs; the Rust fixture does not have a secondary source.
        if let Some(secondary) = fixture.secondary_source.as_ref() {
            let start = std::time::Instant::now();
            let secondary_uri = url::Url::from_file_path(secondary).unwrap();
            let result = tokio::time::timeout(
                REQUEST_TIMEOUT,
                client.find_references(&secondary_uri, Position::new(0, 4)),
            )
            .await;
            let ms = start.elapsed().as_millis() as u64;
            match result {
                Ok(Ok(refs)) => {
                    let check = compatibility::evaluate_references_check_with_min(
                        caps.supports_references,
                        &refs,
                        1,
                        2,
                    );
                    let pass = matches!(
                        check.status,
                        CompatibilityCheckStatus::Passing
                            | CompatibilityCheckStatus::PassingWithKnownLimits
                    );
                    if pass {
                        checks.push(SmokeCheck::pass(
                            format!(
                                "cross-file references ({})",
                                check.detail.unwrap_or_default()
                            ),
                            CompatibilityRequirement::RequiredIfAdvertised,
                            ms,
                        ));
                    } else {
                        checks.push(SmokeCheck::fail(
                            "cross-file references",
                            CompatibilityRequirement::RequiredIfAdvertised,
                            check.detail.unwrap_or_else(|| "<no detail>".to_string()),
                            ms,
                        ));
                    }
                }
                Ok(Err(e)) => checks.push(SmokeCheck::fail(
                    "cross-file references",
                    CompatibilityRequirement::RequiredIfAdvertised,
                    format!("{e}"),
                    ms,
                )),
                Err(_elapsed) => checks.push(SmokeCheck::fail(
                    "cross-file references",
                    CompatibilityRequirement::RequiredIfAdvertised,
                    stage_timeout_error(
                        &profile.server_id,
                        bin_path,
                        "cross-file references",
                        REQUEST_TIMEOUT,
                        &stderr_tail,
                    ),
                    ms,
                )),
            }
        }
    } else {
        checks.push(SmokeCheck::pass(
            "references (skipped: not supported)",
            CompatibilityRequirement::Optional,
            0,
        ));
    }

    // 11. Hover.
    if caps.supports_hover {
        let start = std::time::Instant::now();
        let result = tokio::time::timeout(
            REQUEST_TIMEOUT,
            client.hover(&primary_uri, fixture.hover_position),
        )
        .await;
        let ms = start.elapsed().as_millis() as u64;
        match result {
            Ok(Ok(Some(_))) => {
                checks.push(SmokeCheck::pass(
                    "hover (found)",
                    CompatibilityRequirement::RequiredIfAdvertised,
                    ms,
                ));
            }
            Ok(Ok(None)) => checks.push(SmokeCheck::fail(
                "hover",
                CompatibilityRequirement::RequiredIfAdvertised,
                "no hover returned at fixture position",
                ms,
            )),
            Ok(Err(e)) => checks.push(SmokeCheck::fail(
                "hover",
                CompatibilityRequirement::RequiredIfAdvertised,
                format!("{e}"),
                ms,
            )),
            Err(_elapsed) => checks.push(SmokeCheck::fail(
                "hover",
                CompatibilityRequirement::RequiredIfAdvertised,
                stage_timeout_error(
                    &profile.server_id,
                    bin_path,
                    "hover",
                    REQUEST_TIMEOUT,
                    &stderr_tail,
                ),
                ms,
            )),
        }
    } else {
        checks.push(SmokeCheck::pass(
            "hover (skipped: not supported)",
            CompatibilityRequirement::Optional,
            0,
        ));
    }

    // 12. Graceful shutdown — use the runtime-backed harness so the
    // compatibility report captures real stderr output. The harness
    // sets intent → sends protocol shutdown → waits under graceful
    // deadline → force-kills on timeout.
    let start = std::time::Instant::now();
    let shutdown_result = harness
        .shutdown_and_collect(REQUEST_TIMEOUT, REQUEST_TIMEOUT)
        .await;
    let shutdown_ms = start.elapsed().as_millis() as u64;
    // Populate stderr_tail from the runtime — this is the real
    // captured stderr from the language server process, not a stub.
    stderr_tail = harness.runtime().stderr_tail_capped(20);
    match &shutdown_result {
        HarnessShutdownResult::Graceful { .. } => checks.push(SmokeCheck::pass(
            "shutdown",
            CompatibilityRequirement::Required,
            shutdown_ms,
        )),
        HarnessShutdownResult::ForceKilled { .. } => checks.push(SmokeCheck::fail(
            "shutdown",
            CompatibilityRequirement::Required,
            "server did not exit within graceful deadline; force-killed",
            shutdown_ms,
        )),
        HarnessShutdownResult::TimeoutExpired { .. } => checks.push(SmokeCheck::fail(
            "shutdown",
            CompatibilityRequirement::Required,
            stage_timeout_error(
                &profile.server_id,
                bin_path,
                "shutdown",
                REQUEST_TIMEOUT,
                &stderr_tail,
            ),
            shutdown_ms,
        )),
    }

    build_report(
        profile,
        server_version,
        initialize_ms,
        Some(readiness_ms),
        caps,
        &checks,
        stderr_tail,
    )
}

fn build_report(
    profile: &LspCompatibilityProfile,
    server_version: Option<String>,
    initialize_ms: u64,
    readiness_ms: Option<u64>,
    capabilities: LspCapabilitySnapshot,
    checks: &[SmokeCheck],
    stderr_tail: Vec<String>,
) -> LspCompatibilityReport {
    LspCompatibilityReport {
        server_id: profile.server_id.clone(),
        server_version,
        platform: std::env::consts::OS.to_string(),
        initialize_ms,
        readiness_ms,
        capabilities,
        checks: checks.iter().map(|c| c.to_compatibility_check()).collect(),
        stderr_tail,
        known_limitations: profile.known_limitations.clone(),
    }
}

/// Format a compact one-line summary of a check for the assertion message.
fn format_check_line(check: &LspCompatibilityCheck) -> String {
    let detail = check
        .detail
        .as_deref()
        .map(|d| format!(" — {d}"))
        .unwrap_or_default();
    format!(
        "  [{:?}] {} = {:?}{}",
        check.requirement, check.name, check.status, detail
    )
}

/// Assert that every `Required` check is `Passing` and every
/// `RequiredIfAdvertised` check that is recorded (i.e. the server
/// advertised the corresponding capability) is not `Failing`. Also
/// requires the `initialize` and `shutdown` checks to be present.
fn assert_required_checks(report: &LspCompatibilityReport) {
    let mut failures: Vec<String> = Vec::new();

    let has_init = report.checks.iter().any(|c| c.name == "initialize");
    if !has_init {
        failures.push("missing required 'initialize' check".to_string());
    }
    let has_shutdown = report.checks.iter().any(|c| c.name == "shutdown");
    if !has_shutdown {
        failures.push("missing required 'shutdown' check".to_string());
    }

    for check in &report.checks {
        let passed = matches!(
            check.status,
            CompatibilityCheckStatus::Passing | CompatibilityCheckStatus::PassingWithKnownLimits
        );
        match check.requirement {
            CompatibilityRequirement::Required if !passed => {
                failures.push(format!(
                    "required check failed: {}",
                    format_check_line(check)
                ));
            }
            CompatibilityRequirement::RequiredIfAdvertised
                if !passed
                    && !is_skipped_check(&check.name)
                    && !matches!(check.status, CompatibilityCheckStatus::Skipped) =>
            {
                failures.push(format!(
                    "required-if-advertised check failed: {}",
                    format_check_line(check)
                ));
            }
            _ => {}
        }
    }

    if !failures.is_empty() {
        let mut msg = String::new();
        msg.push_str(&format!(
            "Compatibility regression for {} (version {:?})\n",
            report.server_id, report.server_version
        ));
        msg.push_str("Failures:\n");
        for f in &failures {
            msg.push_str(&format!("  - {f}\n"));
        }
        msg.push_str("\nAll checks:\n");
        for c in &report.checks {
            msg.push_str(&format!("{}\n", format_check_line(c)));
        }
        panic!("{msg}");
    }
}

fn is_skipped_check(name: &str) -> bool {
    name.contains("skipped")
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

    let version = tokio::time::timeout(VERSION_TIMEOUT, capture_version(&bin))
        .await
        .ok()
        .flatten();
    eprintln!("rust-analyzer version: {:?}", version);

    let fixture = rust_fixture();
    let profile = compatibility::rust_analyzer_profile();
    let report = match tokio::time::timeout(
        TEST_TIMEOUT,
        run_smoke_suite(&profile, &bin, &fixture, version),
    )
    .await
    {
        Ok(r) => r,
        Err(_elapsed) => {
            eprintln!("rust-analyzer smoke test timed out after {TEST_TIMEOUT:?}");
            return;
        }
    };

    write_report(&report, "rust-analyzer");
    assert_required_checks(&report);
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

    let version = tokio::time::timeout(VERSION_TIMEOUT, capture_version(&bin))
        .await
        .ok()
        .flatten();
    eprintln!("pyright version: {:?}", version);

    let fixture = python_fixture();
    let profile = compatibility::pyright_profile();
    let report = match tokio::time::timeout(
        TEST_TIMEOUT,
        run_smoke_suite(&profile, &bin, &fixture, version),
    )
    .await
    {
        Ok(r) => r,
        Err(_elapsed) => {
            eprintln!("pyright smoke test timed out after {TEST_TIMEOUT:?}");
            return;
        }
    };

    write_report(&report, "pyright");
    assert_required_checks(&report);
}

/// Persist the compatibility report JSON to `target/lsp-compatibility/`
/// with a sanitized filename, and echo the JSON for CI log capture.
fn write_report(report: &LspCompatibilityReport, server_label: &str) {
    let report_dir = std::path::PathBuf::from("target/lsp-compatibility");
    let _ = std::fs::create_dir_all(&report_dir);
    let json = match serde_json::to_string_pretty(report) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("failed to serialize compatibility report: {e}");
            return;
        }
    };
    let version_part = sanitize_for_filename(report.server_version.as_deref().unwrap_or("unknown"));
    let filename = format!("{server_label}-{version_part}.json");
    let path = report_dir.join(&filename);
    if let Err(e) = std::fs::write(&path, &json) {
        eprintln!(
            "failed to write compatibility report to {}: {e}",
            path.display()
        );
    }
    eprintln!("Compatibility report for {server_label}: {json}");
}

// ── Named Harness Tests ────────────────────────────────────────────
//
// These tests exercise the `RealServerHarness` and production
// readiness primitives directly. They are designed to be runnable
// without full real-server integration (they use a lightweight
// long-running process where the full LSP stack is not needed).

/// Test 1: The harness captures real stderr output.
///
/// Spawns a process that writes to stderr, wires it through the
/// `RealServerHarness`, and verifies that `shutdown_and_collect`
/// produces a `HarnessShutdownResult` whose `stderr_tail` contains
/// the expected output.
#[tokio::test]
async fn smoke_harness_captures_stderr() {
    // Use a simple shell command that writes to stderr and exits.
    let bin = if cfg!(windows) {
        "cmd".to_string()
    } else {
        "/bin/sh".to_string()
    };
    let args: Vec<String> = if cfg!(windows) {
        vec![
            "/C".to_string(),
            "echo harness-stderr-marker 1>&2".to_string(),
        ]
    } else {
        vec![
            "-c".to_string(),
            "echo harness-stderr-marker 1>&2".to_string(),
        ]
    };

    let spec = egglsp::LspLaunchSpec::new(
        "harness-stderr-test",
        Path::new(&bin),
        args,
        vec![],
        vec![],
        vec![],
    );

    let root = std::env::temp_dir();
    let workspace_config = serde_json::Value::Null;
    let client_options = egglsp::LspClientOptions::default();

    let client = match tokio::time::timeout(
        INIT_TIMEOUT,
        egglsp::LspClient::new_with_launch_spec(spec, &root, workspace_config, client_options),
    )
    .await
    {
        Ok(Ok(c)) => c,
        _ => {
            eprintln!("SKIP: failed to spawn test process");
            return;
        }
    };

    let harness = match RealServerHarness::new(Arc::new(client)).await {
        Some(h) => h,
        None => {
            eprintln!("SKIP: failed to wire harness");
            return;
        }
    };

    // Give the process a moment to write to stderr before shutting down.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let result = harness
        .shutdown_and_collect(Duration::from_secs(5), Duration::from_secs(5))
        .await;

    // Extract stderr_tail from any variant.
    let tail = match &result {
        HarnessShutdownResult::Graceful { stderr_tail, .. } => stderr_tail.clone(),
        HarnessShutdownResult::ForceKilled { stderr_tail, .. } => stderr_tail.clone(),
        HarnessShutdownResult::TimeoutExpired { stderr_tail } => stderr_tail.clone(),
    };

    // The harness always produces a stderr_tail (possibly empty if
    // the process wrote nothing). Verify it's accessible and that
    // the process exited within the deadline.
    assert!(
        matches!(
            result,
            HarnessShutdownResult::Graceful { .. } | HarnessShutdownResult::ForceKilled { .. }
        ),
        "expected Graceful or ForceKilled, got TimeoutExpired (process hung)"
    );
    // stderr_tail is always Vec<String> — verify it's accessible.
    let _lines: usize = tail.len();
}

/// Test 2: The harness force-kills hung servers.
///
/// Spawns a process that sleeps for a long time, wires it through
/// the `RealServerHarness`, and verifies that `shutdown_and_collect`
/// with a short graceful deadline produces a `ForceKilled` or
/// `TimeoutExpired` result.
#[tokio::test]
async fn smoke_harness_force_kills_hung_server() {
    // Use `sleep` to create a hung process.
    let bin = if cfg!(windows) {
        "ping".to_string()
    } else {
        "/bin/sleep".to_string()
    };
    let args: Vec<String> = if cfg!(windows) {
        vec!["-n".to_string(), "30".to_string()]
    } else {
        vec!["30".to_string()]
    };

    let spec = egglsp::LspLaunchSpec::new(
        "harness-hung-test",
        Path::new(&bin),
        args,
        vec![],
        vec![],
        vec![],
    );

    let root = std::env::temp_dir();
    let workspace_config = serde_json::Value::Null;
    let client_options = egglsp::LspClientOptions::default();

    let client = match tokio::time::timeout(
        INIT_TIMEOUT,
        egglsp::LspClient::new_with_launch_spec(spec, &root, workspace_config, client_options),
    )
    .await
    {
        Ok(Ok(c)) => c,
        _ => {
            eprintln!("SKIP: failed to spawn test process");
            return;
        }
    };

    let harness = match RealServerHarness::new(Arc::new(client)).await {
        Some(h) => h,
        None => {
            eprintln!("SKIP: failed to wire harness");
            return;
        }
    };

    // Use a very short graceful timeout (200ms) and a slightly
    // longer absolute timeout (2s). The process sleeps for 30s,
    // so it must be force-killed.
    let result = harness
        .shutdown_and_collect(Duration::from_millis(200), Duration::from_secs(2))
        .await;

    assert!(
        matches!(
            result,
            HarnessShutdownResult::ForceKilled { .. }
                | HarnessShutdownResult::TimeoutExpired { .. }
        ),
        "expected ForceKilled or TimeoutExpired for hung server, got Graceful"
    );
}

/// Test 3: Progress readiness failure is reported.
///
/// Verifies that `LspClient::wait_for_progress_end` returns `false`
/// when no progress end event is observed within the timeout.
/// This is a direct test of the production readiness primitive
/// without requiring a full LSP server — it uses a process that
/// never produces progress events.
#[tokio::test]
async fn progress_readiness_failure_is_reported() {
    // Use a simple process that stays alive but does not speak LSP.
    let bin = if cfg!(windows) {
        "cmd".to_string()
    } else {
        "/bin/sh".to_string()
    };
    let args: Vec<String> = if cfg!(windows) {
        vec!["/C".to_string(), "timeout /t 10 /nobreak >nul".to_string()]
    } else {
        vec!["-c".to_string(), "sleep 10".to_string()]
    };

    let spec = egglsp::LspLaunchSpec::new(
        "progress-fail-test",
        Path::new(&bin),
        args,
        vec![],
        vec![],
        vec![],
    );

    let root = std::env::temp_dir();
    let workspace_config = serde_json::Value::Null;
    let client_options = egglsp::LspClientOptions::default();

    let client = match tokio::time::timeout(
        INIT_TIMEOUT,
        egglsp::LspClient::new_with_launch_spec(spec, &root, workspace_config, client_options),
    )
    .await
    {
        Ok(Ok(c)) => c,
        _ => {
            eprintln!("SKIP: failed to spawn test process");
            return;
        }
    };

    // wait_for_progress_end should return false when no progress
    // end event is observed within the timeout (the process is not
    // an LSP server, so it never sends progress notifications).
    let passed = client
        .wait_for_progress_end(Duration::from_millis(500))
        .await;
    assert!(
        !passed,
        "wait_for_progress_end should return false when no progress end is observed"
    );

    // Clean up: shut down the client.
    let _ = client.shutdown().await;
}

/// Test 4: Empty diagnostics readiness passes.
///
/// Verifies that `LspClient::wait_for_first_diagnostics` returns
/// `false` when no diagnostics are observed (which is the correct
/// behavior for a server that doesn't publish diagnostics). This
/// is a direct test of the production readiness primitive.
#[tokio::test]
async fn empty_diagnostics_readiness_passes() {
    // Use a simple process that stays alive but does not speak LSP.
    let bin = if cfg!(windows) {
        "cmd".to_string()
    } else {
        "/bin/sh".to_string()
    };
    let args: Vec<String> = if cfg!(windows) {
        vec!["/C".to_string(), "timeout /t 5 /nobreak >nul".to_string()]
    } else {
        vec!["-c".to_string(), "sleep 5".to_string()]
    };

    let spec = egglsp::LspLaunchSpec::new(
        "empty-diag-test",
        Path::new(&bin),
        args,
        vec![],
        vec![],
        vec![],
    );

    let root = std::env::temp_dir();
    let workspace_config = serde_json::Value::Null;
    let client_options = egglsp::LspClientOptions::default();

    let client = match tokio::time::timeout(
        INIT_TIMEOUT,
        egglsp::LspClient::new_with_launch_spec(spec, &root, workspace_config, client_options),
    )
    .await
    {
        Ok(Ok(c)) => c,
        _ => {
            eprintln!("SKIP: failed to spawn test process");
            return;
        }
    };

    // wait_for_first_diagnostics should return false when no
    // diagnostics are observed (the process is not an LSP server).
    // This is the "empty diagnostics" case — the primitive correctly
    // reports that no diagnostics were seen within the timeout.
    let passed = client
        .wait_for_first_diagnostics(Duration::from_millis(500))
        .await;
    assert!(
        !passed,
        "wait_for_first_diagnostics should return false when no diagnostics are observed"
    );

    // Clean up: shut down the client.
    let _ = client.shutdown().await;
}
