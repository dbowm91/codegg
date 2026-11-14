//! One embedding-host session: its workspace, resolved agent/model and the
//! single loop instance turns are taken out of and returned to.

use crate::agent::r#loop::AgentLoop;
use crate::headless::HeadlessRuntime;
use crate::host::protocol::{ErrorClass, RpcError, SessionOpenResult};
use crate::provider::inline::InlineProviderDescriptor;
use codegg_core::approval::{ApprovalMode, SandboxProfile};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{watch, Mutex};

#[derive(Clone)]
pub(crate) struct Session {
    pub id: String,
    pub model: String,
    pub agent: String,
    pub workspace: String,
    pub model_request: String,
    state: Arc<Mutex<SessionState>>,
}

pub(crate) struct SessionState {
    pub loop_instance: Option<AgentLoop>,
    pub active: Option<ActiveTurn>,
}

pub(crate) struct ActiveTurn {
    pub turn_id: String,
    cancel: watch::Sender<bool>,
    pub cancelled: Arc<AtomicBool>,
}

pub(crate) struct TurnAdmission {
    pub loop_instance: AgentLoop,
    pub cancel_rx: watch::Receiver<bool>,
    pub cancelled: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdmissionError {
    Busy,
    Unavailable,
}

impl Session {
    pub(crate) fn new(runtime: HeadlessRuntime) -> Self {
        Self {
            id: runtime.session_id().to_owned(),
            model: runtime.model_selection().to_owned(),
            agent: runtime.agent_name().to_owned(),
            workspace: runtime.workspace_root().to_string_lossy().into_owned(),
            model_request: runtime.model_request().to_owned(),
            state: Arc::new(Mutex::new(SessionState {
                loop_instance: Some(runtime.into_loop()),
                active: None,
            })),
        }
    }

    pub(crate) fn open_result(&self) -> SessionOpenResult {
        SessionOpenResult {
            session_id: self.id.clone(),
            model: self.model.clone(),
            agent: self.agent.clone(),
            workspace: self.workspace.clone(),
        }
    }

    pub(crate) async fn begin_turn(&self, turn_id: &str) -> Result<TurnAdmission, AdmissionError> {
        let mut state = self.state.lock().await;
        if state.active.is_some() {
            return Err(AdmissionError::Busy);
        }
        let Some(loop_instance) = state.loop_instance.take() else {
            return Err(AdmissionError::Unavailable);
        };
        let (cancel, cancel_rx) = watch::channel(false);
        let cancelled = Arc::new(AtomicBool::new(false));
        state.active = Some(ActiveTurn {
            turn_id: turn_id.to_owned(),
            cancel,
            cancelled: cancelled.clone(),
        });
        Ok(TurnAdmission {
            loop_instance,
            cancel_rx,
            cancelled,
        })
    }

    pub(crate) async fn complete_turn(&self, loop_instance: AgentLoop, turn_id: &str) {
        let mut state = self.state.lock().await;
        if state
            .active
            .as_ref()
            .is_some_and(|active| active.turn_id == turn_id)
        {
            state.active = None;
        }
        if state.loop_instance.is_none() {
            state.loop_instance = Some(loop_instance);
        }
    }

    pub(crate) async fn cancel(&self, turn_id: &str) -> bool {
        let state = self.state.lock().await;
        match &state.active {
            Some(active) if active.turn_id == turn_id => {
                active.cancelled.store(true, Ordering::SeqCst);
                let _ = active.cancel.send(true);
                true
            }
            _ => false,
        }
    }

