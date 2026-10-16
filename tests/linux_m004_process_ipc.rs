#![cfg(target_os = "linux")]

use std::ffi::OsString;
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use codegg::managed_process::{
    EnvironmentPolicy, ManagedProcessRequest, ManagedProcessService, ProcessProvenance,
    SandboxExecutionOutcome, SandboxRequest,
};
use codegg::security::sandbox::{SandboxConfig, SandboxMode};

const OPT_IN: &str = "CODEGG_RUN_M004_BWRAP_NATIVE";
const RESTRICTED_USERNS_OPT_IN: &str = "CODEGG_RUN_M004_RESTRICTED_USERNS";
const M005_OPT_IN: &str = "CODEGG_RUN_M005_OFFLINE_NATIVE";

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

#[test]
fn offline_network_compiles_private_network_namespace_with_m004() {
    let workspace = tempfile::tempdir().expect("workspace");
    let spec = SandboxConfig::new()
        .with_enabled(true)
        .with_mode(SandboxMode::WorkspaceWrite)
        .with_linux_offline_network(true)
        .launch_spec(
            "/bin/sh",
            &["-c".into(), "true".into()],
            Some(workspace.path()),
        )
        .expect("offline M005 launch spec");
    let (args, _) =
        codegg::security::sandbox::linux_mount_view::compile(&spec, workspace.path(), |_| Ok(256))
            .expect("compile offline M005 mount view");

    assert!(spec.linux_process_isolation);
    assert!(args.iter().any(|arg| arg == "--unshare-pid"));
    assert!(args.iter().any(|arg| arg == "--unshare-ipc"));
    assert!(args.iter().any(|arg| arg == "--unshare-net"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_network_blocks_host_network_and_inherited_socket_access() {
    if std::env::var(M005_OPT_IN).as_deref() != Ok("1") {
        eprintln!("skipped: set {M005_OPT_IN}=1 to run the native M005 child probe");
        return;
    }
    if let Err(reason) = codegg::security::sandbox::landlock::probe_process_scopes() {
        panic!("M005 candidate requires qualified M004 process/IPC setup: {reason}");
    }

    let tcp4 = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("host IPv4 listener");
    let tcp4_addr = tcp4.local_addr().expect("IPv4 listener address");
    let tcp6 = std::net::TcpListener::bind(("::1", 0)).expect("host IPv6 listener");
    let tcp6_addr = tcp6.local_addr().expect("IPv6 listener address");
    let udp4 = std::net::UdpSocket::bind(("127.0.0.1", 0)).expect("host IPv4 UDP listener");
    udp4.set_read_timeout(Some(Duration::from_secs(2)))
        .expect("set IPv4 UDP timeout");
    let udp4_addr = udp4.local_addr().expect("IPv4 UDP address");
    let udp6 = std::net::UdpSocket::bind(("::1", 0)).expect("host IPv6 UDP listener");
    udp6.set_read_timeout(Some(Duration::from_secs(2)))
        .expect("set IPv6 UDP timeout");
    let udp6_addr = udp6.local_addr().expect("IPv6 UDP address");

    // Keep a host-connected TCP socket open across the launch. M004's
    // close-range handoff must remove this bridge before the helper execs.
    let inherited_stream = std::net::TcpStream::connect(tcp4_addr).expect("inherited host socket");
    let mut inherited_peer = tcp4.accept().expect("accept inherited host socket").0;
    inherited_peer
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("set inherited peer timeout");
    let inherited_fd = unsafe { libc::fcntl(inherited_stream.as_raw_fd(), libc::F_DUPFD, 256) };
    assert!(inherited_fd >= 256, "duplicate inherited socket descriptor");
    let inherited_fd = unsafe { OwnedFd::from_raw_fd(inherited_fd) };
    assert_eq!(
        unsafe { libc::fcntl(inherited_fd.as_raw_fd(), libc::F_SETFD, 0) },
        0,
        "make inherited network descriptor inheritable"
    );

    let workspace = tempfile::tempdir().expect("workspace");
    let marker = workspace.path().join("target-ran");
    let probe = format!(
        "import os, socket\n\
         probes = [\n\
           (socket.AF_INET, ({:?}, {})),\n\
           (socket.AF_INET6, ({:?}, {})),\n\
         ]\n\
         for family, address in probes:\n\
           s = socket.socket(family, socket.SOCK_STREAM); s.settimeout(1)\n\
           if s.connect_ex(address) == 0: raise SystemExit(41)\n\
         for family, address in [(socket.AF_INET, ({:?}, {})), (socket.AF_INET6, ({:?}, {}))]:\n\
           s = socket.socket(family, socket.SOCK_DGRAM)\n\
           try: s.sendto(b'network-leak', address)\n\
           except OSError: pass\n\
         try:\n\
           socket.getaddrinfo('codegg-m005-probe.invalid', 443, type=socket.SOCK_STREAM)\n\
           raise SystemExit(42)\n\
         except OSError: pass\n\
         external = socket.socket(); external.settimeout(1)\n\
         if external.connect_ex(('198.51.100.1', 443)) == 0: raise SystemExit(43)\n\
         fd = int(os.environ['CODEGG_M005_INHERITED_FD'])\n\
         try: os.fstat(fd)\n\
         except OSError: open({:?}, 'w').write('offline')\n\
         else:\n\
           os.write(fd, b'inherited-socket-leak')\n\
           raise SystemExit(44)",
        tcp4_addr.ip().to_string(),
        tcp4_addr.port(),
        tcp6_addr.ip().to_string(),
        tcp6_addr.port(),
        udp4_addr.ip().to_string(),
        udp4_addr.port(),
        udp6_addr.ip().to_string(),
        udp6_addr.port(),
        marker.to_string_lossy(),
    );
    let spec = SandboxConfig::new()
        .with_enabled(true)
        .with_mode(SandboxMode::WorkspaceWrite)
        .with_linux_offline_network(true)
        .launch_spec("python3", &["-c".into(), probe], Some(workspace.path()))
        .expect("offline M005 launch spec");
    let mut request = ManagedProcessRequest::new(
        [spec.target.as_os_str().to_owned()]
            .into_iter()
            .chain(spec.args.iter().map(OsString::from))
            .collect(),
        workspace.path().to_path_buf(),
        ProcessProvenance::new("linux-m005", "offline-loopback-probe"),
    );
    request.timeout = Some(Duration::from_secs(15));
    request.environment_policy = EnvironmentPolicy::sanitized().with_var(
        "CODEGG_M005_INHERITED_FD",
        inherited_fd.as_raw_fd().to_string(),
    );
    request.sandbox = SandboxRequest::Required(spec);

    let result = ManagedProcessService::run(request)
        .await
        .expect("required offline network namespace should launch");
    assert!(
        result.exit_status.success(),
        "child reached host loopback or failed: {}",
        result.stderr.to_string_lossy()
    );
    assert_eq!(
        std::fs::read_to_string(marker).expect("offline target marker"),
        "offline"
    );
    let mut datagram = [0_u8; 128];
    assert!(
        udp4.recv_from(&mut datagram).is_err(),
        "offline child reached host IPv4 UDP listener"
    );
    assert!(
        udp6.recv_from(&mut datagram).is_err(),
        "offline child reached host IPv6 UDP listener"
    );
    let mut inherited_data = [0_u8; 128];
    assert!(
        inherited_peer.read(&mut inherited_data).is_err(),
        "offline child communicated through an inherited host socket"
    );
    drop((tcp4, tcp6, inherited_stream));
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
    let pathname_socket_root = tempfile::tempdir().expect("pathname socket fixture");
    let pathname_socket = pathname_socket_root.path().join("host.sock");
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
    let pathname_server_script = format!(
        "import socket,time; s=socket.socket(socket.AF_UNIX); s.bind({:?}); s.listen(); time.sleep(30)",
        pathname_socket
    );
    let mut pathname_socket_server = Command::new("python3")
        .arg("-c")
        .arg(&pathname_server_script)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start host pathname-socket fixture");
    for _ in 0..100 {
        if pathname_socket.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(pathname_socket.exists(), "pathname fixture did not bind");

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

    let rustup_path =
        std::env::split_paths(&std::env::var_os("PATH").expect("test runner PATH is available"))
            .map(|directory| directory.join("rustup"))
            .find(|path| path.is_file())
            .expect("Rustup is installed by the Linux test toolchain");
    let rustup_home = std::env::var_os("RUSTUP_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").expect("test runner HOME is available"))
                .join(".rustup")
        });

    let probe_script = format!(
        "import os,socket,signal,sys\n\
         s=socket.socket(socket.AF_UNIX)\n\
         try: s.connect('\\0{socket_name}'); print('abstract-socket-allowed'); sys.exit(10)\n\
         except PermissionError: print('abstract-socket-denied')\n\
         except OSError as e: print('abstract-socket-error:'+str(e.errno)); sys.exit(11)\n\
         p=socket.socket(socket.AF_UNIX)\n\
         try: p.connect({pathname_socket:?}); print('pathname-socket-allowed'); sys.exit(15)\n\
         except (FileNotFoundError, PermissionError): print('pathname-socket-denied')\n\
         except OSError as e: print('pathname-socket-error:'+str(e.errno)); sys.exit(16)\n\
         try: os.kill({signal_pid}, signal.SIGUSR1); print('signal-allowed'); sys.exit(12)\n\
         except (PermissionError, ProcessLookupError): print('signal-denied')\n\
         print('host-proc-hidden' if not os.path.exists('/proc/{host_pid}') else 'host-proc-visible')\n\
         print('host-proc-environ-hidden' if not os.path.exists('/proc/{host_pid}/environ') else 'host-proc-environ-visible')\n\
         print('host-proc-fd-hidden' if not os.path.exists('/proc/{host_pid}/fd') else 'host-proc-fd-visible')\n\
         print('ambient-fd-closed' if not os.path.exists('/proc/self/fd/{ambient_fd}') else 'ambient-fd-open')\n\
         import ctypes; libc=ctypes.CDLL(None, use_errno=True)\n\
         ctypes.set_errno(0); ptrace_result=libc.ptrace(16, {host_pid}, 0, 0); ptrace_error=ctypes.get_errno()\n\
         ctypes.set_errno(0); readv_result=libc.process_vm_readv({host_pid}, None, 0, None, 0, 0); readv_error=ctypes.get_errno()\n\
         ctypes.set_errno(0); writev_result=libc.process_vm_writev({host_pid}, None, 0, None, 0, 0); writev_error=ctypes.get_errno()\n\
         print('seccomp-denied-tracing' if ptrace_result == -1 and ptrace_error == 38 and readv_result == -1 and readv_error == 38 and writev_result == -1 and writev_error == 38 else 'seccomp-tracing-allowed')\n\
         seccomp_mode=libc.prctl(21, 0, 0, 0, 0)\n\
         result=libc.unshare(0x10000000); unshare_error=ctypes.get_errno()\n\
         mount_result=libc.mount(None, None, None, 0, None); mount_error=ctypes.get_errno()\n\
         print('seccomp-denied-namespace-change' if seccomp_mode == 2 and result == -1 and unshare_error == 38 else 'seccomp-namespace-change-allowed')\n\
         print('seccomp-denied-mount-change' if mount_result == -1 and mount_error == 38 else 'seccomp-mount-change-allowed')\n\
         if seccomp_mode != 2 or result != -1 or unshare_error != 38 or mount_result != -1 or mount_error != 38: sys.exit(14)\n\
         import subprocess; subprocess.run(['git','--version'], check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE); print('git-subprocess-ok')\n\
         rustup_env=os.environ.copy(); rustup_env['RUSTUP_HOME']={rustup_home:?}\n\
         subprocess.run([{rustup_path:?},'toolchain','list'], check=True, env=rustup_env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)\n\
         subprocess.run([{rustup_path:?},'run','stable','rustc','--version'], check=True, env=rustup_env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)\n\
         print('rustup-procfs-ok')\n",
        signal_pid = signal_target.id(),
        host_pid = std::process::id(),
        ambient_fd = ambient_fd.as_raw_fd(),
        rustup_path = rustup_path,
        rustup_home = rustup_home
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
    stop(&mut pathname_socket_server);
    stop(&mut signal_target);
    let result = result.expect("required M004 process tier should launch");
    assert!(
        result.exit_status.success(),
        "stderr: {}",
        result.stderr.to_string_lossy()
    );
    let stdout = result.stdout.to_string_lossy();
    assert!(stdout.contains("abstract-socket-denied"), "{stdout}");
    assert!(stdout.contains("pathname-socket-denied"), "{stdout}");
    assert!(stdout.contains("signal-denied"), "{stdout}");
    assert!(stdout.contains("host-proc-hidden"), "{stdout}");
    assert!(stdout.contains("host-proc-environ-hidden"), "{stdout}");
    assert!(stdout.contains("host-proc-fd-hidden"), "{stdout}");
    assert!(stdout.contains("ambient-fd-closed"), "{stdout}");
    assert!(stdout.contains("seccomp-denied-tracing"), "{stdout}");
    assert!(
        stdout.contains("seccomp-denied-namespace-change"),
        "{stdout}"
    );
    assert!(stdout.contains("seccomp-denied-mount-change"), "{stdout}");
    assert!(stdout.contains("git-subprocess-ok"), "{stdout}");
    assert!(stdout.contains("rustup-procfs-ok"), "{stdout}");
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
            assert!(guarantees
                .iter()
                .any(|item| item.contains("seccomp filter")));
        }
        other => panic!("expected enforced M004 result, got {other:?}"),
    }
}

