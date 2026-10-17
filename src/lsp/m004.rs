//! Daemon-owned, execution-scoped M004 LSP services.
//!
//! A constrained LSP service is created for one turn and one canonical
//! workspace. It cannot reuse the daemon's FullHost client map, and every
//! initial launch and restart goes through the same required M004 helper.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, Weak};

use codegg_core::approval::SandboxProfile;
use tokio_util::sync::CancellationToken;

#[cfg(target_os = "linux")]
use crate::managed_process::{EnvironmentPolicy, ProcessProvenance};

pub(crate) struct M004LspServiceScope {
    service: Arc<egglsp::service::LspService>,
    shutdown: CancellationToken,
}

struct RegisteredServiceScope {
    service: Weak<egglsp::service::LspService>,
    profile: SandboxProfile,
    workspace_root: PathBuf,
}

fn registered_scopes() -> &'static parking_lot::Mutex<HashMap<usize, RegisteredServiceScope>> {
    static SCOPES: OnceLock<parking_lot::Mutex<HashMap<usize, RegisteredServiceScope>>> =
        OnceLock::new();
    SCOPES.get_or_init(|| parking_lot::Mutex::new(HashMap::new()))
}

fn service_identity(service: &egglsp::service::LspService) -> usize {
    service as *const egglsp::service::LspService as usize
}

#[cfg(target_os = "linux")]
fn register_scope(
    service: &Arc<egglsp::service::LspService>,
    profile: SandboxProfile,
    workspace_root: PathBuf,
) {
    registered_scopes().lock().insert(
        service_identity(service),
        RegisteredServiceScope {
            service: Arc::downgrade(service),
            profile,
            workspace_root,
        },
    );
}

fn unregister_scope(service: &egglsp::service::LspService) {
    registered_scopes()
        .lock()
        .remove(&service_identity(service));
}

impl M004LspServiceScope {
    #[cfg(target_os = "linux")]
    pub(crate) fn new(
        profile: SandboxProfile,
        workspace_root: PathBuf,
        config: egglsp::config::LspConfig,
    ) -> Result<Arc<Self>, String> {
        if !profile.requires_filesystem_containment() {
            return Err("M004 LSP scope requires a constrained profile".into());
        }
        let workspace_root = workspace_root
            .canonicalize()
            .map_err(|error| format!("M004 LSP workspace root is unavailable: {error}"))?;
        if !workspace_root.is_dir() {
            return Err("M004 LSP workspace root is not a directory".into());
        }
        let shutdown = CancellationToken::new();
        let launcher = Arc::new(M004LspProcessLauncher {
            profile,
            workspace_root: workspace_root.clone(),
            shutdown: shutdown.clone(),
        });
        let service = egglsp::service::LspService::new_arc_with_launcher(config, launcher);
        register_scope(&service, profile, workspace_root);
        Ok(Arc::new(Self { service, shutdown }))
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn new(
        _profile: SandboxProfile,
        _workspace_root: PathBuf,
        _config: egglsp::config::LspConfig,
    ) -> Result<Arc<Self>, String> {
        Err("constrained M004 LSP launches are available only on Linux".into())
    }

    pub(crate) fn service(&self) -> Arc<egglsp::service::LspService> {
        self.service.clone()
    }

    pub(crate) async fn shutdown(&self) {
        self.service.shutdown_all().await;
        self.shutdown.cancel();
        unregister_scope(&self.service);
    }
}

pub(crate) fn service_is_scoped_for(
    service: &egglsp::service::LspService,
    profile: SandboxProfile,
    root: &Path,
) -> bool {
    if !profile.requires_filesystem_containment() {
        return false;
    }
    let Ok(canonical_root) = root.canonicalize() else {
        return false;
    };
    let identity = service_identity(service);
    let mut scopes = registered_scopes().lock();
    scopes.retain(|_, scope| scope.service.strong_count() > 0);
    scopes.get(&identity).is_some_and(|scope| {
        scope.profile == profile
            && scope.workspace_root == canonical_root
            && scope
                .service
                .upgrade()
                .is_some_and(|registered| std::ptr::eq(Arc::as_ptr(&registered), service))
    })
}

impl Drop for M004LspServiceScope {
    fn drop(&mut self) {
        self.shutdown.cancel();
        unregister_scope(&self.service);
        let service = self.service.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move { service.shutdown_all().await });
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use egglsp::config::{LspConfig, LspRule};
    use tempfile::TempDir;

    fn python() -> PathBuf {
        PathBuf::from("/usr/bin/python3")
    }