    pub(crate) async fn cancel_active(&self) {
        let state = self.state.lock().await;
        if let Some(active) = &state.active {
            active.cancelled.store(true, Ordering::SeqCst);
            let _ = active.cancel.send(true);
        }
    }
}

pub(crate) async fn canonical_workspace(raw: &str) -> Result<PathBuf, RpcError> {
    if !Path::new(raw).is_absolute() {
        return Err(RpcError::new(
            ErrorClass::Workspace,
            "workspace must be an absolute path",
        ));
    }
    let path = tokio::fs::canonicalize(raw).await.map_err(|_| {
        RpcError::new(
            ErrorClass::Workspace,
            "workspace root does not exist or is not accessible",
        )
    })?;
    let metadata = tokio::fs::metadata(&path).await.map_err(|_| {
        RpcError::new(
            ErrorClass::Workspace,
            "workspace root does not exist or is not accessible",
        )
    })?;
    if !metadata.is_dir() {
        return Err(RpcError::new(
            ErrorClass::Workspace,
            "workspace root is not a directory",
        ));
    }
    Ok(path)
}

pub(crate) fn parse_approval(raw: &str) -> Result<ApprovalMode, RpcError> {
    if raw.eq_ignore_ascii_case("ask") {
        return Ok(ApprovalMode::Interactive);
    }
    ApprovalMode::parse(raw).ok_or_else(|| {
        RpcError::new(
            ErrorClass::InvalidRequest,
            "approval is not a supported approval mode",
        )
    })
}

pub(crate) fn parse_sandbox(raw: &str) -> Result<SandboxProfile, RpcError> {
    SandboxProfile::parse(raw).ok_or_else(|| {
        RpcError::new(
            ErrorClass::InvalidRequest,
            "sandbox is not a supported sandbox profile",
        )
    })
}

pub(crate) fn parse_descriptor(
    value: serde_json::Value,
) -> Result<InlineProviderDescriptor, RpcError> {
    let descriptor: InlineProviderDescriptor = serde_json::from_value(value).map_err(|_| {
        RpcError::new(
            ErrorClass::InvalidRequest,
            "provider descriptor is malformed",
        )
    })?;
    descriptor
        .validate()
        .map_err(|error| RpcError::new(ErrorClass::InvalidRequest, error.to_string()))?;
    Ok(descriptor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn workspace_must_be_absolute_exist_and_be_a_directory() {
        let relative = canonical_workspace("relative/path")
            .await
            .expect_err("relative");
        assert_eq!(relative.code, -32000);
        assert_eq!(relative.data.expect("class").class, ErrorClass::Workspace);

        let missing_dir =
            std::env::temp_dir().join(format!("codegg-host-missing-{}", uuid::Uuid::new_v4()));
        let missing = canonical_workspace(&missing_dir.to_string_lossy())
            .await
            .expect_err("missing");
        assert_eq!(missing.data.expect("class").class, ErrorClass::Workspace);

        let file = std::env::temp_dir().join(format!("codegg-host-file-{}", uuid::Uuid::new_v4()));
        tokio::fs::write(&file, b"x").await.expect("write");
        let not_dir = canonical_workspace(&file.to_string_lossy())
            .await
            .expect_err("file");
        assert_eq!(not_dir.data.expect("class").class, ErrorClass::Workspace);
        let _ = tokio::fs::remove_file(file).await;
    }

    #[test]
    fn approval_and_sandbox_names_are_parsed_with_the_documented_aliases() {
        assert_eq!(
            parse_approval("ask").expect("ask"),
            ApprovalMode::Interactive
        );
        assert_eq!(parse_approval("yolo").expect("yolo"), ApprovalMode::Yolo);
        assert_eq!(
            parse_approval("auto").expect("auto"),
            ApprovalMode::Automatic
        );
        assert!(parse_approval("sometimes").is_err());
        assert_eq!(
            parse_sandbox("full-host").expect("full-host"),
            SandboxProfile::FullHost
        );
        assert_eq!(
            parse_sandbox("workspace-write").expect("workspace-write"),
            SandboxProfile::WorkspaceWrite
        );
        assert!(parse_sandbox("everything").is_err());
    }

    #[test]
    fn a_rejected_descriptor_names_fields_and_never_values() {
        let error = parse_descriptor(json!({
            "id": "fake",
            "baseUrl": "https://example.invalid/v1",
            "protocol": "chat_completions",
            "credential": {"kind": "api_key", "value": "super-secret-key"},
            "models": [{"id": "m", "contextWindow": 0}],
        }))
        .expect_err("refused");
        assert!(error.message.contains("models[0].contextWindow"));
        assert!(!error.message.contains("super-secret-key"));

        let malformed = parse_descriptor(json!({
            "id": "fake",
            "baseUrl": 5,
        }))
        .expect_err("refused");
        assert_eq!(malformed.message, "provider descriptor is malformed");
    }
}
