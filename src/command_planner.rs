use std::path::PathBuf;

use crate::command_intent::{CommandIntent, CommandIntentKind, ExecutionCapability};

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ExecutionBackend {
    RawShell,
    ManagedProcess,
    NativeTool(String),
    TestRunner,
    PythonScripting,
    Rejected,
}

impl ExecutionBackend {
    pub fn label(&self) -> &str {
        match self {
            Self::RawShell => "raw-shell",
            Self::ManagedProcess => "managed-process",
            Self::NativeTool(name) => name,
            Self::TestRunner => "test-runner",
            Self::PythonScripting => "python-scripting",
            Self::Rejected => "rejected",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PermissionRequest {
    pub capability: ExecutionCapability,
    pub reason: String,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CommandPlan {
    pub intent: CommandIntent,
    pub backend: ExecutionBackend,
    pub permissions: Vec<PermissionRequest>,
    pub timeout_secs: Option<u64>,
    pub cwd: Option<PathBuf>,
}

impl CommandPlan {
    pub fn is_executable(&self) -> bool {
        !matches!(self.backend, ExecutionBackend::Rejected)
    }

    pub fn requires_any_permission(&self) -> bool {
        self.permissions.iter().any(|p| p.required)
    }
}

pub fn plan_execution(intent: &CommandIntent) -> CommandPlan {
    let backend = select_backend(intent);
    let permissions = generate_permissions(intent, &backend);
    let timeout_secs = select_timeout(intent);

    CommandPlan {
        intent: intent.clone(),
        backend,
        permissions,
        timeout_secs,
        cwd: None,
    }
}

fn select_backend(intent: &CommandIntent) -> ExecutionBackend {
    match intent.kind {
        CommandIntentKind::Test => ExecutionBackend::TestRunner,
        CommandIntentKind::PythonAnalyze
        | CommandIntentKind::PythonTransform
        | CommandIntentKind::PythonVerify => ExecutionBackend::PythonScripting,
        CommandIntentKind::GitReadOnly => ExecutionBackend::NativeTool("egggit".to_string()),
        CommandIntentKind::SearchReadOnly => ExecutionBackend::ManagedProcess,
        CommandIntentKind::FileRead => ExecutionBackend::ManagedProcess,
        CommandIntentKind::Build | CommandIntentKind::Lint | CommandIntentKind::Format => {
            ExecutionBackend::ManagedProcess
        }
        CommandIntentKind::GitMutating => ExecutionBackend::RawShell,
        CommandIntentKind::FileWrite | CommandIntentKind::FileEdit => ExecutionBackend::RawShell,
        CommandIntentKind::RawShell => ExecutionBackend::RawShell,
        CommandIntentKind::Rejected => ExecutionBackend::Rejected,
    }
}

fn generate_permissions(
    intent: &CommandIntent,
    backend: &ExecutionBackend,
) -> Vec<PermissionRequest> {
    let mut perms = Vec::new();

    if matches!(backend, ExecutionBackend::Rejected) {
        return perms;
    }

    for cap in &intent.risk.capabilities {
        let (reason, required) = match cap {
            ExecutionCapability::ReadWorkspace => ("read workspace files".to_string(), false),
            ExecutionCapability::WriteWorkspace => ("write workspace files".to_string(), true),
            ExecutionCapability::Subprocess => ("spawn subprocess".to_string(), false),
            ExecutionCapability::Network => ("access network".to_string(), true),
            ExecutionCapability::EnvAccess => ("access environment variables".to_string(), false),
            ExecutionCapability::DependencyInstall => ("install dependencies".to_string(), true),
            ExecutionCapability::OutsideWorkspace => {
                ("access files outside workspace".to_string(), true)
            }
            ExecutionCapability::DestructiveFileMutation => {
                ("destructive file mutation".to_string(), true)
            }
            ExecutionCapability::GitMutation => ("git mutation".to_string(), true),
            ExecutionCapability::ContextPromotion => {
                ("promote output to model context".to_string(), false)
            }
        };

        perms.push(PermissionRequest {
            capability: *cap,
            reason,
            required,
        });
    }

    perms
}

fn select_timeout(intent: &CommandIntent) -> Option<u64> {
    match intent.kind {
        CommandIntentKind::Test => Some(300),
        CommandIntentKind::Build => Some(120),
        CommandIntentKind::PythonAnalyze | CommandIntentKind::PythonTransform => Some(60),
        CommandIntentKind::PythonVerify => Some(300),
        CommandIntentKind::GitReadOnly => Some(30),
        CommandIntentKind::GitMutating => Some(60),
        CommandIntentKind::SearchReadOnly => Some(30),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_intent::classify_command;

    #[test]
    fn test_command_routes_to_test_runner() {
        let intent = classify_command("cargo test");
        let plan = plan_execution(&intent);
        assert_eq!(plan.backend, ExecutionBackend::TestRunner);
        assert!(plan.is_executable());
    }

    #[test]
    fn python_command_routes_to_python_scripting() {
        let intent = classify_command("python3 script.py");
        let plan = plan_execution(&intent);
        assert_eq!(plan.backend, ExecutionBackend::PythonScripting);
        assert!(plan.is_executable());
    }

    #[test]
    fn git_status_routes_to_native() {
        let intent = classify_command("git status");
        let plan = plan_execution(&intent);
        assert_eq!(
            plan.backend,
            ExecutionBackend::NativeTool("egggit".to_string())
        );
        assert!(!plan.requires_any_permission());
    }

    #[test]
    fn git_push_requires_permission() {
        let intent = classify_command("git push origin main");
        let plan = plan_execution(&intent);
        assert!(plan.requires_any_permission());
        assert!(plan.permissions.iter().any(|p| p.required));
    }

    #[test]
    fn search_routes_to_managed_process() {
        let intent = classify_command("rg 'pattern'");
        let plan = plan_execution(&intent);
        assert_eq!(plan.backend, ExecutionBackend::ManagedProcess);
    }

    #[test]
    fn rejected_command_not_executable() {
        let intent = classify_command("");
        let plan = plan_execution(&intent);
        assert!(!plan.is_executable());
        assert_eq!(plan.backend, ExecutionBackend::Rejected);
    }

    #[test]
    fn test_command_has_timeout() {
        let intent = classify_command("cargo test");
        let plan = plan_execution(&intent);
        assert_eq!(plan.timeout_secs, Some(300));
    }

    #[test]
    fn build_command_has_timeout() {
        let intent = classify_command("cargo build");
        let plan = plan_execution(&intent);
        assert_eq!(plan.timeout_secs, Some(120));
    }
}