    fn fixture(root: &Path, host_pid: u32, marker: &Path) -> (LspConfig, PathBuf) {
        let script = root.join("contained_lsp.py");
        std::fs::write(
            &script,
            r#"import json, os, sys
def read_message():
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line in (b'\r\n', b'\n'):
            break
        key, value = line.decode('ascii').split(':', 1)
        headers[key.lower()] = value.strip()
    length = int(headers['content-length'])
    return json.loads(sys.stdin.buffer.read(length))
def send(value):
    body = json.dumps(value).encode()
    sys.stdout.buffer.write(b'Content-Length: ' + str(len(body)).encode() + b'\r\n\r\n' + body)
    sys.stdout.buffer.flush()
try:
    open('/proc/' + os.environ['CODEGG_M004_HOST_PID'] + '/stat', 'rb').read(1)
    proc_visible = True
except OSError:
    proc_visible = False
with open(os.environ['CODEGG_M004_MARKER'], 'w') as marker:
    marker.write('host_proc_visible=' + str(proc_visible).lower())
while True:
    message = read_message()
    if message is None:
        break
    method = message.get('method')
    if method == 'initialize':
        send({'jsonrpc':'2.0', 'id':message['id'], 'result':{'capabilities':{'hoverProvider':True}}})
    elif method == 'shutdown':
        send({'jsonrpc':'2.0', 'id':message['id'], 'result':None})
    elif method == 'exit':
        with open(os.environ['CODEGG_M004_MARKER'], 'a') as marker:
            marker.write('\nshutdown_received=true')
        break
"#,
        )
        .unwrap();
        let env = HashMap::from([
            ("CODEGG_M004_HOST_PID".into(), host_pid.to_string()),
            ("CODEGG_M004_MARKER".into(), marker.display().to_string()),
        ]);
        let config = LspConfig::Rules(HashMap::from([(
            "rust-analyzer".into(),
            LspRule::Active {
                command: vec![python().display().to_string(), script.display().to_string()],
                extensions: Some(vec!["rs".into()]),
                disabled: None,
                env: Some(env),
                initialization: None,
                workspace_configuration: None,
                restart: None,
            },
        )]));
        (config, script)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn constrained_lsp_process_is_owned_by_m004_scope() {
        if std::process::Command::new(python())
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("skipping native M004 LSP probe: /usr/bin/python3 unavailable");
            return;
        }
        if std::env::var_os("CODEGG_RUN_M004_BWRAP_NATIVE").is_none() {
            eprintln!("skipping native M004 LSP probe: opt-in host qualification is disabled");
            return;
        }
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='m004-lsp-probe'\nversion='0.1.0'\nedition='2021'\n",
        )
        .unwrap();
        std::fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
        let marker = root.join("lsp-probe.txt");
        let (config, script) = fixture(&root, std::process::id(), &marker);
        assert!(script.is_file());
        let scope = M004LspServiceScope::new(SandboxProfile::WorkspaceWrite, root.clone(), config)
            .expect("construct contained LSP service");
        let service = scope.service();
        assert!(service_is_scoped_for(
            &service,
            SandboxProfile::WorkspaceWrite,
            &root
        ));
        service
            .set_managed_document(&root.join("main.rs"), "fn main() {}\n", false)
            .await
            .expect("contained LSP completes initialize and didOpen");
        scope.shutdown().await;
        let result = std::fs::read_to_string(marker).expect("contained LSP wrote probe result");
        assert_eq!(result, "host_proc_visible=false\nshutdown_received=true");
        eprintln!(
            "M004 constrained LSP completed duplex startup, hid host procfs, and shut down cleanly"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn constrained_lsp_launch_fails_before_target_when_userns_are_restricted() {
        if std::env::var_os("CODEGG_RUN_M004_RESTRICTED_USERNS").is_none() {
            eprintln!("skipping restricted-userns M004 LSP probe");
            return;
        }
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='m004-lsp-probe'\nversion='0.1.0'\nedition='2021'\n",
        )
        .unwrap();
        let marker = root.join("must-not-exist");
        let (config, _) = fixture(&root, std::process::id(), &marker);
        let scope = M004LspServiceScope::new(SandboxProfile::WorkspaceWrite, root.clone(), config)
            .expect("construct scoped service even when launch capability is absent");
        let file = root.join("main.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        let result = scope
            .service()
            .set_managed_document(&file, "fn main() {}\n", false)
            .await;
        scope.shutdown().await;
        assert!(result.is_err(), "restricted host unexpectedly started LSP");
        assert!(
            !marker.exists(),
            "LSP target ran despite denied user namespaces"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn scope_identity_binds_exact_profile_and_workspace() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let scope = M004LspServiceScope::new(
            SandboxProfile::WorkspaceWrite,
            root.clone(),
            LspConfig::default(),
        )
        .unwrap();
        let service = scope.service();
        assert!(service_is_scoped_for(
            &service,
            SandboxProfile::WorkspaceWrite,
            &root
        ));
        assert!(!service_is_scoped_for(
            &service,
            SandboxProfile::ReadOnly,
            &root
        ));
        assert!(!service_is_scoped_for(
            &service,
            SandboxProfile::WorkspaceWrite,
            &root.join("elsewhere")
        ));
        scope.shutdown().await;
        assert!(!service_is_scoped_for(
            &service,
            SandboxProfile::WorkspaceWrite,
            &root
        ));

        let forged = egglsp::service::LspService::new_arc_with_launcher(
            LspConfig::default(),
            Arc::new(M004LspProcessLauncher {
                profile: SandboxProfile::WorkspaceWrite,
                workspace_root: root.clone(),
                shutdown: CancellationToken::new(),
            }),
        );
        assert!(!service_is_scoped_for(
            &forged,
            SandboxProfile::WorkspaceWrite,
            &root
        ));
    }
}

#[cfg(target_os = "linux")]
struct M004LspProcessLauncher {
    profile: SandboxProfile,
    workspace_root: PathBuf,
    shutdown: CancellationToken,
}

#[cfg(target_os = "linux")]
#[async_trait::async_trait]
impl egglsp::launch::LspProcessLauncher for M004LspProcessLauncher {
    async fn launch(
        &self,
        launch: &egglsp::launch::LspLaunchSpec,
        root: &Path,
    ) -> Result<egglsp::launch::LspProcess, egglsp::error::LspError> {
        let canonical_root = root.canonicalize().map_err(|error| {
            egglsp::error::LspError::LaunchFailed(format!(
                "M004 LSP root cannot be canonicalized: {error}"
            ))
        })?;
        if canonical_root != self.workspace_root {
            return Err(egglsp::error::LspError::LaunchFailed(
                "M004 LSP service refused a root outside its execution scope".into(),
            ));
        }
        if launch.command.as_os_str().is_empty() || !launch.command.is_absolute() {
            return Err(egglsp::error::LspError::LaunchFailed(
                "M004 LSP command must be an absolute resolved executable".into(),
            ));
        }

        let config = crate::security::sandbox::sandbox_config_for_profile(
            self.profile,
            &self.workspace_root,
        )
        .ok_or_else(|| {
            egglsp::error::LspError::LaunchFailed(
                "M004 LSP refused an unconstrained sandbox profile".into(),
            )
        })?
        .with_linux_process_isolation(true);
        let mut sandbox = config
            .launch_spec(&launch.command, &launch.args, Some(&self.workspace_root))
            .map_err(|error| egglsp::error::LspError::LaunchFailed(error.to_string()))?;

        let mut environment = EnvironmentPolicy::sanitized();
        for (name, value) in &launch.env {
            environment = environment.with_var(name, value);
        }
        sandbox.env = environment
            .sandbox_interactive_entries(&ProcessProvenance::default())
            .map_err(|error| egglsp::error::LspError::LaunchFailed(error.to_string()))?;

        let cancellation = self.shutdown.child_token();
        let live = crate::managed_process::spawn_sandboxed_interactive_child(
            sandbox,
            &self.workspace_root,
            &environment,
            crate::managed_process::SandboxInteractiveIo::pipes(),
            &cancellation,
            &self.shutdown,
        )
        .await
        .map_err(|error| egglsp::error::LspError::LaunchFailed(error.to_string()))?;
        let mut child = live.child;
        let stdin = child.stdin.take().ok_or_else(|| {
            egglsp::error::LspError::LaunchFailed("M004 LSP stdin was not captured".into())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            egglsp::error::LspError::LaunchFailed("M004 LSP stdout was not captured".into())
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            egglsp::error::LspError::LaunchFailed("M004 LSP stderr was not captured".into())
        })?;

        Ok(egglsp::launch::LspProcess {
            stdin: Some(stdin),
            stdout: Some(stdout),
            stderr: Some(tokio::io::BufReader::new(stderr)),
            child,
            status_task: Some(live.status_task),
        })
    }
}
