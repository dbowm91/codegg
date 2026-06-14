use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tempfile::TempDir;
use tokio::time::timeout;

use egglsp::{LspClient, LspClientOptions, LspError, LspLaunchSpec};

/// Production LSP client harness for integration tests.
pub struct ProductionClientHarness {
    pub tempdir: TempDir,
    pub root: PathBuf,
    pub source_path: PathBuf,
    pub scenario_path: PathBuf,
    pub transcript_path: PathBuf,
    pub client: Arc<LspClient>,
    scenario_name: String,
}

impl ProductionClientHarness {
    pub async fn start(
        scenario: serde_json::Value,
        options: LspClientOptions,
        configuration: serde_json::Value,
    ) -> Result<Self, LspError> {
        let tempdir = tempfile::tempdir().map_err(LspError::Io)?;
        let root = tempdir.path().to_path_buf();
        let source_path = root.join("src/lib.rs");
        let scenario_path = root.join("scenario.json");
        let transcript_path = root.join("transcript.jsonl");
        let root_uri = path_to_uri(&root);
        let source_uri = path_to_uri(&source_path);
        let scenario_name = scenario
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("production-scenario")
            .to_string();

        std::fs::create_dir_all(root.join("src")).map_err(LspError::Io)?;
        std::fs::write(
            root.join("Cargo.toml"),
            r#"[package]
name = "egglsp-production-test"
version = "0.1.0"
edition = "2021"
"#,
        )
        .map_err(LspError::Io)?;
        std::fs::write(&source_path, "pub fn harness_marker() {}\n").map_err(LspError::Io)?;
        let scenario = substitute_placeholders(
            scenario,
            &root,
            &root_uri,
            &source_path,
            &source_uri,
            &scenario_path,
            &transcript_path,
        );
        let configuration = substitute_placeholders(
            configuration,
            &root,
            &root_uri,
            &source_path,
            &source_uri,
            &scenario_path,
            &transcript_path,
        );
        std::fs::write(
            &scenario_path,
            serde_json::to_string_pretty(&scenario).map_err(LspError::Json)?,
        )
        .map_err(LspError::Io)?;

        let launch = LspLaunchSpec::new(
            "egglsp-test-server",
            fake_server_binary_path(),
            Vec::new(),
            vec![
                (
                    "CODEGG_FAKE_LSP_SCENARIO".to_string(),
                    scenario_path.to_string_lossy().to_string(),
                ),
                (
                    "CODEGG_FAKE_LSP_TRANSCRIPT".to_string(),
                    transcript_path.to_string_lossy().to_string(),
                ),
            ],
            vec!["rust".to_string()],
            vec!["rs".to_string()],
        );

        let client =
            Arc::new(LspClient::new_with_launch_spec(launch, &root, configuration, options).await?);

        let harness = Self {
            tempdir,
            root,
            source_path,
            scenario_path,
            transcript_path,
            client,
            scenario_name,
        };

        if let Err(err) = harness.client.initialize(None).await {
            let diagnostics = harness.diagnostics().await;
            return Err(LspError::RequestFailed(format!(
                "failed to initialize production harness: {err}\n{diagnostics}"
            )));
        }

        if let Err(err) = harness.client.send_initialized().await {
            let diagnostics = harness.diagnostics().await;
            return Err(LspError::RequestFailed(format!(
                "failed to send initialized notification: {err}\n{diagnostics}"
            )));
        }

        Ok(harness)
    }

    pub async fn shutdown(self) -> Result<(), LspError> {
        let shutdown_result = self.client.shutdown().await;

        let wait_result = {
            let mut process = self.client.process.lock().await;
            timeout(Duration::from_secs(5), process.child.wait()).await
        };

        let diagnostics = self.diagnostics().await;

        match (shutdown_result, wait_result) {
            (Ok(()), Ok(Ok(_status))) => Ok(()),
            (Ok(()), Ok(Err(err))) => Err(LspError::RequestFailed(format!(
                "failed to wait for fake server exit: {err}\n{diagnostics}"
            ))),
            (Ok(()), Err(_elapsed)) => Err(LspError::RequestFailed(format!(
                "timed out waiting for fake server exit\n{diagnostics}"
            ))),
            (Err(err), _) => Err(LspError::RequestFailed(format!(
                "client shutdown failed: {err}\n{diagnostics}"
            ))),
        }
    }

