use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use tempfile::TempDir;
use tokio::process::Child;

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

    /// Read all available stderr output from the fake server child process.
    ///
    /// This is useful for diagnostics when a test fails. The child's stderr
    /// must still be available (not taken by `child.stderr()`). Returns the
    /// stderr content as a string, or a placeholder if unavailable.
    #[allow(dead_code)]
    pub async fn read_stderr(child: &mut Child) -> String {
        use tokio::io::AsyncReadExt;
        if let Some(mut stderr) = child.stderr.take() {
            let mut buf = Vec::new();
            let _ = tokio::time::timeout(
                std::time::Duration::from_millis(500),
                stderr.read_to_end(&mut buf),
            )
            .await;
            String::from_utf8_lossy(&buf).into_owned()
        } else {
            "(stderr not captured)".to_string()
        }
    }

    /// Dump server diagnostics (stderr + transcript) for debugging test failures.
    ///
    /// Call this in test failure paths or after assertion failures to get
    /// actionable output. Reads stderr from the child process and the
    /// transcript file written by the fake server.
    #[allow(dead_code)]
    pub async fn dump_diagnostics(child: &mut Child, harness: &FakeLspHarness) {
        eprintln!("\n=== Fake LSP Server Diagnostics ===");

        // Dump stderr
        let stderr = Self::read_stderr(child).await;
        if !stderr.is_empty() {
            eprintln!("--- stderr ---");
            for line in stderr.lines() {
                eprintln!("  {line}");
            }
        }

        // Dump transcript
        if harness.transcript_path.exists() {
            eprintln!("--- transcript ({}) ---", harness.transcript_path.display());
            match std::fs::read_to_string(&harness.transcript_path) {
                Ok(content) => {
                    for line in content.lines() {
                        eprintln!("  {line}");
                    }
                }
                Err(e) => eprintln!("  (could not read transcript: {e})"),
            }
        } else {
            eprintln!("--- transcript: not yet written ---");
        }

        eprintln!("=== End Diagnostics ===\n");
    }

    /// Path to the fake server binary.
    ///
    /// Searches in order:
    /// 1. `EGGLSP_TEST_SERVER` environment variable
    /// 2. A Cargo-built `egglsp-test-server` binary resolved from `cargo build`
    /// 3. Panics with a helpful message
    pub fn fake_server_path() -> String {
        static SERVER_PATH: OnceLock<String> = OnceLock::new();
        SERVER_PATH
            .get_or_init(|| {
                if let Ok(path) = std::env::var("EGGLSP_TEST_SERVER") {
                    return path;
                }

                let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .and_then(|path| path.parent())
                    .expect("failed to resolve workspace root")
                    .to_path_buf();
                let manifest_path = workspace_root.join("Cargo.toml");
                let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
                let mut command = Command::new(cargo);
                command
                    .arg("build")
                    .arg("--locked")
                    .arg("--manifest-path")
                    .arg(&manifest_path)
                    .arg("-p")
                    .arg("egglsp-test-server")
                    .arg("--bin")
                    .arg("egglsp-test-server")
                    .arg("--message-format=json-render-diagnostics")
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                if std::env::var("PROFILE").ok().as_deref() == Some("release") {
                    command.arg("--release");
                }

                let output = command
                    .output()
                    .unwrap_or_else(|e| panic!("failed to run cargo build for fake server: {e}"));
                if !output.status.success() {
                    panic!(
                        "cargo build for egglsp-test-server failed:\n{}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }

                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else {
                        continue;
                    };
                    if message.get("reason").and_then(|value| value.as_str())
                        == Some("compiler-artifact")
                        && message
                            .get("target")
                            .and_then(|value| value.get("name"))
                            .and_then(|value| value.as_str())
                            == Some("egglsp-test-server")
                    {
                        if let Some(path) =
                            message.get("executable").and_then(|value| value.as_str())
                        {
                            return path.to_string();
                        }
                    }
                }

                panic!(
                    "Could not find egglsp-test-server binary from Cargo output.\n\
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
