#![cfg(all(windows, feature = "windows-sandbox-test-support"))]

//! Test-only actual-child probe contract shared by native Windows feasibility runs.
//! Each action gets a fresh managed child so a compatibility failure cannot hide
//! unrelated security observations.

use codegg::managed_process::{
    EnvironmentPolicy, ManagedProcessRequest, ManagedProcessService, ProcessProvenance,
    SandboxExecutionOutcome, SandboxRequest, TerminationReason,
};
use codegg::security::sandbox::SandboxLaunchSpec;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum Backend {
    TestOnlyAppContainer,
    FullHostCompatibilityBaseline,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    ProjectionReadWrite,
    GitInit,
    GitStatus,
    PythonVersion,
    CargoVersion,
    DirectHostRead,
    JunctionHostRead,
    CaseAliasHostRead,
    ExtendedPathHostRead,
    HardlinkHostRead,
    AlternateStreamRead,
    SiblingHostRead,
    OutsideCreateWrite,
    PipeConnect,
    NullDeviceAccess,
    UncHostReadUnavailable,
    InheritedHandleNotTransferred,
    RestartRecoveryUnavailable,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum State {
    Allowed,
    Denied,
    Leaked,
    Failed,
    Blocked,
    NotAvailable,
    NotRun,
}

#[derive(Debug, Serialize)]
struct Observation {
    backend: Backend,
    host_version: String,
    host_privilege: String,
    token: String,
    job_verified: bool,
    action: Action,
    state: State,
    expected: &'static str,
    resource_identity: String,
    child_evidence: String,
    provenance: String,
    error: Option<String>,
    elapsed_ms: u128,
    executed: bool,
}

struct ProbeResources<'a> {
    credential: &'a Path,
    junction_credential: &'a Path,
    hardlink: &'a Path,
    ads: &'a Path,
    sibling: &'a Path,
    case_alias: &'a Path,
    extended_path: &'a Path,
}

fn powershell() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .expect("SystemRoot is set")
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe")
}

fn find_executable(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}

fn host_privilege() -> String {
    std::env::var("CODEGG_C019_HOST_PRIVILEGE")
        .unwrap_or_else(|_| "runner privilege metadata unavailable".to_owned())
}

fn resource_identity(path: &Path) -> String {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };

    match std::fs::File::open(path) {
        Ok(file) => {
            let mut information = BY_HANDLE_FILE_INFORMATION::default();
            if unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut information) }
                == 0
            {
                return format!(
                    "path={} identity_error={}",
                    path.display(),
                    std::io::Error::last_os_error()
                );
            }
            let file_index = (u64::from(information.nFileIndexHigh) << 32)
                | u64::from(information.nFileIndexLow);
            format!(
                "path={} volume={} file_index={} links={}",
                path.display(),
                information.dwVolumeSerialNumber,
                file_index,
                information.nNumberOfLinks
            )
        }
        Err(error) => format!("path={} identity_error={error}", path.display()),
    }
}