    pub async fn diagnostics(&self) -> String {
        let pending = self.client.pending_request_count().await;
        let transport = self.client.transport_state_snapshot().await;
        let child_status = {
            let mut process = self.client.process.lock().await;
            match process.child.try_wait() {
                Ok(Some(status)) => format!("{status:?}"),
                Ok(None) => "running".to_string(),
                Err(err) => format!("error: {err}"),
            }
        };

        let transcript_tail = transcript_tail(&self.transcript_path);

        let mut out = String::new();
        push_line(&mut out, &format!("scenario: {}", self.scenario_name));
        push_line(&mut out, &format!("root: {}", self.root.display()));
        push_line(&mut out, &format!("source: {}", self.source_path.display()));
        push_line(
            &mut out,
            &format!("scenario file: {}", self.scenario_path.display()),
        );
        push_line(
            &mut out,
            &format!("transcript file: {}", self.transcript_path.display()),
        );
        push_line(&mut out, &format!("pending requests: {pending}"));
        push_line(&mut out, &format!("transport: {transport:?}"));
        push_line(&mut out, &format!("child exit: {child_status}"));
        push_line(&mut out, "--- transcript tail ---");
        if transcript_tail.is_empty() {
            push_line(&mut out, "(transcript empty)");
        } else {
            out.push_str(&transcript_tail);
            if !out.ends_with('\n') {
                out.push('\n');
            }
        }
        out
    }
}

fn fake_server_binary_path() -> PathBuf {
    if let Ok(path) = std::env::var("EGGLSP_TEST_SERVER") {
        return PathBuf::from(path);
    }

    option_env!("CARGO_BIN_EXE_egglsp-test-server")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            panic!(
                "Could not find egglsp-test-server binary.\n\
                 Build the egglsp package with Cargo, or set \
                 EGGLSP_TEST_SERVER=/path/to/binary"
            )
        })
}

fn transcript_tail(path: &Path) -> String {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return "(transcript unavailable)".to_string();
    };

    const MAX_LINES: usize = 40;
    let mut lines: Vec<&str> = contents.lines().rev().take(MAX_LINES).collect();
    lines.reverse();
    lines.join("\n")
}

fn push_line(out: &mut String, line: &str) {
    out.push_str(line);
    out.push('\n');
}

fn path_to_uri(path: &Path) -> String {
    url::Url::from_file_path(path)
        .expect("invalid file path")
        .to_string()
}

fn substitute_placeholders(
    value: serde_json::Value,
    root: &Path,
    root_uri: &str,
    source_path: &Path,
    source_uri: &str,
    scenario_path: &Path,
    transcript_path: &Path,
) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => match s.as_str() {
            "__ROOT_URI__" => serde_json::Value::String(root_uri.to_string()),
            "__ROOT_PATH__" => serde_json::Value::String(root.display().to_string()),
            "__ROOT_NAME__" => serde_json::Value::String(
                root.file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default(),
            ),
            "__SOURCE_URI__" => serde_json::Value::String(source_uri.to_string()),
            "__SOURCE_PATH__" => serde_json::Value::String(source_path.display().to_string()),
            "__SCENARIO_PATH__" => serde_json::Value::String(scenario_path.display().to_string()),
            "__TRANSCRIPT_PATH__" => {
                serde_json::Value::String(transcript_path.display().to_string())
            }
            _ => serde_json::Value::String(s),
        },
        serde_json::Value::Array(values) => serde_json::Value::Array(
            values
                .into_iter()
                .map(|value| {
                    substitute_placeholders(
                        value,
                        root,
                        root_uri,
                        source_path,
                        source_uri,
                        scenario_path,
                        transcript_path,
                    )
                })
                .collect(),
        ),
        serde_json::Value::Object(values) => serde_json::Value::Object(
            values
                .into_iter()
                .map(|(key, value)| {
                    (
                        key,
                        substitute_placeholders(
                            value,
                            root,
                            root_uri,
                            source_path,
                            source_uri,
                            scenario_path,
                            transcript_path,
                        ),
                    )
                })
                .collect(),
        ),
        other => other,
    }
}
