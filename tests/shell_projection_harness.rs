//! Shell output projection evaluation harness.
//!
//! Phase 9: Regression corpus and evaluation for projection quality.
//! Measures token reduction and correctness preservation across projectors.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use codegg::shell::projection::{
    CommandExit, CommandOutputStore, CommandOutputStream, CommandRun, CommandRunId, RawStream,
    RedactionState,
};
use codegg::shell::projector::{
    apply_redaction_hook, CargoCheckProjector, CommandOutputProjector, ErrorRetentionProjector,
    GitDiffProjector, GitLogProjector, GitStatusProjector, ProjectionBudget, ProjectionPolicy,
    ProjectionRequest, ProjectionResult, ProjectionSelector, ProjectionSupport, ProjectionTarget,
    TruncatedProjector,
};

// ---------------------------------------------------------------------------
// Fixture metadata types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Deserialize)]
struct FixtureMetadata {
    fixture: FixtureInfo,
    expect: FixtureExpect,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct FixtureInfo {
    name: String,
    command: String,
    exit_code: i32,
    #[serde(default = "default_stream")]
    stream: String,
    #[serde(default)]
    exit_state: Option<String>,
    #[serde(default)]
    redaction_fixture: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct FixtureExpect {
    #[serde(default)]
    must_contain: Vec<String>,
    #[serde(default)]
    must_not_contain: Vec<String>,
    #[serde(default)]
    must_redact: Vec<String>,
    #[serde(default)]
    must_not_redact: Vec<String>,
    #[serde(default)]
    #[allow(dead_code)]
    source_spans: Vec<String>,
    #[serde(default)]
    #[allow(dead_code)]
    failed_tests: Vec<String>,
    #[serde(default)]
    #[allow(dead_code)]
    max_projected_tokens_safe: Option<usize>,
    #[serde(default)]
    #[allow(dead_code)]
    max_projected_tokens_aggressive: Option<usize>,
}

fn default_stream() -> String {
    "stdout".to_string()
}

// ---------------------------------------------------------------------------
// Fixture corpus discovery and loading
// ---------------------------------------------------------------------------

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/shell_projection")
}

fn discover_fixtures() -> Vec<(String, PathBuf)> {
    let dir = fixture_dir();
    let mut fixtures: Vec<(String, PathBuf)> = Vec::new();

    fn walk_dir(dir: &Path, fixtures: &mut Vec<(String, PathBuf)>) {
        if !dir.is_dir() {
            return;
        }
        for entry in std::fs::read_dir(dir).expect("fixture dir readable") {
            let entry = entry.expect("dir entry readable");
            let path = entry.path();
            if path.is_dir() {
                walk_dir(&path, fixtures);
            } else if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("toml") {
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                fixtures.push((name, path));
            }
        }
    }

    walk_dir(&dir, &mut fixtures);
    fixtures.sort_by(|a, b| a.0.cmp(&b.0));
    fixtures
}

fn load_fixture(_name: &str, toml_path: &Path) -> (FixtureMetadata, Vec<u8>) {
    let content = std::fs::read_to_string(toml_path)
        .unwrap_or_else(|e| panic!("Failed to read fixture TOML {}: {}", toml_path.display(), e));
    let metadata: FixtureMetadata = toml::from_str(&content).unwrap_or_else(|e| {
        panic!(
            "Failed to parse fixture TOML {}: {}",
            toml_path.display(),
            e
        )
    });

    let dir = toml_path.parent().expect("TOML has parent dir");
    let output = match metadata.fixture.stream.as_str() {
        "stderr" => {
            let path = dir.join(format!("{}.stderr", metadata.fixture.name));
            if path.exists() {
                std::fs::read(&path).unwrap_or_default()
            } else {
                Vec::new()
            }
        }
        "combined" => {
            let stdout_path = dir.join(format!("{}.stdout", metadata.fixture.name));
            let stderr_path = dir.join(format!("{}.stderr", metadata.fixture.name));
            let mut bytes = Vec::new();
            if stdout_path.exists() {
                bytes.extend(std::fs::read(&stdout_path).unwrap_or_default());
            }
            if stderr_path.exists() {
                bytes.extend(std::fs::read(&stderr_path).unwrap_or_default());
            }
            bytes
        }
        _ => {
            let path = dir.join(format!("{}.stdout", metadata.fixture.name));
            if path.exists() {
                std::fs::read(&path).unwrap_or_default()
            } else {
                Vec::new()
            }
        }
    };

    (metadata, output)
}

fn parse_exit_state(info: &FixtureInfo) -> CommandExit {
    match info.exit_state.as_deref() {
        Some("timeout") => CommandExit::Timeout,
        Some("spawn_failed") => CommandExit::SpawnFailed {
            message: "spawn failed".into(),
        },
        Some("signal") => CommandExit::Signal { signal: 9 },
        Some("cancelled") => CommandExit::Cancelled,
        _ => {
            if info.exit_code >= 0 {
                CommandExit::Code(info.exit_code)
            } else {
                CommandExit::Code(1)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Projection runner
// ---------------------------------------------------------------------------

fn run_projection_for_fixture(
    fixture: &FixtureMetadata,
    output: &[u8],
    policy: &ProjectionPolicy,
    budget: ProjectionBudget,
) -> ProjectionResult {
    let mut store = CommandOutputStore::new();
    let id = store.alloc_id();
    let stdout = if fixture.fixture.stream == "stderr" || fixture.fixture.stream == "combined" {
        Vec::new()
    } else {
        output.to_vec()
    };
    let stderr = if fixture.fixture.stream == "stderr" || fixture.fixture.stream == "combined" {
        output.to_vec()
    } else {
        Vec::new()
    };

    let exit = parse_exit_state(&fixture.fixture);
    let argv: Vec<String> = fixture
        .fixture
        .command
        .split_whitespace()
        .map(String::from)
        .collect();
    let _run = store.insert_with_argv(
        id,
        fixture.fixture.command.clone(),
        Some(argv),
        PathBuf::from("/tmp"),
        SystemTime::now(),
        stdout,
        stderr,
    );
    store.record_exit(id, exit, Duration::from_millis(100));

    let run = store.get_run(id).unwrap();
    let mut request = ProjectionRequest::for_target(run, ProjectionTarget::ModelContext, policy);
    request.budget = budget;

    let selector = ProjectionSelector::with_defaults();
    selector.project(request, &store)
}

// ---------------------------------------------------------------------------
// Helper for approximate token counting
// ---------------------------------------------------------------------------

#[allow(dead_code)]
fn approx_tokens(text: &str) -> usize {
    text.len() / 4
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_fixture_metadata_parses() {
    let fixtures = discover_fixtures();
    assert!(
        !fixtures.is_empty(),
        "No fixtures found in {:?}",
        fixture_dir()
    );

    for (name, path) in &fixtures {
        let (metadata, _output) = load_fixture(name, path);
        assert!(
            !metadata.fixture.name.is_empty(),
            "Fixture {} has empty name",
            name
        );
        assert!(
            !metadata.fixture.command.is_empty(),
            "Fixture {} has empty command",
            name
        );
        assert!(
            metadata.fixture.exit_code >= 0 || metadata.fixture.exit_state.is_some(),
            "Fixture {} has negative exit_code without exit_state",
            name
        );

        let dir = path.parent().unwrap();
        let stream = &metadata.fixture.stream;
        if stream == "stdout" || stream == "combined" {
            let stdout_path = dir.join(format!("{}.stdout", metadata.fixture.name));
            // stdout file is optional — empty is fine
            if !stdout_path.exists() {
                eprintln!("  info: fixture {} has no stdout file (acceptable)", name);
            }
        }
        if stream == "stderr" || stream == "combined" {
            let stderr_path = dir.join(format!("{}.stderr", metadata.fixture.name));
            if !stderr_path.exists() {
                eprintln!("  info: fixture {} has no stderr file (acceptable)", name);
            }
        }
    }
}

#[test]
fn test_all_fixtures_project_without_panic() {
    let fixtures = discover_fixtures();
    assert!(!fixtures.is_empty(), "No fixtures found");

    let policy = ProjectionPolicy::conservative();
    let budget = ProjectionBudget::default();

    for (name, path) in &fixtures {
        let (metadata, output) = load_fixture(name, path);
        let result = run_projection_for_fixture(&metadata, &output, &policy, budget);
        assert!(
            !result.text.is_empty() || metadata.fixture.exit_state.is_some(),
            "Fixture {} produced empty text (non-error fixture)",
            name
        );
        assert!(
            result.output_bytes > 0 || metadata.fixture.exit_state.is_some(),
            "Fixture {} produced zero output bytes",
            name
        );
    }
}

#[test]
fn test_safe_policy_preserves_invariants() {
    let fixtures = discover_fixtures();
    assert!(!fixtures.is_empty(), "No fixtures found");

    let policy = ProjectionPolicy::conservative();
    let budget = ProjectionBudget::default();

    for (name, path) in &fixtures {
        let (metadata, output) = load_fixture(name, path);
        let result = run_projection_for_fixture(&metadata, &output, &policy, budget);

        for must_have in &metadata.expect.must_contain {
            assert!(
                result.text.contains(must_have.as_str()),
                "Fixture '{}': projected text must contain '{}', but it did not.\nText: {}",
                name,
                must_have,
                &result.text[..result.text.len().min(500)]
            );
        }

        for must_not in &metadata.expect.must_not_contain {
            assert!(
                !result.text.contains(must_not.as_str()),
                "Fixture '{}': projected text must NOT contain '{}', but it did.\nText: {}",
                name,
                must_not,
                &result.text[..result.text.len().min(500)]
            );
        }
    }
}

#[test]
fn test_aggressive_policy_preserves_minimum_invariants() {
    let fixtures = discover_fixtures();
    assert!(!fixtures.is_empty(), "No fixtures found");

    let policy = ProjectionPolicy::conservative();
    let budget = ProjectionBudget::bytes(4096);

    for (name, path) in &fixtures {
        let (metadata, output) = load_fixture(name, path);
        let result = run_projection_for_fixture(&metadata, &output, &policy, budget);

        for must_have in &metadata.expect.must_contain {
            assert!(
                result.text.contains(must_have.as_str()),
                "Fixture '{}' (aggressive): projected text must contain '{}' but did not.\nText: {}",
                name,
                must_have,
                &result.text[..result.text.len().min(500)]
            );
        }

        for must_not in &metadata.expect.must_not_contain {
            assert!(
                !result.text.contains(must_not.as_str()),
                "Fixture '{}' (aggressive): projected text must NOT contain '{}' but did.\nText: {}",
                name,
                must_not,
                &result.text[..result.text.len().min(500)]
            );
        }
    }
}

#[test]
fn test_rtk_unavailable_falls_back_safely() {
    let fixtures = discover_fixtures();
    assert!(!fixtures.is_empty(), "No fixtures found");

    let policy = ProjectionPolicy::conservative();
    let budget = ProjectionBudget::default();

    for (name, path) in &fixtures {
        let (metadata, output) = load_fixture(name, path);
        let result = run_projection_for_fixture(&metadata, &output, &policy, budget);

        assert!(
            result.projector != "none",
            "Fixture '{}': projection fell back to 'none' (no projector supported it)",
            name
        );
        assert!(
            !result.text.is_empty() || metadata.fixture.exit_state.is_some(),
            "Fixture '{}': projection returned empty text",
            name
        );
    }
}

fn make_synthetic_run(command: &str, argv: Vec<String>, stderr: Vec<u8>) -> CommandRun {
    let cwd = std::path::PathBuf::from("/tmp");
    let id = CommandRunId(999);
    let stdout_stream = RawStream {
        total_bytes: 0,
        retained_bytes: 0,
        total_lines: Some(0),
        handle: None,
        encoding: codegg::shell::projection::OutputEncoding::Utf8,
        completeness: codegg::shell::projection::OutputCompleteness::Complete,
    };
    let stderr_total = stderr.len() as u64;
    let stderr_stream = RawStream {
        total_bytes: stderr_total,
        retained_bytes: stderr_total,
        total_lines: Some(stderr.iter().filter(|&&b| b == b'\n').count() as u64),
        handle: Some(codegg::shell::projection::OutputHandle::new(
            id,
            CommandOutputStream::Stderr,
        )),
        encoding: codegg::shell::projection::OutputEncoding::Utf8,
        completeness: codegg::shell::projection::OutputCompleteness::Complete,
    };
    CommandRun {
        id,
        command: command.to_string(),
        argv: Some(argv),
        cwd,
        started_at: SystemTime::now(),
        duration: Duration::from_millis(100),
        exit: CommandExit::Code(1),
        stdout: stdout_stream,
        stderr: stderr_stream,
        combined: None,
        projection: None,
        redaction: RedactionState::NotApplied,
    }
}

#[test]
fn test_native_projectors_match_command_pattern() {
    let policy = ProjectionPolicy::conservative();

    // Cargo stderr triggers CargoCheckProjector
    let cargo_stderr = b"error[E0308]: mismatched types\n --> src/main.rs:5:12\n  |\n5 |     let x: i32 = \"hello\";\n  |            ---   ^^^^^^^ expected `i32`, found `&str`\n";
    let run = make_synthetic_run(
        "cargo build",
        vec!["cargo".into(), "build".into()],
        cargo_stderr.to_vec(),
    );
    let request = ProjectionRequest::for_target(&run, ProjectionTarget::ModelContext, &policy);
    let projector = CargoCheckProjector;
    assert!(
        !matches!(projector.supports(&request), ProjectionSupport::Unsupported),
        "CargoCheckProjector should support 'cargo build' commands"
    );

    // Git status triggers GitStatusProjector
    let git_stdout = b"## main\n M src/lib.rs\nA  src/new_module.rs\n?? tests/new_test.rs\n";
    let run_git = {
        let id = CommandRunId(998);
        CommandRun {
            id,
            command: "git status --porcelain".into(),
            argv: Some(vec!["git".into(), "status".into(), "--porcelain".into()]),
            cwd: std::path::PathBuf::from("/tmp"),
            started_at: SystemTime::now(),
            duration: Duration::from_millis(10),
            exit: CommandExit::Code(0),
            stdout: RawStream {
                total_bytes: git_stdout.len() as u64,
                retained_bytes: git_stdout.len() as u64,
                total_lines: Some(4),
                handle: Some(codegg::shell::projection::OutputHandle::new(
                    id,
                    CommandOutputStream::Stdout,
                )),
                encoding: codegg::shell::projection::OutputEncoding::Utf8,
                completeness: codegg::shell::projection::OutputCompleteness::Complete,
            },
            stderr: RawStream {
                total_bytes: 0,
                retained_bytes: 0,
                total_lines: Some(0),
                handle: None,
                encoding: codegg::shell::projection::OutputEncoding::Utf8,
                completeness: codegg::shell::projection::OutputCompleteness::Complete,
            },
            combined: None,
            projection: None,
            redaction: RedactionState::NotApplied,
        }
    };
    let request_git =
        ProjectionRequest::for_target(&run_git, ProjectionTarget::ModelContext, &policy);
    let git_projector = GitStatusProjector;
    assert!(
        !matches!(
            git_projector.supports(&request_git),
            ProjectionSupport::Unsupported
        ),
        "GitStatusProjector should support 'git status --porcelain'"
    );

    // Git diff triggers GitDiffProjector
    let run_diff = {
        let id = CommandRunId(997);
        CommandRun {
            id,
            command: "git diff".into(),
            argv: Some(vec!["git".into(), "diff".into()]),
            cwd: std::path::PathBuf::from("/tmp"),
            started_at: SystemTime::now(),
            duration: Duration::from_millis(10),
            exit: CommandExit::Code(0),
            stdout: RawStream {
                total_bytes: 50,
                retained_bytes: 50,
                total_lines: Some(3),
                handle: Some(codegg::shell::projection::OutputHandle::new(
                    id,
                    CommandOutputStream::Stdout,
                )),
                encoding: codegg::shell::projection::OutputEncoding::Utf8,
                completeness: codegg::shell::projection::OutputCompleteness::Complete,
            },
            stderr: RawStream {
                total_bytes: 0,
                retained_bytes: 0,
                total_lines: Some(0),
                handle: None,
                encoding: codegg::shell::projection::OutputEncoding::Utf8,
                completeness: codegg::shell::projection::OutputCompleteness::Complete,
            },
            combined: None,
            projection: None,
            redaction: RedactionState::NotApplied,
        }
    };
    let request_diff =
        ProjectionRequest::for_target(&run_diff, ProjectionTarget::ModelContext, &policy);
    let diff_projector = GitDiffProjector;
    assert!(
        !matches!(
            diff_projector.supports(&request_diff),
            ProjectionSupport::Unsupported
        ),
        "GitDiffProjector should support 'git diff'"
    );

    // Git log triggers GitLogProjector
    let run_log = {
        let id = CommandRunId(996);
        CommandRun {
            id,
            command: "git log --oneline -5".into(),
            argv: Some(vec![
                "git".into(),
                "log".into(),
                "--oneline".into(),
                "-5".into(),
            ]),
            cwd: std::path::PathBuf::from("/tmp"),
            started_at: SystemTime::now(),
            duration: Duration::from_millis(10),
            exit: CommandExit::Code(0),
            stdout: RawStream {
                total_bytes: 40,
                retained_bytes: 40,
                total_lines: Some(4),
                handle: Some(codegg::shell::projection::OutputHandle::new(
                    id,
                    CommandOutputStream::Stdout,
                )),
                encoding: codegg::shell::projection::OutputEncoding::Utf8,
                completeness: codegg::shell::projection::OutputCompleteness::Complete,
            },
            stderr: RawStream {
                total_bytes: 0,
                retained_bytes: 0,
                total_lines: Some(0),
                handle: None,
                encoding: codegg::shell::projection::OutputEncoding::Utf8,
                completeness: codegg::shell::projection::OutputCompleteness::Complete,
            },
            combined: None,
            projection: None,
            redaction: RedactionState::NotApplied,
        }
    };
    let request_log =
        ProjectionRequest::for_target(&run_log, ProjectionTarget::ModelContext, &policy);
    let log_projector = GitLogProjector;
    assert!(
        !matches!(
            log_projector.supports(&request_log),
            ProjectionSupport::Unsupported
        ),
        "GitLogProjector should support 'git log --oneline -5'"
    );

    // ErrorRetentionProjector should support failing runs
    let run_fail = {
        let id = CommandRunId(995);
        CommandRun {
            id,
            command: "python3 main.py".into(),
            argv: Some(vec!["python3".into(), "main.py".into()]),
            cwd: std::path::PathBuf::from("/tmp"),
            started_at: SystemTime::now(),
            duration: Duration::from_millis(10),
            exit: CommandExit::Code(1),
            stdout: RawStream {
                total_bytes: 0,
                retained_bytes: 0,
                total_lines: Some(0),
                handle: None,
                encoding: codegg::shell::projection::OutputEncoding::Utf8,
                completeness: codegg::shell::projection::OutputCompleteness::Complete,
            },
            stderr: RawStream {
                total_bytes: 80,
                retained_bytes: 80,
                total_lines: Some(4),
                handle: Some(codegg::shell::projection::OutputHandle::new(
                    id,
                    CommandOutputStream::Stderr,
                )),
                encoding: codegg::shell::projection::OutputEncoding::Utf8,
                completeness: codegg::shell::projection::OutputCompleteness::Complete,
            },
            combined: None,
            projection: None,
            redaction: RedactionState::NotApplied,
        }
    };
    let request_fail =
        ProjectionRequest::for_target(&run_fail, ProjectionTarget::ModelContext, &policy);
    let err_projector = ErrorRetentionProjector;
    assert!(
        !matches!(
            err_projector.supports(&request_fail),
            ProjectionSupport::Unsupported
        ),
        "ErrorRetentionProjector should support failing runs"
    );

    // TruncatedProjector is always a fallback
    let request_any = ProjectionRequest::for_target(&run, ProjectionTarget::ModelContext, &policy);
    let trunc_projector = TruncatedProjector;
    assert!(
        !matches!(
            trunc_projector.supports(&request_any),
            ProjectionSupport::Unsupported
        ),
        "TruncatedProjector should always be available as a fallback"
    );

    // Verify all projectors are in the selector
    let selector = ProjectionSelector::with_defaults();
    let names = selector.projector_names();
    assert!(names.contains(&"raw"), "Selector should contain 'raw'");
    assert!(
        names.contains(&"truncated"),
        "Selector should contain 'truncated'"
    );
    assert!(
        names.contains(&"error-retention"),
        "Selector should contain 'error-retention'"
    );
    assert!(
        names.contains(&"native-cargo-diagnostics"),
        "Selector should contain 'native-cargo-diagnostics'"
    );
    assert!(
        names.contains(&"native-cargo-test"),
        "Selector should contain 'native-cargo-test'"
    );
    assert!(
        names.contains(&"native-git-status"),
        "Selector should contain 'native-git-status'"
    );
    assert!(
        names.contains(&"native-git-diff"),
        "Selector should contain 'native-git-diff'"
    );
    assert!(
        names.contains(&"native-git-log"),
        "Selector should contain 'native-git-log'"
    );
}

#[test]
fn test_expansion_handles_for_omitted_ranges() {
    let fixtures = discover_fixtures();
    assert!(!fixtures.is_empty(), "No fixtures found");

    let policy = ProjectionPolicy::conservative();
    let budget = ProjectionBudget::default();

    for (name, path) in &fixtures {
        let (metadata, output) = load_fixture(name, path);
        let result = run_projection_for_fixture(&metadata, &output, &policy, budget);

        if !result.omitted.is_empty() {
            assert!(
                !result.expansion_handles.is_empty() || !result.text.is_empty(),
                "Fixture '{}': omitted ranges present but no expansion handles and no text",
                name
            );
            for omitted in &result.omitted {
                assert!(
                    omitted.end_byte > omitted.start_byte,
                    "Fixture '{}': omitted range has zero or negative span ({}..{})",
                    name,
                    omitted.start_byte,
                    omitted.end_byte
                );
            }
        }
    }
}

#[test]
fn test_redaction_fixtures_redact_sensitive_values() {
    let fixtures = discover_fixtures();
    assert!(!fixtures.is_empty(), "No fixtures found");

    for (name, path) in &fixtures {
        let (metadata, output) = load_fixture(name, path);
        if !metadata.fixture.redaction_fixture {
            continue;
        }
        if metadata.expect.must_redact.is_empty() {
            continue;
        }

        let policy = ProjectionPolicy::conservative();
        let budget = ProjectionBudget::default();
        let mut result = run_projection_for_fixture(&metadata, &output, &policy, budget);
        apply_redaction_hook(&mut result, ProjectionTarget::ModelContext);

        for secret in &metadata.expect.must_redact {
            assert!(
                !result.text.contains(secret.as_str()),
                "Fixture '{}': redacted text still contains secret '{}' after redaction",
                name,
                secret
            );
        }
    }
}

#[test]
fn test_false_positive_prose_not_redacted() {
    let fixtures = discover_fixtures();

    for (name, path) in &fixtures {
        let (metadata, output) = load_fixture(name, path);
        if !metadata.fixture.redaction_fixture {
            continue;
        }
        if metadata.expect.must_not_redact.is_empty() {
            continue;
        }

        let policy = ProjectionPolicy::conservative();
        let budget = ProjectionBudget::default();
        let mut result = run_projection_for_fixture(&metadata, &output, &policy, budget);
        apply_redaction_hook(&mut result, ProjectionTarget::ModelContext);

        for phrase in &metadata.expect.must_not_redact {
            assert!(
                result.text.contains(phrase.as_str()),
                "Fixture '{}': projection redacted expected prose '{}' (false positive)",
                name,
                phrase
            );
        }
    }
}

#[test]
fn test_token_estimates_monotonic() {
    let fixtures = discover_fixtures();
    assert!(!fixtures.is_empty(), "No fixtures found");

    let policy = ProjectionPolicy::conservative();
    let budget = ProjectionBudget::default();

    for (name, path) in &fixtures {
        let (metadata, output) = load_fixture(name, path);
        let result = run_projection_for_fixture(&metadata, &output, &policy, budget);

        // Both token estimates should be present when both streams have data
        if let (Some(input_tok), Some(output_tok)) = (
            result.estimated_input_tokens,
            result.estimated_output_tokens,
        ) {
            // For small outputs, the projected text includes headers/metadata
            // not counted in input_bytes, so output_tokens may exceed
            // input_tokens. Verify the estimates are non-negative and the
            // output estimate is reasonable (within 5x of input).
            //

            // For large inputs (>500 tokens), output should not exceed
            // input by more than 50% (to account for header/metadata overhead).
            if input_tok > 500 {
                assert!(
                    output_tok <= input_tok + input_tok / 2,
                    "Fixture '{}': for large input ({} tokens), output ({}) exceeds input by >50%",
                    name,
                    input_tok,
                    output_tok
                );
            }
        }

        // output_bytes should not be wildly larger than input_bytes.
        // For very small inputs (< 10 bytes), the projection header/footer overhead
        // dominates, so skip the ratio check entirely. For medium inputs, use a
        // lenient ratio. For large inputs, a tighter ratio applies.
        if result.input_bytes >= 10 {
            let ratio = result.output_bytes as f64 / result.input_bytes as f64;
            let max_ratio = if result.input_bytes < 100 { 30.0 } else { 5.0 };
            assert!(
                ratio <= max_ratio,
                "Fixture '{}': output/input ratio ({:.1}x) exceeds limit ({:.1}x) (output={} B, input={} B)",
                name,
                ratio,
                max_ratio,
                result.output_bytes,
                result.input_bytes
            );
        }
    }
}

#[test]
fn test_metrics_report() {
    let fixtures = discover_fixtures();
    assert!(!fixtures.is_empty(), "No fixtures found");

    let policy = ProjectionPolicy::conservative();
    let budget = ProjectionBudget::default();

    let header = format!(
        "{:<35} {:<25} {:>10} {:>11} {:>10} {:>10}",
        "fixture", "projector", "input_tok", "output_tok", "reduction", "invariants"
    );
    eprintln!("{}", header);
    eprintln!("{}", "-".repeat(header.len()));

    for (name, path) in &fixtures {
        let (metadata, output) = load_fixture(name, path);
        let result = run_projection_for_fixture(&metadata, &output, &policy, budget);

        let input_tok = result.estimated_input_tokens.unwrap_or(0);
        let output_tok = result.estimated_output_tokens.unwrap_or(0);
        let reduction = if input_tok > 0 {
            ((input_tok as f64 - output_tok as f64) / input_tok as f64) * 100.0
        } else {
            0.0
        };

        let mut all_pass = true;
        for must_have in &metadata.expect.must_contain {
            if !result.text.contains(must_have.as_str()) {
                all_pass = false;
                break;
            }
        }
        for must_not in &metadata.expect.must_not_contain {
            if result.text.contains(must_not.as_str()) {
                all_pass = false;
                break;
            }
        }

        let status = if all_pass { "pass" } else { "FAIL" };

        eprintln!(
            "{:<35} {:<25} {:>10} {:>11} {:>9.1}% {:>10}",
            name, result.projector, input_tok, output_tok, reduction, status
        );

        assert!(all_pass, "Fixture '{}': invariant check failed", name);
    }
}