fn child_script(action: Action) -> &'static str {
    match action {
        Action::ProjectionReadWrite => {
            "$p=[IO.Path]::Combine($env:CODEGG_C014_PROJECTION,'stage.txt'); [IO.File]::WriteAllText($p,'stage-ok'); if ([IO.File]::ReadAllText($p) -eq 'stage-ok') {[Console]::WriteLine('C019_RESULT=allowed')} else {[Console]::WriteLine('C019_RESULT=failed')}"
        }
        Action::GitInit => {
            "$p=[IO.Path]::Combine($env:CODEGG_C014_PROJECTION,'git-init'); & $env:C019_GIT init --quiet $p; if ($LASTEXITCODE -eq 0) {[Console]::WriteLine('C019_RESULT=allowed')} else {[Console]::WriteLine('C019_RESULT=failed')}"
        }
        Action::GitStatus => {
            "$p=[IO.Path]::Combine($env:CODEGG_C014_PROJECTION,'git-init'); & $env:C019_GIT init --quiet $p; if ($LASTEXITCODE -eq 0) {& $env:C019_GIT -C $p status --porcelain; if ($LASTEXITCODE -eq 0) {[Console]::WriteLine('C019_RESULT=allowed')} else {[Console]::WriteLine('C019_RESULT=failed')}} else {[Console]::WriteLine('C019_RESULT=failed')}"
        }
        Action::PythonVersion => {
            "& $env:C019_PYTHON --version; if ($LASTEXITCODE -eq 0) {[Console]::WriteLine('C019_RESULT=allowed')} else {[Console]::WriteLine('C019_RESULT=failed')}"
        }
        Action::CargoVersion => {
            "& $env:C019_CARGO --version; if ($LASTEXITCODE -eq 0) {[Console]::WriteLine('C019_RESULT=allowed')} else {[Console]::WriteLine('C019_RESULT=failed')}"
        }
        Action::DirectHostRead => "$p=$env:C019_CREDENTIAL",
        Action::JunctionHostRead => "$p=$env:C019_JUNCTION_CREDENTIAL",
        Action::CaseAliasHostRead => "$p=$env:C019_CASE_ALIAS",
        Action::ExtendedPathHostRead => "$p=$env:C019_EXTENDED_PATH",
        Action::HardlinkHostRead => "$p=$env:C019_HARDLINK",
        Action::AlternateStreamRead => "$p=$env:C019_ADS",
        Action::SiblingHostRead => "$p=$env:C019_SIBLING",
        Action::OutsideCreateWrite => {
            "$p=$env:C019_OUTSIDE_WRITE; try {[IO.File]::WriteAllText($p,'synthetic-c019-write'); 'C019_RESULT=leaked'} catch {'C019_RESULT=denied'}"
        }
        Action::PipeConnect => {
            "$pipe=[IO.Pipes.NamedPipeClientStream]::new('.', $env:C019_PIPE, [IO.Pipes.PipeDirection]::InOut); try {$pipe.Connect(500); 'C019_RESULT=leaked'} catch {'C019_RESULT=denied'} finally {$pipe.Dispose()}"
        }
        Action::NullDeviceAccess => {
            "$s=$null; try {$s=[IO.File]::Open('\\\\.\\NUL',[IO.FileMode]::Open,[IO.FileAccess]::ReadWrite,[IO.FileShare]::ReadWrite); $s.WriteByte(0); [Console]::WriteLine('C019_RESULT=allowed')} catch {[Console]::WriteLine('C019_RESULT=denied:'+$_.Exception.GetType().Name)} finally {if ($s) {$s.Dispose()}}"
        }
        Action::UncHostReadUnavailable => "'C019_RESULT=not_run'",
        Action::InheritedHandleNotTransferred => "'C019_RESULT=not_run'",
        Action::RestartRecoveryUnavailable => "'C019_RESULT=not_run'",
    }
}

fn read_script(action: Action) -> String {
    let base = child_script(action);
    let token = "[Console]::WriteLine('C019_CHILD_STARTED');";
    if matches!(
        action,
        Action::DirectHostRead
            | Action::JunctionHostRead
            | Action::CaseAliasHostRead
            | Action::ExtendedPathHostRead
            | Action::HardlinkHostRead
            | Action::AlternateStreamRead
            | Action::SiblingHostRead
    ) {
        format!(
            "{token} {base}; try {{ [IO.File]::ReadAllText($p) | Out-Null; 'C019_RESULT=leaked' }} catch {{ 'C019_RESULT=denied' }}"
        )
    } else {
        format!("{token} {base}")
    }
}

