#![cfg(all(windows, feature = "windows-sandbox-test-support"))]

//! Test-only actual-child probe contract shared by native Windows feasibility runs.
//! Each action gets a fresh managed child so a compatibility failure cannot hide
//! unrelated security observations.

use codegg::managed_process::{
    EnvironmentPolicy, ManagedProcessRequest, ManagedProcessService, ProcessProvenance,
    SandboxExecutionOutcome, SandboxRequest,
};
use codegg::security::sandbox::SandboxLaunchSpec;
use serde::Serialize;
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
    HardlinkHostRead,
    AlternateStreamRead,
    SiblingHostRead,
    OutsideCreateWrite,
    PipeConnect,
    NullDeviceAccess,
    InheritedHandleNotTransferred,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum State {
    Allowed,
    Denied,
    Leaked,
    Failed,
    NotAvailable,
}

#[derive(Debug, Serialize)]
struct Observation {
    backend: Backend,
    host_version: String,
    token: String,
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

fn child_script(action: Action) -> &'static str {
    match action {
        Action::ProjectionReadWrite => {
            "$p=Join-Path $env:C019_STAGE 'stage.txt'; [IO.File]::WriteAllText($p,'stage-ok'); if ([IO.File]::ReadAllText($p) -eq 'stage-ok') {'C019_RESULT=allowed'} else {'C019_RESULT=failed'}"
        }
        Action::GitInit => {
            "$p=Join-Path $env:C019_STAGE 'git-init'; & $env:C019_GIT init --quiet $p 2>&1 | Out-String | Write-Output; if ($LASTEXITCODE -eq 0) {'C019_RESULT=allowed'} else {'C019_RESULT=failed'}"
        }
        Action::GitStatus => {
            "& $env:C019_GIT -C (Join-Path $env:C019_STAGE 'git-init') status --porcelain 2>&1 | Out-String | Write-Output; if ($LASTEXITCODE -eq 0) {'C019_RESULT=allowed'} else {'C019_RESULT=failed'}"
        }
        Action::PythonVersion => {
            "& $env:C019_PYTHON --version 2>&1 | Out-String | Write-Output; if ($LASTEXITCODE -eq 0) {'C019_RESULT=allowed'} else {'C019_RESULT=failed'}"
        }
        Action::CargoVersion => {
            "& $env:C019_CARGO --version 2>&1 | Out-String | Write-Output; if ($LASTEXITCODE -eq 0) {'C019_RESULT=allowed'} else {'C019_RESULT=failed'}"
        }
        Action::DirectHostRead => "$p=$env:C019_CREDENTIAL",
        Action::JunctionHostRead => "$p=$env:C019_JUNCTION_CREDENTIAL",
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
            "$s=$null; try {$s=[IO.File]::Open('\\\\.\\NUL',[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::ReadWrite); $s.WriteByte(0); 'C019_RESULT=allowed'} catch { 'C019_RESULT=denied:'+$_.Exception.GetType().Name } finally {if ($s) {$s.Dispose()}}"
        }
        Action::InheritedHandleNotTransferred => "'C019_RESULT=not_run'",
    }
}

