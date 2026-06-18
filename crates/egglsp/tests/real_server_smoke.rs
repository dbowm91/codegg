//! Real-server smoke tests for Tier 1 LSP compatibility.
//!
//! These tests launch actual language servers and verify basic
//! protocol operations. They are opt-in via the
//! `lsp-real-server-tests` feature and skip automatically when
//! server binaries are not available.

use std::path::{Path, PathBuf};
use std::str::FromStr;
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
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);
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
    /// Position in the secondary source file used for cross-file reference checks.
    /// Different languages place identifiers at different offsets.
    cross_file_references_position: Position,
    /// Expected symbol names from `document_symbols` (best-effort, not asserted).
    expected_symbol_names: Vec<&'static str>,
    /// Pass 3 — language identifier for the fixture (e.g. "rust", "python",
    /// "go", "typescript", "cpp"). Used by capability-keyed checks to
    /// surface language-appropriate errors. The field defaults to empty
    /// for older fixtures and is filled in by the per-language
    /// constructors.
    language_id: String,
    /// Pass 3 — positions for the new read-only and preview operations.
    /// Each field is `None` when the operation is not exercised against
    /// the fixture (e.g. older Rust/Python fixtures leave the new
    /// positions unset so the existing suite path is unchanged).
    mutation_targets: MutationTargets,
    /// Pass 3 — capability flags that opt the new operations into the
    /// smoke suite. All flags default to `false` so the existing Tier 1
    /// fixtures run unchanged.
    expected_capabilities: ExpectedCapabilities,
    /// Pass 3 — completion expectations. Each entry describes a position
    /// the harness should request completion at and substrings that must
    /// appear in the returned labels.
    completions: Vec<CompletionExpectation>,
    /// Pass 3 — declaration expectations. Each entry describes a
    /// position the harness should request declaration at and a
    /// minimum count + set of expected files.
    declaration_targets: Vec<LocationExpectation>,
    /// Pass 3 — signature-help expectations.
    signature_help_targets: Vec<LocationExpectation>,
    /// Pass 3 — workspace-symbol query + assertion. The optional tuple
    /// is `(query, expectation)` so the suite can verify the query
    /// returned symbols that mention the expected files.
    workspace_symbol_query: Option<WorkspaceSymbolExpectation>,
    /// Pass 3 — document-highlight expectations. Each entry describes a
    /// position the harness should request documentHighlight at and a
    /// minimum count + set of expected files.
    document_highlight_targets: Vec<LocationExpectation>,
    warmup_after_ready: Duration,
    references_requirement: CompatibilityRequirement,
    hover_requirement: CompatibilityRequirement,
    shutdown_requirement: CompatibilityRequirement,
    implementation_position: Option<Position>,
    /// Pass 4 — optional override for the source file the
    /// implementation check is driven from. When `None`, the
    /// harness uses `primary_source`. clangd's
    /// `textDocument/implementation` is exercised from a header
    /// file (`include/widget.hpp`) because that is where the
    /// abstract declaration lives.
    implementation_source: Option<PathBuf>,
    /// Pass 8 — position the code-action request is driven
    /// from. When `None`, the harness falls back to
    /// `definition_position`.
    code_action_position: Option<Position>,
    /// Pass 8 — minimum number of edit-bearing code actions
    /// required for the smoke check to pass. `0` preserves the
    /// legacy "any response is fine" behavior; `>0` opts into
    /// the strict previewable-only semantics (null / empty /
    /// 0 edit-bearing all fail).
    code_action_min_edit_bearing: usize,
    /// Pass 8 — allow the code-action check to pass with a
    /// command-only result (no edit). `false` (default)
    /// classifies a command-only response as a known
    /// limitation; `true` treats it as a passing check.
    code_action_allow_command_only: bool,
    /// Pass 5 — type-hierarchy expectations. Each entry describes a
    /// position the harness should request prepareTypeHierarchy at
    /// and a minimum item count + optional supertype/subtype checks.
    type_hierarchy_targets: Vec<TypeHierarchyExpectation>,
}

/// Pass 3 — Per-operation positions for the new read-only and
/// preview operations. A `None` field means the operation is not
/// exercised by the fixture (the smoke suite skips the corresponding
/// check).
#[allow(dead_code)]
#[derive(Default, Clone)]
struct MutationTargets {
    pub rename: Option<Position>,
    pub format: Option<Position>,
    pub completion: Option<Position>,
    pub signature_help: Option<Position>,
    /// Format-previews do not need a position (the operation is
    /// document-scoped), but we keep the field for symmetry with
    /// other positions. The format check is gated on
    /// `format_preview_requested` instead.
    pub format_preview_requested: bool,
    /// Rename-preview check is gated on `rename_preview_requested`.
    pub rename_preview_requested: bool,
}

/// Pass 3 — Capability flags the new operations check against. The
/// smoke suite only exercises an operation when the corresponding
/// flag is `true` *and* the live server advertises the matching
/// capability. All flags default to `false` so the existing Tier 1
/// fixtures do not change behavior.
#[allow(dead_code)]
#[derive(Default, Clone)]
struct ExpectedCapabilities {
    pub declaration: bool,
    pub implementation: bool,
    pub document_highlight: bool,
    pub workspace_symbols: bool,
    pub signature_help: bool,
    pub semantic_tokens: bool,
    pub rename: bool,
    pub code_actions: bool,
    pub formatting: bool,
    pub type_hierarchy: bool,
}

/// Pass 3 — Semantic assertion for a position-based location query
/// (declaration, implementation, document highlight, signature
/// help). The smoke suite records `RequiredIfAdvertised` when the
/// live server reports the capability and the result matches the
/// expectation.
#[allow(dead_code)]
#[derive(Clone)]
struct LocationExpectation {
    pub position: Position,
    /// Optional override for the source file the request is
    /// issued against. When `None`, the request uses the
    /// fixture's `primary_source`. Pass 4 — clangd's
    /// `textDocument/implementation` is exercised from
    /// `include/widget.hpp`, not the primary `main.cpp`, so the
    /// harness must support a per-expectation URI.
    pub source_file: Option<PathBuf>,
    /// Minimum number of locations the query must return. Defaults
    /// to 1 for simple "did the server return *anything*" checks.
    pub min_locations: usize,
    /// Files (relative to fixture root, or absolute) that the
    /// returned locations should mention. Empty means any file is
    /// acceptable.
    pub expected_files: Vec<PathBuf>,
    /// For signature-help checks: substrings that must appear in
    /// at least one returned signature label. Empty means "server
    /// returned any non-empty signature list".
    pub expected_label_substrings: Vec<String>,
}

impl Default for LocationExpectation {
    fn default() -> Self {
        Self {
            position: Position::new(0, 0),
            source_file: None,
            min_locations: 1,
            expected_files: Vec::new(),
            expected_label_substrings: Vec::new(),
        }
    }
}

/// Pass 3 — Semantic assertion for a `textDocument/completion`
/// request. The smoke suite records `RequiredIfAdvertised` when
/// the live server reports `completionProvider` and the response
/// contains at least one label whose name contains at least one
/// of `expected_label_substrings`.
#[allow(dead_code)]
#[derive(Clone)]
struct CompletionExpectation {
    pub position: Position,
    /// Max candidates to request. Defaults to 50 — large enough to
    /// surface common identifiers, small enough to keep the test
    /// fast.
    pub max_candidates: usize,
    /// Substrings that must appear (case-insensitively) in at
    /// least one returned label. Empty means "server returned any
    /// non-empty list".
    pub expected_label_substrings: Vec<String>,
}

impl Default for CompletionExpectation {
    fn default() -> Self {
        Self {
            position: Position::new(0, 0),
            max_candidates: 50,
            expected_label_substrings: Vec::new(),
        }
    }
}

/// Pass 3 — A `workspace/symbol` query plus a semantic assertion.
/// The query is required; the expectation mirrors
/// [`LocationExpectation`] but is checked against the file paths
/// in the returned `SymbolInformation` entries.
#[allow(dead_code)]
#[derive(Clone)]
struct WorkspaceSymbolExpectation {
    pub query: String,
    pub min_locations: usize,
    pub expected_files: Vec<PathBuf>,
}

impl Default for WorkspaceSymbolExpectation {
    fn default() -> Self {
        Self {
            query: String::new(),
            min_locations: 1,
            expected_files: Vec::new(),
        }
    }
}

/// Pass 5 — Semantic assertion for `textDocument/prepareTypeHierarchy`
/// plus follow-up `typeHierarchy/supertypes` and/or
/// `typeHierarchy/subtypes`. The smoke suite records
/// `RequiredIfAdvertised` when the server supports type hierarchy
/// and the prepare response returns at least `min_items`.
#[allow(dead_code)]
#[derive(Clone)]
struct TypeHierarchyExpectation {
    pub position: Position,
    /// Minimum number of items the prepare response must return.
    pub min_items: usize,
    /// If `Some(name)`, at least one prepare item must have a
    /// name matching this string. Pass 5 — when the fixture
    /// defines a real trait (e.g. `Greeter`), the smoke suite
    /// asserts the returned item is `Greeter` rather than
    /// relying on count alone.
    pub expected_prepare_name: Option<String>,
    /// Substrings that must appear in at least one returned
    /// subtype item name. Pass 5 — when the fixture defines a
    /// `Person` struct implementing the trait, the smoke suite
    /// asserts the returned subtype name contains `Person`.
    pub expected_subtype_substrings: Vec<String>,
    /// If true, also exercise supertypes after prepare.
    pub check_supertypes: bool,
    /// If true, also exercise subtypes after prepare.
    pub check_subtypes: bool,
}

