use mxc_sdk::v1::policy::FilesystemPolicy;
use mxc_sdk::v1::{self, ContainerRequest, Containment, WaitResult};
use serde_json::json;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(windows)]
use tokio::io::AsyncWriteExt;
#[cfg(windows)]
use tokio::net::windows::named_pipe::ServerOptions;
#[cfg(windows)]
use tokio::runtime::Runtime;
#[cfg(windows)]
use tokio::task::JoinHandle;

fn run_command(executable: &str, args: &[&str]) -> io::Result<String> {
    let output = Command::new(executable).args(args).output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "{executable} {args:?} exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn find_tool(name: &str) -> Option<PathBuf> {
    run_command("where.exe", &[name])
        .ok()?
        .lines()
        .map(PathBuf::from)
        .find(|path| path.is_file())
}

fn icacls_snapshot(path: &Path) -> io::Result<String> {
    let executable = format!(
        "{}\\System32\\icacls.exe",
        std::env::var("SystemRoot").map_err(io::Error::other)?
    );
    run_command(&executable, &[path.to_string_lossy().as_ref()])
}

fn batch_quote(path: &Path) -> String {
    format!("\"{}\"", path.display())
}

fn create_junction(link: &Path, target: &Path) -> io::Result<()> {
    let command =
        std::env::var("ComSpec").unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".into());
    run_command(
        &command,
        &[
            "/d",
            "/c",
            "mklink",
            "/J",
            &link.to_string_lossy(),
            &target.to_string_lossy(),
        ],
    )?;
    Ok(())
}

#[cfg(windows)]
fn start_pipe_fixture() -> io::Result<(Runtime, JoinHandle<()>, String)> {
    let pipe_name = format!("codegg-c020-{}", std::process::id());
    let pipe_path = format!(r"\\.\pipe\{pipe_name}");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let _runtime_guard = runtime.enter();
    let server = ServerOptions::new()
        .first_pipe_instance(true)
        .create(&pipe_path)?;
    drop(_runtime_guard);
    let task = runtime.spawn(async move {
        let mut server = server;
        if server.connect().await.is_ok() {
            let _ = server.write_all(b"synthetic-c020-pipe").await;
        }
    });
    Ok((runtime, task, pipe_path))
}

fn root_report(reason: &str, backends: &[impl std::fmt::Debug]) {
    println!(
        "C020_REPORT={}",
        json!({
            "contract": "codegg.mxc_v1_native_feasibility.v1",
            "result": "blocked_or_incompatible_before_child",
            "reason": reason,
            "available_backends": backends.iter().map(|backend| format!("{backend:?}")).collect::<Vec<_>>(),
            "network": "not configured and not claimed"
        })
    );
}

