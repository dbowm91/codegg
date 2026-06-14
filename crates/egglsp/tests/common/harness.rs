use std::path::PathBuf;
use std::sync::OnceLock;

use tempfile::TempDir;

/// Test harness for fake LSP server integration tests.
///
/// Manages a temp directory, scenario file, transcript path, and provides
/// access to the fake server binary path.
pub struct FakeLspHarness {
    #[allow(dead_code)]
    pub tempdir: TempDir,
    pub scenario_path: PathBuf,
    pub transcript_path: PathBuf,
    pub root: PathBuf,
}

impl FakeLspHarness {
    /// Create a new harness with the given scenario JSON.
    ///
    /// Sets up a temp directory with a `scenario.json`, a `transcript.jsonl`
    /// path (empty until the server writes to it), and a minimal `src/lib.rs`.
    pub fn new(scenario: &serde_json::Value) -> Self {
        let tempdir = tempfile::tempdir().expect("failed to create tempdir");
        let root = tempdir.path().to_path_buf();

        // Write scenario file
        let scenario_path = root.join("scenario.json");
        std::fs::write(
            &scenario_path,
            serde_json::to_string_pretty(scenario).unwrap(),
        )
        .expect("failed to write scenario");

        // Transcript path (server writes here)
        let transcript_path = root.join("transcript.jsonl");

        // Create minimal project structure
        std::fs::create_dir_all(root.join("src")).expect("failed to create src dir");
        std::fs::write(root.join("src/lib.rs"), "// test file\n")
            .expect("failed to write src/lib.rs");

        Self {
            tempdir,
            scenario_path,
            transcript_path,
            root,
        }
    }

    /// Path to the scenario file, as a string.
    pub fn scenario_path_str(&self) -> &str {
        self.scenario_path.to_str().unwrap()
    }

    /// Path to the transcript file, as a string.
    pub fn transcript_path_str(&self) -> &str {
        self.transcript_path.to_str().unwrap()
    }

    /// Path to the fake server binary.
    ///
    /// Searches in order:
    /// 1. `EGGLSP_TEST_SERVER` environment variable
    /// 2. `<target_dir>/egglsp-test-server` (relative to CARGO_MANIFEST_DIR)
    /// 3. Panics with a helpful message
    pub fn fake_server_path() -> String {
        static SERVER_PATH: OnceLock<String> = OnceLock::new();
        SERVER_PATH
            .get_or_init(|| {
                // 1. Check environment variable
                if let Ok(path) = std::env::var("EGGLSP_TEST_SERVER") {
                    return path;
                }

                // 2. Look relative to CARGO_MANIFEST_DIR (workspace layout)
                // CARGO_MANIFEST_DIR = crates/egglsp, so we need to go up twice
                if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
                    let manifest = PathBuf::from(&manifest_dir);
                    if let Some(crates_dir) = manifest.parent() {
                        if let Some(workspace_root) = crates_dir.parent() {
                            let target_dirs = [
                                workspace_root.join("target").join("debug"),
                                workspace_root.join("target").join("release"),
                            ];
                            for dir in &target_dirs {
                                let binary = dir.join("egglsp-test-server");
                                if binary.exists() {
                                    return binary.to_string_lossy().into_owned();
                                }
                            }
                        }
                    }
                }

                panic!(
                    "Could not find egglsp-test-server binary.\n\
                     Build it with: cargo build -p egglsp-test-server\n\
                     Or set EGGLSP_TEST_SERVER=/path/to/binary"
                )
            })
            .clone()
    }

    /// Build an `LspServerDef` that points at the fake test server.
    ///
    /// The binary path is resolved at runtime. The `command` field is set
    /// to that path so `LspClient::new` can use it.
    #[allow(dead_code)]
    pub fn fake_server_def() -> egglsp::server::LspServerDef {
        static SERVER_PATH: OnceLock<String> = OnceLock::new();
        let path = SERVER_PATH.get_or_init(Self::fake_server_path);
        // Leak the path string so the LspServerDef can hold &'static str.
        let static_path: &'static str = Box::leak(path.clone().into_boxed_str());
        egglsp::server::LspServerDef {
            id: "fake-lsp-test-server",
            languages: &["rust"],
            extensions: &["rs"],
            repo: "test/fake-server",
            command: static_path,
            args: &[],
            download: None,
        }
    }
}