fn read_script(action: Action) -> String {
    let base = child_script(action);
    if matches!(
        action,
        Action::DirectHostRead
            | Action::JunctionHostRead
            | Action::HardlinkHostRead
            | Action::AlternateStreamRead
            | Action::SiblingHostRead
    ) {
        format!(
            "{base}; try {{ [IO.File]::ReadAllText($p) | Out-Null; 'C019_RESULT=leaked' }} catch {{ 'C019_RESULT=denied' }}"
        )
    } else {
        base.to_owned()
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
            token: "not observed; tool unavailable".to_owned(),
            action,
            state: State::NotAvailable,
            expected: "allowed tool execution or denied protected resource",
            resource_identity: resource.display().to_string(),
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
    let cwd = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .expect("SystemRoot is set")
        .join("System32");
    let mut request = ManagedProcessRequest::new(
        std::iter::once(shell.clone().into_os_string())
            .chain(args.iter().cloned())
            .collect(),
        cwd,
        ProcessProvenance::new("windows-c019", format!("{action:?}")),
    );
    request.timeout = Some(Duration::from_secs(15));
    let mut environment = EnvironmentPolicy::sanitized()
        .with_var("C019_STAGE", stage.as_os_str())
        .with_var("C019_PIPE", pipe)
        .with_var("C019_ACTION", format!("{action:?}"))
        .with_var("C019_CREDENTIAL", resources.credential.as_os_str())
        .with_var(
            "C019_JUNCTION_CREDENTIAL",
            resources.junction_credential.as_os_str(),
        )
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
        });
        Backend::TestOnlyAppContainer
    } else {
        Backend::FullHostCompatibilityBaseline
    };

    let started = std::time::Instant::now();
    let result = ManagedProcessService::run(request).await;
    let elapsed_ms = started.elapsed().as_millis();
    let (state, evidence, error, executed) = match result {
        Ok(result) => {
            let output = result.stdout.to_string_lossy();
            let state = if output.contains("C019_RESULT=allowed") {
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
            let error = (!result.exit_status.success()).then(|| result.stderr.to_string_lossy());
            if restricted {
                assert!(
                    matches!(
                        result.sandbox,
                        SandboxExecutionOutcome::TestOnlyAppContainer {
                            token_verified: true,
                            job_verified: true,
                            ..
                        }
                    ),
                    "selected backend was not verified: {result:?}"
                );
            } else {
                assert!(matches!(result.sandbox, SandboxExecutionOutcome::Disabled));
            }
            (state, output, error, true)
        }
        Err(error) => (State::Failed, String::new(), Some(error.to_string()), false),
    };
    Observation {
        backend,
        host_version: std::env::var("CODEGG_C019_HOST_VERSION")
            .unwrap_or_else(|_| "runner metadata unavailable".to_owned()),
        token: if restricted {
            "AppContainer package token verified; Job membership verified".to_owned()
        } else {
            "unrestricted managed child baseline".to_owned()
        },
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
        resource_identity: resource.display().to_string(),
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
    let stage = PathBuf::from(r"C:\__codegg_test_profile_temp__")
        .join(format!("c019-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir(&stage).expect("create unique disposable AppContainer stage");
    let host = fixture.path().join("original-host");
    let sibling = fixture.path().join("original-sibling");
    let outside_write = fixture.path().join("outside-stage-write.txt");
    std::fs::create_dir(&host).unwrap();
    std::fs::create_dir(&sibling).unwrap();
    let credential = host.join("synthetic-credential.txt");
    let sibling_secret = sibling.join("synthetic-sibling.txt");
    std::fs::write(&credential, "synthetic-c019-secret").unwrap();
    std::fs::write(&sibling_secret, "synthetic-c019-sibling").unwrap();
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
    };

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
        (Action::JunctionHostRead, Path::new("synthetic-junction")),
        (Action::HardlinkHostRead, hardlink.as_path()),
        (Action::AlternateStreamRead, ads.as_path()),
        (Action::SiblingHostRead, sibling_secret.as_path()),
        (Action::OutsideCreateWrite, outside_write.as_path()),
        (Action::PipeConnect, Path::new("synthetic-c019-pipe")),
        (Action::NullDeviceAccess, Path::new(r"\Device\Null")),
    ];
    let mut observations = Vec::with_capacity(actions.len());
    for (action, resource) in actions {
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
        token: "not observed; unsupported handle transfer probe".to_owned(),
        action: Action::InheritedHandleNotTransferred,
        state: State::NotAvailable,
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
    let summary = serde_json::json!({
        "contract": "codegg.windows_native_probe.v1",
        "host": "windows-hosted-runner",
        "observations": observations,
    });
    eprintln!("C019_REPORT={}", serde_json::to_string(&summary).unwrap());
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
    std::fs::remove_dir_all(&stage).expect("remove C019-owned stage after children exit");
}
