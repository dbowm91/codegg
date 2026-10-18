#![cfg(target_os = "linux")]

use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use codegg::git_service::{GitExecutionService as TypedGitExecutionService, GitPayload};
use codegg::managed_process::{
    ManagedProcessRequest, ManagedProcessResult, ManagedProcessService, ProcessProvenance,
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

    // Establish that each host-local fixture is reachable before asking the
    // contained child to prove the opposite. Without these controls a broken
    // listener or an IPv6-disabled runner could look like successful network
    // isolation.
    for address in [tcp4_addr, tcp6_addr] {
        let stream = std::net::TcpStream::connect_timeout(&address, Duration::from_secs(2))
            .unwrap_or_else(|error| panic!("host positive-control TCP {address} failed: {error}"));
        drop(stream);
        let listener = if address.is_ipv4() { &tcp4 } else { &tcp6 };
        let (accepted, _) = listener
            .accept()
            .unwrap_or_else(|error| panic!("accept host positive-control TCP {address}: {error}"));
        drop(accepted);
    }
    for (socket, address) in [(&udp4, udp4_addr), (&udp6, udp6_addr)] {
        socket
            .send_to(b"host-positive-control", address)
            .unwrap_or_else(|error| panic!("host positive-control UDP {address} failed: {error}"));
        let mut buffer = [0_u8; 64];
        let (received, peer) = socket
            .recv_from(&mut buffer)
            .unwrap_or_else(|error| panic!("receive host positive-control UDP {address}: {error}"));
        assert_eq!(&buffer[..received], b"host-positive-control");
        assert_eq!(peer.ip(), address.ip());
    }

    // The negative DNS/external probes below are meaningful only when this
    // runner has working resolver and outbound TCP access. GitHub is a stable
    // external endpoint already required by hosted checkout and CI setup.
    let external_address = ("github.com", 443)
        .to_socket_addrs()
        .expect("host positive-control DNS lookup for github.com")
        .next()
        .expect("host DNS returned no GitHub addresses");
    let external = std::net::TcpStream::connect_timeout(&external_address, Duration::from_secs(5))
        .expect("host positive-control outbound TCP to github.com:443");
    drop(external);

    // Pin the resolver configured for this host. The child must be unable to
    // reach it; this closes the resolver-endpoint gap for rebinding answers,
    // though it does not exercise a DNS server that actually changes answers.
    let host_resolver = std::fs::read_to_string("/etc/resolv.conf")
        .expect("read host resolver configuration")
        .lines()
        .filter_map(|line| line.split_whitespace().collect::<Vec<_>>().get(1).copied())
        .find_map(|address| address.parse::<IpAddr>().ok())
        .expect("host resolver configuration must include a nameserver");
    let dns_query = [
        0x4d, 0x52, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x07, b'e', b'x',
        b'a', b'm', b'p', b'l', b'e', 0x03, b'c', b'o', b'm', 0x00, 0x00, 0x01, 0x00, 0x01,
    ];
    let host_dns_probe = match host_resolver {
        IpAddr::V4(_) => UdpSocket::bind("0.0.0.0:0"),
        IpAddr::V6(_) => UdpSocket::bind("[::]:0"),
    }
    .expect("bind host DNS positive-control socket");
    host_dns_probe
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("set host DNS positive-control timeout");
    host_dns_probe
        .send_to(&dns_query, SocketAddr::new(host_resolver, 53))
        .expect("send host DNS positive-control query");
    let mut dns_response = [0_u8; 512];
    let (dns_response_len, dns_peer) = host_dns_probe
        .recv_from(&mut dns_response)
        .expect("configured host resolver positive-control response");
    assert_eq!(dns_peer.ip(), host_resolver);
    assert!(dns_response_len >= 12, "host DNS response is truncated");
    assert_eq!(
        &dns_response[..2],
        &dns_query[..2],
        "DNS response ID mismatch"
    );

    // A local resolver deliberately returns a different A record for the
    // same question twice. This is the host positive control for a DNS
    // rebinding-shaped answer sequence; the contained child below must not
    // be able to reach this host loopback resolver at all.
    let rebinding_dns = UdpSocket::bind(("127.0.0.1", 0)).expect("rebinding DNS fixture");
    rebinding_dns
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("set rebinding DNS fixture timeout");
    let rebinding_dns_addr = rebinding_dns
        .local_addr()
        .expect("rebinding DNS fixture address");
    let mut rebinding_answers = Vec::new();
    for address in ["127.0.0.1", "198.51.100.1"] {
        let control = UdpSocket::bind(("127.0.0.1", 0)).expect("bind rebinding control socket");
        control
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set rebinding control timeout");
        control
            .send_to(&dns_query, rebinding_dns_addr)
            .expect("send rebinding positive-control query");
        let mut query = [0_u8; 512];
        let (query_len, peer) = rebinding_dns
            .recv_from(&mut query)
            .expect("rebinding DNS fixture receives host query");
        assert_eq!(&query[..query_len], dns_query);
        let mut response = query[..query_len].to_vec();
        response[2] = 0x81;
        response[3] = 0x80;
        response[6] = 0;
        response[7] = 1;
        response.extend_from_slice(&[
            0xc0, 0x0c, // compressed owner name: the queried name
            0, 1, // A
            0, 1, // IN
            0, 0, 0, 1, // one-second TTL
            0, 4, // IPv4 RDATA length
        ]);
        let IpAddr::V4(answer) = address.parse::<IpAddr>().expect("fixture A address") else {
            unreachable!("fixture answers are IPv4")
        };
        response.extend_from_slice(&answer.octets());
        rebinding_dns
            .send_to(&response, peer)
            .expect("send changing DNS answer");
        let mut answer = [0_u8; 512];
        let (answer_len, source) = control
            .recv_from(&mut answer)
            .expect("host positive-control receives rebinding answer");
        assert_eq!(source, rebinding_dns_addr);
        assert!(answer_len >= 4);
        rebinding_answers.push(answer[answer_len - 4..answer_len].to_vec());
    }
    assert_ne!(
        rebinding_answers[0], rebinding_answers[1],
        "the host fixture must actually change the answer for an identical query"
    );

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
        r#"import os, socket
probes = [
    (socket.AF_INET, ({:?}, {})),
    (socket.AF_INET6, ({:?}, {})),
]
for family, address in probes:
    s = socket.socket(family, socket.SOCK_STREAM)
    s.settimeout(1)
    if s.connect_ex(address) == 0:
        raise SystemExit(41)
for family, address in [(socket.AF_INET, ({:?}, {})), (socket.AF_INET6, ({:?}, {}))]:
    s = socket.socket(family, socket.SOCK_DGRAM)
    try:
        s.sendto(b'network-leak', address)
    except OSError:
        pass
try:
    socket.getaddrinfo('example.com', 443, type=socket.SOCK_STREAM)
    raise SystemExit(42)
except OSError:
    pass
dns = socket.socket(socket.AF_INET6 if {:?} else socket.AF_INET, socket.SOCK_DGRAM)
dns.settimeout(0.5)
query = b'\x4d\x52\x01\x00\x00\x01\x00\x00\x00\x00\x00\x00' + b'\x07example\x03com\x00\x00\x01\x00\x01'
try:
    dns.sendto(query, ({:?}, 53))
    dns.recvfrom(512)
except OSError:
    pass
else:
    raise SystemExit(45)
rebinding = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
rebinding.settimeout(0.5)
try:
    rebinding.sendto(query, ('127.0.0.1', {}))
    rebinding.recvfrom(512)
except OSError:
    pass
else:
    raise SystemExit(46)
external = socket.socket()
external.settimeout(1)
if external.connect_ex(('198.51.100.1', 443)) == 0:
    raise SystemExit(43)
fd = {}
try:
    os.fstat(fd)
except OSError:
    open({:?}, 'w').write('offline')
else:
    os.write(fd, b'inherited-socket-leak')
    raise SystemExit(44)
"#,
        tcp4_addr.ip().to_string(),
        tcp4_addr.port(),
        tcp6_addr.ip().to_string(),
        tcp6_addr.port(),
        udp4_addr.ip().to_string(),
        udp4_addr.port(),
        udp6_addr.ip().to_string(),
        udp6_addr.port(),
        u8::from(host_resolver.is_ipv6()),
        host_resolver.to_string(),
        rebinding_dns_addr.port(),
        inherited_fd.as_raw_fd(),
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_only_contained_git_reads_live_objects_and_contains_configured_command() {
    if std::env::var(M005_OPT_IN).as_deref() != Ok("1") {
        eprintln!("skipped: set {M005_OPT_IN}=1 to run the native contained-Git proof");
        return;
    }

    let workspace = tempfile::tempdir().expect("Git workspace");
    let repository = workspace.path();
    let git = std::env::split_paths(&std::env::var_os("PATH").expect("runner PATH"))
        .map(|directory| directory.join("git"))
        .find(|candidate| candidate.is_file())
        .expect("Git is installed on the native Linux runner")
        .canonicalize()
        .expect("resolve Git executable");

    let setup = |args: &[&str]| {
        Command::new(&git)
            .args(args)
            .current_dir(repository)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .expect("prepare Git fixture")
    };
    let init = setup(&["init", "--quiet"]);
    assert!(init.status.success(), "git init failed: {:?}", init);
    std::fs::write(repository.join("tracked.txt"), "live object payload\n")
        .expect("write tracked fixture");
    assert!(setup(&["add", "tracked.txt"]).status.success());
    let identity = setup(&[
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "commit",
        "--quiet",
        "-m",
        "fixture",
    ]);
    assert!(
        identity.status.success(),
        "git commit fixture failed: {}",
        String::from_utf8_lossy(&identity.stderr)
    );

    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .expect("runner HOME is available");
    let host_only = tempfile::Builder::new()
        .prefix("codegg-live-git-host-only-")
        .tempdir_in(home)
        .expect("host-only sentinel directory");
    let secret = host_only.path().join("credential-sentinel");
    let secret_leak = repository.join("credential-leak");
    let proc_leak = repository.join("host-proc-leak");
    std::fs::write(&secret, "host-secret-marker").expect("write host-only secret sentinel");

    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("host TCP fixture");
    listener
        .set_nonblocking(true)
        .expect("make host TCP fixture nonblocking");
    let address = listener.local_addr().expect("host TCP fixture address");
    let positive = std::net::TcpStream::connect_timeout(&address, Duration::from_secs(2))
        .expect("host positive control reaches TCP fixture");
    let (accepted, _) = listener.accept().expect("accept host positive control");
    drop((positive, accepted));

    let workspace_marker = repository.join("contained-command-ran");
    let network_marker = repository.join("network-escape");
    let child_netns = repository.join("child-netns-inode");
    let host_pid = std::process::id();
    let python = std::env::split_paths(&std::env::var_os("PATH").expect("runner PATH"))
        .map(|directory| directory.join("python3"))
        .find(|candidate| candidate.is_file())
        .expect("Python is installed on the native Linux runner")
        .canonicalize()
        .expect("resolve Python executable");
    let python_probe = format!(
        "import socket; socket.create_connection(({:?}, {}), timeout=1); open({:?}, 'w').write('connected')",
        address.ip().to_string(),
        address.port(),
        network_marker.to_string_lossy()
    );
    let script = repository.join("configured-command.sh");
    let script_text = format!(
        "#!/bin/sh\nprintf ran > {}\n/usr/bin/stat -Lc '%i' /proc/self/ns/net > {}\nif test -r {}; then cat {} > {}; fi\nif test -r /proc/{}/cmdline; then cat /proc/{}/cmdline > {}; fi\n{} -c {} 2>/dev/null || true\n",
        shell_quote(&workspace_marker.to_string_lossy()),
        shell_quote(&child_netns.to_string_lossy()),
        shell_quote(&secret.to_string_lossy()),
        shell_quote(&secret.to_string_lossy()),
        shell_quote(&secret_leak.to_string_lossy()),
        host_pid,
        host_pid,
        shell_quote(&proc_leak.to_string_lossy()),
        shell_quote(&python.to_string_lossy()),
        shell_quote(&python_probe)
    );
    std::fs::write(&script, script_text).expect("write configured Git command");
    let mut permissions = std::fs::metadata(&script)
        .expect("stat configured Git command")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script, permissions).expect("make Git command executable");
    let alias = format!("!{}", script.display());
    let configure = setup(&["config", "alias.authority-probe", &alias]);
    assert!(
        configure.status.success(),
        "configure repository-local Git command: {}",
        String::from_utf8_lossy(&configure.stderr)
    );
    let configure_diff = setup(&["config", "diff.external", &script.to_string_lossy()]);
    assert!(
        configure_diff.status.success(),
        "configure repository-local external diff: {}",
        String::from_utf8_lossy(&configure_diff.stderr)
    );
    std::fs::write(
        repository.join("tracked.txt"),
        "dirty live worktree payload\n",
    )
    .expect("make the tracked fixture dirty");

    async fn contained_git(
        git: &Path,
        workspace: &Path,
        args: &[&str],
        label: &str,
    ) -> ManagedProcessResult {
        let args = args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        let spec = SandboxConfig::new()
            .with_enabled(true)
            .with_mode(SandboxMode::WorkspaceWrite)
            .with_linux_offline_network(true)
            .launch_spec(git, &args, Some(workspace))
            .expect("construct test-only M004/M005 launch spec");
        let argv = std::iter::once(spec.target.as_os_str().to_owned())
            .chain(spec.args.iter().map(OsString::from))
            .collect();
        let mut request = ManagedProcessRequest::new(
            argv,
            workspace.to_path_buf(),
            ProcessProvenance::new("linux-trusted-git-c001", label),
        );
        request.timeout = Some(Duration::from_secs(15));
        request.environment_policy = codegg::managed_process::EnvironmentPolicy::sanitized()
            .with_var("GIT_CONFIG_NOSYSTEM", "1")
            .with_var("GIT_CONFIG_GLOBAL", "/dev/null")
            .with_var("HOME", "/tmp/codegg-empty-home");
        request.sandbox = SandboxRequest::Required(spec);
        ManagedProcessService::run(request)
            .await
            .expect("required contained Git must fail closed or run under obtained limits")
    }

    let log = contained_git(&git, repository, &["log", "-1", "--format=%s"], "live-log").await;
    assert!(
        log.exit_status.success(),
        "contained git log failed: {}",
        log.stderr.to_string_lossy()
    );
    assert!(log.stdout.to_string_lossy().contains("fixture"));
    assert!(matches!(
        log.sandbox,
        SandboxExecutionOutcome::Enforced { .. }
    ));

    let diff = contained_git(
        &git,
        repository,
        &["diff", "--", "tracked.txt"],
        "external-diff",
    )
    .await;
    assert!(
        diff.exit_status.success(),
        "contained git diff failed: {}",
        diff.stderr.to_string_lossy()
    );
    assert!(
        workspace_marker.exists(),
        "repository-configured external diff did not execute"
    );
    assert!(matches!(
        diff.sandbox,
        SandboxExecutionOutcome::Enforced { .. }
    ));

    let configured = contained_git(&git, repository, &["authority-probe"], "config-command").await;
    assert!(
        configured.exit_status.success(),
        "contained configured Git command failed: {}",
        configured.stderr.to_string_lossy()
    );
    assert!(
        workspace_marker.exists(),
        "Git's configured command did not execute"
    );
    let child_netns_inode = std::fs::read_to_string(&child_netns)
        .expect("configured command records its network namespace")
        .trim()
        .parse::<u64>()
        .expect("child network namespace inode is numeric");
    assert_ne!(
        child_netns_inode,
        std::fs::metadata("/proc/self/ns/net")
            .expect("host network namespace metadata")
            .ino(),
        "configured Git command shared the host network namespace"
    );
    assert!(
        !secret_leak.exists(),
        "configured command read a host-only credential sentinel"
    );
    assert!(
        !proc_leak.exists(),
        "configured command read a host /proc process"
    );
    assert!(
        !network_marker.exists(),
        "configured command escaped the offline network namespace"
    );
    assert!(
        listener.accept().is_err(),
        "configured Git command reached a host loopback service"
    );
    assert!(matches!(
        configured.sandbox,
        SandboxExecutionOutcome::Enforced { .. }
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_only_contained_git_reads_match_typed_snapshot_payloads() {
    if std::env::var(M005_OPT_IN).as_deref() != Ok("1") {
        eprintln!("skipped: set {M005_OPT_IN}=1 to run the contained Git read parity pilot");
        return;
    }

    let workspace = tempfile::tempdir().expect("Git workspace");
    let repository = workspace.path();
    let git = std::env::split_paths(&std::env::var_os("PATH").expect("runner PATH"))
        .map(|directory| directory.join("git"))
        .find(|candidate| candidate.is_file())
        .expect("Git is installed on the native Linux runner")
        .canonicalize()
        .expect("resolve Git executable");

    let setup = |args: &[&str]| {
        Command::new(&git)
            .args(args)
            .current_dir(repository)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .expect("prepare Git fixture")
    };
    assert!(setup(&["init", "--quiet"]).status.success());
    std::fs::write(repository.join("tracked.txt"), "base payload\n")
        .expect("write tracked fixture");
    assert!(setup(&["add", "tracked.txt"]).status.success());
    let commit = setup(&[
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "commit",
        "--quiet",
        "-m",
        "typed parity fixture",
    ]);
    assert!(
        commit.status.success(),
        "Git fixture commit failed: {}",
        String::from_utf8_lossy(&commit.stderr)
    );

    let resolve_repo_path = |args: &[&str]| {
        let output = setup(args);
        assert!(
            output.status.success(),
            "resolve Git trust-root facts with {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
        if path.is_absolute() {
            path.canonicalize().expect("canonical Git trust-root path")
        } else {
            repository
                .join(path)
                .canonicalize()
                .expect("canonical relative Git trust-root path")
        }
    };
    let selected_root = resolve_repo_path(&["rev-parse", "--show-toplevel"]);
    let selected_gitdir = resolve_repo_path(&["rev-parse", "--absolute-git-dir"]);
    let selected_common_dir = resolve_repo_path(&["rev-parse", "--git-common-dir"]);
    let selected_odb = resolve_repo_path(&["rev-parse", "--git-path", "objects"]);
    assert_eq!(
        selected_root,
        repository.canonicalize().expect("repository root")
    );
    assert_eq!(selected_common_dir, selected_gitdir);
    assert_eq!(selected_odb, selected_gitdir.join("objects"));
    assert!(
        std::fs::symlink_metadata(selected_odb.join("info/alternates")).is_err(),
        "ordinary fixture unexpectedly has an alternate object store"
    );
    let gitdir_identity = std::fs::metadata(&selected_gitdir).expect("stat selected gitdir");
    let odb_identity = std::fs::metadata(&selected_odb).expect("stat selected object directory");
    let gitdir_identity = (gitdir_identity.dev(), gitdir_identity.ino());
    let odb_identity = (odb_identity.dev(), odb_identity.ino());
    std::fs::write(repository.join("tracked.txt"), "changed payload\n")
        .expect("dirty tracked fixture");
    std::fs::write(repository.join("untracked.txt"), "untracked payload\n")
        .expect("untracked fixture");

    async fn contained_read(
        git: &Path,
        repository: &Path,
        selected_gitdir: &Path,
        operation: &codegg_git::GitOperation,
        label: &str,
    ) -> ManagedProcessResult {
        let argv = codegg_git::render_argv(operation);
        let args = argv.iter().skip(1).cloned().collect::<Vec<_>>();
        let spec = SandboxConfig::new()
            .with_enabled(true)
            .with_mode(SandboxMode::WorkspaceWrite)
            .with_linux_offline_network(true)
            .launch_spec(git, &args, Some(repository))
            .expect("construct contained Git read spec");
        assert!(
            spec.protected_readonly_paths
                .contains(&selected_gitdir.to_path_buf()),
            "contained {label} launch did not protect selected gitdir {}",
            selected_gitdir.display()
        );
        assert!(
            spec.path_identities
                .iter()
                .any(|identity| identity.path == selected_gitdir),
            "contained {label} launch did not capture selected gitdir identity"
        );
        let argv = std::iter::once(spec.target.as_os_str().to_owned())
            .chain(spec.args.iter().map(OsString::from))
            .collect();
        let mut request = ManagedProcessRequest::new(
            argv,
            repository.to_path_buf(),
            ProcessProvenance::new("linux-trusted-git-c002", label),
        );
        request.timeout = Some(Duration::from_secs(15));
        request.environment_policy = codegg::managed_process::EnvironmentPolicy::sanitized()
            .with_var("GIT_CONFIG_NOSYSTEM", "1")
            .with_var("GIT_CONFIG_GLOBAL", "/dev/null")
            .with_var("HOME", "/tmp/codegg-empty-home");
        request.sandbox = SandboxRequest::Required(spec);
        ManagedProcessService::run(request)
            .await
            .expect("required contained Git read must run or fail closed")
    }

    let typed = TypedGitExecutionService::new();
    let operations = [
        ("status", codegg_git::GitOperation::Status { short: true }),
        (
            "diff",
            codegg_git::GitOperation::Diff {
                staged: false,
                stat: false,
                name_only: false,
                base_ref: None,
                paths: vec![],
            },
        ),
        (
            "log",
            codegg_git::GitOperation::Log {
                oneline: true,
                max_count: Some(1),
                paths: vec![],
            },
        ),
        (
            "show",
            codegg_git::GitOperation::Show {
                rev: codegg_git::ref_name::RevisionExpr::new("HEAD")
                    .expect("valid fixture revision"),
            },
        ),
        (
            "changed-files",
            codegg_git::GitOperation::ChangedFiles { base_ref: None },
        ),
        (
            "branches",
            codegg_git::GitOperation::BranchList {
                remotes: false,
                all: false,
            },
        ),
        ("worktrees", codegg_git::GitOperation::WorktreeList),
    ];

    for (label, operation) in &operations {
        let snapshot = typed
            .execute(operation, repository)
            .await
            .unwrap_or_else(|error| panic!("typed snapshot {label} failed: {error}"));
        assert!(snapshot.success, "typed snapshot {label} returned nonzero");
        let live = contained_read(&git, repository, &selected_gitdir, operation, label).await;
        assert!(
            live.exit_status.success(),
            "contained live {label} failed: {}",
            live.stderr.to_string_lossy()
        );
        assert!(
            matches!(live.sandbox, SandboxExecutionOutcome::Enforced { .. }),
            "contained live {label} did not report actual enforcement"
        );
        assert_eq!(
            live.stdout.to_string_lossy(),
            snapshot.stdout,
            "native live {label} output differs from the typed snapshot route"
        );
        let gitdir_after =
            std::fs::metadata(&selected_gitdir).expect("stat selected gitdir after read");
        let odb_after = std::fs::metadata(&selected_odb).expect("stat selected ODB after read");
        assert_eq!(
            (gitdir_after.dev(), gitdir_after.ino()),
            gitdir_identity,
            "contained {label} read observed a replaced Git directory"
        );
        assert_eq!(
            (odb_after.dev(), odb_after.ino()),
            odb_identity,
            "contained {label} read observed a replaced object directory"
        );
        match (*label, snapshot.payload.as_ref().expect("typed payload")) {
            ("status", GitPayload::Status(status)) => {
                assert!(status.is_dirty);
                assert!(live.stdout.to_string_lossy().contains("tracked.txt"));
                assert!(live.stdout.to_string_lossy().contains("untracked.txt"));
            }
            ("diff", GitPayload::DiffResult(diff)) => {
                assert_eq!(diff.files.len(), 1);
                assert_eq!(diff.files[0].path, "tracked.txt");
                assert!(live.stdout.to_string_lossy().contains("changed payload"));
            }
            ("diff", GitPayload::DiffSummary(summary)) => {
                assert_eq!(summary.files_changed, 1);
                assert_eq!(summary.files[0].path, "tracked.txt");
            }
            ("log", GitPayload::Log(commits)) => {
                assert_eq!(commits.len(), 1);
                assert_eq!(commits[0].message, "typed parity fixture");
                assert!(live
                    .stdout
                    .to_string_lossy()
                    .contains("typed parity fixture"));
            }
            ("show", GitPayload::Show(show)) => {
                assert!(!show.hash.is_empty());
                assert!(live.stdout.to_string_lossy().contains(&show.hash));
                assert!(show.message.contains("typed parity fixture"));
            }
            ("changed-files", GitPayload::ChangedFiles(files)) => {
                assert!(files.iter().any(|file| file.path == "tracked.txt"));
                assert!(live.stdout.to_string_lossy().contains("tracked.txt"));
            }
            ("branches", GitPayload::Branches(branches)) => {
                assert!(branches.iter().any(|branch| branch.current));
            }
            ("worktrees", GitPayload::Worktrees(worktrees)) => {
                assert!(worktrees.iter().any(|worktree| {
                    worktree.is_current && Path::new(&worktree.path) == repository
                }));
            }
            (label, payload) => panic!("unexpected typed {label} payload: {payload:?}"),
        }
    }

    // Race live reads against Git's own pack replacement and atomic ref
    // movement. A read may observe either committed HEAD, but never a mixed
    // result. This does not claim immutable-generation semantics for
    // arbitrary writers.
    let first_commit = String::from_utf8(setup(&["rev-parse", "HEAD"]).stdout)
        .expect("first commit id is UTF-8")
        .trim()
        .to_owned();
    std::fs::write(repository.join("race.txt"), "second committed generation\n")
        .expect("write second commit fixture");
    assert!(setup(&["add", "race.txt"]).status.success());
    let second_commit = setup(&[
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "commit",
        "--quiet",
        "-m",
        "concurrent ref fixture",
        "--",
        "race.txt",
    ]);
    assert!(
        second_commit.status.success(),
        "second Git fixture commit failed: {}",
        String::from_utf8_lossy(&second_commit.stderr)
    );
    let second_commit = String::from_utf8(setup(&["rev-parse", "HEAD"]).stdout)
        .expect("second commit id is UTF-8")
        .trim()
        .to_owned();
    let branch = String::from_utf8(setup(&["symbolic-ref", "--short", "HEAD"]).stdout)
        .expect("branch name is UTF-8")
        .trim()
        .to_owned();
    let branch_ref = format!("refs/heads/{branch}");
    let expected_logs = [first_commit.as_str(), second_commit.as_str()].map(|commit| {
        String::from_utf8(setup(&["show", "-s", "--oneline", commit]).stdout)
            .expect("commit log output is UTF-8")
    });

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let repack_barrier = std::sync::Arc::clone(&barrier);
    let repack_git = git.clone();
    let repack_repository = repository.to_path_buf();
    let repacker = tokio::task::spawn_blocking(move || {
        repack_barrier.wait();
        for iteration in 0..32 {
            let output = Command::new(&repack_git)
                .args(["repack", "-adf"])
                .current_dir(&repack_repository)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output()
                .expect("run concurrent external Git repack");
            assert!(
                output.status.success(),
                "external repack {iteration} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    });
    let ref_barrier = std::sync::Arc::clone(&barrier);
    let ref_git = git.clone();
    let ref_repository = repository.to_path_buf();
    let ref_worker = tokio::task::spawn_blocking(move || {
        ref_barrier.wait();
        let mut old_commit = second_commit;
        let mut new_commit = first_commit;
        for iteration in 0..32 {
            let output = Command::new(&ref_git)
                .args(["update-ref", &branch_ref, &new_commit, &old_commit])
                .current_dir(&ref_repository)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output()
                .expect("run concurrent external Git ref update");
            assert!(
                output.status.success(),
                "external ref update {iteration} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            std::mem::swap(&mut old_commit, &mut new_commit);
        }
    });
    barrier.wait();
    let log_operation = codegg_git::GitOperation::Log {
        oneline: true,
        max_count: Some(1),
        paths: vec![],
    };
    for iteration in 0..16 {
        let live = contained_read(
            &git,
            repository,
            &selected_gitdir,
            &log_operation,
            &format!("concurrent-repack-{iteration}"),
        )
        .await;
        assert!(
            live.exit_status.success(),
            "contained live log failed during external repack/ref update {iteration}: {}",
            live.stderr.to_string_lossy()
        );
        assert!(
            matches!(live.sandbox, SandboxExecutionOutcome::Enforced { .. }),
            "concurrent read lacked obtained sandbox enforcement"
        );
        let actual = live.stdout.to_string_lossy();
        assert!(
            expected_logs.iter().any(|expected| expected == &*actual),
            "live Git read observed neither committed ref generation: {actual:?}"
        );
    }
    repacker.await.expect("join external repack worker");
    ref_worker.await.expect("join external ref-update worker");
    let gitdir_after_race = std::fs::metadata(&selected_gitdir).expect("stat Git directory");
    let odb_after_race = std::fs::metadata(&selected_odb).expect("stat ODB after repack race");
    assert_eq!(
        (gitdir_after_race.dev(), gitdir_after_race.ino()),
        gitdir_identity,
        "external repack replaced the selected gitdir"
    );
    assert_eq!(
        (odb_after_race.dev(), odb_after_race.ino()),
        odb_identity,
        "external repack replaced the selected ODB directory"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_only_contained_git_add_stages_index_and_objects_in_private_gitdir() {
    // A second invocation of this test binary models the daemon process that
    // crashes after making an index lock durable. The parent test starts that
    // process below, waits for it to exit, and then exercises recovery from
    // only the persisted intent and files.
    if let (Ok(source_gitdir), Ok(publish_index)) = (
        std::env::var("CODEGG_C004_RECOVERY_WORKER_GITDIR"),
        std::env::var("CODEGG_C004_RECOVERY_WORKER_INDEX"),
    ) {
        let lock_path = PathBuf::from(source_gitdir).join("index.lock");
        let bytes = std::fs::read(publish_index).expect("worker reads broker-authored index");
        let mut lock = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(lock_path)
            .expect("worker creates Git-compatible index lock");
        lock.write_all(&bytes)
            .expect("worker writes broker-authored index");
        lock.sync_all()
            .expect("worker syncs interrupted index lock");
        return;
    }

    if std::env::var(M005_OPT_IN).as_deref() != Ok("1") {
        eprintln!("skipped: set {M005_OPT_IN}=1 to run the contained Git staging pilot");
        return;
    }

    let workspace = tempfile::tempdir().expect("Git workspace");
    let repository = workspace.path().join("repository");
    std::fs::create_dir(&repository).expect("create repository root");
    let operation_scratch = tempfile::tempdir().expect("operation-owned Git scratch");
    let private_gitdir = operation_scratch.path().join("private.gitdir");
    let empty_hooks = operation_scratch.path().join("empty-hooks");
    std::fs::create_dir(&empty_hooks).expect("create empty hook directory");

    let git = std::env::split_paths(&std::env::var_os("PATH").expect("runner PATH"))
        .map(|directory| directory.join("git"))
        .find(|candidate| candidate.is_file())
        .expect("Git is installed on the native Linux runner")
        .canonicalize()
        .expect("resolve Git executable");
    let setup = |cwd: &Path, args: &[String]| {
        Command::new(&git)
            .args(args)
            .current_dir(cwd)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .expect("prepare Git staging fixture")
    };

    let init = setup(&repository, &["init".into(), "--quiet".into()]);
    assert!(init.status.success(), "git init failed: {init:?}");
    std::fs::write(repository.join("tracked.txt"), "committed base\n")
        .expect("write committed fixture");
    std::fs::write(
        repository.join(".gitattributes"),
        "tracked.txt filter=probe\n",
    )
    .expect("write repository-controlled filter attribute");
    assert!(setup(
        &repository,
        &[
            "add".into(),
            "--".into(),
            ".gitattributes".into(),
            "tracked.txt".into(),
        ]
    )
    .status
    .success());
    let commit = setup(
        &repository,
        &[
            "-c".into(),
            "user.name=Fixture".into(),
            "-c".into(),
            "user.email=fixture@example.invalid".into(),
            "commit".into(),
            "--quiet".into(),
            "-m".into(),
            "private staging fixture".into(),
        ],
    );
    assert!(
        commit.status.success(),
        "fixture commit failed: {}",
        String::from_utf8_lossy(&commit.stderr)
    );
    let filter_marker = repository.join("source-filter-ran");
    let filter_script = operation_scratch.path().join("source-filter.sh");
    std::fs::write(
        &filter_script,
        format!(
            "#!/bin/sh\nprintf ran > {}\ncat\n",
            shell_quote(&filter_marker.to_string_lossy())
        ),
    )
    .expect("write source-configured filter probe");
    let mut filter_permissions = std::fs::metadata(&filter_script)
        .expect("stat source-configured filter probe")
        .permissions();
    filter_permissions.set_mode(0o755);
    std::fs::set_permissions(&filter_script, filter_permissions)
        .expect("make source-configured filter executable");
    let configure_filter = setup(
        &repository,
        &[
            "config".into(),
            "filter.probe.clean".into(),
            filter_script.to_string_lossy().into_owned(),
        ],
    );
    assert!(
        configure_filter.status.success(),
        "configure source repository filter: {}",
        String::from_utf8_lossy(&configure_filter.stderr)
    );
    let source_gitdir = repository.join(".git");
    let source_odb = source_gitdir.join("objects");
    let source_gitdir_metadata =
        std::fs::metadata(&source_gitdir).expect("stat protected source gitdir");
    let source_odb_metadata = std::fs::metadata(&source_odb).expect("stat protected source ODB");
    let source_gitdir_identity = (source_gitdir_metadata.dev(), source_gitdir_metadata.ino());
    let source_odb_identity = (source_odb_metadata.dev(), source_odb_metadata.ino());
    let original_index =
        std::fs::read(source_gitdir.join("index")).expect("capture source index before staging");
    let head = setup(&repository, &["rev-parse".into(), "HEAD".into()]);
    assert!(
        head.status.success(),
        "resolve source HEAD failed: {head:?}"
    );
    let head = String::from_utf8_lossy(&head.stdout).trim().to_owned();

    // The alternate is broker-authored in operation scratch and points only
    // at this ordinary repository's captured object directory. The source
    // repository's own config, index and alternate files are not consulted.
    let private_init = setup(
        operation_scratch.path(),
        &[
            "init".into(),
            "--bare".into(),
            private_gitdir.to_string_lossy().into_owned(),
        ],
    );
    assert!(
        private_init.status.success(),
        "private gitdir init failed: {}",
        String::from_utf8_lossy(&private_init.stderr)
    );
    let alternates = private_gitdir.join("objects/info/alternates");
    std::fs::write(
        &alternates,
        format!("{}\n", source_gitdir.join("objects").display()),
    )
    .expect("write operation-owned alternate view");
    for args in [
        vec![
            format!("--git-dir={}", private_gitdir.display()),
            "update-ref".into(),
            "refs/heads/main".into(),
            head.clone(),
        ],
        vec![
            format!("--git-dir={}", private_gitdir.display()),
            "symbolic-ref".into(),
            "HEAD".into(),
            "refs/heads/main".into(),
        ],
        vec![
            format!("--git-dir={}", private_gitdir.display()),
            "read-tree".into(),
            "HEAD".into(),
        ],
    ] {
        let prepared = setup(&repository, &args);
        assert!(
            prepared.status.success(),
            "private index baseline setup failed for {args:?}: {}",
            String::from_utf8_lossy(&prepared.stderr)
        );
    }

    std::fs::write(
        repository.join("tracked.txt"),
        "new content must stay in private object storage\n",
    )
    .expect("write changed worktree content");
    let changed_blob = setup(
        &repository,
        &[
            "hash-object".into(),
            "--no-filters".into(),
            "tracked.txt".into(),
        ],
    );
    assert!(changed_blob.status.success(), "hash changed blob failed");
    let changed_blob = String::from_utf8_lossy(&changed_blob.stdout)
        .trim()
        .to_owned();
    assert!(
        !filter_marker.exists(),
        "test oracle unexpectedly invoked the source filter before sandbox launch"
    );

    let args = vec![
        format!("--git-dir={}", private_gitdir.display()),
        format!("--work-tree={}", repository.display()),
        "-c".into(),
        format!("core.hooksPath={}", empty_hooks.display()),
        "-c".into(),
        "core.fsmonitor=false".into(),
        "add".into(),
        "--".into(),
        "tracked.txt".into(),
    ];
    let spec = SandboxConfig::new()
        .with_enabled(true)
        .with_mode(SandboxMode::WorkspaceWrite)
        .with_allowed_paths(vec![
            repository.to_string_lossy().into_owned(),
            operation_scratch.path().to_string_lossy().into_owned(),
        ])
        .with_linux_offline_network(true)
        .launch_spec(&git, &args, Some(&repository))
        .expect("construct scoped M004/M005 Git staging launch spec");
    assert!(
        spec.protected_readonly_paths.contains(&source_gitdir),
        "contained staging launch did not protect the source gitdir"
    );
    assert!(
        spec.path_identities
            .iter()
            .any(|identity| identity.path == source_gitdir),
        "contained staging launch did not capture the source gitdir identity"
    );
    let argv = std::iter::once(spec.target.as_os_str().to_owned())
        .chain(spec.args.iter().map(OsString::from))
        .collect();
    let mut request = ManagedProcessRequest::new(
        argv,
        repository.clone(),
        ProcessProvenance::new("linux-trusted-git-c004", "private-add-staging"),
    );
    request.timeout = Some(Duration::from_secs(15));
    request.environment_policy = codegg::managed_process::EnvironmentPolicy::sanitized()
        .with_var("GIT_CONFIG_NOSYSTEM", "1")
        .with_var("GIT_CONFIG_GLOBAL", "/dev/null")
        .with_var("HOME", "/tmp/codegg-empty-home");
    request.sandbox = SandboxRequest::Required(spec);
    let result = ManagedProcessService::run(request)
        .await
        .expect("required contained staging must run or fail closed");
    assert!(
        result.exit_status.success(),
        "contained git add failed: {}",
        result.stderr.to_string_lossy()
    );
    assert!(
        matches!(result.sandbox, SandboxExecutionOutcome::Enforced { .. }),
        "staging did not report obtained enforcement"
    );
    assert!(
        !filter_marker.exists(),
        "staging loaded the original repository's filter command"
    );

    assert_eq!(
        std::fs::read(source_gitdir.join("index")).expect("source index remains readable"),
        original_index,
        "contained git add changed the protected source index"
    );
    let source_gitdir_after =
        std::fs::metadata(&source_gitdir).expect("stat source gitdir after staging");
    let source_odb_after = std::fs::metadata(&source_odb).expect("stat source ODB after staging");
    assert_eq!(
        (source_gitdir_after.dev(), source_gitdir_after.ino()),
        source_gitdir_identity,
        "contained staging replaced the protected source gitdir"
    );
    assert_eq!(
        (source_odb_after.dev(), source_odb_after.ino()),
        source_odb_identity,
        "contained staging replaced the protected source ODB"
    );
    assert!(
        !source_gitdir.join("index.lock").exists(),
        "contained git add created a source index lock"
    );
    let staged = setup(
        &repository,
        &[
            format!("--git-dir={}", private_gitdir.display()),
            "ls-files".into(),
            "--stage".into(),
            "--".into(),
            "tracked.txt".into(),
        ],
    );
    assert!(staged.status.success(), "read private staged index failed");
    assert!(
        String::from_utf8_lossy(&staged.stdout).contains(&changed_blob),
        "private index does not reference the changed blob: {}",
        String::from_utf8_lossy(&staged.stdout)
    );
    let private_object = private_gitdir
        .join("objects")
        .join(&changed_blob[..2])
        .join(&changed_blob[2..]);
    assert!(
        private_object.is_file(),
        "changed blob was not written to the operation-owned object store"
    );
    let source_object = source_gitdir
        .join("objects")
        .join(&changed_blob[..2])
        .join(&changed_blob[2..]);
    assert!(
        !source_object.exists(),
        "contained git add wrote the new object into the protected source ODB"
    );
    assert!(
        !private_gitdir.join("index.lock").exists(),
        "staging left a private index lock behind"
    );

    // Test the broker's optimistic precondition: a concurrent Git index
    // update after staging must be observed while holding the Git-compatible
    // lock, and must prevent overwriting that update.
    let external_index_update = setup(
        &repository,
        &[
            format!("--git-dir={}", source_gitdir.display()),
            "update-index".into(),
            "--assume-unchanged".into(),
            "--".into(),
            "tracked.txt".into(),
        ],
    );
    assert!(
        external_index_update.status.success(),
        "simulate concurrent source index update: {}",
        String::from_utf8_lossy(&external_index_update.stderr)
    );
    let concurrently_updated_index =
        std::fs::read(source_gitdir.join("index")).expect("read concurrent source index update");
    assert_ne!(concurrently_updated_index, original_index);
    let source_index_lock = source_gitdir.join("index.lock");
    let conflict_lock = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&source_index_lock)
        .expect("acquire Git-compatible index lock for conflict check");
    if std::fs::read(source_gitdir.join("index")).expect("recheck source index under lock")
        != original_index
    {
        drop(conflict_lock);
        std::fs::remove_file(&source_index_lock).expect("release conflict index lock");
    } else {
        panic!("broker model failed to reject a concurrently updated index");
    }
    assert_eq!(
        std::fs::read(source_gitdir.join("index")).expect("concurrent index remains intact"),
        concurrently_updated_index,
        "conflict handling overwrote the concurrent index update"
    );

    // Restore this disposable fixture's baseline after the conflict probe.
    // The publish prototype below uses the captured bytes as its expected
    // index generation and exercises only this one index resource.
    std::fs::write(source_gitdir.join("index"), &original_index)
        .expect("restore disposable source index baseline");
    let private_index =
        std::fs::read(private_gitdir.join("index")).expect("read operation-private staged index");
    let private_blob = Command::new(&git)
        .args([
            format!("--git-dir={}", private_gitdir.display()),
            "cat-file".into(),
            "blob".into(),
            changed_blob.clone(),
        ])
        .current_dir(&repository)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("read validated staged blob for broker import");
    assert!(
        private_blob.status.success(),
        "read private staged blob failed: {}",
        String::from_utf8_lossy(&private_blob.stderr)
    );
    assert_eq!(
        private_blob.stdout, b"new content must stay in private object storage\n",
        "private staged blob does not match the authorized worktree content"
    );

    let mut import = Command::new(&git)
        .args([
            format!("--git-dir={}", source_gitdir.display()),
            "hash-object".into(),
            "-w".into(),
            "--stdin".into(),
            "--no-filters".into(),
        ])
        .current_dir(&repository)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start trusted broker object import");
    import
        .stdin
        .take()
        .expect("broker object import stdin")
        .write_all(&private_blob.stdout)
        .expect("stream validated blob to source ODB");
    let imported = import
        .wait_with_output()
        .expect("wait for trusted broker object import");
    assert!(
        imported.status.success(),
        "broker object import failed: {}",
        String::from_utf8_lossy(&imported.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&imported.stdout).trim(),
        changed_blob,
        "source ODB import returned a different object identity"
    );

    // Persist a minimal operation-owned recovery intent before touching the
    // source index. This remains a test-only broker model: it exercises the
    // restart ordering and identity checks without granting the child access
    // to source metadata.
    let recovery_dir = operation_scratch.path().join("index-promotion-recovery");
    std::fs::create_dir(&recovery_dir).expect("create operation recovery directory");
    let source_gitdir_before_recovery =
        std::fs::metadata(&source_gitdir).expect("capture recovery gitdir identity");
    let expected_gitdir_identity = (
        source_gitdir_before_recovery.dev(),
        source_gitdir_before_recovery.ino(),
    );
    let recovery_expected_index = recovery_dir.join("expected-index");
    let recovery_publish_index = recovery_dir.join("publish-index");
    std::fs::write(&recovery_expected_index, &original_index)
        .expect("persist expected source index generation");
    std::fs::write(&recovery_publish_index, &private_index)
        .expect("persist validated private index output");
    for recovery_file in [&recovery_expected_index, &recovery_publish_index] {
        std::fs::File::open(recovery_file)
            .and_then(|file| file.sync_all())
            .expect("sync operation recovery input");
    }
    let recovery_intent = recovery_dir.join("intent");
    std::fs::write(
        &recovery_intent,
        format!(
            "index-v1\n{}\n{}\n",
            expected_gitdir_identity.0, expected_gitdir_identity.1
        ),
    )
    .expect("persist index promotion intent");
    std::fs::File::open(&recovery_intent)
        .and_then(|file| file.sync_all())
        .expect("sync index promotion intent");
    std::fs::File::open(&recovery_dir)
        .and_then(|directory| directory.sync_all())
        .expect("sync operation recovery directory");

    assert_eq!(
        std::fs::read(source_gitdir.join("index")).expect("revalidate source index"),
        original_index,
        "source index changed between broker validation and lock acquisition"
    );

    // Terminate a separate process after the lock file is durable but before
    // its atomic rename. Recovery must reacquire all authority from the
    // durable intent, validate the selected gitdir identity and expected
    // index, then finish only the exact broker-authored replacement.
    drop(private_index);
    let worker = std::env::current_exe().expect("resolve test binary for recovery worker");
    let interrupted = Command::new(worker)
        .args([
            "--exact",
            "test_only_contained_git_add_stages_index_and_objects_in_private_gitdir",
            "--nocapture",
        ])
        .env("CODEGG_C004_RECOVERY_WORKER_GITDIR", &source_gitdir)
        .env("CODEGG_C004_RECOVERY_WORKER_INDEX", &recovery_publish_index)
        .output()
        .expect("start isolated interrupted-publisher process");
    assert!(
        interrupted.status.success(),
        "interrupted-publisher process failed: stdout={} stderr={}",
        String::from_utf8_lossy(&interrupted.stdout),
        String::from_utf8_lossy(&interrupted.stderr)
    );
    let intent = std::fs::read_to_string(&recovery_intent).expect("read recovery intent");
    let mut intent_lines = intent.lines();
    assert_eq!(intent_lines.next(), Some("index-v1"));
    let intent_dev = intent_lines
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .expect("parse recovery gitdir device identity");
    let intent_ino = intent_lines
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .expect("parse recovery gitdir inode identity");
    assert!(
        intent_lines.next().is_none(),
        "unexpected recovery intent data"
    );

    // Recovery must bind to the original gitdir inode, not merely trust the
    // configured path. Replace that path after the publisher exits and prove
    // the identity check rejects the replacement before it can touch it.
    let displaced_gitdir =
        source_gitdir.with_file_name(format!(".git-c004-displaced-{}", std::process::id()));
    assert!(
        !displaced_gitdir.exists(),
        "gitdir identity probe path already exists"
    );
    std::fs::rename(&source_gitdir, &displaced_gitdir)
        .expect("displace source gitdir for identity substitution probe");
    std::fs::create_dir(&source_gitdir).expect("install replacement source gitdir path");
    let replacement_gitdir =
        std::fs::metadata(&source_gitdir).expect("inspect replacement gitdir identity");
    assert_ne!(
        (replacement_gitdir.dev(), replacement_gitdir.ino()),
        (intent_dev, intent_ino),
        "replacement gitdir unexpectedly has the authorized identity"
    );
    assert!(
        !source_gitdir.join("index").exists() && !source_gitdir.join("index.lock").exists(),
        "identity-mismatched recovery path was modified"
    );
    let displaced_lock = displaced_gitdir.join("index.lock");
    assert!(
        displaced_lock.is_file(),
        "displaced authorized gitdir lost the interrupted publisher lock"
    );
    std::fs::remove_dir(&source_gitdir).expect("remove empty replacement gitdir");
    std::fs::rename(&displaced_gitdir, &source_gitdir)
        .expect("restore original source gitdir after identity probe");

    let recovery_gitdir =
        std::fs::metadata(&source_gitdir).expect("revalidate recovery gitdir identity");
    assert_eq!(
        (recovery_gitdir.dev(), recovery_gitdir.ino()),
        (intent_dev, intent_ino),
        "recovery target gitdir identity changed"
    );
    let expected_recovery_index =
        std::fs::read(&recovery_expected_index).expect("read expected recovery index");
    assert_eq!(
        std::fs::read(source_gitdir.join("index")).expect("revalidate expected source index"),
        expected_recovery_index,
        "recovery must reject an externally changed source index"
    );

    // A path replacement between publisher exit and recovery must not redirect
    // the broker to an attacker-selected file. Verify the no-follow open
    // rejects a symlink at Git's conventional lock path, then restore the
    // exact durable lock created by the interrupted publisher.
    let displaced_lock = recovery_dir.join("displaced-index-lock");
    std::fs::rename(&source_index_lock, &displaced_lock)
        .expect("move interrupted lock for symlink replacement probe");
    std::os::unix::fs::symlink(&recovery_publish_index, &source_index_lock)
        .expect("replace Git index lock with a symlink probe");
    let symlink_open = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(&source_index_lock);
    assert!(
        symlink_open.is_err(),
        "recovery followed a symlink at the Git index lock path"
    );
    std::fs::remove_file(&source_index_lock).expect("remove symlink lock probe");
    std::fs::rename(&displaced_lock, &source_index_lock)
        .expect("restore interrupted Git index lock");

    let recovered_index =
        std::fs::read(&recovery_publish_index).expect("read validated recovery index");
    let lock_file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(&source_index_lock)
        .expect("open interrupted Git index lock without following links");
    let lock_metadata = lock_file
        .metadata()
        .expect("inspect interrupted Git index lock");
    assert!(
        lock_metadata.is_file(),
        "recovery lock is not a regular file"
    );
    let lock_path_metadata = std::fs::symlink_metadata(&source_index_lock)
        .expect("inspect interrupted Git index lock path");
    assert!(
        lock_path_metadata.file_type().is_file(),
        "recovery lock path was replaced by a symlink or special file"
    );
    assert_eq!(
        (lock_metadata.dev(), lock_metadata.ino()),
        (lock_path_metadata.dev(), lock_path_metadata.ino()),
        "recovery lock changed while it was being validated"
    );
    let mut lock_bytes = Vec::new();
    let mut lock_file = lock_file;
    lock_file
        .read_to_end(&mut lock_bytes)
        .expect("read interrupted Git index lock");
    assert_eq!(
        lock_bytes, recovered_index,
        "recovery lock does not match the broker-authored index"
    );

    // Model a non-Git writer replacing index after the publisher died but
    // before recovery publishes. Recovery must preserve that winner and leave
    // its staged lock untouched instead of replaying a stale transaction.
    let competing_index_path = recovery_dir.join("competing-index");
    std::fs::write(&competing_index_path, &concurrently_updated_index)
        .expect("write out-of-band competing index generation");
    std::fs::File::open(&competing_index_path)
        .and_then(|file| file.sync_all())
        .expect("sync competing index generation");
    std::fs::rename(&competing_index_path, source_gitdir.join("index"))
        .expect("atomically install out-of-band competing index generation");
    let competing_source_index =
        std::fs::read(source_gitdir.join("index")).expect("read competing source index");
    assert_eq!(competing_source_index, concurrently_updated_index);
    assert_ne!(
        competing_source_index, expected_recovery_index,
        "recovery must observe the external index generation change"
    );
    assert_eq!(
        std::fs::read(&source_index_lock).expect("read staged lock after conflict"),
        recovered_index,
        "a recovery conflict must not alter the broker-authored lock"
    );

    // Restore the disposable fixture's admitted generation so the succeeding
    // branch can prove the exact prepared output is publishable.
    let restore_index_path = recovery_dir.join("restore-index");
    std::fs::write(&restore_index_path, &expected_recovery_index)
        .expect("write expected index generation for recovery branch");
    std::fs::File::open(&restore_index_path)
        .and_then(|file| file.sync_all())
        .expect("sync expected index generation");
    std::fs::rename(&restore_index_path, source_gitdir.join("index"))
        .expect("restore expected source index generation");
    assert_eq!(
        std::fs::read(source_gitdir.join("index")).expect("recheck restored source index"),
        expected_recovery_index,
        "successful recovery requires the original expected index generation"
    );

    std::fs::rename(&source_index_lock, source_gitdir.join("index"))
        .expect("recover atomic publication of validated staged index");
    std::fs::File::open(&source_gitdir)
        .and_then(|directory| directory.sync_all())
        .expect("sync source gitdir after index recovery");

    assert_eq!(
        std::fs::read(source_gitdir.join("index")).expect("read published source index"),
        recovered_index,
        "recovery did not publish the exact validated private index"
    );
    let source_has_blob = Command::new(&git)
        .args([
            format!("--git-dir={}", source_gitdir.display()),
            "cat-file".into(),
            "-e".into(),
            changed_blob.clone(),
        ])
        .current_dir(&repository)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .status()
        .expect("verify imported source object");
    assert!(
        source_has_blob.success(),
        "imported source object is absent"
    );
    assert!(
        !source_index_lock.exists(),
        "broker promotion left the source index lock behind"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_network_runs_cargo_metadata_and_check_without_network() {
    if std::env::var(M005_OPT_IN).as_deref() != Ok("1") {
        eprintln!("skipped: set {M005_OPT_IN}=1 for the native M005 toolchain probe");
        return;
    }
    if let Err(reason) = codegg::security::sandbox::landlock::probe_process_scopes() {
        panic!("M005 offline toolchain probe requires qualified M004 setup: {reason}");
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

    let rustup_home = shell_quote(&rustup_home.to_string_lossy());
    let cargo_bin = shell_quote(&cargo_home.join("bin").to_string_lossy());
    let cargo_home = shell_quote(&cargo_home.to_string_lossy());
    let linker_dir = shell_quote(
        &linker
            .parent()
            .expect("C linker has a parent directory")
            .to_string_lossy(),
    );
    let linker = shell_quote(&linker.to_string_lossy());
    let linker_env = shell_quote(&linker_env);
    let manifest = shell_quote(&workspace.join("Cargo.toml").to_string_lossy());
    let rustup = shell_quote(&rustup.to_string_lossy());
    let script = format!(
        "export RUSTUP_HOME={rustup_home} CARGO_HOME={cargo_home} CARGO_NET_OFFLINE=true CARGO_BUILD_JOBS=2 CC={linker} HOST_CC={linker} {linker_env}={linker}; export PATH={linker_dir}:{cargo_bin}:$PATH; {rustup} run stable cargo metadata --offline --locked --no-deps --format-version 1 --manifest-path {manifest} >/dev/null && {rustup} run stable cargo check --offline --locked --workspace --manifest-path {manifest}"
    );

    let spec = SandboxConfig::new()
        .with_enabled(true)
        .with_mode(SandboxMode::WorkspaceWrite)
        .with_linux_offline_network(true)
        .launch_spec("/bin/sh", &["-c".into(), script], Some(&workspace))
        .expect("offline M005 toolchain launch spec");
    let mut request = ManagedProcessRequest::new(
        [spec.target.as_os_str().to_owned()]
            .into_iter()
            .chain(spec.args.iter().map(OsString::from))
            .collect(),
        workspace,
        ProcessProvenance::new("linux-m005", "offline-cargo-workspace"),
    );
    request.timeout = Some(Duration::from_secs(15 * 60));
    request.sandbox = SandboxRequest::Required(spec);

    let result = ManagedProcessService::run(request)
        .await
        .expect("required offline toolchain namespace should launch");
    assert!(
        result.exit_status.success(),
        "offline Cargo compatibility failed (status {:?}); stdout: {}; stderr: {}",
        result.exit_status,
        result.stdout.to_string_lossy(),
        result.stderr.to_string_lossy()
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
