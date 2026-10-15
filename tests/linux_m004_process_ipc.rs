#![cfg(target_os = "linux")]

use std::ffi::OsString;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use codegg::managed_process::{
    ManagedProcessRequest, ManagedProcessService, ProcessProvenance, SandboxExecutionOutcome,
    SandboxRequest,
};
use codegg::security::sandbox::{SandboxConfig, SandboxMode};

const OPT_IN: &str = "CODEGG_RUN_M004_BWRAP_NATIVE";

#[test]
fn process_view_compiles_pid_ipc_and_proc_in_safe_mount_order() {
    let workspace = tempfile::tempdir().expect("workspace");
    let spec = SandboxConfig::new()
        .with_enabled(true)
        .with_mode(SandboxMode::WorkspaceWrite)
        .with_linux_process_isolation(true)
        .launch_spec(
            "/bin/sh",
            &["-c".into(), "true".into()],
            Some(workspace.path()),
        )
        .expect("M004 launch spec");
    let (args, _) =
        codegg::security::sandbox::linux_mount_view::compile(&spec, workspace.path(), |_| Ok(256))
            .expect("compile M004 mount view");

    assert!(args.iter().any(|arg| arg == "--unshare-pid"));
    assert!(args.iter().any(|arg| arg == "--unshare-ipc"));
    let proc_dir = args
        .windows(2)
        .position(|pair| pair[0] == "--dir" && pair[1] == "/proc")
        .expect("proc mountpoint directory");
    let proc_mount = args
        .windows(2)
        .position(|pair| pair[0] == "--proc" && pair[1] == "/proc")
        .expect("private proc mount");
    assert!(
        proc_dir < proc_mount,
        "proc mount must follow mountpoint creation"
    );
}

fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn process_tier_hides_host_proc_and_blocks_host_signal_and_abstract_socket() {
    if std::env::var(OPT_IN).as_deref() != Ok("1") {
        eprintln!("skipped: set {OPT_IN}=1 to run the native M004 child probe");
        return;
    }
    if let Err(reason) = codegg::security::sandbox::landlock::probe_process_scopes() {
        let workspace = tempfile::tempdir().expect("workspace");
        let marker = workspace.path().join("target-ran");
        let spec = SandboxConfig::new()
            .with_enabled(true)
            .with_mode(SandboxMode::WorkspaceWrite)
            .with_linux_process_isolation(true)
            .launch_spec(
                "/bin/sh",
                &[
                    "-c".into(),
                    format!("touch {}", shell_quote(&marker.to_string_lossy())),
                ],
                Some(workspace.path()),
            )
            .expect("M004 launch spec");
        let mut request = ManagedProcessRequest::new(
            [spec.target.as_os_str().to_owned()]
                .into_iter()
                .chain(spec.args.iter().map(OsString::from))
                .collect(),
            workspace.path().to_path_buf(),
            ProcessProvenance::new("linux-m004", "unsupported-host-probe"),
        );
        request.sandbox = SandboxRequest::Required(spec);
        let error = ManagedProcessService::run(request)
            .await
            .expect_err("required M004 must refuse on a host without Landlock ABI 6");
        assert!(error.to_string().contains("process-ipc-landlock-abi-6"));
        assert!(!marker.exists(), "unsupported M004 request ran its target");
        eprintln!("M004 positive probes unavailable on this host: {reason}");
        return;
    }

    let ambient_file = tempfile::NamedTempFile::new().expect("ambient descriptor fixture");
    let ambient_fd = unsafe { libc::fcntl(ambient_file.as_raw_fd(), libc::F_DUPFD, 256) };
    assert!(
        ambient_fd >= 256,
        "duplicate ambient fd: {}",
        std::io::Error::last_os_error()
    );
    let ambient_fd = unsafe { OwnedFd::from_raw_fd(ambient_fd) };
    assert_eq!(
        unsafe { libc::fcntl(ambient_fd.as_raw_fd(), libc::F_SETFD, 0) },
        0,
        "make ambient fixture inheritable"
    );

    let workspace = tempfile::tempdir().expect("workspace");
    let socket_name = format!("codegg-m004-{}", std::process::id());
    let signal_marker = workspace.path().join("host-signal-received");
    let server_script = format!(
        "import socket,time; s=socket.socket(socket.AF_UNIX); s.bind('\\0{socket_name}'); s.listen(); time.sleep(30)"
    );
    let mut socket_server = Command::new("python3")
        .arg("-c")
        .arg(&server_script)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start host abstract-socket fixture");

    let signal_script = format!(
        "import signal,time; signal.signal(signal.SIGUSR1, lambda *_: open({:?}, 'w').write('received')); time.sleep(30)",
        signal_marker.to_string_lossy()
    );
    let mut signal_target = Command::new("python3")
        .arg("-c")
        .arg(&signal_script)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start host signal fixture");

    let probe_script = format!(
        "import os,socket,signal,sys\n\
         s=socket.socket(socket.AF_UNIX)\n\
         try: s.connect('\\0{socket_name}'); print('abstract-socket-allowed'); sys.exit(10)\n\
         except PermissionError: print('abstract-socket-denied')\n\
         except OSError as e: print('abstract-socket-error:'+str(e.errno)); sys.exit(11)\n\
         try: os.kill({signal_pid}, signal.SIGUSR1); print('signal-allowed'); sys.exit(12)\n\
         except PermissionError: print('signal-denied')\n\
         print('host-proc-hidden' if not os.path.exists('/proc/{host_pid}') else 'host-proc-visible')\n\
         print('ambient-fd-closed' if not os.path.exists('/proc/self/fd/{ambient_fd}') else 'ambient-fd-open')\n",
        signal_pid = signal_target.id(),
        host_pid = std::process::id(),
        ambient_fd = ambient_fd.as_raw_fd()
    );

    let sandbox = SandboxConfig::new()
        .with_enabled(true)
        .with_mode(SandboxMode::WorkspaceWrite)
        .with_linux_process_isolation(true);
    let spec = sandbox
        .launch_spec(
            "/bin/sh",
            &[
                "-c".into(),
                format!("python3 -c {}", shell_quote(&probe_script)),
            ],
            Some(workspace.path()),
        )
        .expect("M004 launch spec");
    let mut request = ManagedProcessRequest::new(
        [spec.target.as_os_str().to_owned()]
            .into_iter()
            .chain(spec.args.iter().map(OsString::from))
            .collect(),
        workspace.path().to_path_buf(),
        ProcessProvenance::new("linux-m004", "process-ipc-probe"),
    );
    request.timeout = Some(Duration::from_secs(15));
    request.sandbox = SandboxRequest::Required(spec);

    let result = ManagedProcessService::run(request).await;
    stop(&mut socket_server);
    stop(&mut signal_target);
    let result = result.expect("required M004 process tier should launch");
    assert!(
        result.exit_status.success(),
        "stderr: {}",
        result.stderr.to_string_lossy()
    );
    let stdout = result.stdout.to_string_lossy();
    assert!(stdout.contains("abstract-socket-denied"), "{stdout}");
    assert!(stdout.contains("signal-denied"), "{stdout}");
    assert!(stdout.contains("host-proc-hidden"), "{stdout}");
    assert!(stdout.contains("ambient-fd-closed"), "{stdout}");
    assert!(
        !signal_marker.exists(),
        "host fixture received a sandbox signal"
    );
    match result.sandbox {
        SandboxExecutionOutcome::Enforced {
            guarantees, limits, ..
        } => {
            assert!(guarantees
                .iter()
                .any(|item| item.contains("private procfs")));
            assert!(guarantees
                .iter()
                .any(|item| item.contains("abstract Unix socket")));
            assert!(limits
                .iter()
                .any(|item| item.contains("outbound networking")));
            assert!(limits.iter().any(|item| item.contains("seccomp")));
        }
        other => panic!("expected enforced M004 result, got {other:?}"),
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