fn main() {
    if !cfg!(windows) {
        eprintln!("C020 harness must execute on Windows");
        std::process::exit(2);
    }

    let backends = v1::available_backends();
    println!("C020_MXC_BACKENDS={backends:#?}");
    println!("C020_MXC_PLATFORM={:#?}", v1::platform_support());

    let run_root = match std::env::var_os("RUNNER_TEMP") {
        Some(path) => PathBuf::from(path).join("codegg-c020-mxc-v1"),
        None => {
            root_report("RUNNER_TEMP is not set", &backends);
            return;
        }
    };
    if let Err(error) = fs::create_dir(&run_root) {
        root_report(&format!("create unique fixture root: {error}"), &backends);
        return;
    }
    let stage = run_root.join("stage");
    let host = run_root.join("original-host");
    let sibling = run_root.join("original-sibling");
    let outside = run_root.join("outside-stage-write.txt");
    let script_path = stage.join("probe.cmd");
    let secret = host.join("synthetic-c020-secret.txt");
    let sibling_secret = sibling.join("synthetic-c020-sibling.txt");
    let junction = run_root.join("host-junction");
    let hardlink = host.join("synthetic-c020-hardlink.txt");
    let ads = PathBuf::from(format!("{}:c020", secret.display()));
    let extended_secret = PathBuf::from(format!(r"\?{}", secret.display()));
    let setup = (|| -> io::Result<()> {
        fs::create_dir_all(&stage)?;
        fs::create_dir_all(stage.join("localappdata"))?;
        fs::create_dir_all(stage.join("appdata"))?;
        fs::create_dir_all(stage.join("profile"))?;
        fs::create_dir_all(&host)?;
        fs::create_dir_all(&sibling)?;
        fs::write(&secret, "synthetic-c020-secret")?;
        fs::write(&sibling_secret, "synthetic-c020-sibling")?;
        fs::hard_link(&secret, &hardlink)?;
        fs::write(&ads, "synthetic-c020-ads")?;
        create_junction(&junction, &host)?;
        Ok(())
    })();
    if let Err(error) = setup {
        root_report(&format!("synthetic fixture setup: {error}"), &backends);
        let _ = fs::remove_dir_all(&run_root);
        return;
    }
    #[cfg(windows)]
    let (pipe_runtime, pipe_task, pipe_path) = match start_pipe_fixture() {
        Ok(fixture) => fixture,
        Err(error) => {
            root_report(
                &format!("synthetic named-pipe fixture setup: {error}"),
                &backends,
            );
            let _ = fs::remove_dir_all(&run_root);
            return;
        }
    };
    #[cfg(not(windows))]
    let pipe_path = String::new();

    let git = find_tool("git.exe");
    let python = find_tool("python.exe");
    let cargo = find_tool("cargo.exe");
    let mut lines = vec![
        "@echo off".to_owned(),
        format!("echo stage-ok>{}", batch_quote(&stage.join("stage-write.txt"))),
        format!("type {} >nul 2>&1 && echo direct_read=leaked || echo direct_read=denied", batch_quote(&secret)),
        format!("type {} >nul 2>&1 && echo junction_read=leaked || echo junction_read=denied", batch_quote(&junction.join("synthetic-c020-secret.txt"))),
        format!("type {} >nul 2>&1 && echo case_alias_read=leaked || echo case_alias_read=denied", batch_quote(&host.join("SYNTHETIC-C020-SECRET.TXT"))),
        format!("type {} >nul 2>&1 && echo hardlink_read=leaked || echo hardlink_read=denied", batch_quote(&hardlink)),
        format!("type {} >nul 2>&1 && echo ads_read=leaked || echo ads_read=denied", batch_quote(&ads)),
        format!("type {} >nul 2>&1 && echo extended_path_read=leaked || echo extended_path_read=denied", batch_quote(&extended_secret)),
        format!("type {} >nul 2>&1 && echo sibling_read=leaked || echo sibling_read=denied", batch_quote(&sibling_secret)),
        format!("echo synthetic-write>{} 2>nul && echo outside_write=leaked || echo outside_write=denied", batch_quote(&outside)),
        format!("type {} >nul 2>&1 && echo pipe=leaked || echo pipe=denied", batch_quote(Path::new(&pipe_path))),
    ];
    if let Some(git) = &git {
        let repo = stage.join("git-init");
        lines.push(format!(
            "{} init --quiet {}",
            batch_quote(git),
            batch_quote(&repo)
        ));
        lines.push("if errorlevel 1 (echo git_init=failed) else (echo git_init=allowed)".into());
        lines.push(format!(
            "{} -C {} status --porcelain",
            batch_quote(git),
            batch_quote(&repo)
        ));
        lines
            .push("if errorlevel 1 (echo git_status=failed) else (echo git_status=allowed)".into());
    } else {
        lines.push("echo git_init=not_available".into());
    }
    for (name, tool) in [("python", python.as_ref()), ("cargo", cargo.as_ref())] {
        if let Some(tool) = tool {
            lines.push(format!("{} --version", batch_quote(tool)));
            lines.push(format!(
                "if errorlevel 1 (echo {name}=failed) else (echo {name}=allowed)"
            ));
        } else {
            lines.push(format!("echo {name}=not_available"));
        }
    }
    if let Err(error) = fs::write(&script_path, lines.join("\r\n")) {
        root_report(&format!("write stage probe script: {error}"), &backends);
        let _ = fs::remove_dir_all(&run_root);
        return;
    }
    let host_before =
        icacls_snapshot(&host).unwrap_or_else(|error| format!("snapshot failed: {error}"));
    let secret_acl_before =
        icacls_snapshot(&secret).unwrap_or_else(|error| format!("snapshot failed: {error}"));
    let secret_before = fs::read(&secret).unwrap_or_default();
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let system32 = PathBuf::from(&system_root).join("System32");
    let mut readonly_paths = vec![system32.to_string_lossy().into_owned()];
    for tool in [&git, &python, &cargo].into_iter().flatten() {
        if let Some(parent) = tool.parent() {
            readonly_paths.push(parent.to_string_lossy().into_owned());
        }
    }
    let path_value = readonly_paths.iter().cloned().collect::<Vec<_>>().join(";");
    readonly_paths.sort();
    readonly_paths.dedup();
    let policy = FilesystemPolicy {
        readwrite_paths: vec![stage.to_string_lossy().into_owned()],
        readonly_paths,
        denied_paths: vec![
            host.to_string_lossy().into_owned(),
            sibling.to_string_lossy().into_owned(),
        ],
        clear_policy_on_exit: Some(true),
    };
    let command = format!(
        "{} /d /c call {}",
        batch_quote(&system32.join("cmd.exe")),
        batch_quote(&script_path)
    );
    let request = ContainerRequest {
        containment: Containment::ProcessContainer(Default::default()),
        filesystem: Some(policy),
        working_directory: Some(stage.to_string_lossy().into_owned()),
        timeout_ms: Some(60_000),
        environment: Some(vec![
            (
                "SystemDrive".into(),
                std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()),
            ),
            ("SystemRoot".into(), system_root.clone()),
            ("WINDIR".into(), system_root.clone()),
            ("USERPROFILE".into(), stage.join("profile").to_string_lossy().into_owned()),
            ("LOCALAPPDATA".into(), stage.join("localappdata").to_string_lossy().into_owned()),
            ("APPDATA".into(), stage.join("appdata").to_string_lossy().into_owned()),
            ("TEMP".into(), stage.to_string_lossy().into_owned()),
            ("TMP".into(), stage.to_string_lossy().into_owned()),
            ("PATH".into(), path_value),
        ]),
        inherit_default_environment: Some(false),
        ..ContainerRequest::new(command)
    };
    #[cfg(windows)]
    println!("C020_MXC_PROBE={:#?}", v1::probe(Some(&request)));
    #[cfg(not(windows))]
    println!("C020_MXC_PROBE=Windows-only probe; not run on this target");
    let execution = v1::run(request, Default::default());
    #[cfg(windows)]
    pipe_task.abort();
    #[cfg(windows)]
    drop(pipe_runtime);
    let host_after =
        icacls_snapshot(&host).unwrap_or_else(|error| format!("snapshot failed: {error}"));
    let secret_acl_after =
        icacls_snapshot(&secret).unwrap_or_else(|error| format!("snapshot failed: {error}"));
    let secret_after = fs::read(&secret).unwrap_or_default();
    let host_acl_unchanged = host_before == host_after;
    let secret_acl_unchanged = secret_acl_before == secret_acl_after;
    let secret_unchanged = secret_before == secret_after;
    match execution {
        Ok(result) => {
            let stdout = String::from_utf8_lossy(&result.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&result.stderr).into_owned();
            let report = json!({
                "contract": "codegg.mxc_v1_native_feasibility.v1",
                "runner_temp": run_root.display().to_string(),
                "containment_requested": "ProcessContainer",
                "available_backends": backends.iter().map(|backend| format!("{backend:?}")).collect::<Vec<_>>(),
                "result": format!("{:?}", result.outcome),
                "warnings": result.warnings,
                "stdout": stdout,
                "stderr": stderr,
                "git": git.map(|path| path.display().to_string()),
                "python": python.map(|path| path.display().to_string()),
                "cargo": cargo.map(|path| path.display().to_string()),
                "host_acl_before": host_before,
                "host_acl_after": host_after,
                "host_acl_unchanged": host_acl_unchanged,
                "secret_acl_before": secret_acl_before,
                "secret_acl_after": secret_acl_after,
                "secret_acl_unchanged": secret_acl_unchanged,
                "secret_unchanged": secret_unchanged,
                "unsupported": ["UNC fixture", "hostile inherited handle transfer", "independent descendant/cancellation trace"],
                "network": "not configured and not claimed"
            });
            println!("C020_REPORT={report}");
            if !matches!(result.outcome, WaitResult::Exited(0)) {
                eprintln!("C020 MXC child did not exit successfully; preserve report as negative/blocked evidence.");
            }
        }
        Err(error) => {
            println!(
                "C020_REPORT={}",
                json!({
                    "contract": "codegg.mxc_v1_native_feasibility.v1",
                    "runner_temp": run_root.display().to_string(),
                    "result": "execution_error",
                    "error": error.to_string(),
                    "available_backends": backends.iter().map(|backend| format!("{backend:?}")).collect::<Vec<_>>(),
                    "host_acl_before": host_before,
                    "host_acl_after": host_after,
                    "host_acl_unchanged": host_acl_unchanged,
                    "secret_acl_before": secret_acl_before,
                    "secret_acl_after": secret_acl_after,
                    "secret_acl_unchanged": secret_acl_unchanged,
                    "secret_unchanged": secret_unchanged,
                    "network": "not configured and not claimed"
                })
            );
        }
    }
    let _ = fs::remove_dir_all(&run_root);
}