#[tokio::test]
async fn required_process_tier_fails_closed_when_user_namespaces_are_restricted() {
    if std::env::var(RESTRICTED_USERNS_OPT_IN).as_deref() != Ok("1") {
        eprintln!("skipped: set {RESTRICTED_USERNS_OPT_IN}=1 for the restricted-userns host probe");
        return;
    }

    let restriction = std::fs::read_to_string("/proc/sys/user/max_user_namespaces")
        .expect("host must expose the user namespace limit");
    assert_eq!(
        restriction.trim(),
        "0",
        "host user namespaces were not disabled"
    );
    codegg::security::sandbox::landlock::probe_process_scopes()
        .expect("this probe needs Landlock ABI 6 to reach the namespace setup gate");

    let bwrap = codegg::security::sandbox::linux_launch::trusted_bubblewrap_path()
        .expect("trusted Bubblewrap binary required for the restricted-host probe");
    let direct_probe = Command::new(&bwrap)
        .args(["--unshare-user", "--ro-bind", "/", "/", "--", "/bin/true"])
        .output()
        .expect("run a harmless Bubblewrap user-namespace capability probe");
    assert!(
        !direct_probe.status.success(),
        "host unexpectedly allowed user namespaces while max_user_namespaces=0"
    );

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
        ProcessProvenance::new("linux-m004", "restricted-userns-probe"),
    );
    request.sandbox = SandboxRequest::Required(spec);
    let error = ManagedProcessService::run(request)
        .await
        .expect_err("required M004 launch must refuse under the restricted host policy");
    assert!(
        error.to_string().contains("sandbox") || error.to_string().contains("Bubblewrap"),
        "unexpected refusal reason: {error}"
    );
    assert!(
        !marker.exists(),
        "required M004 ran its target after user-namespace setup was denied"
    );
    eprintln!("restricted user-namespace launch refused before target exec: {error}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn process_tier_cancellation_kills_namespace_descendants() {
    if std::env::var(OPT_IN).as_deref() != Ok("1") {
        eprintln!("skipped: set {OPT_IN}=1 to run the native M004 child probe");
        return;
    }
    if let Err(reason) = codegg::security::sandbox::landlock::probe_process_scopes() {
        eprintln!("M004 positive probes unavailable on this host: {reason}");
        return;
    }

    let workspace = tempfile::tempdir().expect("workspace");
    let started = workspace.path().join("descendant-started");
    let marker = workspace.path().join("descendant-survived-cancel");
    let child_script = format!(
        "import subprocess,time; subprocess.Popen(['/bin/sh','-c',\"touch {}; sleep 1; touch {}\"], start_new_session=True); time.sleep(30)",
        shell_quote(&started.to_string_lossy()),
        shell_quote(&marker.to_string_lossy()),
    );
    let spec = SandboxConfig::new()
        .with_enabled(true)
        .with_mode(SandboxMode::WorkspaceWrite)
        .with_linux_process_isolation(true)
        .launch_spec(
            "python3",
            &["-c".into(), child_script],
            Some(workspace.path()),
        )
        .expect("M004 launch spec");
    let mut request = ManagedProcessRequest::new(
        [spec.target.as_os_str().to_owned()]
            .into_iter()
            .chain(spec.args.iter().map(OsString::from))
            .collect(),
        workspace.path().to_path_buf(),
        ProcessProvenance::new("linux-m004", "descendant-cancel-probe"),
    );
    let cancellation = CancellationToken::new();
    request.cancellation = cancellation.clone();
    request.sandbox = SandboxRequest::Required(spec);

    let task = tokio::spawn(ManagedProcessService::run(request));
    for _ in 0..200 {
        if started.exists() {
            break;
        }
        if task.is_finished() {
            let result = task.await.expect("join early M004 completion");
            panic!("M004 launch ended before the descendant started: {result:?}");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(started.exists(), "M004 descendant fixture did not start");
    cancellation.cancel();
    let result = task
        .await
        .expect("join cancellation task")
        .expect("cancellation result");
    assert_eq!(
        result.termination,
        codegg::managed_process::TerminationReason::Cancelled
    );
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert!(
        !marker.exists(),
        "M004 cancellation left a namespace descendant running"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_process_tiers_receive_distinct_pid_and_ipc_namespaces() {
    if std::env::var(OPT_IN).as_deref() != Ok("1") {
        eprintln!("skipped: set {OPT_IN}=1 to run the native M004 child probe");
        return;
    }
    if let Err(reason) = codegg::security::sandbox::landlock::probe_process_scopes() {
        eprintln!("M004 positive probes unavailable on this host: {reason}");
        return;
    }

    let first_workspace = tempfile::tempdir().expect("first workspace");
    let second_workspace = tempfile::tempdir().expect("second workspace");
    let request = |workspace: &Path, operation: &str| {
        let spec = SandboxConfig::new()
            .with_enabled(true)
            .with_mode(SandboxMode::WorkspaceWrite)
            .with_linux_process_isolation(true)
            .launch_spec(
                "python3",
                &[
                    "-c".into(),
                    "import os; print(os.stat('/proc/self/ns/pid').st_ino); print(os.stat('/proc/self/ns/ipc').st_ino)".into(),
                ],
                Some(workspace),
            )
            .expect("M004 launch spec");
        let mut request = ManagedProcessRequest::new(
            [spec.target.as_os_str().to_owned()]
                .into_iter()
                .chain(spec.args.iter().map(OsString::from))
                .collect(),
            workspace.to_path_buf(),
            ProcessProvenance::new("linux-m004", operation),
        );
        request.timeout = Some(Duration::from_secs(15));
        request.sandbox = SandboxRequest::Required(spec);
        request
    };

    let (first, second) = tokio::join!(
        ManagedProcessService::run(request(first_workspace.path(), "concurrent-first")),
        ManagedProcessService::run(request(second_workspace.path(), "concurrent-second")),
    );
    let first = first.expect("first concurrent M004 launch");
    let second = second.expect("second concurrent M004 launch");
    assert!(
        first.exit_status.success(),
        "first stderr: {}",
        first.stderr.to_string_lossy()
    );
    assert!(
        second.exit_status.success(),
        "second stderr: {}",
        second.stderr.to_string_lossy()
    );
    let namespace_ids = |stdout: &str| {
        let ids = stdout
            .lines()
            .map(|line| line.trim().parse::<u64>().expect("namespace inode"))
            .collect::<Vec<_>>();
        assert_eq!(
            ids.len(),
            2,
            "expected PID and IPC namespace inodes: {stdout}"
        );
        [ids[0], ids[1]]
    };
    let first_ids = namespace_ids(&first.stdout.to_string_lossy());
    let second_ids = namespace_ids(&second.stdout.to_string_lossy());
    assert_ne!(
        first_ids[0], second_ids[0],
        "concurrent jobs shared a PID namespace"
    );
    assert_ne!(
        first_ids[1], second_ids[1],
        "concurrent jobs shared an IPC namespace"
    );
    let host_pid = std::fs::metadata("/proc/self/ns/pid")
        .expect("host PID namespace")
        .ino();
    let host_ipc = std::fs::metadata("/proc/self/ns/ipc")
        .expect("host IPC namespace")
        .ino();
    assert_ne!(
        first_ids[0], host_pid,
        "first job retained the host PID namespace"
    );
    assert_ne!(
        second_ids[0], host_pid,
        "second job retained the host PID namespace"
    );
    assert_ne!(
        first_ids[1], host_ipc,
        "first job retained the host IPC namespace"
    );
    assert_ne!(
        second_ids[1], host_ipc,
        "second job retained the host IPC namespace"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn process_tier_runs_workspace_check_and_managed_process_tests() {
    if std::env::var("CODEGG_RUN_M004_CARGO_NATIVE").as_deref() != Ok("1") {
        eprintln!("skipped: set CODEGG_RUN_M004_CARGO_NATIVE=1 for Cargo compatibility probes");
        return;
    }
    if let Err(reason) = codegg::security::sandbox::landlock::probe_process_scopes() {
        panic!("Cargo compatibility probe requires the positive M004 host profile: {reason}");
    }

    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .expect("test runner HOME is available");
    let rustup_home = std::env::var_os("RUSTUP_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".rustup"));
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".cargo"));
    let rustup = std::env::split_paths(&std::env::var_os("PATH").expect("runner PATH"))
        .map(|directory| directory.join("rustup"))
        .find(|candidate| candidate.is_file())
        .expect("Rustup is installed by the Linux test toolchain")
        .canonicalize()
        .expect("resolve rustup executable");
    let host_triple_output = Command::new(&rustup)
        .args(["run", "stable", "rustc", "-vV"])
        .env("RUSTUP_HOME", &rustup_home)
        .env("CARGO_HOME", &cargo_home)
        .output()
        .expect("query the installed Rust host triple");
    assert!(
        host_triple_output.status.success(),
        "installed rustc query failed: {}",
        String::from_utf8_lossy(&host_triple_output.stderr)
    );
    let host_triple = String::from_utf8_lossy(&host_triple_output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .expect("rustc -vV host triple")
        .to_string();
    let linker_env = format!(
        "CARGO_TARGET_{}_LINKER",
        host_triple.replace('-', "_").to_ascii_uppercase()
    );
    let linker = std::env::split_paths(&std::env::var_os("PATH").expect("runner PATH"))
        .map(|directory| directory.join("cc"))
        .find(|candidate| candidate.is_file())
        .expect("C linker is installed on the Linux test runner")
        .canonicalize()
        .expect("resolve C linker executable");
    let cargo =
        std::env::split_paths(&std::env::var_os("PATH").expect("test runner PATH is available"))
            .map(|directory| directory.join("cargo"))
            .find(|candidate| candidate.is_file())
            .expect("Cargo is installed by the Rust test toolchain");
    let cargo_target = cargo.canonicalize().expect("resolve Cargo launcher");
    let uses_rustup_shim = cargo_target
        .file_name()
        .is_some_and(|name| name == "rustup" || name == "rustup.exe");
    for (operation, args) in [
        (
            "cargo-workspace-check",
            vec!["check", "--locked", "--workspace"],
        ),
        (
            "cargo-managed-process-tests",
            vec![
                "test",
                "--locked",
                "-p",
                "codegg",
                "--lib",
                "managed_process",
                // Protected-mount tests require probing and starting the host
                // Bubblewrap binary. The M004 child intentionally cannot see
                // that host launch dependency; the hosted workflow runs those
                // regressions separately on the host after this probe.
                "--",
                "--skip",
                "protected_mount_",
            ],
        ),
    ] {
        // The protected launch intentionally gives the target HOME=/tmp and
        // clears inherited environment. Re-establish only the runner's
        // installed Rust toolchain and Cargo caches so rustup and Cargo do not
        // look for an empty /tmp/.rustup.
        let mut launch_script = format!(
            "export RUSTUP_HOME={}; export CARGO_HOME={}; export CARGO_BUILD_JOBS=2; export CC={}; export HOST_CC={}; export {}={}; export PATH={}:$PATH; ",
            shell_quote(&rustup_home.to_string_lossy()),
            shell_quote(&cargo_home.to_string_lossy()),
            shell_quote(&linker.to_string_lossy()),
            shell_quote(&linker.to_string_lossy()),
            linker_env,
            shell_quote(&linker.to_string_lossy()),
            shell_quote(
                &linker
                    .parent()
                    .expect("C linker has a parent directory")
                    .to_string_lossy()
            ),
        );
        if uses_rustup_shim {
            launch_script.push_str(&format!(
                "exec {} run stable cargo",
                shell_quote(&cargo_target.to_string_lossy())
            ));
        } else {
            let cargo_dir = cargo_target
                .parent()
                .expect("Cargo executable has a parent directory");
            launch_script.push_str(&format!(
                "export PATH={}:$PATH; exec {}",
                shell_quote(&cargo_dir.to_string_lossy()),
                shell_quote(&cargo_target.to_string_lossy())
            ));
        }
        for arg in args {
            launch_script.push(' ');
            launch_script.push_str(&shell_quote(arg));
        }
        let spec = SandboxConfig::new()
            .with_enabled(true)
            .with_mode(SandboxMode::WorkspaceWrite)
            .with_linux_process_isolation(true)
            .launch_spec("/bin/sh", &["-c".into(), launch_script], Some(&workspace))
            .expect("M004 Cargo launch spec");
        let mut request = ManagedProcessRequest::new(
            [spec.target.as_os_str().to_owned()]
                .into_iter()
                .chain(spec.args.iter().map(OsString::from))
                .collect(),
            workspace.clone(),
            ProcessProvenance::new("linux-m004", operation),
        );
        request.timeout = Some(Duration::from_secs(15 * 60));
        request.sandbox = SandboxRequest::Required(spec);
        let result = ManagedProcessService::run(request)
            .await
            .unwrap_or_else(|error| panic!("M004 {operation} failed to launch: {error}"));
        assert!(
            result.exit_status.success(),
            "M004 {operation} failed (status {:?}); stdout: {}; stderr: {}",
            result.exit_status,
            result.stdout.to_string_lossy(),
            result.stderr.to_string_lossy()
        );
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
