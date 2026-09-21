//! Consent-gated, bounded training-data capture.

use super::{redact_sensitive, ToolAdvisorCandidate};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use url::Url;

pub const EVENT_SCHEMA_VERSION: u16 = 1;
const MAX_EVENT_BYTES: usize = 128 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolAdvisorTrainingEvent {
    pub schema_version: u16,
    pub event_id: String,
    pub created_at_ms: u64,
    pub advisor_model_version: Option<String>,
    pub mode: String,
    pub surface_fingerprint: String,
    pub candidates: Vec<ToolAdvisorCandidate>,
    #[serde(default)]
    pub scores: Vec<f64>,
    pub selected_tool: Option<String>,
    pub outcome: String,
    pub latency_ms: u64,
    #[serde(default)]
    pub context: Option<String>,
    pub metadata_consent: bool,
    pub content_consent: bool,
}

impl ToolAdvisorTrainingEvent {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != EVENT_SCHEMA_VERSION || self.event_id.trim().is_empty() {
            return Err(anyhow!(
                "unsupported or missing training event identity/version"
            ));
        }
        if self.candidates.len() > super::MAX_CANDIDATES
            || self.scores.len() > super::MAX_CANDIDATES
        {
            return Err(anyhow!("training event candidate/score limit exceeded"));
        }
        let encoded = serde_json::to_vec(self)?;
        if encoded.len() > MAX_EVENT_BYTES {
            return Err(anyhow!("training event exceeds {} bytes", MAX_EVENT_BYTES));
        }
        if self.context.is_some() && !self.content_consent {
            return Err(anyhow!(
                "training content requires explicit content consent"
            ));
        }
        Ok(())
    }

    fn sanitized_for(&self, include_content: bool) -> Result<Self> {
        let mut event = self.clone();
        if !include_content || !event.content_consent {
            event.context = None;
        } else if let Some(context) = &event.context {
            event.context = Some(redact_sensitive(context));
        }
        for candidate in &mut event.candidates {
            candidate.name = redact_sensitive(&candidate.name);
            candidate.description = redact_sensitive(&candidate.description);
        }
        event.selected_tool = event.selected_tool.as_deref().map(redact_sensitive);
        event.outcome = redact_sensitive(&event.outcome);
        event.validate()?;
        Ok(event)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrainingDataPolicy {
    pub capture: String,
    pub include_content: bool,
    pub max_bytes: u64,
    pub max_age_days: u32,
    pub remote_enabled: bool,
    pub remote_endpoint: Option<String>,
    pub remote_include_content: bool,
}

impl Default for TrainingDataPolicy {
    fn default() -> Self {
        Self {
            capture: "off".into(),
            include_content: false,
            max_bytes: 16 * 1024 * 1024,
            max_age_days: 30,
            remote_enabled: false,
            remote_endpoint: None,
            remote_include_content: false,
        }
    }
}

impl TrainingDataPolicy {
    pub fn from_config(config: Option<&codegg_config::schema::ToolAdvisorConfig>) -> Result<Self> {
        let Some(config) = config else {
            return Ok(Self::default());
        };
        let training = config.training_data.as_ref();
        let remote = config.remote.as_ref();
        let policy = Self {
            capture: training
                .and_then(|value| value.capture.clone())
                .unwrap_or_else(|| "off".into()),
            include_content: training
                .and_then(|value| value.include_content)
                .unwrap_or(false),
            max_bytes: training
                .and_then(|value| value.max_bytes)
                .unwrap_or(16 * 1024 * 1024)
                .clamp(1024, 512 * 1024 * 1024),
            max_age_days: training.and_then(|value| value.max_age_days).unwrap_or(30),
            remote_enabled: remote.and_then(|value| value.enabled).unwrap_or(false),
            remote_endpoint: remote.and_then(|value| value.endpoint.clone()),
            remote_include_content: remote
                .and_then(|value| value.include_content)
                .unwrap_or(false),
        };
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> Result<()> {
        if !matches!(self.capture.as_str(), "off" | "local") {
            return Err(anyhow!("training capture must be off or local"));
        }
        if self.remote_enabled {
            let endpoint = self
                .remote_endpoint
                .as_deref()
                .ok_or_else(|| anyhow!("remote telemetry requires an explicit endpoint"))?;
            let url = Url::parse(endpoint).context("parse remote telemetry endpoint")?;
            if url.scheme() != "https" || url.host_str().is_none() {
                return Err(anyhow!(
                    "remote telemetry endpoint must be HTTPS with a host"
                ));
            }
        }
        if self.remote_include_content && !(self.remote_enabled && self.include_content) {
            return Err(anyhow!(
                "remote content requires both remote and content consent"
            ));
        }
        Ok(())
    }

    pub fn remote_can_send(&self) -> bool {
        self.remote_enabled && self.remote_endpoint.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrainingDataStatus {
    Disabled,
    LocalCapture,
    RemoteReady,
}

pub trait TrainingSink: Send + Sync {
    fn submit(&self, event: &ToolAdvisorTrainingEvent) -> Result<()>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoopSink;

impl TrainingSink for NoopSink {
    fn submit(&self, _event: &ToolAdvisorTrainingEvent) -> Result<()> {
        Ok(())
    }
}

pub struct LocalSpoolSink {
    root: PathBuf,
    max_bytes: u64,
    max_age: Duration,
    include_content: bool,
}

impl LocalSpoolSink {
    pub fn new(root: impl Into<PathBuf>, policy: &TrainingDataPolicy) -> Result<Self> {
        policy.validate()?;
        let root = root.into();
        fs::create_dir_all(&root)
            .with_context(|| format!("create training spool {}", root.display()))?;
        Ok(Self {
            root,
            max_bytes: policy.max_bytes,
            max_age: Duration::from_secs(policy.max_age_days as u64 * 86_400),
            include_content: policy.include_content,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn inspect(&self) -> Result<Vec<ToolAdvisorTrainingEvent>> {
        let mut events = Vec::new();
        for entry in fs::read_dir(&self.root)
            .with_context(|| format!("read training spool {}", self.root.display()))?
        {
            let entry = entry?;
            if entry
                .path()
                .extension()
                .and_then(|extension| extension.to_str())
                != Some("json")
            {
                continue;
            }
            match fs::read(entry.path())
                .ok()
                .and_then(|bytes| serde_json::from_slice::<ToolAdvisorTrainingEvent>(&bytes).ok())
            {
                Some(event) if event.validate().is_ok() => events.push(event),
                _ => {
                    let _ = fs::rename(entry.path(), entry.path().with_extension("corrupt"));
                }
            }
        }
        events.sort_by(|left, right| {
            left.created_at_ms
                .cmp(&right.created_at_ms)
                .then_with(|| left.event_id.cmp(&right.event_id))
        });
        Ok(events)
    }

    pub fn export(&self, destination: &Path) -> Result<usize> {
        let events = self.inspect()?;
        let temp = destination.with_extension("jsonl.tmp");
        let mut file = File::create(&temp)
            .with_context(|| format!("create training export {}", temp.display()))?;
        for event in &events {
            writeln!(file, "{}", serde_json::to_string(event)?)?;
        }
        file.sync_all()?;
        fs::rename(&temp, destination)
            .with_context(|| format!("install training export {}", destination.display()))?;
        Ok(events.len())
    }

    pub fn purge(&self) -> Result<usize> {
        let mut removed = 0;
        for entry in fs::read_dir(&self.root)? {
            let path = entry?.path();
            if matches!(
                path.extension().and_then(|extension| extension.to_str()),
                Some("json") | Some("corrupt")
            ) {
                fs::remove_file(path)?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    fn prune(&self) -> Result<()> {
        let now = SystemTime::now();
        let mut files: Vec<_> = fs::read_dir(&self.root)?
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .and_then(|extension| extension.to_str())
                    == Some("json")
            })
            .collect();
        files.sort_by_key(|entry| {
            entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH)
        });
        let mut total: u64 = files
            .iter()
            .filter_map(|entry| entry.metadata().ok().map(|metadata| metadata.len()))
            .sum();
        for entry in files {
            let metadata = entry.metadata()?;
            let expired = self.max_age.as_secs() > 0
                && metadata
                    .modified()
                    .ok()
                    .and_then(|modified| now.duration_since(modified).ok())
                    .is_some_and(|age| age > self.max_age);
            if expired || total > self.max_bytes {
                total = total.saturating_sub(metadata.len());
                fs::remove_file(entry.path())?;
            }
        }
        Ok(())
    }
}

impl TrainingSink for LocalSpoolSink {
    fn submit(&self, event: &ToolAdvisorTrainingEvent) -> Result<()> {
        let event = event.sanitized_for(self.include_content)?;
        let bytes = serde_json::to_vec_pretty(&event)?;
        let path = self.root.join(format!("{}.json", event.event_id));
        let temp = self.root.join(format!("{}.json.tmp", event.event_id));
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?
            .write_all(&bytes)?;
        fs::rename(temp, path)?;
        self.prune()
    }
}

#[async_trait]
pub trait RemoteTransport: Send + Sync {
    async fn send(&self, endpoint: &str, bearer_token: Option<&str>, body: String) -> Result<()>;
}

pub struct HttpRemoteTransport {
    client: eggfetch_core::Client,
}

impl HttpRemoteTransport {
    pub fn new() -> Self {
        Self {
            client: crate::http_client::ordinary_http_client_builder(
                eggfetch_core::Timeout::from_secs(10),
            )
            .build(),
        }
    }
}

impl Default for HttpRemoteTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl RemoteTransport for HttpRemoteTransport {
    async fn send(&self, endpoint: &str, bearer_token: Option<&str>, body: String) -> Result<()> {
        let url = Url::parse(endpoint)?;
        if url.scheme() != "https" {
            return Err(anyhow!("remote telemetry refuses non-HTTPS endpoint"));
        }
        let mut request = self
            .client
            .post(endpoint)?
            .header("Content-Type", "application/json")
            .header("Accept", "application/json");
        if let Some(token) = bearer_token {
            request = request.header("Authorization", &format!("Bearer {token}"));
        }
        let response = request.body(body).send().await?;
        if !response.status().is_success() {
            return Err(anyhow!("remote telemetry returned {}", response.status()));
        }
        Ok(())
    }
}

pub struct RemoteSink<T> {
    transport: T,
    endpoint: String,
    bearer_token: Option<String>,
    include_content: bool,
}

impl<T> RemoteSink<T>
where
    T: RemoteTransport,
{
    pub fn new(
        transport: T,
        policy: &TrainingDataPolicy,
        bearer_token: Option<String>,
    ) -> Result<Self> {
        policy.validate()?;
        let endpoint = policy
            .remote_endpoint
            .clone()
            .ok_or_else(|| anyhow!("remote sink requires explicit endpoint"))?;
        Ok(Self {
            transport,
            endpoint,
            bearer_token,
            include_content: policy.remote_include_content,
        })
    }

    pub async fn submit(&self, event: &ToolAdvisorTrainingEvent) -> Result<()> {
        let event = event.sanitized_for(self.include_content)?;
        self.transport
            .send(
                &self.endpoint,
                self.bearer_token.as_deref(),
                serde_json::to_string(&event)?,
            )
            .await
    }
}

pub fn default_spool_root() -> Option<PathBuf> {
    dirs::data_local_dir().map(|root| root.join("codegg").join("tool-advisor").join("training"))
}

pub fn status(policy: &TrainingDataPolicy) -> TrainingDataStatus {
    if policy.remote_can_send() {
        TrainingDataStatus::RemoteReady
    } else if policy.capture == "local" {
        TrainingDataStatus::LocalCapture
    } else {
        TrainingDataStatus::Disabled
    }
}

pub fn new_event(
    candidates: Vec<ToolAdvisorCandidate>,
    context: Option<String>,
    content_consent: bool,
) -> ToolAdvisorTrainingEvent {
    ToolAdvisorTrainingEvent {
        schema_version: EVENT_SCHEMA_VERSION,
        event_id: uuid::Uuid::new_v4().to_string(),
        created_at_ms: SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
        advisor_model_version: None,
        mode: "observe".into(),
        surface_fingerprint: String::new(),
        candidates,
        scores: Vec::new(),
        selected_tool: None,
        outcome: "unknown".into(),
        latency_ms: 0,
        context,
        metadata_consent: true,
        content_consent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn event() -> ToolAdvisorTrainingEvent {
        new_event(
            vec![ToolAdvisorCandidate {
                name: "read".into(),
                description: "Read files".into(),
                category: "ReadOnly".into(),
                disclosure: "core".into(),
                synthetic_identity: false,
            }],
            Some("token=secret sk-abc123".into()),
            true,
        )
    }

    #[test]
    fn default_policy_is_off_and_rejects_remote_without_explicit_https() {
        let policy = TrainingDataPolicy::default();
        assert_eq!(status(&policy), TrainingDataStatus::Disabled);
        let mut remote = policy.clone();
        remote.remote_enabled = true;
        assert!(remote.validate().is_err());
        remote.remote_endpoint = Some("http://localhost:8080/collect".into());
        assert!(remote.validate().is_err());
    }

    #[test]
    fn local_spool_inspect_export_purge_is_bounded_and_redacted() {
        let directory = tempfile::tempdir().expect("spool directory");
        let policy = TrainingDataPolicy {
            capture: "local".into(),
            include_content: false,
            ..Default::default()
        };
        let spool = LocalSpoolSink::new(directory.path(), &policy).expect("spool");
        spool.submit(&event()).expect("capture");
        let events = spool.inspect().expect("inspect");
        assert_eq!(events.len(), 1);
        assert!(events[0].context.is_none());
        let export = directory.path().join("export.jsonl");
        assert_eq!(spool.export(&export).expect("export"), 1);
        assert_eq!(spool.purge().expect("purge"), 1);
        assert!(spool.inspect().expect("empty").is_empty());
    }

    #[test]
    fn content_consent_and_metadata_consent_are_independent() {
        let mut event = event();
        event.content_consent = false;
        assert!(event.validate().is_err());
        let invalid_policy = TrainingDataPolicy {
            remote_enabled: true,
            remote_endpoint: Some("https://telemetry.example.invalid/events".into()),
            remote_include_content: true,
            ..Default::default()
        };
        assert!(invalid_policy.validate().is_err());
        let valid_policy = TrainingDataPolicy {
            remote_enabled: true,
            remote_endpoint: Some("https://telemetry.example.invalid/events".into()),
            include_content: true,
            remote_include_content: true,
            ..Default::default()
        };
        assert!(valid_policy.validate().is_ok());
    }

    struct FakeTransport {
        calls: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl RemoteTransport for FakeTransport {
        async fn send(
            &self,
            endpoint: &str,
            bearer_token: Option<&str>,
            body: String,
        ) -> Result<()> {
            self.calls
                .lock()
                .expect("calls")
                .push(format!("{endpoint}|{}|{body}", bearer_token.unwrap_or("")));
            Ok(())
        }
    }

    #[tokio::test]
    async fn explicit_remote_sink_uses_fake_transport_only_after_consent() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let policy = TrainingDataPolicy {
            remote_enabled: true,
            remote_endpoint: Some("https://telemetry.example.invalid/events".into()),
            ..Default::default()
        };
        let sink = RemoteSink::new(
            FakeTransport {
                calls: calls.clone(),
            },
            &policy,
            Some("token".into()),
        )
        .expect("remote sink");
        sink.submit(&event()).await.expect("send");
        assert_eq!(calls.lock().expect("calls").len(), 1);
        assert!(calls.lock().expect("calls")[0].contains("telemetry.example.invalid"));
    }
}