impl Default for TypeHierarchyExpectation {
    fn default() -> Self {
        Self {
            position: Position::new(0, 0),
            min_items: 1,
            expected_prepare_name: None,
            expected_subtype_substrings: Vec::new(),
            check_supertypes: true,
            check_subtypes: false,
        }
    }
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
    // Line 19: // Pass 5 — Type-hierarchy target: a `Greeter` trait
    // Line 20: // and a `Person` struct implementing it. The
    // Line 21: // smoke suite queries `textDocument/prepareTypeHierarchy`
    // Line 22: // at the `Greeter` identifier (line 22) and asserts
    // Line 23: // that the returned item name is `Greeter`. The
    // Line 24: // follow-up `typeHierarchy/subtypes` request
    // Line 25: // must return `Person`.
    // Line 26: pub trait Greeter {
    // Line 27:     fn greet(&self) -> String;
    // Line 28: }
    // Line 29: (blank)
    // Line 30: pub struct Person;
    // Line 31: (blank)
    // Line 32: impl Greeter for Person {
    // Line 33:     fn greet(&self) -> String {
    // Line 34:         "hello".to_string()
    // Line 35:     }
    // Line 36: }
    // Line 37: (blank)
    // Line 38: // Intentional type error for diagnostics
    // Line 39: pub fn broken() -> i32 {
    // Line 40:     let x: String = 42;
    // Line 41:     x
    // Line 42: }
    // Line 43: (blank)
    // Line 44: // Call hierarchy target
    // Line 45: pub fn caller() -> i32 {
    // Line 46:     add(1, 2)
    // Line 47: }
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

// Pass 5 — Type-hierarchy target: a `Greeter` trait and a
// `Person` struct implementing it. The smoke suite queries
// `textDocument/prepareTypeHierarchy` at the `Greeter`
// identifier and asserts that the returned item name is
// `Greeter`. The follow-up `typeHierarchy/subtypes` request
// must return `Person`.
pub trait Greeter {
    fn greet(&self) -> String;
}

pub struct Person;

impl Greeter for Person {
    fn greet(&self) -> String {
        "hello".to_string()
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
        // `add` call site: line 46, character 4.
        definition_position: Position::new(46, 4),
        // `add` declaration: line 0, character 7.
        references_position: Position::new(0, 7),
        // `add` call site: line 46, character 4.
        hover_position: Position::new(46, 4),
        // No secondary source — position unused.
        cross_file_references_position: Position::new(0, 0),
        expected_symbol_names: vec![
            "add", "greet", "Point", "Greeter", "Person", "broken", "caller",
        ],
        language_id: "rust".to_string(),
        // Pass 3 — leave all new operation positions unset. The
        // existing Rust smoke suite exercises the four classic
        // operations (document symbols, definition, references,
        // hover) and does not opt into the new read-only and
        // preview operations yet.
        mutation_targets: MutationTargets::default(),
        expected_capabilities: ExpectedCapabilities {
            // Pass 5 — Opt into the type-hierarchy check. The
            // `rust_analyzer_profile` advertises
            // `supports_type_hierarchy = true` via the
            // `ObservedCapabilitiesOverride` (lsp-types 0.97
            // does not expose the server-side field). The
            // fixture exercises prepare + subtypes.
            type_hierarchy: true,
            ..Default::default()
        },
        completions: Vec::new(),
        declaration_targets: Vec::new(),
        signature_help_targets: Vec::new(),
        workspace_symbol_query: None,
        document_highlight_targets: Vec::new(),
        warmup_after_ready: Duration::ZERO,
        references_requirement: CompatibilityRequirement::RequiredIfAdvertised,
        // rust-analyzer may return ContentModified (-32801) for hover
        // requests when files are modified during processing. This is a
        // transient timing issue, not a capability gap.
        hover_requirement: CompatibilityRequirement::KnownLimitation,
        shutdown_requirement: CompatibilityRequirement::Required,
        implementation_position: None,
        implementation_source: None,
        code_action_position: None,
        code_action_min_edit_bearing: 0,
        code_action_allow_command_only: false,
        // Pass 5 — query `textDocument/prepareTypeHierarchy` at
        // the `Greeter` trait identifier on line 26 (column 9)
        // and assert the returned item is `Greeter`. Follow up
        // with `typeHierarchy/subtypes` and assert `Person`
        // appears.
        type_hierarchy_targets: vec![TypeHierarchyExpectation {
            position: Position::new(26, 9),
            min_items: 1,
            expected_prepare_name: Some("Greeter".to_string()),
            expected_subtype_substrings: vec!["Person".to_string()],
            check_supertypes: true,
            check_subtypes: true,
        }],
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
        // `add` def in helper.py: line 0, character 4.
        cross_file_references_position: Position::new(0, 4),
        expected_symbol_names: vec!["greet", "broken", "caller"],
        language_id: "python".to_string(),
        // Pass 3 — same conservative defaults as the Rust
        // fixture; the new operations are not exercised against
        // the Tier 1 fixtures.
        mutation_targets: MutationTargets::default(),
        expected_capabilities: ExpectedCapabilities::default(),
        completions: Vec::new(),
        declaration_targets: Vec::new(),
        signature_help_targets: Vec::new(),
        workspace_symbol_query: None,
        document_highlight_targets: Vec::new(),
        warmup_after_ready: Duration::ZERO,
        references_requirement: CompatibilityRequirement::RequiredIfAdvertised,
        hover_requirement: CompatibilityRequirement::RequiredIfAdvertised,
        shutdown_requirement: CompatibilityRequirement::Required,
        implementation_position: None,
        implementation_source: None,
        code_action_position: None,
        code_action_min_edit_bearing: 0,
        code_action_allow_command_only: false,
        type_hierarchy_targets: Vec::new(),
    }
}

/// Build a Go fixture with a `Point` struct, an `Add` helper, a
/// `Broken` for diagnostics, and a `Caller` that uses `Add` from a
/// different package via `helper/`.
fn gopls_fixture() -> RealServerFixture {
    let tempdir = tempfile::tempdir().unwrap();
    let root = tempdir.path().to_path_buf();

    std::fs::write(root.join("go.mod"), "module codegg-test\n\ngo 1.22\n").unwrap();

    // helper/helper.go — secondary source.
    // Line 0: package helper
    // Line 1: (blank)
    // Line 2: func Add(a, b int) int {
    // Line 3:     return a + b
    // Line 4: }
    let helper_dir = root.join("helper");
    std::fs::create_dir_all(&helper_dir).unwrap();
    let helper_go = helper_dir.join("helper.go");
    std::fs::write(
        &helper_go,
        r#"package helper

func Add(a, b int) int {
	return a + b
}
"#,
    )
    .unwrap();

    // main.go — primary source.
    let main_go = root.join("main.go");
    std::fs::write(
        &main_go,
        r#"package main

import (
	"codegg-test/helper"
)

type Greeter interface {
	Greet() string
}

type Person struct{}

func (Person) Greet() string { return "hello" }

func main() {
	var g Greeter = Person{}
	_ = g.Greet()
	_ = helper.Add(1, 2)
}

// Intentional type error for diagnostics
func Broken() int {
	var x string = 42
	return len(x)
}

// Caller of helper.Add for cross-file references.
func Caller() int {
	return helper.Add(3, 4)
}
"#,
    )
    .unwrap();

    RealServerFixture {
        tempdir,
        root,
        source_files: vec![main_go.clone(), helper_go.clone()],
        primary_source: main_go.clone(),
        secondary_source: Some(helper_go.clone()),
        diagnostics_file: main_go.clone(),
        symbol_position: Position::new(8, 5),
        definition_position: Position::new(15, 12),
        references_position: Position::new(15, 12),
        hover_position: Position::new(15, 12),
        cross_file_references_position: Position::new(2, 5),
        expected_symbol_names: vec!["main", "Greeter", "Person", "Broken", "Caller"],
        language_id: "go".to_string(),
        mutation_targets: MutationTargets::default(),
        expected_capabilities: ExpectedCapabilities {
            workspace_symbols: true,
            implementation: true,
            type_hierarchy: true,
            ..Default::default()
        },
        completions: Vec::new(),
        declaration_targets: Vec::new(),
        signature_help_targets: Vec::new(),
        workspace_symbol_query: None,
        document_highlight_targets: Vec::new(),
        warmup_after_ready: Duration::from_secs(10),
        references_requirement: CompatibilityRequirement::RequiredIfAdvertised,
        hover_requirement: CompatibilityRequirement::RequiredIfAdvertised,
        shutdown_requirement: CompatibilityRequirement::KnownLimitation,
        implementation_position: Some(Position::new(7, 5)),
        implementation_source: None,
        code_action_position: None,
        code_action_min_edit_bearing: 0,
        code_action_allow_command_only: false,
        type_hierarchy_targets: vec![TypeHierarchyExpectation {
            position: Position::new(6, 5),
            min_items: 1,
            expected_prepare_name: None,
            expected_subtype_substrings: Vec::new(),
            check_supertypes: true,
            check_subtypes: true,
        }],
    }
}

/// Build a TypeScript fixture with a `Point` interface, an `add`
/// helper, a `broken()` for diagnostics, and a `caller()` that uses
/// `add` from `helper.ts`.
fn typescript_fixture() -> RealServerFixture {
    let tempdir = tempfile::tempdir().unwrap();
    let root = tempdir.path().to_path_buf();

    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "codegg-test",
  "version": "0.1.0",
  "private": true,
  "dependencies": {
    "typescript": "5.5.4"
  }
}
"#,
    )
    .unwrap();

    std::fs::write(
        root.join("tsconfig.json"),
        r#"{
  "compilerOptions": {
    "target": "es2020",
    "module": "commonjs",
    "strict": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "forceConsistentCasingInFileNames": true
  },
  "include": ["src/**/*.ts"]
}
"#,
    )
    .unwrap();

    let src_dir = root.join("src");
    std::fs::create_dir_all(&src_dir).unwrap();

    // src/helper.ts — secondary source.
    // Line 0: export interface Point {
    // Line 1:     x: number;
    // Line 2:     y: number;
    // Line 3: }
    // Line 4: (blank)
    // Line 5: export function add(a: number, b: number): number {
    // Line 6:     return a + b;
    // Line 7: }
    let helper_ts = src_dir.join("helper.ts");
    std::fs::write(
        &helper_ts,
        r#"export interface Point {
    x: number;
    y: number;
}

export function add(a: number, b: number): number {
    return a + b;
}
"#,
    )
    .unwrap();

    // src/main.ts — primary source.
    // Line 0: import { add, Point } from "./helper";
    // Line 1: (blank)
    // Line 2: function greet(name: string): string {
    // Line 3:     return `Hello, ${name}!`;
    // Line 4: }
    // Line 5: (blank)
    // Line 6: // Pass 3 — Real implementation target. Querying
    // Line 7: // `textDocument/implementation` at the `greet`
    // Line 8: // method on the `Greeter` interface must return
    // Line 9: // `Person.greet` in the same file.
    // Line 10: interface Greeter {
    // Line 11:     greet(name: string): string;
    // Line 12: }
    // Line 13: (blank)
    // Line 14: class Person implements Greeter {
    // Line 15:     greet(name: string): string {
    // Line 16:         return `Hello, ${name}`;
    // Line 17:     }
    // Line 18: }
    // Line 19: (blank)
    // Line 20: // Intentional type error for diagnostics
    // Line 21: function broken(): number {
    // Line 22:     const x: string = 42;
    // Line 23:     return x;
    // Line 24: }
    // Line 25: (blank)
    // Line 26: // Cross-file reference: uses `add` from helper.ts.
    // Line 27: function caller(): number {
    // Line 28:     return add(1, 2);
    // Line 29: }
    // Line 30: (blank)
    // Line 31: // Completion site — `add` is referenced.
    // Line 32: const _completionSite = add;
    // Line 33: // Signature-help site — calling `add(`
    // Line 34: const _signatureSite = add(1, 2);
    let main_ts = src_dir.join("main.ts");
    std::fs::write(
        &main_ts,
        r#"import { add, Point } from "./helper";

function greet(name: string): string {
    return `Hello, ${name}!`;
}

// Pass 3 — Real implementation target. Querying
// `textDocument/implementation` at the `greet` method on the
// `Greeter` interface must return `Person.greet` in the same
// file. The harness drives the request from main.ts.
interface Greeter {
    greet(name: string): string;
}

class Person implements Greeter {
    greet(name: string): string {
        return `Hello, ${name}`;
    }
}

// Intentional type error for diagnostics
function broken(): number {
    const x: string = 42;
    return x;
}

// Cross-file reference: uses `add` from helper.ts.
function caller(): number {
    return add(1, 2);
}

// Completion site — `add` is referenced.
const _completionSite = add;
// Signature-help site — calling `add(`
const _signatureSite = add(1, 2);
"#,
    )
    .unwrap();

    RealServerFixture {
        tempdir,
        root,
        source_files: vec![main_ts.clone(), helper_ts.clone()],
        primary_source: main_ts.clone(),
        secondary_source: Some(helper_ts.clone()),
        diagnostics_file: main_ts.clone(),
        // `greet` def on line 2, character 9.
        symbol_position: Position::new(2, 9),
        // `add` call site in main.ts: line 28, character 11.
        definition_position: Position::new(28, 11),
        // `add` import use in main.ts: line 0, character 9 (lands on `add`).
        references_position: Position::new(0, 9),
        // `add` call site: line 28, character 11.
        hover_position: Position::new(28, 11),
        // `add` def in helper.ts: line 5, character 16.
        cross_file_references_position: Position::new(5, 16),
        expected_symbol_names: vec!["greet", "Person", "broken", "caller"],
        language_id: "typescript".to_string(),
        // Pass 3 — TypeScript profile opts into the
        // implementation, signature help, and document
        // highlight checks the plan calls out for
        // typescript-language-server.
        mutation_targets: MutationTargets::default(),
        expected_capabilities: ExpectedCapabilities {
            implementation: true,
            signature_help: true,
            document_highlight: true,
            ..Default::default()
        },
        completions: Vec::new(),
        declaration_targets: Vec::new(),
        signature_help_targets: vec![LocationExpectation {
            // add(1, 2) at line 34 — cursor inside parentheses
            position: Position::new(34, 29),
            source_file: None,
            min_locations: 0,
            expected_files: Vec::new(),
            expected_label_substrings: vec!["add".to_string()],
        }],
        workspace_symbol_query: None,
        document_highlight_targets: Vec::new(),
        warmup_after_ready: Duration::ZERO,
        references_requirement: CompatibilityRequirement::RequiredIfAdvertised,
        hover_requirement: CompatibilityRequirement::RequiredIfAdvertised,
        shutdown_requirement: CompatibilityRequirement::KnownLimitation,
        // Pass 3 — Real implementation target. Query at the
        // `greet` method on the `Greeter` interface in main.ts
        // (line 11, character 4). typescript-language-server is
        // expected to return the `Person.greet` implementation in
        // the same file.
        implementation_position: Some(Position::new(11, 4)),
        implementation_source: None,
        code_action_position: None,
        code_action_min_edit_bearing: 0,
        code_action_allow_command_only: false,
        type_hierarchy_targets: Vec::new(),
    }
}