async fn execute(
    action: Action,
    resource: &Path,
    stage: &Path,
    git: Option<&Path>,
    python: Option<&Path>,
    cargo: Option<&Path>,
    resources: &ProbeResources<'_>,
    outside_write: &Path,
    pipe: &str,
    restricted: bool,
) -> Observation {
    if matches!(action, Action::UncHostReadUnavailable) {
        return Observation {
            backend: Backend::TestOnlyAppContainer,
            host_version: std::env::var("CODEGG_C019_HOST_VERSION")
                .unwrap_or_else(|_| "runner metadata unavailable".to_owned()),
            host_privilege: host_privilege(),
            token: "not observed; UNC fixture unavailable".to_owned(),
            job_verified: false,
            action,
            state: State::NotRun,
            expected: "denied if a synthetic UNC resource is provisioned",
            resource_identity: resource.display().to_string(),
            child_evidence: String::new(),
            provenance: format!("job=windows-c019 attempt={action:?}"),
            error: Some("no isolated SMB fixture is configured on the hosted runner".to_owned()),
            elapsed_ms: 0,
            executed: false,
        };
    }
    let tool_available = match action {
        Action::GitInit | Action::GitStatus => git,
        Action::PythonVersion => python,
        Action::CargoVersion => cargo,
        _ => Some(Path::new("present")),
    };
    if tool_available.is_none() {
        return Observation {
            backend: Backend::TestOnlyAppContainer,
            host_version: std::env::var("CODEGG_C019_HOST_VERSION")
                .unwrap_or_else(|_| "runner metadata unavailable".to_owned()),
            host_privilege: host_privilege(),
            token: "not observed; tool unavailable".to_owned(),
            job_verified: false,
            action,
            state: State::NotAvailable,
            expected: "allowed tool execution or denied protected resource",
            resource_identity: resource_identity(resource),
            child_evidence: String::new(),
            provenance: format!("job=windows-c019 attempt={action:?}"),
            error: Some("tool is not installed on this runner".to_owned()),
            elapsed_ms: 0,
            executed: false,
        };
    }
    let shell = powershell();
    let script = read_script(action);
    let args = vec![
        OsString::from("-NoLogo"),
        OsString::from("-NoProfile"),
        OsString::from("-NonInteractive"),
        OsString::from("-Command"),
        OsString::from(script.clone()),
    ];
    // Restricted probes request the private AppContainer temp projection;
    // the unrestricted device baseline starts from the OS runtime directly.
    let cwd = if restricted {
        PathBuf::from(r"C:\__codegg_test_profile_temp__")
    } else {
        std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .expect("SystemRoot is set")
            .join("System32")
    };
    let mut request = ManagedProcessRequest::new(
        std::iter::once(shell.clone().into_os_string())
            .chain(args.iter().cloned())
            .collect(),
        cwd,
        ProcessProvenance::new("windows-c019", format!("{action:?}")),
    );
    request.timeout = Some(Duration::from_secs(30));
    let mut environment = EnvironmentPolicy::sanitized()
        .with_var("C019_PIPE", pipe)
        .with_var("C019_ACTION", format!("{action:?}"))
        .with_var("C019_CREDENTIAL", resources.credential.as_os_str())
        .with_var(
            "C019_JUNCTION_CREDENTIAL",
            resources.junction_credential.as_os_str(),
        )
        .with_var("C019_CASE_ALIAS", resources.case_alias.as_os_str())
        .with_var("C019_EXTENDED_PATH", resources.extended_path.as_os_str())
        .with_var("C019_HARDLINK", resources.hardlink.as_os_str())
        .with_var("C019_ADS", resources.ads.as_os_str())
        .with_var("C019_SIBLING", resources.sibling.as_os_str())
        .with_var("C019_OUTSIDE_WRITE", outside_write.as_os_str());
    if let Some(path) = git {
        environment = environment.with_var("C019_GIT", path.as_os_str());
    }
    if let Some(path) = python {
        environment = environment.with_var("C019_PYTHON", path.as_os_str());
    }
    if let Some(path) = cargo {
        environment = environment.with_var("C019_CARGO", path.as_os_str());
    }
    request.environment_policy = environment;
    let backend = if restricted {
        request.sandbox = SandboxRequest::Required(SandboxLaunchSpec {
            target: shell.clone(),
            args: args
                .iter()
                .map(|argument| argument.to_string_lossy().into_owned())
                .collect(),
            read_paths: Vec::new(),
            write_paths: vec![stage.to_path_buf()],
            deny_paths: Vec::new(),
            linux_ipc_scoped: false,
        });
        Backend::TestOnlyAppContainer
    } else {
        Backend::FullHostCompatibilityBaseline
    };

    let started = std::time::Instant::now();
    eprintln!("C019_START action={action:?} restricted={restricted}");
    let result = ManagedProcessService::run(request).await;
    let elapsed_ms = started.elapsed().as_millis();
    let (state, evidence, error, executed, token, job_verified) = match result {
        Ok(result) => {
            let output = result.stdout.to_string_lossy();
            let token = output
                .lines()
                .find_map(|line| line.strip_prefix("C019_TOKEN="))
                .unwrap_or("child identity not queried; AppContainer SID and Job are verified by the managed launcher")
                .to_owned();
            let mut state = if !matches!(result.termination, TerminationReason::Exited) {
                State::Blocked
            } else if output.contains("C019_RESULT=allowed") {
                State::Allowed
            } else if output.contains("C019_RESULT=denied") {
                State::Denied
            } else if output.contains("C019_RESULT=leaked") {
                State::Leaked
            } else if output.contains("C019_RESULT=not_run") {
                State::NotAvailable
            } else {
                State::Failed
            };
            let mut error = if !matches!(result.termination, TerminationReason::Exited) {
                Some(format!("child termination: {:?}", result.termination))
            } else {
                (!result.exit_status.success()).then(|| result.stderr.to_string_lossy())
            };
            let sandbox_verified = if restricted {
                matches!(
                    &result.sandbox,
                    SandboxExecutionOutcome::TestOnlyAppContainer {
                        token_verified: true,
                        job_verified: true,
                        ..
                    }
                )
            } else {
                matches!(&result.sandbox, SandboxExecutionOutcome::Disabled)
            };
            if !sandbox_verified {
                state = State::Failed;
                let detail = format!("unexpected sandbox outcome: {:?}", result.sandbox);
                error = Some(error.map_or(detail.clone(), |prior| format!("{prior}; {detail}")));
            }
            (
                state,
                output,
                error,
                true,
                token,
                restricted && sandbox_verified,
            )
        }
        Err(error) => (
            if matches!(
                &error,
                codegg::managed_process::ManagedProcessError::SandboxFailed(_)
            ) {
                State::Blocked
            } else {
                State::Failed
            },
            String::new(),
            Some(error.to_string()),
            false,
            "child not started".to_owned(),
            false,
        ),
    };
    Observation {
        backend,
        host_version: std::env::var("CODEGG_C019_HOST_VERSION")
            .unwrap_or_else(|_| "runner metadata unavailable".to_owned()),
        host_privilege: host_privilege(),
        token,
        job_verified,
        action,
        state,
        expected: if matches!(
            action,
            Action::ProjectionReadWrite
                | Action::GitInit
                | Action::GitStatus
                | Action::PythonVersion
                | Action::CargoVersion
                | Action::NullDeviceAccess
        ) {
            "allowed"
        } else if matches!(action, Action::InheritedHandleNotTransferred) {
            "not transferred by the managed handle allowlist"
        } else {
            "denied"
        },
        resource_identity: if matches!(
            action,
            Action::ProjectionReadWrite | Action::GitInit | Action::GitStatus
        ) {
            "per-child AppContainer-private temporary projection (dynamic path supplied to child)"
                .to_owned()
        } else {
            resource_identity(resource)
        },
        child_evidence: evidence,
        provenance: format!("job=windows-c019 attempt={action:?}"),
        error,
        elapsed_ms,
        executed,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn c019_shared_native_security_probe_contract() {
    let fixture = tempfile::tempdir().expect("C019 disposable fixture");
    // This sentinel asks the managed test launcher to create and grant a fresh
    // per-child AppContainer-private temp projection. No host path is leased.
    let stage = PathBuf::from(r"C:\__codegg_test_profile_temp__");
    let host = fixture.path().join("original-host");
    let sibling = fixture.path().join("original-sibling");
    let outside_write = fixture.path().join("outside-stage-write.txt");
    std::fs::create_dir(&host).unwrap();
    std::fs::create_dir(&sibling).unwrap();
    let credential = host.join("synthetic-credential.txt");
    let sibling_secret = sibling.join("synthetic-sibling.txt");
    let case_alias = host.join("SYNTHETIC-CREDENTIAL.TXT");
    std::fs::write(&credential, "synthetic-c019-secret").unwrap();
    std::fs::write(&sibling_secret, "synthetic-c019-sibling").unwrap();
    let extended_path = PathBuf::from(format!(r"\\?\{}", credential.display()));
    let hardlink = host.join("credential-hardlink.txt");
    std::fs::hard_link(&credential, &hardlink).expect("synthetic hardlink");
    let ads = PathBuf::from(format!("{}:c019", credential.display()));
    std::fs::write(&ads, "synthetic-c019-ads").expect("synthetic ADS");
    let junction = fixture.path().join("credential-junction");
    let junction_result = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&junction)
        .arg(&host)
        .output()
        .expect("cmd.exe is present");
    assert!(
        junction_result.status.success(),
        "junction fixture failed: {}{}",
        String::from_utf8_lossy(&junction_result.stdout),
        String::from_utf8_lossy(&junction_result.stderr)
    );
    let junction_credential = junction.join("synthetic-credential.txt");
    let pipe_name = format!("codegg-c019-{}", uuid::Uuid::new_v4().simple());
    let pipe_path = format!(r"\\.\pipe\{pipe_name}");
    let _pipe_server = tokio::net::windows::named_pipe::ServerOptions::new()
        .first_pipe_instance(true)
        .create(&pipe_path)
        .expect("synthetic IPC fixture");
    let resources = ProbeResources {
        credential: &credential,
        junction_credential: &junction_credential,
        hardlink: &hardlink,
        ads: &ads,
        sibling: &sibling_secret,
        case_alias: &case_alias,
        extended_path: &extended_path,
    };
    let host_dacl_before = codegg::managed_process::windows_acl_lease_dacl_snapshot(&host).unwrap();
    let sibling_dacl_before =
        codegg::managed_process::windows_acl_lease_dacl_snapshot(&sibling).unwrap();
    let credential_before = std::fs::read(&credential).unwrap();

    let git = find_executable("git.exe");
    let python = find_executable("python.exe");
    let cargo = find_executable("cargo.exe");
    let actions = [
        (Action::ProjectionReadWrite, stage.as_path()),
        (Action::GitInit, stage.as_path()),
        (Action::GitStatus, stage.as_path()),
        (Action::PythonVersion, Path::new("python.exe")),
        (Action::CargoVersion, Path::new("cargo.exe")),
        (Action::DirectHostRead, credential.as_path()),
        (Action::JunctionHostRead, junction_credential.as_path()),
        (Action::CaseAliasHostRead, case_alias.as_path()),
        (Action::ExtendedPathHostRead, extended_path.as_path()),
        (Action::HardlinkHostRead, hardlink.as_path()),
        (Action::AlternateStreamRead, ads.as_path()),
        (Action::SiblingHostRead, sibling_secret.as_path()),
        (Action::OutsideCreateWrite, outside_write.as_path()),
        (Action::PipeConnect, Path::new("synthetic-c019-pipe")),
        (Action::NullDeviceAccess, Path::new(r"\Device\Null")),
        (
            Action::UncHostReadUnavailable,
            Path::new("unc-synthetic-host-fixture-not-configured"),
        ),
    ];
    let mut observations = Vec::with_capacity(actions.len());
    for (action, resource) in actions {
        eprintln!("C019_FIXTURE action={action:?}");
        observations.push(
            execute(
                action,
                resource,
                &stage,
                git.as_deref(),
                python.as_deref(),
                cargo.as_deref(),
                &resources,
                &outside_write,
                &pipe_name,
                true,
            )
            .await,
        );
    }
    observations.push(
        execute(
            Action::NullDeviceAccess,
            Path::new(r"\Device\Null"),
            &stage,
            git.as_deref(),
            python.as_deref(),
            cargo.as_deref(),
            &resources,
            &outside_write,
            &pipe_name,
            false,
        )
        .await,
    );
    for observation in &observations {
        eprintln!("C019_PROBE={}", serde_json::to_string(observation).unwrap());
    }
    observations.push(Observation {
        backend: Backend::TestOnlyAppContainer,
        host_version: std::env::var("CODEGG_C019_HOST_VERSION")
            .unwrap_or_else(|_| "runner metadata unavailable".to_owned()),
        host_privilege: host_privilege(),
        token: "not observed; unsupported handle transfer probe".to_owned(),
        job_verified: false,
        action: Action::InheritedHandleNotTransferred,
        state: State::NotRun,
        expected: "not transferred by the managed handle allowlist",
        resource_identity: "synthetic inherited host file handle".to_owned(),
        child_evidence: String::new(),
        provenance: "job=windows-c019 attempt=InheritedHandleNotTransferred".to_owned(),
        error: Some(
            "the canonical managed launcher explicitly passes only stdio handles; a hostile-handle transfer experiment is outside this fixture and remains not run".to_owned(),
        ),
        elapsed_ms: 0,
        executed: false,
    });
    eprintln!(
        "C019_PROBE={}",
        serde_json::to_string(observations.last().unwrap()).unwrap()
    );
    observations.push(Observation {
        backend: Backend::TestOnlyAppContainer,
        host_version: std::env::var("CODEGG_C019_HOST_VERSION")
            .unwrap_or_else(|_| "runner metadata unavailable".to_owned()),
        host_privilege: host_privilege(),
        token: "not applicable; fixture has no durable request state".to_owned(),
        job_verified: false,
        action: Action::RestartRecoveryUnavailable,
        state: State::NotRun,
        expected: "no interrupted durable sandbox request is replayed",
        resource_identity: "ephemeral test fixture only".to_owned(),
        child_evidence: String::new(),
        provenance: "job=windows-c019 attempt=RestartRecoveryUnavailable".to_owned(),
        error: Some(
            "the test-only process fixture has no persisted request or startup recovery path"
                .to_owned(),
        ),
        elapsed_ms: 0,
        executed: false,
    });
    eprintln!(
        "C019_PROBE={}",
        serde_json::to_string(observations.last().unwrap()).unwrap()
    );
    let summary = serde_json::json!({
        "contract": "codegg.windows_native_probe.v1",
        "host": "windows-hosted-runner",
        "fixture_integrity": {
            "stage": "per-child AppContainer-private temporary projection",
            "host_dacl_before_sha256": format!("{:x}", Sha256::digest(&host_dacl_before)),
            "sibling_dacl_before_sha256": format!("{:x}", Sha256::digest(&sibling_dacl_before)),
            "host_credential_unchanged": std::fs::read(&credential).unwrap() == credential_before,
            "host_dacl_unchanged": codegg::managed_process::windows_acl_lease_dacl_snapshot(&host).unwrap() == host_dacl_before,
            "sibling_dacl_unchanged": codegg::managed_process::windows_acl_lease_dacl_snapshot(&sibling).unwrap() == sibling_dacl_before,
            "credential_identity": resource_identity(&credential),
            "hardlink_identity": resource_identity(&hardlink),
        },
        "observations": observations,
    });
    eprintln!("C019_REPORT={}", serde_json::to_string(&summary).unwrap());
    assert_eq!(std::fs::read(&credential).unwrap(), credential_before);
    assert_eq!(
        codegg::managed_process::windows_acl_lease_dacl_snapshot(&host).unwrap(),
        host_dacl_before
    );
    assert_eq!(
        codegg::managed_process::windows_acl_lease_dacl_snapshot(&sibling).unwrap(),
        sibling_dacl_before
    );
    assert!(
        observations.iter().any(|observation| observation.executed),
        "C019 must execute at least one actual child operation"
    );
    assert!(
        observations.iter().any(|observation| {
            matches!(observation.action, Action::ProjectionReadWrite)
                && matches!(observation.state, State::Allowed)
        }),
        "C019 must prove the disposable stage allow operation"
    );
    if outside_write.exists() {
        std::fs::remove_file(&outside_write)
            .expect("remove C019-owned outside-stage file if the probe unexpectedly wrote it");
    }
}
