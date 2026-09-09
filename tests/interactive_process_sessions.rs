//! Integration fixtures for M001: scheduler-owned PTY engine.
//!
//! These tests exercise the real supported-host PTY backend (Unix
//! `openpty`): genuinely interactive round-trips, resize observation,
//! bounded scrollback under large output, scheduler admission release,
//! workspace/environment policy, process-group cleanup, TERM/KILL
//! escalation, shutdown, and failure semantics. They are the M001
//! "supported platform fixture" evidence.

use std::ffi::OsString;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use codegg::interactive_process::{
    is_supported, platform_name, InteractiveHandle, InteractiveProcessService, PtySize,
    SessionState, SpawnSpec,
};
use codegg::scheduler::admission::AdmissionController;
use codegg_core::workspace::{ExecutionContext, InMemoryWorkspaceStore, WorkspaceRegistry};
use codegg_core::workspace_services::{
    ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
};
use tokio_util::sync::CancellationToken;

const POLL_TIMEOUT: Duration = Duration::from_secs(15);

async fn test_context(root: &Path) -> Arc<ExecutionContext> {
    let store = Arc::new(InMemoryWorkspaceStore::new());
    let registry = WorkspaceRegistry::load(store).await.expect("registry");
    let record = registry.get_or_register(root).await.expect("register");
    let _services = WorkspaceServiceRegistry::new(
        registry,
        Arc::new(ProductionWorkspaceServicesFactory),
        WorkspaceServicePolicy::default(),
    );
    ExecutionContext::new(
        record,
        Some("session-integration".to_string()),
        CancellationToken::new(),
    )
}

fn test_admission(process_slots: u32) -> Arc<AdmissionController> {
    let mut config = codegg::scheduler::config::ResolvedSchedulerConfig::default();
    config.resources.max_process_slots = process_slots;
    Arc::new(AdmissionController::new(config))
}

fn service() -> InteractiveProcessService {
    InteractiveProcessService::new(test_admission(8))
}

