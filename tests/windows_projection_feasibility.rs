#![cfg(all(windows, feature = "windows-sandbox-test-support"))]

use codegg::managed_process::{
    ManagedProcessRequest, ManagedProcessService, ProcessProvenance, SandboxExecutionOutcome,
    SandboxRequest,
};
use codegg::security::sandbox::SandboxLaunchSpec;
use std::ffi::OsString;
use std::fs::File;
use std::os::windows::io::AsRawHandle;
use std::path::PathBuf;
use std::time::Duration;
use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT};

fn powershell() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .expect("SystemRoot is set")
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe")
}

fn find_git() -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|directory| directory.join("git.exe"))
        .find(|candidate| candidate.is_file())
}

fn probe(variable: &str, label: &str) -> String {
    format!(
        "$p = [Environment]::GetEnvironmentVariable('{}'); try {{ $null = Get-Content -LiteralPath $p -Raw -ErrorAction Stop; Write-Output '{}=readable' }} catch {{ Write-Output '{}=denied:$($_.Exception.GetType().Name)' }}",
        variable,
        label,
        label,
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn c014_projection_tool_smoke_and_unprojected_host_denials() {
    let git = find_git().expect("C014 requires Git on the Windows runner");
    let fixture = tempfile::tempdir().expect("C014 disposable fixture");
    let projection = PathBuf::from(r"C:\__codegg_test_profile_temp__");
    let host = fixture.path().join("unprojected-host");
    let sibling = fixture.path().join("ungranted-sibling");
    std::fs::create_dir(&host).unwrap();
    std::fs::create_dir(&sibling).unwrap();

    let credential = host.join("synthetic-credential.txt");
    let protected_alias_target = host.join("protected-target.txt");
    let sibling_secret = sibling.join("sibling-secret.txt");
    std::fs::write(&credential, "synthetic-c014-credential").unwrap();
    std::fs::write(&protected_alias_target, "synthetic-c014-protected").unwrap();
    std::fs::write(&sibling_secret, "synthetic-c014-sibling").unwrap();
    let hardlink = host.join("hardlink.txt");
    std::fs::hard_link(&credential, &hardlink).expect("create synthetic NTFS hardlink");
    let ads = PathBuf::from(format!("{}:c014", credential.display()));
    std::fs::write(&ads, "synthetic-c014-ads").expect("create synthetic NTFS ADS");
    let inherited_candidate =
        File::open(&credential).expect("open synthetic inherited-handle target");
    assert_ne!(
        unsafe {
            SetHandleInformation(
                inherited_candidate.as_raw_handle() as HANDLE,
                HANDLE_FLAG_INHERIT,
                HANDLE_FLAG_INHERIT,
            )
        },
        0,
        "mark synthetic credential handle inheritable for C014 probe"
    );
    let junction = fixture.path().join("credential-junction");
    let junction_result = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&junction)
        .arg(&host)
        .output()
        .expect("cmd.exe is available");
    assert!(
        junction_result.status.success(),
        "could not create synthetic junction: {}",
        String::from_utf8_lossy(&junction_result.stderr)
    );
    let junction_credential = junction.join("synthetic-credential.txt");

    let pipe_name = format!("codegg-c014-{}", uuid::Uuid::new_v4().simple());
    let pipe_path = format!(r"\\.\pipe\{pipe_name}");
    let _pipe_server = tokio::net::windows::named_pipe::ServerOptions::new()
        .first_pipe_instance(true)
        .create(&pipe_path)
        .expect("create synthetic daemon-pipe fixture");

    let script = format!(
        r#"
$ErrorActionPreference = 'Stop'
$projectFile = Join-Path $env:CODEGG_C014_PROJECTION 'staged-input.txt'
Set-Content -LiteralPath $projectFile -Value 'staged-c014-content' -NoNewline
$readBack = Get-Content -LiteralPath $projectFile -Raw
if ($readBack -ne 'staged-c014-content') {{ throw 'projection read/write mismatch' }}
Write-Output 'projection=read-write-ok'
& $env:CODEGG_C014_GIT init --quiet $env:CODEGG_C014_PROJECTION
if ($LASTEXITCODE -ne 0) {{ throw "git init failed: $LASTEXITCODE" }}
& $env:CODEGG_C014_GIT -C $env:CODEGG_C014_PROJECTION status --porcelain
if ($LASTEXITCODE -ne 0) {{ throw "git status failed: $LASTEXITCODE" }}
Write-Output 'git=repository-status-ok'
{}
{}
{}
{}
{}
$pipe = [System.IO.Pipes.NamedPipeClientStream]::new('.', '{}', [System.IO.Pipes.PipeDirection]::InOut, [System.IO.Pipes.PipeOptions]::None)
try {{ $pipe.Connect(1000); Write-Output 'daemon-pipe=connected' }} catch {{ Write-Output "daemon-pipe=denied:$($_.Exception.GetType().Name)" }} finally {{ $pipe.Dispose() }}
$rawHandle = [IntPtr]([Int64]$env:C014_INHERITED_HANDLE)
try {{ $safeHandle = [Microsoft.Win32.SafeHandles.SafeFileHandle]::new($rawHandle, $false); $stream = [System.IO.FileStream]::new($safeHandle, [System.IO.FileAccess]::Read); $contents = [System.IO.StreamReader]::new($stream).ReadToEnd(); if ($contents.Contains('synthetic-c014-credential')) {{ Write-Output 'inherited-handle=leaked' }} else {{ Write-Output 'inherited-handle=unavailable' }} }} catch {{ Write-Output "inherited-handle=denied:$($_.Exception.GetType().Name)" }}
"#,
        probe("C014_CREDENTIAL", "direct"),
        probe("C014_JUNCTION", "junction"),
        probe("C014_HARDLINK", "hardlink"),
        probe("C014_ADS", "ads"),
        probe("C014_SIBLING", "sibling"),
        pipe_name,
    );
    let executable = powershell();
    let args = vec![
        OsString::from("-NoLogo"),
        OsString::from("-NoProfile"),
        OsString::from("-NonInteractive"),
        OsString::from("-Command"),
        OsString::from(script),
    ];
    let mut request = ManagedProcessRequest::new(
        std::iter::once(executable.clone().into_os_string())
            .chain(args.iter().cloned())
            .collect(),
        projection.clone(),
        ProcessProvenance::new("windows-c014", "projection-feasibility"),
    );
    request.timeout = Some(Duration::from_secs(30));
    request.environment_policy = codegg::managed_process::EnvironmentPolicy::sanitized()
        .with_var("CODEGG_C014_PROJECTION", projection.as_os_str())
        .with_var("CODEGG_C014_GIT", git.as_os_str())
        .with_var("HOME", projection.as_os_str())
        .with_var("GIT_CONFIG_NOSYSTEM", "1")
        .with_var(
            "GIT_CONFIG_GLOBAL",
            projection.join("gitconfig").as_os_str(),
        )
        .with_var("C014_CREDENTIAL", credential.as_os_str())
        .with_var("C014_JUNCTION", junction_credential.as_os_str())
        .with_var("C014_HARDLINK", hardlink.as_os_str())
        .with_var("C014_ADS", ads.as_os_str())
        .with_var("C014_SIBLING", sibling_secret.as_os_str())
        .with_var(
            "C014_INHERITED_HANDLE",
            (inherited_candidate.as_raw_handle() as usize).to_string(),
        );
    request.sandbox = SandboxRequest::Required(SandboxLaunchSpec {
        target: executable,
        args: args
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect(),
        read_paths: Vec::new(),
        write_paths: vec![projection],
        deny_paths: Vec::new(),
    });

    let started = std::time::Instant::now();
    let result = ManagedProcessService::run(request)
        .await
        .expect("restricted projection feasibility child should run");
    let elapsed = started.elapsed();
    let output = result.stdout.to_string_lossy();
    eprintln!("C014 elapsed_ms={}", elapsed.as_millis());
    eprintln!("C014 child stdout:\n{output}");
    eprintln!("C014 child stderr:\n{}", result.stderr.to_string_lossy());
    assert!(
        result.exit_status.success(),
        "{output}\n{}",
        result.stderr.to_string_lossy()
    );
    assert!(output.contains("projection=read-write-ok"), "{output}");
    assert!(output.contains("git=repository-status-ok"), "{output}");
    for label in ["direct", "junction", "hardlink", "ads", "sibling"] {
        assert!(
            output.contains(&format!("{label}=denied:")),
            "C014 host read was not denied for {label}: {output}"
        );
    }
    assert!(
        output.contains("daemon-pipe=denied:"),
        "C014 synthetic daemon pipe was reachable: {output}"
    );
    assert!(
        output.contains("inherited-handle=denied:")
            || output.contains("inherited-handle=unavailable"),
        "C014 inherited host handle was reachable: {output}"
    );
    assert_ne!(
        unsafe {
            SetHandleInformation(
                inherited_candidate.as_raw_handle() as HANDLE,
                HANDLE_FLAG_INHERIT,
                0,
            )
        },
        0,
        "clear synthetic credential handle inheritance after probe"
    );
    assert!(matches!(
        result.sandbox,
        SandboxExecutionOutcome::TestOnlyAppContainer {
            token_verified: true,
            job_verified: true,
            ..
        }
    ));
    assert!(!result.sandbox.is_contained());
}