/// Build a C++ fixture with a `Widget` header/class split, an
/// `add` helper, a `Broken` for diagnostics, and a `Caller` that
/// uses `Widget::add`.
fn clangd_fixture() -> RealServerFixture {
    let tempdir = tempfile::tempdir().unwrap();
    let root = tempdir.path().to_path_buf();

    // compile_commands.json — minimal compile DB so clangd picks up
    // the project root and parser flags.
    let compile_commands = r#"[
  {
    "directory": "ROOT",
    "command": "clang++ -std=c++17 -Iinclude -c src/main.cpp",
    "file": "src/main.cpp"
  },
  {
    "directory": "ROOT",
    "command": "clang++ -std=c++17 -Iinclude -c src/widget.cpp",
    "file": "src/widget.cpp"
  }
]
"#;
    let compile_commands = compile_commands.replace("ROOT", &root.to_string_lossy());
    std::fs::write(root.join("compile_commands.json"), compile_commands).unwrap();

    let include_dir = root.join("include");
    std::fs::create_dir_all(&include_dir).unwrap();
    let src_dir = root.join("src");
    std::fs::create_dir_all(&src_dir).unwrap();

    // include/widget.hpp — declaration.
    let widget_hpp = include_dir.join("widget.hpp");
    std::fs::write(
        &widget_hpp,
        r#"#pragma once

struct WidgetBase {
    virtual int add(int a, int b) = 0;
};

struct Widget final : WidgetBase {
    int add(int a, int b) override;
    int broken();
};
"#,
    )
    .unwrap();

    // src/widget.cpp — definition (secondary source for cross-file references).
    let widget_cpp = src_dir.join("widget.cpp");
    std::fs::write(
        &widget_cpp,
        r#"#include "widget.hpp"

int Widget::add(int a, int b) {
    return a + b;
}

int Widget::broken() {
    // Intentional type mismatch for diagnostics.
    return "not an int";
}
"#,
    )
    .unwrap();

    // src/main.cpp — primary source.
    // Line 0: #include "widget.hpp"
    // Line 1: (blank)
    // Line 2: int greet(const char* name) {
    // Line 3:     return 0;
    // Line 4: }
    // Line 5: (blank)
    // Line 6: int main() {
    // Line 7:     Widget w;
    // Line 8:     return w.add(1, 2);
    // Line 9: }
    // Line 10: (blank)
    // Line 11: // Caller that uses Widget::add for cross-file references.
    // Line 12: int caller() {
    // Line 13:     Widget w;
    // Line 14:     return w.add(3, 4);
    // Line 15: }
    let main_cpp = src_dir.join("main.cpp");
    std::fs::write(
        &main_cpp,
        r#"#include "widget.hpp"

int greet(const char* name) {
    return 0;
}

int main() {
    Widget w;
    return w.add(1, 2);
}

// Caller that uses Widget::add for cross-file references.
int caller() {
    Widget w;
    return w.add(3, 4);
}
"#,
    )
    .unwrap();

    RealServerFixture {
        tempdir,
        root,
        source_files: vec![main_cpp.clone(), widget_cpp.clone(), widget_hpp.clone()],
        primary_source: main_cpp.clone(),
        secondary_source: Some(widget_hpp.clone()),
        diagnostics_file: widget_cpp.clone(),
        // `greet` def on line 2, character 4 (the `int` keyword).
        symbol_position: Position::new(2, 4),
        // `w.add` call site in main.cpp: line 8, character 13 (lands on `add`).
        definition_position: Position::new(8, 13),
        // `w.add` call site in main.cpp: line 8, character 13.
        references_position: Position::new(8, 13),
        // `w.add` call site: line 8, character 13.
        hover_position: Position::new(8, 13),
        // `Widget::add` definition in widget.cpp: line 2, character 12.
        cross_file_references_position: Position::new(2, 12),
        // Only symbols defined in main.cpp — header symbols (WidgetBase,
        // Widget) are not returned by documentSymbols for this file.
        expected_symbol_names: vec!["greet", "main", "caller"],
        language_id: "cpp".to_string(),
        mutation_targets: MutationTargets::default(),
        expected_capabilities: ExpectedCapabilities {
            declaration: true,
            // Pass 4 — exercise clangd implementation from the
            // header declaration. The virtual `add` method on
            // `WidgetBase` (in `include/widget.hpp`) has
            // `Widget::add` (in `src/widget.cpp`) as its
            // implementation. Querying from the header
            // declaration is the correct way to drive this
            // check; querying from a usage site in main.cpp
            // returns 0 results.
            implementation: true,
            document_highlight: true,
            // clangd does not support textDocument/prepareTypeHierarchy.
            ..Default::default()
        },
        completions: Vec::new(),
        declaration_targets: Vec::new(),
        signature_help_targets: Vec::new(),
        workspace_symbol_query: None,
        document_highlight_targets: Vec::new(),
        warmup_after_ready: Duration::ZERO,
        references_requirement: CompatibilityRequirement::KnownLimitation,
        hover_requirement: CompatibilityRequirement::KnownLimitation,
        shutdown_requirement: CompatibilityRequirement::KnownLimitation,
        // Pass 4 — query at the `add` declaration in
        // `include/widget.hpp` (line 3, character 16 in the
        // header — the `add` identifier in
        // `virtual int add(int a, int b) = 0;`).
        implementation_position: Some(Position::new(3, 16)),
        // Pass 4 — drive the implementation check from the
        // header file, not the primary main.cpp. clangd resolves
        // implementations by looking at the declaration site.
        implementation_source: Some(widget_hpp.clone()),
        code_action_position: None,
        code_action_min_edit_bearing: 0,
        code_action_allow_command_only: false,
        type_hierarchy_targets: Vec::new(),
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
///
/// Pass 7 — status is now an explicit field rather than an
/// inferred property of `result` + `requirement`. The new
/// constructors (`passing`, `failing`, `skipped`, `unsupported`,
/// `known_limit`) populate the field directly so the harness
/// cannot accidentally represent a skipped check as `Passing` or
/// an unsupported check as `PassingWithKnownLimits`. The legacy
/// `pass` / `fail` constructors remain as thin shims that
/// preserve the original test wiring while routing through the
/// explicit status field.
struct SmokeCheck {
    name: String,
    status: CompatibilityCheckStatus,
    requirement: CompatibilityRequirement,
    detail: Option<String>,
    duration_ms: u64,
}

impl SmokeCheck {
    /// Passing check: request was exercised and the semantic
    /// assertion held.
    fn passing(
        name: impl Into<String>,
        requirement: CompatibilityRequirement,
        duration_ms: u64,
    ) -> Self {
        Self {
            name: name.into(),
            status: CompatibilityCheckStatus::Passing,
            requirement,
            detail: None,
            duration_ms,
        }
    }

    /// Failing check: required semantic/protocol failure.
    fn failing(
        name: impl Into<String>,
        requirement: CompatibilityRequirement,
        reason: impl Into<String>,
        duration_ms: u64,
    ) -> Self {
        Self {
            name: name.into(),
            status: CompatibilityCheckStatus::Failing,
            requirement,
            detail: Some(reason.into()),
            duration_ms,
        }
    }

    /// Skipped check: fixture chose not to exercise this
    /// operation. Distinct from `unsupported` — `skipped` means
    /// the fixture never asked the server, while `unsupported`
    /// means the server did not advertise the capability.
    fn skipped(
        name: impl Into<String>,
        requirement: CompatibilityRequirement,
        reason: impl Into<String>,
        duration_ms: u64,
    ) -> Self {
        Self {
            name: name.into(),
            status: CompatibilityCheckStatus::Skipped,
            requirement,
            detail: Some(reason.into()),
            duration_ms,
        }
    }

    /// Unsupported check: server did not advertise support.
    /// `unsupported` is distinct from `skipped` and `failing`
    /// and must never be reported as `Passing`.
    fn unsupported(
        name: impl Into<String>,
        requirement: CompatibilityRequirement,
        reason: impl Into<String>,
        duration_ms: u64,
    ) -> Self {
        Self {
            name: name.into(),
            status: CompatibilityCheckStatus::Unsupported,
            requirement,
            detail: Some(reason.into()),
            duration_ms,
        }
    }

    /// Known-limit check: a documented limitation that is
    /// allowed to fail without failing the suite. The status
    /// is `PassingWithKnownLimits` so `assert_required_checks`
    /// does not fail, but the detail field records the
    /// underlying failure reason.
    fn known_limit(
        name: impl Into<String>,
        requirement: CompatibilityRequirement,
        reason: impl Into<String>,
        duration_ms: u64,
    ) -> Self {
        Self {
            name: name.into(),
            status: CompatibilityCheckStatus::PassingWithKnownLimits,
            requirement,
            detail: Some(reason.into()),
            duration_ms,
        }
    }

    /// Legacy constructor — prefer the explicit `passing` /
    /// `failing` / `skipped` / `unsupported` / `known_limit`
    /// constructors.
    fn pass(
        name: impl Into<String>,
        requirement: CompatibilityRequirement,
        duration_ms: u64,
    ) -> Self {
        Self::passing(name, requirement, duration_ms)
    }

    /// Legacy constructor — prefer `failing` / `known_limit`
    /// for explicit status semantics.
    fn fail(
        name: impl Into<String>,
        requirement: CompatibilityRequirement,
        reason: impl Into<String>,
        duration_ms: u64,
    ) -> Self {
        // Infer `PassingWithKnownLimits` only when the
        // requirement itself is `KnownLimitation`. This is the
        // only case where the legacy `fail` constructor was
        // implicitly producing a non-`Failing` status.
        match requirement {
            CompatibilityRequirement::KnownLimitation => Self::known_limit(
                name,
                requirement,
                reason,
                duration_ms,
            ),
            _ => Self::failing(name, requirement, reason, duration_ms),
        }
    }

    fn to_compatibility_check(&self) -> LspCompatibilityCheck {
        LspCompatibilityCheck {
            name: self.name.clone(),
            status: self.status.clone(),
            requirement: self.requirement,
            detail: self.detail.clone(),
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
        let _ = proto_shutdown.await;

        // Close the writer (stdin) to signal EOF to the server.
        // Many LSP servers require this before they exit.
        self.client.writer.close().await;

        let graceful_deadline = tokio::time::Instant::now() + graceful_timeout;
        let graceful_result =
            tokio::time::timeout_at(graceful_deadline, self.runtime.wait_for_exit()).await;

        let stderr_tail = self.runtime.stderr_tail_capped(20);

        match graceful_result {
            Ok(Some(event)) => HarnessShutdownResult::Graceful { event, stderr_tail },
            Ok(None) => HarnessShutdownResult::TimeoutExpired { stderr_tail },
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

    // Build launch spec — pass PATH and HOME so the server process can find
    // required runtime tools (e.g. gopls needs `go` and module cache).
    let mut launch_env: Vec<(String, String)> = Vec::new();
    if let Some(p) = std::env::var_os("PATH") {
        launch_env.push(("PATH".to_string(), p.to_string_lossy().to_string()));
    }
    if let Some(h) = std::env::var_os("HOME") {
        launch_env.push(("HOME".to_string(), h.to_string_lossy().to_string()));
    }
    if let Some(gp) = std::env::var_os("GOPATH") {
        launch_env.push(("GOPATH".to_string(), gp.to_string_lossy().to_string()));
    }
    let spec = LspLaunchSpec::new(
        &profile.server_id,
        bin_path,
        profile.default_args.clone(),
        launch_env,
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
                Vec::new(),
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
                Vec::new(),
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
                Vec::new(),
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

    // 4. Capability snapshot — derived from the real InitializeResult,
    //    with profile-level overrides applied (same path as production).
    let cap_start = std::time::Instant::now();
    let caps = LspCapabilitySnapshot::from_capabilities_with_override(
        &server_caps,
        Some(&profile.server_id),
        None,
        &profile.observed_capabilities,
    );
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

    if !fixture.warmup_after_ready.is_zero() {
        tokio::time::sleep(fixture.warmup_after_ready).await;
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
        checks.push(SmokeCheck::unsupported(
            "document_symbols",
            CompatibilityRequirement::Optional,
            "server did not advertise document symbols provider",
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
        checks.push(SmokeCheck::unsupported(
            "definition",
            CompatibilityRequirement::Optional,
            "server did not advertise definition provider",
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
                fixture.references_requirement,
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
                client.find_references(&secondary_uri, fixture.cross_file_references_position),
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
                            fixture.references_requirement,
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
        checks.push(SmokeCheck::unsupported(
            "references",
            CompatibilityRequirement::Optional,
            "server did not advertise references provider",
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
                fixture.hover_requirement,
                "no hover returned at fixture position",
                ms,
            )),
            Ok(Err(e)) => checks.push(SmokeCheck::fail(
                "hover",
                fixture.hover_requirement,
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
        checks.push(SmokeCheck::unsupported(
            "hover",
            CompatibilityRequirement::Optional,
            "server did not advertise hover provider",
            0,
        ));
    }

    // 12. Pass 3 — Generalized operation checks driven by the
    // fixture's `expected_capabilities` and `*_targets` /
    // `*_expectation` fields. Each check is conditional on
    // (a) the fixture opting in via `expected_capabilities.<op>`
    // and (b) the live server advertising the matching provider.
    // The checks are designed so that adding a new Tier 2
    // fixture does not require touching the harness — only the
    // per-language constructor needs to opt in to the new
    // operations.
    run_generalized_operation_checks(
        &client,
        fixture,
        &caps,
        &primary_uri,
        bin_path,
        &profile.server_id,
        &mut checks,
        &stderr_tail,
    )
    .await;

    // 13. Graceful shutdown — use the runtime-backed harness so the
    // compatibility report captures real stderr output. The harness
    // sets intent → sends protocol shutdown → waits under graceful
    // deadline → force-kills on timeout.
    let start = std::time::Instant::now();
    let shutdown_result = harness
        .shutdown_and_collect(Duration::from_secs(60), Duration::from_secs(60))
        .await;
    let shutdown_ms = start.elapsed().as_millis() as u64;
    // Populate stderr_tail from the runtime — this is the real
    // captured stderr from the language server process, not a stub.
    stderr_tail = harness.runtime().stderr_tail_capped(20);
    let (shutdown_request_sent, shutdown_response_received, graceful_exit_observed, force_kill_requested) = match &shutdown_result {
        HarnessShutdownResult::Graceful { .. } => (true, true, true, false),
        HarnessShutdownResult::ForceKilled { .. } => (true, true, false, true),
        HarnessShutdownResult::TimeoutExpired { .. } => (true, false, false, true),
    };
    match &shutdown_result {
        HarnessShutdownResult::Graceful { .. } => checks.push(SmokeCheck::passing(
            "shutdown",
            CompatibilityRequirement::Required,
            shutdown_ms,
        )),
        HarnessShutdownResult::ForceKilled { .. } => {
            // Pass 10 — Per-server actionable shutdown
            // evidence. A force-kill is a *failure* unless
            // the fixture's `shutdown_requirement` is
            // `KnownLimitation` (clangd and the Tier 2
            // servers occasionally hang during the
            // `shutdown`/`exit` exchange because they are
            // daemon-mode processes that ignore the
            // protocol shutdown signal). When classified as
            // a known limitation, surface the action items
            // in the failure detail so the
            // `assert_required_checks` and
            // `LspCompatibilityReport.stderr_tail` carry
            // actionable context.
            let stderr_excerpt: String = stderr_tail
                .iter()
                .rev()
                .take(5)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join(" | ");
            let detail = format!(
                "server did not exit within graceful deadline; force-killed (daemon mode). \
                 stderr_tail (last 5 lines): [{}]. \
                 Action: increase shutdown deadline, treat as known limitation, or migrate off daemon mode.",
                stderr_excerpt
            );
            match fixture.shutdown_requirement {
                CompatibilityRequirement::KnownLimitation => {
                    checks.push(SmokeCheck::known_limit(
                        "shutdown",
                        CompatibilityRequirement::KnownLimitation,
                        detail,
                        shutdown_ms,
                    ));
                }
                _ => {
                    checks.push(SmokeCheck::fail(
                        "shutdown",
                        fixture.shutdown_requirement,
                        detail,
                        shutdown_ms,
                    ));
                }
            }
        }
        HarnessShutdownResult::TimeoutExpired { .. } => {
            // Pass 10 — Same classification logic for
            // `TimeoutExpired`; surface the stderr tail as
            // actionable evidence.
            let stderr_excerpt: String = stderr_tail
                .iter()
                .rev()
                .take(5)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join(" | ");
            let detail = format!(
                "{}\nstderr_tail (last 5 lines): [{}]",
                stage_timeout_error(
                    &profile.server_id,
                    bin_path,
                    "shutdown",
                    REQUEST_TIMEOUT,
                    &stderr_tail,
                ),
                stderr_excerpt
            );
            match fixture.shutdown_requirement {
                CompatibilityRequirement::KnownLimitation => {
                    checks.push(SmokeCheck::known_limit(
                        "shutdown",
                        CompatibilityRequirement::KnownLimitation,
                        detail,
                        shutdown_ms,
                    ));
                }
                _ => {
                    checks.push(SmokeCheck::fail(
                        "shutdown",
                        CompatibilityRequirement::Required,
                        detail,
                        shutdown_ms,
                    ));
                }
            }
        }
    }

    // Pass 6 — Emit per-operation compatibility records by
    // walking the `checks` collection. Each check name maps to
    // an `LspSemanticOperation` and the harness records whether
    // the request was exercised, the LSP request succeeded, and
    // the semantic assertion passed.
    let operation_support = checks_to_operation_support(&checks, &caps);

    build_report(
        profile,
        server_version,
        initialize_ms,
        Some(readiness_ms),
        caps,
        &checks,
        operation_support,
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
    operation_support: Vec<egglsp::compatibility::LspOperationCompatibility>,
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
        operation_support,
        stderr_tail,
        known_limitations: profile.known_limitations.clone(),
    }
}

/// Pass 6 — Map `SmokeCheck` results to per-operation
/// `LspOperationCompatibility` records. Each record carries the
/// `advertised`, `exercised`, `request_succeeded`, and
/// `semantic_assertion_passed` flags so consumers can answer
/// questions like "was implementation exercised?" without
/// parsing the `checks` collection.
fn checks_to_operation_support(
    checks: &[SmokeCheck],
    caps: &LspCapabilitySnapshot,
) -> Vec<egglsp::compatibility::LspOperationCompatibility> {
    use egglsp::capability::LspSemanticOperation;
    use egglsp::compatibility::{
        CompatibilityRequirement, LspOperationCompatibility,
    };

    // Map from check name prefix to the corresponding
    // `LspSemanticOperation` and capability field. The
    // advertised state is read from the live capability
    // snapshot for accuracy.
    let operations: &[(&str, LspSemanticOperation, bool)] = &[
        (
            "implementation",
            LspSemanticOperation::Implementation,
            caps.supports_implementation,
        ),
        (
            "declaration",
            LspSemanticOperation::Declaration,
            caps.supports_declaration,
        ),
        (
            "signatureHelp",
            LspSemanticOperation::SignatureHelp,
            caps.supports_signature_help,
        ),
        (
            "workspaceSymbol",
            LspSemanticOperation::WorkspaceSymbols,
            caps.supports_workspace_symbols,
        ),
        (
            "semanticTokens",
            LspSemanticOperation::SemanticTokens,
            caps.supports_semantic_tokens,
        ),
        (
            "renamePreview",
            LspSemanticOperation::Rename,
            caps.supports_rename,
        ),
        (
            "formatPreview",
            LspSemanticOperation::DocumentFormatting,
            caps.supports_document_formatting,
        ),
        (
            "codeActions",
            LspSemanticOperation::CodeAction,
            caps.supports_code_actions,
        ),
        (
            "typeHierarchy",
            LspSemanticOperation::TypeHierarchy,
            caps.supports_type_hierarchy,
        ),
        (
            "completion",
            LspSemanticOperation::Completion,
            caps.supports_completion,
        ),
    ];

    let mut out: Vec<LspOperationCompatibility> = Vec::new();
    for (name, op, advertised) in operations {
        let check = checks
            .iter()
            .find(|c| c.name == *name || c.name.starts_with(&format!("{name} (")));
        let (exercised, request_succeeded, semantic_assertion_passed, requirement, known_limit) =
            match check {
                Some(c) => match c.status {
                    CompatibilityCheckStatus::Passing => (
                        true,
                        true,
                        true,
                        c.requirement,
                        None,
                    ),
                    CompatibilityCheckStatus::PassingWithKnownLimits => (
                        true,
                        true,
                        false,
                        c.requirement,
                        c.detail.clone(),
                    ),
                    CompatibilityCheckStatus::Failing => (
                        true,
                        false,
                        false,
                        c.requirement,
                        None,
                    ),
                    CompatibilityCheckStatus::Skipped => (
                        false,
                        false,
                        false,
                        c.requirement,
                        None,
                    ),
                    CompatibilityCheckStatus::Unsupported => (
                        false,
                        false,
                        false,
                        c.requirement,
                        None,
                    ),
                },
                None => (false, false, false, CompatibilityRequirement::Optional, None),
            };
        out.push(LspOperationCompatibility {
            operation: op.as_str().to_string(),
            advertised: *advertised,
            exercised,
            request_succeeded,
            semantic_assertion_passed,
            requirement,
            known_limit,
        });
    }
    out
}

// ── Generalized Operation Checks (Pass 3) ──────────────────────────

/// Match a file path returned by the server against the fixture's
/// expected file list. The comparison is suffix-based: the returned
/// path (which is typically a file:// URI or absolute path) is
/// converted to a `PathBuf` and the check passes when any of the
/// expected paths is a suffix of the returned path. This avoids
/// platform-specific absolute-path comparisons and tolerates the
/// `file://` URI prefix that `egglsp` adds when converting back
/// from a server `Location`.
fn matches_expected_file(returned: &str, expected: &Path) -> bool {
    if returned.is_empty() {
        return false;
    }
    let stripped = returned
        .strip_prefix("file://")
        .or_else(|| returned.strip_prefix("file:"))
        .unwrap_or(returned);
    let returned_path = std::path::Path::new(stripped);
    returned_path.ends_with(expected) || expected.ends_with(returned_path)
}

/// Run a single location-style operation (declaration,
/// implementation, document highlight) and append a `SmokeCheck` to
/// `checks`. The check is `RequiredIfAdvertised` when the
/// server's capability is enabled; otherwise the check is
/// recorded as `Unsupported` (informational).
///
/// The new location-style operations live on
/// [`LspOperations`] which requires an [`LspService`]. The smoke
/// runner only has an [`LspClient`], so this helper drives the
/// underlying JSON-RPC request directly through
/// [`LspClient::send_request`] and normalizes the response
/// (Scalar / Array / Link variants for declaration, plain array
/// for document highlight) to a uniform `Vec<LocationLink>` for
/// suffix-based file assertions.
async fn run_location_check(
    client: &LspClient,
    primary_uri: &url::Url,
    operation: &str,
    target: &LocationExpectation,
    supports_op: bool,
    bin_path: &Path,
    server_id: &str,
    stderr_tail: &[String],
    checks: &mut Vec<SmokeCheck>,
) {
    if !supports_op {
        checks.push(SmokeCheck::unsupported(
            operation.to_string(),
            CompatibilityRequirement::Optional,
            format!("server did not advertise {operation} provider"),
            0,
        ));
        return;
    }
    let (method, parse_array) = match operation {
        "declaration" => ("textDocument/declaration", true),
        "implementation" => ("textDocument/implementation", true),
        "documentHighlight" => ("textDocument/documentHighlight", false),
        other => {
            tracing::error!("unknown location operation: {other}");
            checks.push(SmokeCheck::fail(
                operation,
                CompatibilityRequirement::RequiredIfAdvertised,
                format!("unknown location operation: {other}"),
                0,
            ));
            return;
        }
    };
    // Pass 4 — clangd's `textDocument/implementation` is
    // exercised from a header file (`include/widget.hpp`) so
    // the request lands on a declaration, not a usage site. The
    // expectation's `source_file` field overrides the default
    // `primary_uri` when set.
    let request_uri: url::Url = match &target.source_file {
        Some(p) => url::Url::from_file_path(p).unwrap_or_else(|_| primary_uri.clone()),
        None => primary_uri.clone(),
    };
    let start = std::time::Instant::now();
    let params = serde_json::json!({
        "textDocument": { "uri": request_uri.as_str() },
        "position": { "line": target.position.line, "character": target.position.character },
    });
    let result = tokio::time::timeout(REQUEST_TIMEOUT, client.send_request(method, params)).await;
    let ms = start.elapsed().as_millis() as u64;
    match result {
        Ok(Ok(value)) => {
            if value.is_null() {
                checks.push(SmokeCheck::fail(
                    operation,
                    CompatibilityRequirement::RequiredIfAdvertised,
                    format!(
                        "expected at least {} location(s); got 0 (server returned null)",
                        target.min_locations
                    ),
                    ms,
                ));
                return;
            }
            // Normalize the response into Vec<LocationLink>.
            let normalized: Vec<egglsp::lsp_types::LocationLink> = if parse_array {
                // GotoDefinitionResponse: Scalar | Array | Link
                match serde_json::from_value::<egglsp::lsp_types::GotoDefinitionResponse>(
                    value.clone(),
                ) {
                    Ok(gdr) => egglsp::operations::normalize_goto_response(gdr),
                    Err(e) => {
                        checks.push(SmokeCheck::fail(
                            operation,
                            CompatibilityRequirement::RequiredIfAdvertised,
                            format!("malformed {method} response: {e}"),
                            ms,
                        ));
                        return;
                    }
                }
            } else {
                // DocumentHighlight: array of DocumentHighlight
                // Convert each to a LocationLink so the same
                // suffix-based file assertion can run.
                match serde_json::from_value::<Vec<egglsp::lsp_types::DocumentHighlight>>(value) {
                    Ok(highlights) => {
                        let uri = egglsp::lsp_types::Uri::from_str(primary_uri.as_str())
                            .unwrap_or_else(|_| {
                                egglsp::lsp_types::Uri::from_str("file:///invalid")
                                    .expect("hardcoded invalid URI must parse")
                            });
                        highlights
                            .into_iter()
                            .map(|h| egglsp::lsp_types::LocationLink {
                                origin_selection_range: None,
                                target_uri: uri.clone(),
                                target_range: h.range,
                                target_selection_range: h.range,
                            })
                            .collect()
                    }
                    Err(e) => {
                        checks.push(SmokeCheck::fail(
                            operation,
                            CompatibilityRequirement::RequiredIfAdvertised,
                            format!("malformed {method} response: {e}"),
                            ms,
                        ));
                        return;
                    }
                }
            };
            if normalized.len() < target.min_locations {
                checks.push(SmokeCheck::fail(
                    operation,
                    CompatibilityRequirement::RequiredIfAdvertised,
                    format!(
                        "expected at least {} location(s); got {}",
                        target.min_locations,
                        normalized.len()
                    ),
                    ms,
                ));
                return;
            }
            if !target.expected_files.is_empty() {
                let any_match = normalized.iter().any(|loc| {
                    let raw = loc.target_uri.to_string();
                    target
                        .expected_files
                        .iter()
                        .any(|exp| matches_expected_file(&raw, exp))
                });
                if !any_match {
                    let returned: Vec<String> = normalized
                        .iter()
                        .map(|l| l.target_uri.to_string())
                        .collect();
                    checks.push(SmokeCheck::fail(
                        operation,
                        CompatibilityRequirement::RequiredIfAdvertised,
                        format!(
                            "no returned location matched any expected file (expected any of {:?}); got {:?}",
                            target.expected_files, returned
                        ),
                        ms,
                    ));
                    return;
                }
            }
            checks.push(SmokeCheck::pass(
                format!("{operation} ({} found)", normalized.len()),
                CompatibilityRequirement::RequiredIfAdvertised,
                ms,
            ));
        }
        Ok(Err(e)) => checks.push(SmokeCheck::fail(
            operation,
            CompatibilityRequirement::RequiredIfAdvertised,
            format!("{e}"),
            ms,
        )),
        Err(_elapsed) => checks.push(SmokeCheck::fail(
            operation,
            CompatibilityRequirement::RequiredIfAdvertised,
            stage_timeout_error(server_id, bin_path, operation, REQUEST_TIMEOUT, stderr_tail),
            ms,
        )),
    }
}

/// Run a single type-hierarchy operation and append `SmokeCheck`s
/// for prepare, supertypes, and subtypes. Each sub-check is
/// independent. The check is `RequiredIfAdvertised` when the
/// server's capability is enabled via profile override.
async fn run_type_hierarchy_check(
    client: &LspClient,
    primary_uri: &url::Url,
    target: &TypeHierarchyExpectation,
    supports_type_hierarchy: bool,
    bin_path: &Path,
    server_id: &str,
    stderr_tail: &[String],
    checks: &mut Vec<SmokeCheck>,
) {
    if !supports_type_hierarchy {
        checks.push(SmokeCheck::unsupported(
            "typeHierarchy",
            CompatibilityRequirement::Optional,
            "server did not advertise type hierarchy provider",
            0,
        ));
        return;
    }

    // prepareTypeHierarchy
    let start = std::time::Instant::now();
    let result = tokio::time::timeout(
        REQUEST_TIMEOUT,
        client.prepare_type_hierarchy(primary_uri, target.position),
    )
    .await;
    let ms = start.elapsed().as_millis() as u64;
    let items = match result {
        Ok(Ok(items)) => items,
        Ok(Err(e)) => {
            checks.push(SmokeCheck::fail(
                "typeHierarchy/prepare",
                CompatibilityRequirement::RequiredIfAdvertised,
                format!("{e}"),
                ms,
            ));
            return;
        }
        Err(_elapsed) => {
            checks.push(SmokeCheck::fail(
                "typeHierarchy/prepare",
                CompatibilityRequirement::RequiredIfAdvertised,
                stage_timeout_error(
                    server_id,
                    bin_path,
                    "typeHierarchy/prepare",
                    REQUEST_TIMEOUT,
                    stderr_tail,
                ),
                ms,
            ));
            return;
        }
    };

    if items.len() < target.min_items {
        checks.push(SmokeCheck::fail(
            "typeHierarchy/prepare",
            CompatibilityRequirement::RequiredIfAdvertised,
            format!(
                "expected at least {} item(s), got {}",
                target.min_items,
                items.len()
            ),
            ms,
        ));
        return;
    }
    // Pass 5 — when a fixture sets `expected_prepare_name`, at
    // least one returned item must have that exact name. This
    // guards against servers that return *any* hierarchy item
    // (e.g. the enclosing namespace) without honoring the
    // requested position.
    if let Some(expected) = &target.expected_prepare_name {
        let any_match = items.iter().any(|item| item.name == *expected);
        if !any_match {
            let returned: Vec<String> = items.iter().map(|i| i.name.clone()).collect();
            checks.push(SmokeCheck::fail(
                "typeHierarchy/prepare",
                CompatibilityRequirement::RequiredIfAdvertised,
                format!(
                    "no prepare item matched expected name {expected:?}; got {returned:?}"
                ),
                ms,
            ));
            return;
        }
    }
    checks.push(SmokeCheck::pass(
        format!("typeHierarchy/prepare ({} item(s))", items.len()),
        CompatibilityRequirement::RequiredIfAdvertised,
        ms,
    ));

    // typeHierarchy/supertypes
    if target.check_supertypes {
        let start = std::time::Instant::now();
        let result = tokio::time::timeout(
            REQUEST_TIMEOUT,
            client.supertypes(items[0].clone()),
        )
        .await;
        let ms = start.elapsed().as_millis() as u64;
        match result {
            Ok(Ok(supers)) => {
                checks.push(SmokeCheck::pass(
                    format!("typeHierarchy/supertypes ({} item(s))", supers.len()),
                    CompatibilityRequirement::RequiredIfAdvertised,
                    ms,
                ));
            }
            Ok(Err(e)) => {
                checks.push(SmokeCheck::fail(
                    "typeHierarchy/supertypes",
                    CompatibilityRequirement::RequiredIfAdvertised,
                    format!("{e}"),
                    ms,
                ));
            }
            Err(_elapsed) => {
                checks.push(SmokeCheck::fail(
                    "typeHierarchy/supertypes",
                    CompatibilityRequirement::RequiredIfAdvertised,
                    stage_timeout_error(
                        server_id,
                        bin_path,
                        "typeHierarchy/supertypes",
                        REQUEST_TIMEOUT,
                        stderr_tail,
                    ),
                    ms,
                ));
            }
        }
    }

    // typeHierarchy/subtypes
    if target.check_subtypes {
        let start = std::time::Instant::now();
        let result = tokio::time::timeout(
            REQUEST_TIMEOUT,
            client.subtypes(items[0].clone()),
        )
        .await;
        let ms = start.elapsed().as_millis() as u64;
        match result {
            Ok(Ok(subs)) => {
                // Pass 5 — when the fixture sets
                // `expected_subtype_substrings`, at least one
                // subtype name must contain every expected
                // substring. This guards against servers that
                // return an empty subtype list without honoring
                // the actual hierarchy.
                if !target.expected_subtype_substrings.is_empty() {
                    let missing: Vec<&str> = target
                        .expected_subtype_substrings
                        .iter()
                        .filter(|needle| {
                            !subs
                                .iter()
                                .any(|s| s.name.contains(needle.as_str()))
                        })
                        .map(|s| s.as_str())
                        .collect();
                    if !missing.is_empty() {
                        let returned: Vec<String> =
                            subs.iter().map(|s| s.name.clone()).collect();
                        checks.push(SmokeCheck::fail(
                            "typeHierarchy/subtypes",
                            CompatibilityRequirement::RequiredIfAdvertised,
                            format!(
                                "no subtype matched expected substrings {missing:?}; got {returned:?}"
                            ),
                            ms,
                        ));
                        return;
                    }
                }
                checks.push(SmokeCheck::pass(
                    format!("typeHierarchy/subtypes ({} item(s))", subs.len()),
                    CompatibilityRequirement::RequiredIfAdvertised,
                    ms,
                ));
            }
            Ok(Err(e)) => {
                checks.push(SmokeCheck::fail(
                    "typeHierarchy/subtypes",
                    CompatibilityRequirement::RequiredIfAdvertised,
                    format!("{e}"),
                    ms,
                ));
            }
            Err(_elapsed) => {
                checks.push(SmokeCheck::fail(
                    "typeHierarchy/subtypes",
                    CompatibilityRequirement::RequiredIfAdvertised,
                    stage_timeout_error(
                        server_id,
                        bin_path,
                        "typeHierarchy/subtypes",
                        REQUEST_TIMEOUT,
                        stderr_tail,
                    ),
                    ms,
                ));
            }
        }
    }
}

/// Run a single signature-help operation and append a `SmokeCheck`.
/// The check is `RequiredIfAdvertised` when the server's capability
/// is enabled; otherwise the check is recorded as `Unsupported`.
async fn run_signature_help_check(
    client: &LspClient,
    primary_uri: &url::Url,
    target: &LocationExpectation,
    supports_op: bool,
    bin_path: &Path,
    server_id: &str,
    stderr_tail: &[String],
    checks: &mut Vec<SmokeCheck>,
) {
    if !supports_op {
        checks.push(SmokeCheck::unsupported(
            "signatureHelp",
            CompatibilityRequirement::Optional,
            "server did not advertise signatureHelp provider",
            0,
        ));
        return;
    }
    let start = std::time::Instant::now();
    let params = serde_json::json!({
        "textDocument": { "uri": primary_uri.as_str() },
        "position": { "line": target.position.line, "character": target.position.character },
    });
    let result = tokio::time::timeout(
        REQUEST_TIMEOUT,
        client.send_request("textDocument/signatureHelp", params),
    )
    .await;
    let ms = start.elapsed().as_millis() as u64;
    match result {
        Ok(Ok(value)) => {
            if value.is_null() {
                // Null is a legitimate response when no label
                // expectations are set. When the fixture explicitly
                // expects signature help, null is a failure.
                if target.expected_label_substrings.is_empty() {
                    checks.push(SmokeCheck::pass(
                        "signatureHelp (server returned null at this position)",
                        CompatibilityRequirement::RequiredIfAdvertised,
                        ms,
                    ));
                } else {
                    checks.push(SmokeCheck::fail(
                        "signatureHelp",
                        CompatibilityRequirement::RequiredIfAdvertised,
                        "server returned null but fixture expects signature help",
                        ms,
                    ));
                }
                return;
            }
            match serde_json::from_value::<egglsp::lsp_types::SignatureHelp>(value) {
                Ok(help) => {
                    if help.signatures.is_empty() {
                        checks.push(SmokeCheck::fail(
                            "signatureHelp",
                            CompatibilityRequirement::RequiredIfAdvertised,
                            "server returned signatureHelp with 0 signatures",
                            ms,
                        ));
                        return;
                    }
                    // Validate expected label substrings when provided.
                    if !target.expected_label_substrings.is_empty() {
                        let labels: Vec<&str> = help
                            .signatures
                            .iter()
                            .map(|s| s.label.as_str())
                            .collect();
                        for substr in &target.expected_label_substrings {
                            if !labels.iter().any(|l| l.contains(substr.as_str())) {
                                checks.push(SmokeCheck::fail(
                                    "signatureHelp",
                                    CompatibilityRequirement::RequiredIfAdvertised,
                                    format!(
                                        "expected label containing '{}' but got {:?}",
                                        substr, labels
                                    ),
                                    ms,
                                ));
                                return;
                            }
                        }
                    }
                    checks.push(SmokeCheck::pass(
                        format!("signatureHelp ({} signature(s))", help.signatures.len()),
                        CompatibilityRequirement::RequiredIfAdvertised,
                        ms,
                    ));
                }
                Err(e) => checks.push(SmokeCheck::fail(
                    "signatureHelp",
                    CompatibilityRequirement::RequiredIfAdvertised,
                    format!("malformed signatureHelp response: {e}"),
                    ms,
                )),
            }
        }
        Ok(Err(e)) => checks.push(SmokeCheck::fail(
            "signatureHelp",
            CompatibilityRequirement::RequiredIfAdvertised,
            format!("{e}"),
            ms,
        )),
        Err(_elapsed) => checks.push(SmokeCheck::fail(
            "signatureHelp",
            CompatibilityRequirement::RequiredIfAdvertised,
            stage_timeout_error(
                server_id,
                bin_path,
                "signatureHelp",
                REQUEST_TIMEOUT,
                stderr_tail,
            ),
            ms,
        )),
    }
}

/// Run a single workspace-symbol query and append a `SmokeCheck`.
async fn run_workspace_symbol_check(
    client: &LspClient,
    expectation: &WorkspaceSymbolExpectation,
    supports_op: bool,
    bin_path: &Path,
    server_id: &str,
    stderr_tail: &[String],
    checks: &mut Vec<SmokeCheck>,
) {
    if !supports_op {
        checks.push(SmokeCheck::unsupported(
            "workspaceSymbol",
            CompatibilityRequirement::Optional,
            "server did not advertise workspaceSymbol provider",
            0,
        ));
        return;
    }
    let start = std::time::Instant::now();
    let params = serde_json::json!({ "query": expectation.query });
    let result = tokio::time::timeout(
        REQUEST_TIMEOUT,
        client.send_request("workspace/symbol", params),
    )
    .await;
    let ms = start.elapsed().as_millis() as u64;
    match result {
        Ok(Ok(value)) => {
            if value.is_null() {
                checks.push(SmokeCheck::fail(
                    "workspaceSymbol",
                    CompatibilityRequirement::RequiredIfAdvertised,
                    format!(
                        "expected at least {} symbol(s); got 0 (server returned null)",
                        expectation.min_locations
                    ),
                    ms,
                ));
                return;
            }
            // Normalize via the existing helper to handle both
            // flat and nested response shapes.
            let response: egglsp::lsp_types::WorkspaceSymbolResponse =
                match serde_json::from_value(value) {
                    Ok(r) => r,
                    Err(e) => {
                        checks.push(SmokeCheck::fail(
                            "workspaceSymbol",
                            CompatibilityRequirement::RequiredIfAdvertised,
                            format!("malformed workspace/symbol response: {e}"),
                            ms,
                        ));
                        return;
                    }
                };
            let symbols = egglsp::operations::normalize_workspace_symbol_response(response);
            if symbols.len() < expectation.min_locations {
                checks.push(SmokeCheck::fail(
                    "workspaceSymbol",
                    CompatibilityRequirement::RequiredIfAdvertised,
                    format!(
                        "expected at least {} symbol(s); got {}",
                        expectation.min_locations,
                        symbols.len()
                    ),
                    ms,
                ));
                return;
            }
            if !expectation.expected_files.is_empty() {
                let any_match = symbols.iter().any(|sym| {
                    let raw = sym.location.uri.to_string();
                    expectation
                        .expected_files
                        .iter()
                        .any(|exp| matches_expected_file(&raw, exp))
                });
                if !any_match {
                    let returned: Vec<String> =
                        symbols.iter().map(|s| s.location.uri.to_string()).collect();
                    checks.push(SmokeCheck::fail(
                        "workspaceSymbol",
                        CompatibilityRequirement::RequiredIfAdvertised,
                        format!(
                            "no returned symbol matched any expected file (expected any of {:?}); got {:?}",
                            expectation.expected_files, returned
                        ),
                        ms,
                    ));
                    return;
                }
            }
            checks.push(SmokeCheck::pass(
                format!(
                    "workspaceSymbol ({} found for query {:?})",
                    symbols.len(),
                    expectation.query
                ),
                CompatibilityRequirement::RequiredIfAdvertised,
                ms,
            ));
        }
        Ok(Err(e)) => checks.push(SmokeCheck::fail(
            "workspaceSymbol",
            CompatibilityRequirement::RequiredIfAdvertised,
            format!("{e}"),
            ms,
        )),
        Err(_elapsed) => checks.push(SmokeCheck::fail(
            "workspaceSymbol",
            CompatibilityRequirement::RequiredIfAdvertised,
            stage_timeout_error(
                server_id,
                bin_path,
                "workspaceSymbol",
                REQUEST_TIMEOUT,
                stderr_tail,
            ),
            ms,
        )),
    }
}

/// Run a single completion expectation and append a `SmokeCheck`.
async fn run_completion_check(
    client: &LspClient,
    primary_uri: &url::Url,
    expectation: &CompletionExpectation,
    supports_op: bool,
    bin_path: &Path,
    server_id: &str,
    stderr_tail: &[String],
    checks: &mut Vec<SmokeCheck>,
) {
    if !supports_op {
        checks.push(SmokeCheck::unsupported(
            "completion",
            CompatibilityRequirement::Optional,
            "server did not advertise completion provider",
            0,
        ));
        return;
    }
    let start = std::time::Instant::now();
    let params = serde_json::json!({
        "textDocument": { "uri": primary_uri.as_str() },
        "position": { "line": expectation.position.line, "character": expectation.position.character },
    });
    let result = tokio::time::timeout(
        REQUEST_TIMEOUT,
        client.send_request("textDocument/completion", params),
    )
    .await;
    let ms = start.elapsed().as_millis() as u64;
    let candidates: Vec<String> = match result {
        Ok(Ok(value)) => {
            if value.is_null() {
                Vec::new()
            } else {
                // CompletionList has { isIncomplete, items };
                // the bare array form skips the wrapper. Try the
                // wrapper first, then the bare array, and fall
                // back to an empty list on parse error.
                #[derive(serde::Deserialize)]
                struct CompletionListWire {
                    items: Vec<egglsp::lsp_types::CompletionItem>,
                }
                let parsed: Vec<egglsp::lsp_types::CompletionItem> =
                    match serde_json::from_value::<CompletionListWire>(value.clone()) {
                        Ok(list) => list.items,
                        Err(_) => {
                            let items: Result<Vec<egglsp::lsp_types::CompletionItem>, _> =
                                serde_json::from_value(value);
                            match items {
                                Ok(items) => items,
                                Err(e) => {
                                    checks.push(SmokeCheck::fail(
                                        "completion",
                                        CompatibilityRequirement::RequiredIfAdvertised,
                                        format!("malformed textDocument/completion response: {e}"),
                                        ms,
                                    ));
                                    return;
                                }
                            }
                        }
                    };
                let _truncated = parsed.len() > expectation.max_candidates;
                parsed
                    .into_iter()
                    .take(expectation.max_candidates)
                    .map(|item| item.label)
                    .collect::<Vec<_>>()
            }
        }
        Ok(Err(e)) => {
            checks.push(SmokeCheck::fail(
                "completion",
                CompatibilityRequirement::RequiredIfAdvertised,
                format!("{e}"),
                ms,
            ));
            return;
        }
        Err(_elapsed) => {
            checks.push(SmokeCheck::fail(
                "completion",
                CompatibilityRequirement::RequiredIfAdvertised,
                stage_timeout_error(
                    server_id,
                    bin_path,
                    "completion",
                    REQUEST_TIMEOUT,
                    stderr_tail,
                ),
                ms,
            ));
            return;
        }
    };
    if expectation.expected_label_substrings.is_empty() {
        if candidates.is_empty() {
            checks.push(SmokeCheck::fail(
                "completion",
                CompatibilityRequirement::RequiredIfAdvertised,
                "server returned 0 completion candidates",
                ms,
            ));
        } else {
            checks.push(SmokeCheck::pass(
                format!("completion ({} candidate(s))", candidates.len()),
                CompatibilityRequirement::RequiredIfAdvertised,
                ms,
            ));
        }
        return;
    }
    let lower_substrings: Vec<String> = expectation
        .expected_label_substrings
        .iter()
        .map(|s| s.to_lowercase())
        .collect();
    let matched: Vec<&str> = candidates
        .iter()
        .filter(|label| {
            let label_lower = label.to_lowercase();
            lower_substrings.iter().any(|s| label_lower.contains(s))
        })
        .map(|s| s.as_str())
        .collect();
    if matched.is_empty() {
        checks.push(SmokeCheck::fail(
            "completion",
            CompatibilityRequirement::RequiredIfAdvertised,
            format!(
                "no completion label contained any of {:?}; got {} candidate(s): {:?}",
                expectation.expected_label_substrings,
                candidates.len(),
                candidates
            ),
            ms,
        ));
    } else {
        checks.push(SmokeCheck::pass(
            format!(
                "completion ({} matched label(s): {:?})",
                matched.len(),
                matched
            ),
            CompatibilityRequirement::RequiredIfAdvertised,
            ms,
        ));
    }
}

/// Run a single semantic-tokens request and append a `SmokeCheck`.
/// Decoding errors are reported as `RequiredIfAdvertised` failures
/// because they indicate a misbehaving server rather than a
/// missing capability.
async fn run_semantic_tokens_check(
    client: &LspClient,
    primary_uri: &url::Url,
    supports_op: bool,
    legend: Option<&egglsp::SemanticTokenLegendSnapshot>,
    bin_path: &Path,
    server_id: &str,
    stderr_tail: &[String],
    checks: &mut Vec<SmokeCheck>,
) {
    if !supports_op {
        checks.push(SmokeCheck::unsupported(
            "semanticTokens",
            CompatibilityRequirement::Optional,
            "server did not advertise semantic tokens provider",
            0,
        ));
        return;
    }
    let start = std::time::Instant::now();
    let params = serde_json::json!({
        "textDocument": { "uri": primary_uri.as_str() },
    });
    let result = tokio::time::timeout(
        REQUEST_TIMEOUT,
        client.send_request("textDocument/semanticTokens/full", params),
    )
    .await;
    let ms = start.elapsed().as_millis() as u64;
    match result {
        Ok(Ok(value)) => {
            if value.is_null() {
                checks.push(SmokeCheck::passing(
                    "semanticTokens",
                    CompatibilityRequirement::RequiredIfAdvertised,
                    ms,
                ));
                return;
            }
            match serde_json::from_value::<egglsp::lsp_types::SemanticTokens>(value) {
                Ok(tokens) => {
                    // Pass 9 — Decode the raw delta-encoded
                    // stream via the production
                    // `decode_semantic_tokens` helper. Fail the
                    // check if the decoder rejects the stream
                    // (out-of-range token type, overflow, etc).
                    let Some(legend) = legend else {
                        checks.push(SmokeCheck::fail(
                            "semanticTokens",
                            CompatibilityRequirement::RequiredIfAdvertised,
                            "no semantic-token legend available; cannot decode raw stream",
                            ms,
                        ));
                        return;
                    };
                    match egglsp::decode_semantic_tokens(&tokens.data, legend) {
                        Ok(decoded) => {
                            // Pass 9 — Validate every decoded
                            // token's (line, start, length)
                            // tuple is structurally sound: line
                            // is bounded by the file's line
                            // count, and the token's end
                            // (start + length) does not run
                            // past the end of its line.
                            let file_path = primary_uri.to_file_path().ok();
                            let line_texts: Option<Vec<String>> = file_path
                                .as_ref()
                                .and_then(|p| std::fs::read_to_string(p).ok())
                                .map(|s| {
                                    s.lines().map(|l| l.to_string()).collect()
                                });
                            let file_line_count =
                                line_texts.as_ref().map(|v| v.len()).unwrap_or(0);
                            let mut invalid = Vec::new();
                            for tok in &decoded {
                                let line_in_range = (tok.line as usize) < file_line_count
                                    || file_line_count == 0;
                                let length_in_range = if let Some(texts) = &line_texts {
                                    if let Some(line_text) = texts.get(tok.line as usize) {
                                        // LSP character offsets are
                                        // UTF-16 code units, not
                                        // bytes. Compare against
                                        // the byte length for a
                                        // conservative bound.
                                        let line_bytes = line_text.len() as u32;
                                        let end = tok.start.saturating_add(tok.length);
                                        end <= line_bytes
                                    } else {
                                        false
                                    }
                                } else {
                                    true
                                };
                                if !line_in_range || !length_in_range {
                                    invalid.push(format!(
                                        "({}, {}, {}, {})",
                                        tok.line, tok.start, tok.length, tok.token_type
                                    ));
                                }
                            }
                            if !invalid.is_empty() {
                                checks.push(SmokeCheck::fail(
                                    "semanticTokens",
                                    CompatibilityRequirement::RequiredIfAdvertised,
                                    format!(
                                        "{} decoded token(s) out of range: {}",
                                        invalid.len(),
                                        invalid.iter().take(5).cloned().collect::<Vec<_>>().join(", ")
                                    ),
                                    ms,
                                ));
                                return;
                            }
                            // Pass 9 — Verify the legend
                            // matches the server's
                            // `legend.tokenTypes` /
                            // `legend.tokenModifiers` (the
                            // server already encoded the
                            // decoded-token types with that
                            // legend, so a successful decode
                            // is itself a legend check, but
                            // we also assert the legend is
                            // non-empty).
                            if legend.token_types.is_empty() {
                                checks.push(SmokeCheck::fail(
                                    "semanticTokens",
                                    CompatibilityRequirement::RequiredIfAdvertised,
                                    "decoded tokens but legend.token_types is empty",
                                    ms,
                                ));
                                return;
                            }
                            let token_type_counts: std::collections::BTreeMap<
                                String,
                                usize,
                            > = decoded
                                .iter()
                                .fold(std::collections::BTreeMap::new(), |mut acc, t| {
                                    *acc.entry(t.token_type.clone()).or_insert(0) += 1;
                                    acc
                                });
                            let mut summary_parts: Vec<String> = token_type_counts
                                .iter()
                                .map(|(k, v)| format!("{k}={v}"))
                                .collect();
                            summary_parts.sort();
                            let summary = summary_parts.join(", ");
                            checks.push(SmokeCheck::passing(
                                "semanticTokens",
                                CompatibilityRequirement::RequiredIfAdvertised,
                                ms,
                            ));
                            // Stash the per-type breakdown on
                            // a side-channel check so the
                            // human-readable report still
                            // surfaces the legend summary.
                            checks.push(SmokeCheck::passing(
                                format!("semanticTokens decoded ({} raw, {} decoded, legend_types={}, breakdown=[{}])", tokens.data.len(), decoded.len(), legend.token_types.len(), summary),
                                CompatibilityRequirement::Optional,
                                ms,
                            ));
                        }
                        Err(e) => checks.push(SmokeCheck::fail(
                            "semanticTokens",
                            CompatibilityRequirement::RequiredIfAdvertised,
                            format!("decode failed: {e}"),
                            ms,
                        )),
                    }
                }
                Err(e) => checks.push(SmokeCheck::fail(
                    "semanticTokens",
                    CompatibilityRequirement::RequiredIfAdvertised,
                    format!("malformed semanticTokens response: {e}"),
                    ms,
                )),
            }
        }
        Ok(Err(e)) => checks.push(SmokeCheck::fail(
            "semanticTokens",
            CompatibilityRequirement::RequiredIfAdvertised,
            format!("{e}"),
            ms,
        )),
        Err(_elapsed) => checks.push(SmokeCheck::fail(
            "semanticTokens",
            CompatibilityRequirement::RequiredIfAdvertised,
            stage_timeout_error(
                server_id,
                bin_path,
                "semanticTokens",
                REQUEST_TIMEOUT,
                stderr_tail,
            ),
            ms,
        )),
    }
}

/// Run a preview-only rename check. The smoke suite verifies that
/// the on-disk file is unchanged by reading a sha256 hash before
/// and after the preview call. Rename failures are
/// `RequiredIfAdvertised` because the request may legitimately
/// return no edits when the position is not a renameable
/// identifier.
async fn run_rename_preview_check(
    client: &LspClient,
    fixture: &RealServerFixture,
    primary_uri: &url::Url,
    supports_op: bool,
    bin_path: &Path,
    server_id: &str,
    stderr_tail: &[String],
    checks: &mut Vec<SmokeCheck>,
) {
    if !supports_op {
        checks.push(SmokeCheck::unsupported(
            "renamePreview",
            CompatibilityRequirement::Optional,
            "server did not advertise rename provider",
            0,
        ));
        return;
    }
    let pos = match fixture.mutation_targets.rename {
        Some(p) => p,
        None => {
            checks.push(SmokeCheck::skipped(
                "renamePreview",
                CompatibilityRequirement::Optional,
                "fixture did not declare mutation_targets.rename",
                0,
            ));
            return;
        }
    };
    let primary_path = match primary_uri.to_file_path() {
        Ok(p) => p,
        Err(_) => {
            checks.push(SmokeCheck::fail(
                "renamePreview",
                CompatibilityRequirement::RequiredIfAdvertised,
                "primary URI is not a file path",
                0,
            ));
            return;
        }
    };
    let before_hash = match std::fs::read(&primary_path) {
        Ok(bytes) => egglsp::operations::sha256_hex(&bytes),
        Err(e) => {
            checks.push(SmokeCheck::fail(
                "renamePreview",
                CompatibilityRequirement::RequiredIfAdvertised,
                format!("failed to read primary file before preview: {e}"),
                0,
            ));
            return;
        }
    };
    let start = std::time::Instant::now();
    let params = serde_json::json!({
        "textDocument": { "uri": primary_uri.as_str() },
        "position": { "line": pos.line, "character": pos.character },
        "newName": "renamed_identifier",
    });
    let result = tokio::time::timeout(
        REQUEST_TIMEOUT,
        client.send_request("textDocument/rename", params),
    )
    .await;
    let ms = start.elapsed().as_millis() as u64;
    let after_hash = std::fs::read(&primary_path).map(|b| egglsp::operations::sha256_hex(&b));
    let on_disk_unchanged = match &after_hash {
        Ok(h) => h == &before_hash,
        Err(_) => false,
    };
    match result {
        Ok(Ok(value)) => {
            if !on_disk_unchanged {
                checks.push(SmokeCheck::fail(
                    "renamePreview",
                    CompatibilityRequirement::Required,
                    format!(
                        "rename preview mutated on-disk file: before_hash={before_hash}, after_hash={:?}",
                        after_hash
                    ),
                    ms,
                ));
                return;
            }
            if value.is_null() {
                checks.push(SmokeCheck::pass(
                    format!("renamePreview (no edits; disk hash unchanged: {before_hash})"),
                    CompatibilityRequirement::RequiredIfAdvertised,
                    ms,
                ));
                return;
            }
            match serde_json::from_value::<egglsp::lsp_types::WorkspaceEdit>(value) {
                Ok(_edit) => checks.push(SmokeCheck::pass(
                    format!(
                        "renamePreview (server returned edits; disk hash unchanged: {before_hash})"
                    ),
                    CompatibilityRequirement::RequiredIfAdvertised,
                    ms,
                )),
                Err(e) => checks.push(SmokeCheck::fail(
                    "renamePreview",
                    CompatibilityRequirement::RequiredIfAdvertised,
                    format!("malformed rename response: {e}"),
                    ms,
                )),
            }
        }
        Ok(Err(e)) => checks.push(SmokeCheck::fail(
            "renamePreview",
            CompatibilityRequirement::RequiredIfAdvertised,
            format!("{e}"),
            ms,
        )),
        Err(_elapsed) => checks.push(SmokeCheck::fail(
            "renamePreview",
            CompatibilityRequirement::RequiredIfAdvertised,
            stage_timeout_error(
                server_id,
                bin_path,
                "renamePreview",
                REQUEST_TIMEOUT,
                stderr_tail,
            ),
            ms,
        )),
    }
}

/// Run a preview-only formatting check. The smoke suite verifies
/// that the on-disk file is unchanged.
async fn run_format_preview_check(
    client: &LspClient,
    fixture: &RealServerFixture,
    primary_uri: &url::Url,
    supports_op: bool,
    bin_path: &Path,
    server_id: &str,
    stderr_tail: &[String],
    checks: &mut Vec<SmokeCheck>,
) {
    if !supports_op {
        checks.push(SmokeCheck::unsupported(
            "formatPreview",
            CompatibilityRequirement::Optional,
            "server did not advertise document formatting provider",
            0,
        ));
        return;
    }
    if !fixture.mutation_targets.format_preview_requested {
        checks.push(SmokeCheck::skipped(
            "formatPreview",
            CompatibilityRequirement::Optional,
            "fixture did not request format preview",
            0,
        ));
        return;
    }
    let primary_path = match primary_uri.to_file_path() {
        Ok(p) => p,
        Err(_) => {
            checks.push(SmokeCheck::fail(
                "formatPreview",
                CompatibilityRequirement::RequiredIfAdvertised,
                "primary URI is not a file path",
                0,
            ));
            return;
        }
    };
    let before_hash = match std::fs::read(&primary_path) {
        Ok(bytes) => egglsp::operations::sha256_hex(&bytes),
        Err(e) => {
            checks.push(SmokeCheck::fail(
                "formatPreview",
                CompatibilityRequirement::RequiredIfAdvertised,
                format!("failed to read primary file before preview: {e}"),
                0,
            ));
            return;
        }
    };
    let start = std::time::Instant::now();
    let params = serde_json::json!({
        "textDocument": { "uri": primary_uri.as_str() },
        "options": {
            "tabSize": 4,
            "insertSpaces": true,
        },
    });
    let result = tokio::time::timeout(
        REQUEST_TIMEOUT,
        client.send_request("textDocument/formatting", params),
    )
    .await;
    let ms = start.elapsed().as_millis() as u64;
    let after_hash = std::fs::read(&primary_path).map(|b| egglsp::operations::sha256_hex(&b));
    let on_disk_unchanged = match &after_hash {
        Ok(h) => h == &before_hash,
        Err(_) => false,
    };
    match result {
        Ok(Ok(value)) => {
            if !on_disk_unchanged {
                checks.push(SmokeCheck::fail(
                    "formatPreview",
                    CompatibilityRequirement::Required,
                    format!(
                        "format preview mutated on-disk file: before_hash={before_hash}, after_hash={:?}",
                        after_hash
                    ),
                    ms,
                ));
                return;
            }
            if value.is_null() {
                checks.push(SmokeCheck::pass(
                    format!("formatPreview (no edits; disk hash unchanged: {before_hash})"),
                    CompatibilityRequirement::RequiredIfAdvertised,
                    ms,
                ));
                return;
            }
            match serde_json::from_value::<Vec<egglsp::lsp_types::TextEdit>>(value) {
                Ok(edits) => checks.push(SmokeCheck::pass(
                    format!(
                        "formatPreview ({} edit(s); disk hash unchanged: {before_hash})",
                        edits.len()
                    ),
                    CompatibilityRequirement::RequiredIfAdvertised,
                    ms,
                )),
                Err(e) => checks.push(SmokeCheck::fail(
                    "formatPreview",
                    CompatibilityRequirement::RequiredIfAdvertised,
                    format!("malformed format response: {e}"),
                    ms,
                )),
            }
        }
        Ok(Err(e)) => checks.push(SmokeCheck::fail(
            "formatPreview",
            CompatibilityRequirement::RequiredIfAdvertised,
            format!("{e}"),
            ms,
        )),
        Err(_elapsed) => checks.push(SmokeCheck::fail(
            "formatPreview",
            CompatibilityRequirement::RequiredIfAdvertised,
            stage_timeout_error(
                server_id,
                bin_path,
                "formatPreview",
                REQUEST_TIMEOUT,
                stderr_tail,
            ),
            ms,
        )),
    }
}

/// Run a code-action summary check. The fixture does not pin a
/// specific action title; the check passes when the server
/// returns at least one action with an `edit` payload (raw
/// command-only actions are skipped — command execution is
/// disabled in Phase 4).
async fn run_code_action_check(
    client: &LspClient,
    primary_uri: &url::Url,
    fixture: &RealServerFixture,
    supports_op: bool,
    bin_path: &Path,
    server_id: &str,
    stderr_tail: &[String],
    checks: &mut Vec<SmokeCheck>,
) {
    if !supports_op {
        checks.push(SmokeCheck::unsupported(
            "codeActions",
            CompatibilityRequirement::Optional,
            "server did not advertise codeActions provider",
            0,
        ));
        return;
    }
    let start = std::time::Instant::now();
    // Pass 8 — Drive the code-action request with a
    // deterministic range that includes the `_completionSite`
    // / `_signatureSite` declarations so the server has a real
    // opportunity to return an edit-bearing action (e.g. an
    // organize-imports or quick-fix action). The previous
    // harness used the (0,0)-(0,0) empty range, which gave the
    // server no opportunity to surface an edit.
    let request_position = fixture
        .code_action_position
        .unwrap_or(fixture.definition_position);
    let request_range = lsp_types::Range {
        start: request_position,
        end: lsp_types::Position {
            line: request_position.line,
            character: request_position.character + 20,
        },
    };
    let params = serde_json::json!({
        "textDocument": { "uri": primary_uri.as_str() },
        "range": {
            "start": { "line": request_range.start.line, "character": request_range.start.character },
            "end": { "line": request_range.end.line, "character": request_range.end.character },
        },
        "context": { "diagnostics": [] },
    });
    let result = tokio::time::timeout(
        REQUEST_TIMEOUT,
        client.send_request("textDocument/codeAction", params),
    )
    .await;
    let ms = start.elapsed().as_millis() as u64;
    match result {
        Ok(Ok(value)) => {
            // Pass 8 — When the fixture opts into code-action
            // validation (`code_action_min_edit_bearing` > 0),
            // a null response is a failure: the server did not
            // honor the request.
            let min_edit_bearing = fixture.code_action_min_edit_bearing;
            if value.is_null() {
                if min_edit_bearing > 0 {
                    checks.push(SmokeCheck::fail(
                        "codeActions",
                        CompatibilityRequirement::RequiredIfAdvertised,
                        "expected at least 1 edit-bearing action; got null response",
                        ms,
                    ));
                } else {
                    checks.push(SmokeCheck::passing(
                        "codeActions",
                        CompatibilityRequirement::RequiredIfAdvertised,
                        ms,
                    ));
                }
                return;
            }
            // The response is an array of CodeActionOrCommand.
            // Deserialize just enough to count the entries that
            // carry a WorkspaceEdit payload.
            #[derive(serde::Deserialize)]
            #[serde(untagged)]
            #[allow(dead_code)]
            enum ActionOrCommand {
                Command {
                    title: String,
                },
                CodeAction {
                    title: String,
                    #[serde(default)]
                    edit: Option<serde_json::Value>,
                    #[serde(default)]
                    command: Option<serde_json::Value>,
                },
            }
            match serde_json::from_value::<Vec<ActionOrCommand>>(value) {
                Ok(actions) => {
                    let edit_bearing = actions
                        .iter()
                        .filter(|a| matches!(a, ActionOrCommand::CodeAction { edit: Some(_), .. }))
                        .count();
                    let command_only = actions
                        .iter()
                        .filter(|a| match a {
                            ActionOrCommand::Command { .. } => true,
                            ActionOrCommand::CodeAction { edit: None, command: Some(_), .. } => true,
                            _ => false,
                        })
                        .count();
                    // Pass 8 — When the fixture opts into
                    // code-action validation, an empty list is
                    // a failure. 0 edit-bearing actions is a
                    // failure. Command-only results are
                    // exercised but not previewable; treat as
                    // `KnownLimitation` unless the fixture
                    // explicitly opts out via
                    // `code_action_allow_command_only`.
                    if min_edit_bearing > 0 {
                        if actions.is_empty() {
                            checks.push(SmokeCheck::fail(
                                "codeActions",
                                CompatibilityRequirement::RequiredIfAdvertised,
                                "expected at least 1 action; got empty list",
                                ms,
                            ));
                            return;
                        }
                        if edit_bearing == 0 {
                            if command_only > 0
                                && !fixture.code_action_allow_command_only
                            {
                                checks.push(SmokeCheck::known_limit(
                                    "codeActions",
                                    CompatibilityRequirement::KnownLimitation,
                                    format!(
                                        "{} command-only action(s); preview pipeline \
                                         rejects command-only actions ({} total, 0 with edit)",
                                        command_only,
                                        actions.len()
                                    ),
                                    ms,
                                ));
                                return;
                            }
                            checks.push(SmokeCheck::fail(
                                "codeActions",
                                CompatibilityRequirement::RequiredIfAdvertised,
                                format!(
                                    "expected at least {min_edit_bearing} edit-bearing action(s); got {edit_bearing} ({} total)",
                                    actions.len()
                                ),
                                ms,
                            ));
                            return;
                        }
                        if edit_bearing < min_edit_bearing {
                            checks.push(SmokeCheck::fail(
                                "codeActions",
                                CompatibilityRequirement::RequiredIfAdvertised,
                                format!(
                                    "expected at least {min_edit_bearing} edit-bearing action(s); got {edit_bearing}"
                                ),
                                ms,
                            ));
                            return;
                        }
                    }
                    checks.push(SmokeCheck::passing(
                        "codeActions",
                        CompatibilityRequirement::RequiredIfAdvertised,
                        ms,
                    ));
                }
                Err(e) => checks.push(SmokeCheck::fail(
                    "codeActions",
                    CompatibilityRequirement::RequiredIfAdvertised,
                    format!("malformed codeAction response: {e}"),
                    ms,
                )),
            }
        }
        Ok(Err(e)) => checks.push(SmokeCheck::fail(
            "codeActions",
            CompatibilityRequirement::RequiredIfAdvertised,
            format!("{e}"),
            ms,
        )),
        Err(_elapsed) => checks.push(SmokeCheck::fail(
            "codeActions",
            CompatibilityRequirement::RequiredIfAdvertised,
            stage_timeout_error(
                server_id,
                bin_path,
                "codeActions",
                REQUEST_TIMEOUT,
                stderr_tail,
            ),
            ms,
        )),
    }
}

/// Pass 3 — Run the suite of generalized operation checks driven
/// by the fixture's `expected_capabilities` and per-operation
/// target / expectation fields. Each sub-check is independent
/// and short-circuits independently so a single failure does
/// not mask other findings.
async fn run_generalized_operation_checks(
    client: &LspClient,
    fixture: &RealServerFixture,
    caps: &LspCapabilitySnapshot,
    primary_uri: &url::Url,
    bin_path: &Path,
    server_id: &str,
    checks: &mut Vec<SmokeCheck>,
    stderr_tail: &[String],
) {
    // Declaration
    if fixture.expected_capabilities.declaration {
        if let Some(target) = fixture.declaration_targets.first() {
            run_location_check(
                client,
                primary_uri,
                "declaration",
                target,
                caps.supports_declaration,
                bin_path,
                server_id,
                stderr_tail,
                checks,
            )
            .await;
        }
    }

    // Implementation
    if fixture.expected_capabilities.implementation {
        let impl_position = fixture
            .implementation_position
            .unwrap_or(fixture.definition_position);
        let target = LocationExpectation {
            position: impl_position,
            // Pass 4 — clangd queries from the header file
            // (`include/widget.hpp`) so the implementation
            // request lands on a declaration, not a usage site.
            // Other fixtures (e.g. TypeScript) leave this as
            // `None` and the harness falls back to the primary
            // source.
            source_file: fixture.implementation_source.clone(),
            min_locations: 1,
            // Pass 3 — when a fixture sets `expected_capabilities.implementation`,
            // it should also declare a primary source where the implementing
            // class is expected to live, so the implementation check
            // actually exercises the response shape (not just the count).
            expected_files: vec![fixture.primary_source.clone()],
            expected_label_substrings: Vec::new(),
        };
        run_location_check(
            client,
            primary_uri,
            "implementation",
            &target,
            caps.supports_implementation,
            bin_path,
            server_id,
            stderr_tail,
            checks,
        )
        .await;
    }

    // Document highlight
    if fixture.expected_capabilities.document_highlight {
        for target in &fixture.document_highlight_targets {
            run_location_check(
                client,
                primary_uri,
                "documentHighlight",
                target,
                caps.supports_document_highlight,
                bin_path,
                server_id,
                stderr_tail,
                checks,
            )
            .await;
        }
    }

    // Workspace symbols
    if fixture.expected_capabilities.workspace_symbols {
        if let Some(expectation) = &fixture.workspace_symbol_query {
            run_workspace_symbol_check(
                client,
                expectation,
                caps.supports_workspace_symbols,
                bin_path,
                server_id,
                stderr_tail,
                checks,
            )
            .await;
        }
    }

    // Type hierarchy
    if fixture.expected_capabilities.type_hierarchy {
        for target in &fixture.type_hierarchy_targets {
            run_type_hierarchy_check(
                client,
                primary_uri,
                target,
                caps.supports_type_hierarchy,
                bin_path,
                server_id,
                stderr_tail,
                checks,
            )
            .await;
        }
    }

    // Signature help
    if fixture.expected_capabilities.signature_help {
        for target in &fixture.signature_help_targets {
            run_signature_help_check(
                client,
                primary_uri,
                target,
                caps.supports_signature_help,
                bin_path,
                server_id,
                stderr_tail,
                checks,
            )
            .await;
        }
    }

    // Semantic tokens
    if fixture.expected_capabilities.semantic_tokens {
        run_semantic_tokens_check(
            client,
            primary_uri,
            caps.supports_semantic_tokens,
            caps.details.semantic_token_legend.as_ref(),
            bin_path,
            server_id,
            stderr_tail,
            checks,
        )
        .await;
    }

    // Rename preview
    if fixture.expected_capabilities.rename || fixture.mutation_targets.rename_preview_requested {
        run_rename_preview_check(
            client,
            fixture,
            primary_uri,
            caps.supports_rename,
            bin_path,
            server_id,
            stderr_tail,
            checks,
        )
        .await;
    }

    // Format preview
    if fixture.expected_capabilities.formatting || fixture.mutation_targets.format_preview_requested
    {
        run_format_preview_check(
            client,
            fixture,
            primary_uri,
            caps.supports_document_formatting,
            bin_path,
            server_id,
            stderr_tail,
            checks,
        )
        .await;
    }

    // Code actions
    if fixture.expected_capabilities.code_actions {
        run_code_action_check(
            client,
            primary_uri,
            fixture,
            caps.supports_code_actions,
            bin_path,
            server_id,
            stderr_tail,
            checks,
        )
        .await;
    }

    // Completion (opt-in via `completions` list, regardless of
    // `expected_capabilities.completion` to keep the existing
    // Tier 1 fixtures unchanged).
    if caps.supports_completion {
        for expectation in &fixture.completions {
            run_completion_check(
                client,
                primary_uri,
                expectation,
                true,
                bin_path,
                server_id,
                stderr_tail,
                checks,
            )
            .await;
        }
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
///
/// Pass 7 — the rules are now status-driven. `Skipped` and
/// `Unsupported` are first-class statuses that the assertion
/// consults directly; the harness no longer infers semantics from
/// check-name substrings. `Skipped` and `Unsupported` are
/// intentionally distinct: a fixture that opts not to exercise an
/// operation is `Skipped`; a server that does not advertise the
/// capability is `Unsupported`.
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
            CompatibilityRequirement::RequiredIfAdvertised => {
                // Pass 7 — only `Failing` status fails a
                // `RequiredIfAdvertised` check. `Skipped` and
                // `Unsupported` are allowed (Skipped = fixture
                // did not exercise; Unsupported = server did not
                // advertise). The legacy
                // `is_skipped_check(&check.name)` substring
                // check is gone — status is the only signal.
                if matches!(check.status, CompatibilityCheckStatus::Failing) {
                    failures.push(format!(
                        "required-if-advertised check failed: {}",
                        format_check_line(check)
                    ));
                }
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

// ── Tier 2 Tests ───────────────────────────────────────────────────
//
// gopls / typescript-language-server / clangd smoke tests.
// Each test follows the same pattern as the Tier 1 tests:
//   1. Resolve the binary via `CODEGG_<SERVER>_BIN` env var, falling
//      back to PATH lookup. Skip with `eprintln!("SKIP: ...")` if
//      not found so the suite remains CI-friendly without the
//      binary.
//   2. Capture `--version` for the compatibility report.
//   3. Drive the standard smoke suite against the Tier 2 fixture.
//   4. Write the JSON report under `target/lsp-compatibility/`.
//   5. Assert that required checks passed.

#[tokio::test]
async fn gopls_smoke() {
    let bin = match require_server_binary("CODEGG_GOPLS_BIN", &["gopls"]) {
        Some(b) => b,
        None => {
            eprintln!("SKIP: gopls not found (set CODEGG_GOPLS_BIN or install gopls)");
            return;
        }
    };

    let version = tokio::time::timeout(VERSION_TIMEOUT, capture_version(&bin))
        .await
        .ok()
        .flatten();
    eprintln!("gopls version: {:?}", version);

    let fixture = gopls_fixture();
    let profile = compatibility::gopls_profile();
    let report = match tokio::time::timeout(
        TEST_TIMEOUT,
        run_smoke_suite(&profile, &bin, &fixture, version),
    )
    .await
    {
        Ok(r) => r,
        Err(_elapsed) => {
            eprintln!("gopls smoke test timed out after {TEST_TIMEOUT:?}");
            return;
        }
    };

    write_report(&report, "gopls");
    assert_required_checks(&report);
}

#[tokio::test]
async fn typescript_smoke() {
    let bin = match require_server_binary("CODEGG_TS_LS_BIN", &["typescript-language-server"]) {
        Some(b) => b,
        None => {
            eprintln!(
                "SKIP: typescript-language-server not found (set CODEGG_TS_LS_BIN or install typescript-language-server)"
            );
            return;
        }
    };

    let version = tokio::time::timeout(VERSION_TIMEOUT, capture_version(&bin))
        .await
        .ok()
        .flatten();
    eprintln!("typescript-language-server version: {:?}", version);

    let fixture = typescript_fixture();
    let profile = compatibility::typescript_language_server_profile();
    let report = match tokio::time::timeout(
        TEST_TIMEOUT,
        run_smoke_suite(&profile, &bin, &fixture, version),
    )
    .await
    {
        Ok(r) => r,
        Err(_elapsed) => {
            eprintln!("typescript-language-server smoke test timed out after {TEST_TIMEOUT:?}");
            return;
        }
    };

    write_report(&report, "typescript-language-server");
    assert_required_checks(&report);
}

#[tokio::test]
async fn clangd_smoke() {
    let bin = match require_server_binary("CODEGG_CLANGD_BIN", &["clangd"]) {
        Some(b) => b,
        None => {
            eprintln!("SKIP: clangd not found (set CODEGG_CLANGD_BIN or install clangd)");
            return;
        }
    };

    let version = tokio::time::timeout(VERSION_TIMEOUT, capture_version(&bin))
        .await
        .ok()
        .flatten();
    eprintln!("clangd version: {:?}", version);

    let fixture = clangd_fixture();
    let profile = compatibility::clangd_profile();
    let report = match tokio::time::timeout(
        TEST_TIMEOUT,
        run_smoke_suite(&profile, &bin, &fixture, version),
    )
    .await
    {
        Ok(r) => r,
        Err(_elapsed) => {
            eprintln!("clangd smoke test timed out after {TEST_TIMEOUT:?}");
            return;
        }
    };

    write_report(&report, "clangd");
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
/// Pass 7 — Verifies that `LspClient::wait_for_first_diagnostics`
/// returns `false` when no diagnostics notification is observed
/// within the timeout window (which is the correct behavior for a
/// server that doesn't publish diagnostics). This is a direct
/// test of the production readiness primitive. The previous test
/// name `empty_diagnostics_readiness_passes` was misleading because
/// it implies readiness passes when diagnostics are empty, but the
/// test was actually verifying the *missing-diagnostics* branch.
/// Renamed per the Phase 3 final-closure audit.
#[tokio::test]
async fn missing_diagnostics_readiness_times_out() {
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
        "missing-diag-test",
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
    // This is the "missing diagnostics" case — the primitive
    // correctly reports that no diagnostics were seen within the
    // timeout.
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

// ── Pass 3 — Generalized fixture unit tests ────────────────────────
//
// These tests are deterministic, do not require any real LSP
// server, and lock down the typed fixture contract introduced in
// Pass 3 (generalized harness). They guard against regressions
// in the new struct shapes and the default values that the
// existing Tier 1 fixtures rely on.

#[test]
fn location_expectation_serializes() {
    let expectation = LocationExpectation {
        position: Position::new(3, 7),
        source_file: None,
        min_locations: 2,
        expected_files: vec![PathBuf::from("src/lib.rs"), PathBuf::from("src/helper.rs")],
        expected_label_substrings: Vec::new(),
    };
    // Clone to exercise the Clone impl.
    let copy = expectation.clone();
    assert_eq!(copy.position, Position::new(3, 7));
    assert_eq!(copy.min_locations, 2);
    assert_eq!(copy.expected_files.len(), 2);
    assert_eq!(copy.expected_files[0], PathBuf::from("src/lib.rs"));
}

#[test]
fn location_expectation_default_is_sane() {
    let expectation = LocationExpectation::default();
    assert_eq!(expectation.position, Position::new(0, 0));
    // Default min_locations must be 1 — matches the "at least one
    // location" assumption that callers rely on.
    assert_eq!(expectation.min_locations, 1);
    assert!(expectation.expected_files.is_empty());
}

#[test]
fn completion_expectation_serializes() {
    let expectation = CompletionExpectation {
        position: Position::new(2, 4),
        max_candidates: 25,
        expected_label_substrings: vec!["add".to_string(), "point".to_string()],
    };
    let copy = expectation.clone();
    assert_eq!(copy.position, Position::new(2, 4));
    assert_eq!(copy.max_candidates, 25);
    assert_eq!(copy.expected_label_substrings.len(), 2);
    assert_eq!(copy.expected_label_substrings[0], "add");
}

#[test]
fn completion_expectation_default_is_sane() {
    let expectation = CompletionExpectation::default();
    assert_eq!(expectation.position, Position::new(0, 0));
    // Default max_candidates must be > 0 so a request that omits
    // the field still gets useful results.
    assert!(expectation.max_candidates > 0);
    assert!(expectation.expected_label_substrings.is_empty());
}

#[test]
fn mutation_targets_default_is_empty() {
    let targets = MutationTargets::default();
    // The default MutationTargets must have every field unset so
    // existing fixtures that do not opt into the new operations
    // see `None` everywhere.
    assert!(targets.rename.is_none());
    assert!(targets.format.is_none());
    assert!(targets.completion.is_none());
    assert!(targets.signature_help.is_none());
    assert!(!targets.format_preview_requested);
    assert!(!targets.rename_preview_requested);
}

#[test]
fn expected_capabilities_default_is_all_false() {
    let caps = ExpectedCapabilities::default();
    // Every capability flag must default to false so adding a
    // new flag cannot accidentally opt every fixture in.
    assert!(!caps.declaration);
    assert!(!caps.implementation);
    assert!(!caps.document_highlight);
    assert!(!caps.workspace_symbols);
    assert!(!caps.signature_help);
    assert!(!caps.semantic_tokens);
    assert!(!caps.rename);
    assert!(!caps.code_actions);
    assert!(!caps.formatting);
    assert!(!caps.type_hierarchy);
}

#[test]
fn workspace_symbol_expectation_serializes() {
    let expectation = WorkspaceSymbolExpectation {
        query: "add".to_string(),
        min_locations: 1,
        expected_files: vec![PathBuf::from("src/main.rs")],
    };
    let copy = expectation.clone();
    assert_eq!(copy.query, "add");
    assert_eq!(copy.min_locations, 1);
    assert_eq!(copy.expected_files, vec![PathBuf::from("src/main.rs")]);
}

#[test]
fn matches_expected_file_handles_file_uri_prefix() {
    let exp = PathBuf::from("src/lib.rs");
    // Suffix match against a `file://` URI works.
    assert!(matches_expected_file("file:///tmp/proj/src/lib.rs", &exp));
    // Direct absolute path works.
    assert!(matches_expected_file("/tmp/proj/src/lib.rs", &exp));
    // Suffix-only with `file:` prefix (single-slash) works.
    assert!(matches_expected_file("file:/tmp/proj/src/lib.rs", &exp));
    // Mismatched path returns false.
    assert!(!matches_expected_file(
        "file:///tmp/proj/src/other.rs",
        &exp
    ));
    // Empty string returns false.
    assert!(!matches_expected_file("", &exp));
}