/// Poll scrollback until `predicate` holds or the timeout expires.
async fn wait_for_output(
    service: &InteractiveProcessService,
    handle: &InteractiveHandle,
    predicate: impl Fn(&str) -> bool,
) -> String {
    let started = Instant::now();
    let mut seq = 0_u64;
    let mut collected = Vec::new();
    while started.elapsed() < POLL_TIMEOUT {
        let read = service
            .read_output(handle, seq, 64 * 1024)
            .await
            .expect("read output");
        seq = read.next_seq;
        collected.extend_from_slice(&read.bytes);
        let text = String::from_utf8_lossy(&collected).into_owned();
        if predicate(&text) {
            return text;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!(
        "timed out waiting for PTY output; got: {:?}",
        String::from_utf8_lossy(&collected)
    );
}

async fn wait_for_exit(
    service: &InteractiveProcessService,
    handle: &InteractiveHandle,
) -> codegg::interactive_process::SessionSnapshot {
    let started = Instant::now();
    loop {
        let snapshot = service.snapshot(handle).await.expect("snapshot");
        if matches!(snapshot.state, SessionState::Exited) {
            return snapshot;
        }
        if started.elapsed() > POLL_TIMEOUT {
            panic!("timed out waiting for session exit");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn platform_fixture_reports_supported_unix_host() {
    assert!(is_supported(), "integration fixture requires a Unix host");
    assert!(!platform_name().is_empty());
    let dir = tempfile::tempdir().expect("workspace");
    let ctx = test_context(dir.path()).await;
    let service = service();
    let handle = service
        .spawn(&ctx, SpawnSpec::new(vec![OsString::from("true")]))
        .await
        .expect("true spawns");
    let snapshot = wait_for_exit(&service, &handle).await;
    assert_eq!(snapshot.exit.map(|exit| exit.code), Some(Some(0)));
    let _ = service.remove(&handle).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn interactive_cat_round_trip_accepts_input() {
    let dir = tempfile::tempdir().expect("workspace");
    let ctx = test_context(dir.path()).await;
    let service = service();
    let handle = service
        .spawn(&ctx, SpawnSpec::new(vec![OsString::from("cat")]))
        .await
        .expect("cat spawns");
    service
        .write_input(&handle, b"pty-round-trip\n")
        .await
        .expect("write input");
    let text = wait_for_output(&service, &handle, |text| text.contains("pty-round-trip")).await;
    assert!(text.contains("pty-round-trip"));
    let snapshot = service.snapshot(&handle).await.expect("snapshot");
    assert!(matches!(snapshot.state, SessionState::Running));
    assert!(snapshot.next_seq > 0);
    service.terminate(&handle).await.expect("terminate");
    let _ = service.remove(&handle).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn interactive_shell_runs_commands_and_reports_exit_status() {
    let dir = tempfile::tempdir().expect("workspace");
    let ctx = test_context(dir.path()).await;
    let service = service();
    let handle = service
        .spawn(&ctx, SpawnSpec::new(vec![OsString::from("sh")]))
        .await
        .expect("sh spawns");
    service
        .write_input(&handle, b"echo pty-shell-alive\n")
        .await
        .expect("write echo");
    wait_for_output(&service, &handle, |text| text.contains("pty-shell-alive")).await;
    service
        .write_input(&handle, b"exit 7\n")
        .await
        .expect("write exit");
    let snapshot = wait_for_exit(&service, &handle).await;
    assert_eq!(snapshot.exit.map(|exit| exit.code), Some(Some(7)));
    // Scrollback survives natural exit for post-mortem reads.
    let read = service
        .read_output(&handle, 0, 64 * 1024)
        .await
        .expect("post-exit read");
    assert!(String::from_utf8_lossy(&read.bytes).contains("pty-shell-alive"));
    // Input after exit is rejected; the permit was already released.
    assert!(service.write_input(&handle, b"nope\n").await.is_err());
    let _ = service.remove(&handle).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resize_updates_size_and_observes_winsize() {
    let dir = tempfile::tempdir().expect("workspace");
    let ctx = test_context(dir.path()).await;
    let service = service();
    let handle = service
        .spawn(&ctx, SpawnSpec::new(vec![OsString::from("sh")]))
        .await
        .expect("sh spawns");
    let size = PtySize::new(100, 40).expect("size");
    service.resize(&handle, size).await.expect("resize");
    let snapshot = service.snapshot(&handle).await.expect("snapshot");
    assert_eq!(snapshot.size, size);
    // Portable size observation: the child sees the new winsize.
    service
        .write_input(&handle, b"stty size\n")
        .await
        .expect("write stty");
    let text = wait_for_output(&service, &handle, |text| text.contains("40 100")).await;
    assert!(text.contains("40 100"));
    service.terminate(&handle).await.expect("terminate");
    let _ = service.remove(&handle).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn large_output_is_bounded_with_sequence_cursors() {
    let dir = tempfile::tempdir().expect("workspace");
    let ctx = test_context(dir.path()).await;
    let service = service();
    let handle = service
        .spawn(
            &ctx,
            SpawnSpec::new(vec![
                OsString::from("sh"),
                OsString::from("-c"),
                OsString::from("yes | head -c 300000"),
            ]),
        )
        .await
        .expect("yes spawns");
    let snapshot = wait_for_exit(&service, &handle).await;
    assert!(
        snapshot.total_bytes > snapshot.retained_bytes as u64,
        "expected truncation: {snapshot:?}"
    );
    assert!(snapshot.truncated);
    assert!(snapshot.retained_bytes <= codegg::interactive_process::DEFAULT_SCROLLBACK_BYTES);
    // Sequence cursors advance monotonically; rereading from the cursor is stable.
    let first = service
        .read_output(&handle, 0, 1024)
        .await
        .expect("read head");
    assert!(first.gap, "head read must report the truncation gap");
    let tail = service
        .read_output(&handle, first.next_seq.saturating_sub(512), 512)
        .await
        .expect("read tail");
    assert!(!tail.gap);
    assert_eq!(tail.next_seq, snapshot.next_seq);
    let _ = service.remove(&handle).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn environment_overrides_apply_while_denied_vars_stay_stripped() {
    let dir = tempfile::tempdir().expect("workspace");
    let ctx = test_context(dir.path()).await;
    let service = service();
    let mut spec = SpawnSpec::new(vec![
        OsString::from("sh"),
        OsString::from("-c"),
        OsString::from("env"),
    ]);
    spec.env_overrides = vec![
        (
            OsString::from("CODEGG_PTY_TEST_VAR"),
            OsString::from("pty-visible"),
        ),
        (OsString::from("LD_PRELOAD"), OsString::from("/tmp/evil.so")),
    ];
    let handle = service.spawn(&ctx, spec).await.expect("env spawns");
    let snapshot = wait_for_exit(&service, &handle).await;
    assert!(matches!(snapshot.state, SessionState::Exited));
    let read = service
        .read_output(&handle, 0, 64 * 1024)
        .await
        .expect("read env");
    let text = String::from_utf8_lossy(&read.bytes).into_owned();
    assert!(text.contains("CODEGG_PTY_TEST_VAR=pty-visible"), "{text}");
    assert!(!text.contains("LD_PRELOAD"), "{text}");
    assert!(text.contains("TERM=xterm-256color"), "{text}");
    let _ = service.remove(&handle).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn terminate_cleans_the_process_group_tree() {
    let dir = tempfile::tempdir().expect("workspace");
    let ctx = test_context(dir.path()).await;
    let service = service();
    let handle = service
        .spawn(
            &ctx,
            SpawnSpec::new(vec![
                OsString::from("sh"),
                OsString::from("-c"),
                OsString::from("sleep 60 & echo CHILD:$!; wait"),
            ]),
        )
        .await
        .expect("sleeper spawns");
    let text = wait_for_output(&service, &handle, |text| text.contains("CHILD:")).await;
    let pid: i32 = text
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("CHILD:")
                .and_then(|pid| pid.trim().parse().ok())
        })
        .expect("child pid in output");
    service.terminate(&handle).await.expect("terminate");
    // The backgrounded descendant must die with the group, not linger.
    let started = Instant::now();
    loop {
        #[allow(unsafe_code)]
        let alive = unsafe { libc::kill(pid, 0) } == 0;
        if !alive {
            break;
        }
        if started.elapsed() > Duration::from_secs(5) {
            panic!("descendant pid {pid} survived process-group termination");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let _ = service.remove(&handle).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn terminate_escalates_past_sigterm_traps() {
    let dir = tempfile::tempdir().expect("workspace");
    let ctx = test_context(dir.path()).await;
    let service = service();
    let handle = service
        .spawn(
            &ctx,
            SpawnSpec::new(vec![
                OsString::from("sh"),
                OsString::from("-c"),
                OsString::from("trap \"\" TERM; sleep 30"),
            ]),
        )
        .await
        .expect("trap spawns");
    let started = Instant::now();
    service
        .terminate_with_grace(&handle, Duration::from_millis(200))
        .await
        .expect("escalated terminate");
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "SIGKILL escalation must bound termination"
    );
    let snapshot = service.snapshot(&handle).await.expect("snapshot");
    assert!(matches!(snapshot.state, SessionState::Exited));
    let _ = service.remove(&handle).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn permits_release_on_exit_and_terminate() {
    let dir = tempfile::tempdir().expect("workspace");
    let ctx = test_context(dir.path()).await;
    let admission = test_admission(1);
    let service = InteractiveProcessService::new(admission.clone());

    // Natural exit releases the single slot for the next spawn.
    let first = service
        .spawn(&ctx, SpawnSpec::new(vec![OsString::from("true")]))
        .await
        .expect("first admitted");
    wait_for_exit(&service, &first).await;
    assert_eq!(admission.used_process_slots(), 0);
    let second = service
        .spawn(&ctx, SpawnSpec::new(vec![OsString::from("cat")]))
        .await
        .expect("slot released after exit");
    // Termination also releases the slot.
    service.terminate(&second).await.expect("terminate");
    assert_eq!(admission.used_process_slots(), 0);
    let third = service
        .spawn(&ctx, SpawnSpec::new(vec![OsString::from("cat")]))
        .await
        .expect("slot released after terminate");
    service.shutdown().await;
    let _ = service.remove(&first).await;
    let _ = service.remove(&second).await;
    let _ = service.remove(&third).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_terminates_everything_and_rejects_new_spawns() {
    let dir = tempfile::tempdir().expect("workspace");
    let ctx = test_context(dir.path()).await;
    let service = InteractiveProcessService::new(test_admission(8));
    let first = service
        .spawn(&ctx, SpawnSpec::new(vec![OsString::from("cat")]))
        .await
        .expect("first spawns");
    let second = service
        .spawn(&ctx, SpawnSpec::new(vec![OsString::from("sleep")]))
        .await
        .expect("second spawns");
    // `sleep` without argv waits on stdin briefly; give both a reader tick.
    tokio::time::sleep(Duration::from_millis(100)).await;
    service.shutdown().await;
    for handle in [&first, &second] {
        let snapshot = service.snapshot(handle).await.expect("snapshot");
        assert!(
            matches!(snapshot.state, SessionState::Exited),
            "shutdown must exit every session: {snapshot:?}"
        );
    }
    let error = service
        .spawn(&ctx, SpawnSpec::new(vec![OsString::from("cat")]))
        .await
        .expect_err("post-shutdown spawn must fail");
    assert!(matches!(
        error,
        codegg::interactive_process::InteractiveError::ShuttingDown
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn removed_handles_are_gone() {
    let dir = tempfile::tempdir().expect("workspace");
    let ctx = test_context(dir.path()).await;
    let service = service();
    let handle = service
        .spawn(&ctx, SpawnSpec::new(vec![OsString::from("true")]))
        .await
        .expect("true spawns");
    wait_for_exit(&service, &handle).await;
    service.remove(&handle).await.expect("remove");
    assert!(matches!(
        service.snapshot(&handle).await,
        Err(codegg::interactive_process::InteractiveError::UnknownHandle)
    ));
}
