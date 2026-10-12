#![cfg(all(windows, feature = "windows-sandbox-test-support"))]

use codegg::managed_process::{
    ManagedProcessRequest, ManagedProcessService, ProcessProvenance, SandboxExecutionOutcome,
    SandboxRequest,
};
use codegg::security::sandbox::SandboxLaunchSpec;
use std::ffi::OsString;
use std::path::PathBuf;

fn powershell() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .expect("SystemRoot is set")
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn appcontainer_cannot_read_preexisting_secret_through_reparse_hardlink_or_ads() {
    let fixture = tempfile::tempdir().unwrap();
    let workspace = fixture.path().join("existing-workspace");
    let credentials = fixture.path().join("existing-credentials");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&credentials).unwrap();
    let allowed_file = workspace.join("existing-source.txt");
    let secret_file = credentials.join("credential.json");
    std::fs::write(&allowed_file, "workspace-readable").unwrap();
    std::fs::write(&secret_file, "synthetic-access-token").unwrap();

    let hardlink = workspace.join("linked-credential.json");
    std::fs::hard_link(&secret_file, &hardlink).expect("NTFS hardlink fixture creation");
    let alternate_stream = PathBuf::from(format!("{}:api-token", secret_file.display()));
    std::fs::write(&alternate_stream, "synthetic-ads-token").expect("NTFS ADS fixture creation");

    let junction = workspace.join("credential-junction");
    let junction_result = std::process::Command::new("cmd.exe")
        .args([
            OsString::from("/d"),
            OsString::from("/c"),
            OsString::from("mklink"),
            OsString::from("/J"),
            junction.as_os_str().to_owned(),
            credentials.as_os_str().to_owned(),
        ])
        .output()
        .expect("cmd.exe is present on the Windows runner");
    assert!(
        junction_result.status.success(),
        "junction fixture creation failed: {}{}",
        String::from_utf8_lossy(&junction_result.stdout),
        String::from_utf8_lossy(&junction_result.stderr)
    );

    let workspace_before =
        codegg::managed_process::windows_acl_lease_dacl_snapshot(&workspace).unwrap();
    let credentials_before =
        codegg::managed_process::windows_acl_lease_dacl_snapshot(&credentials).unwrap();
    let executable = powershell();
    let script = r#"
$paths = @(
    $env:CODEGG_ADV_CREDENTIAL,
    $env:CODEGG_ADV_JUNCTION_CREDENTIAL,
    $env:CODEGG_ADV_HARDLINK_CREDENTIAL,
    $env:CODEGG_ADV_ALTERNATE_STREAM
)
$results = foreach ($path in $paths) {
    try {
        [IO.File]::ReadAllText($path) | Out-Null
        'leaked'
    } catch {
        'denied'
    }
}
$allowed = [IO.File]::ReadAllText($env:CODEGG_ADV_ALLOWED).Trim()
$newFile = [IO.Path]::Combine($env:CODEGG_ADV_WORKSPACE, 'created-by-child.txt')
[IO.File]::WriteAllText($newFile, 'workspace-write')
$written = [IO.File]::ReadAllText($newFile).Trim()
[Console]::Out.Write(($results -join '|') + '|' + $allowed + '|' + $written)
"#;
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
        std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .expect("SystemRoot is set")
            .join("System32"),
        ProcessProvenance::new("windows-sandbox-adversarial", "path-escape-matrix"),
    );
    request.sandbox = SandboxRequest::Required(SandboxLaunchSpec {
        target: executable,
        args: args
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect(),
        read_paths: Vec::new(),
        write_paths: vec![workspace.clone()],
        deny_paths: vec![credentials.clone()],
    });
    request.environment_policy = codegg::managed_process::EnvironmentPolicy::sanitized()
        .with_var("CODEGG_ADV_ALLOWED", allowed_file.as_os_str())
        .with_var("CODEGG_ADV_WORKSPACE", workspace.as_os_str())
        .with_var("CODEGG_ADV_CREDENTIAL", secret_file.as_os_str())
        .with_var(
            "CODEGG_ADV_JUNCTION_CREDENTIAL",
            junction.join("credential.json").as_os_str(),
        )
        .with_var("CODEGG_ADV_HARDLINK_CREDENTIAL", hardlink.as_os_str())
        .with_var("CODEGG_ADV_ALTERNATE_STREAM", alternate_stream.as_os_str());

    let result = ManagedProcessService::run(request)
        .await
        .expect("restricted child runs against existing disposable roots");
    assert!(
        result.exit_status.success(),
        "{}",
        result.stderr.to_string_lossy()
    );
    assert_eq!(
        result.stdout.to_string_lossy(),
        "denied|denied|denied|denied|workspace-readable|workspace-write"
    );
    assert!(matches!(
        &result.sandbox,
        SandboxExecutionOutcome::TestOnlyAppContainer {
            token_verified: true,
            job_verified: true,
            ..
        }
    ));
    assert!(result.cleanup.job_object_established);
    assert!(!result.sandbox.is_contained());
    assert_eq!(
        std::fs::read_to_string(workspace.join("created-by-child.txt")).unwrap(),
        "workspace-write"
    );
    assert_eq!(
        codegg::managed_process::windows_acl_lease_dacl_snapshot(&workspace).unwrap(),
        workspace_before
    );
    assert_eq!(
        codegg::managed_process::windows_acl_lease_dacl_snapshot(&credentials).unwrap(),
        credentials_before
    );
}
