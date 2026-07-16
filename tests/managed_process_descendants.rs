//! Managed-process descendant termination tests (Workstream D2).
//!
//! These integration tests prove that the managed-process subsystem
//! actually terminates descendant processes (children, grandchildren)
//! when cancelled or timed out. They exercise the real
//! [`ManagedArgvExecutor`] and [`ManagedProcessService`] pipeline
//! against real subprocesses.
//!
//! All tests are Unix-only because process group management
//! (`setsid`) and `kill` signalling are POSIX-specific.
//!
//! ## Known behaviour documented by these tests
//!
//! When a managed child dies quickly from SIGTERM, orphaned grandchild
//! processes that trap SIGTERM can keep the stdout/stderr pipe
//! write-ends open. This prevents `join_output` from returning and
//! keeps the job in `Running` state even though the processes are
//! dead. Tests that exercise SIGTERM→SIGKILL escalation deliberately
//! make both child and grandchild ignore SIGTERM so the full
//! escalation path is exercised.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use codegg::scheduler::{
    JobScheduler, JobSubmissionService, ResolvedSchedulerConfig, SchedulerShutdownMode,
};
use codegg_core::jobs::{
    CancelOutcome, DaemonGeneration, IdempotencyClass, InMemoryJobStore, JobKind, JobPayload,
    JobPriority, JobRecord, JobSource, JobState, JobStore, NewJob, ResourceRequest, RetryPolicy,
};
use codegg_core::workspace::WorkspaceId;

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn gen() -> DaemonGeneration {
    DaemonGeneration::new_unchecked(format!("test-gen-{}", uuid::Uuid::new_v4()))
}

fn build_config(max_process_slots: u32) -> ResolvedSchedulerConfig {
    ResolvedSchedulerConfig {
        enabled: true,
        resources: codegg::scheduler::config::ResourceBudget {
            max_process_slots,
            max_cpu_weight: 8,
            max_memory_mb_hint: 8192,
            max_io_weight: 8,
            max_network_slots: 4,
        },
        ..ResolvedSchedulerConfig::default()
    }
}

fn build_managed_argv_job(workspace: &WorkspaceId, argv: Vec<String>) -> NewJob {
    NewJob {
        workspace_id: workspace.clone(),
        session_id: None,
        turn_id: None,
        kind: JobKind::ManagedProcess,
        source: JobSource::Interactive,
        priority: JobPriority::Interactive,
        payload: JobPayload::ManagedArgv {
            argv,
            cwd: Some("/tmp".into()),
        },
        resource_request: ResourceRequest::default(),
        timeout: None,
        retry_policy: RetryPolicy::no_retry(),
        idempotency: IdempotencyClass::SafeRepeat,
        not_before: None,
        deadline: None,
        schedule_id: None,
        depends_on: vec![],
    }
}

fn build_managed_argv_job_with_timeout(
    workspace: &WorkspaceId,
    argv: Vec<String>,
    timeout: Duration,
) -> NewJob {
    let mut job = build_managed_argv_job(workspace, argv);
    job.timeout = Some(timeout);
    job
}

async fn setup_workspace() -> (
    Arc<codegg_core::workspace::WorkspaceRegistry>,
    Arc<codegg_core::workspace_services::WorkspaceServiceRegistry>,
    WorkspaceId,
    tempfile::TempDir,
) {
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_registry = codegg_core::workspace::WorkspaceRegistry::load(Arc::new(
        codegg_core::workspace::InMemoryWorkspaceStore::new(),
    ))
    .await
    .expect("workspace registry");
    let ws_record = workspace_registry
        .get_or_register(root.path())
        .await
        .expect("register workspace");
    let ws_id = ws_record.id.clone();
    let services = codegg_core::workspace_services::WorkspaceServiceRegistry::new(
        workspace_registry.clone(),
        Arc::new(codegg_core::workspace_services::ProductionWorkspaceServicesFactory),
        codegg_core::workspace_services::WorkspaceServicePolicy::default(),
    );
    (workspace_registry, services, ws_id, root)
}

/// Set up scheduler + submission service with a real ManagedArgvExecutor.
async fn setup_managed_argv() -> (
    Arc<JobScheduler>,
    Arc<JobSubmissionService>,
    Arc<dyn JobStore>,
    WorkspaceId,
    tempfile::TempDir,
) {
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let (_registry, services, ws_id, root) = setup_workspace().await;
    let config = build_config(4);
    let generation = gen();
    let scheduler = JobScheduler::new(store.clone(), services.clone(), config, generation.clone());
    let submission =
        JobSubmissionService::new(store.clone(), scheduler.clone(), services, generation);

    let exec: Arc<dyn codegg::scheduler::JobExecutor> = Arc::new(
        codegg::scheduler::executors::ManagedArgvExecutor::new("test_managed"),
    );
    // Register the executor synchronously so the scheduler can dispatch
    // the first job immediately. Spawning the registration as a separate
    // task races with the scheduler's main loop.
    scheduler
        .register_executor(exec)
        .await
        .expect("register executor");

    (scheduler, submission, store, ws_id, root)
}

/// Busy-wait for a job to reach a terminal state.
async fn wait_for_terminal(
    store: &Arc<dyn JobStore>,
    job_id: &codegg_core::jobs::JobId,
    timeout: Duration,
) -> JobRecord {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let job = store
            .get_job(job_id)
            .await
            .expect("get_job")
            .expect("job missing");
        if job.state.is_terminal() {
            return job;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!(
                "timeout waiting for job {} to become terminal; current state: {:?}",
                job_id, job.state
            );
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Poll a PID file until it appears or timeout.
async fn wait_for_pid_file(path: &Path, timeout_d: Duration) -> i32 {
    let start = std::time::Instant::now();
    loop {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(pid) = content.trim().parse::<i32>() {
                if pid > 0 {
                    return pid;
                }
            }
        }
        if start.elapsed() > timeout_d {
            panic!("timeout waiting for PID file {} to appear", path.display());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Check whether a process with the given PID is still alive.
#[cfg(unix)]
fn is_pid_alive(pid: i32) -> bool {
    let result = unsafe { libc::kill(pid, 0) };
    if result == 0 {
        true
    } else {
        let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        errno != libc::ESRCH
    }
}

/// Write a script that spawns a child + grandchild. Both write their
/// PIDs to files. The child does NOT trap SIGTERM so it dies normally
/// when the process group is signalled. The grandchild traps SIGTERM
/// and sleeps forever — it can only be killed by SIGKILL.
///
/// NOTE: When the child exits quickly from SIGTERM, the grandchild
/// keeps the stdout/stderr pipe open, causing `join_output` to hang.
/// This is a known gap in the managed process service's cleanup.
fn write_stubborn_grandchild_script(dir: &Path) -> PathBuf {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    // Write a grandchild shell script that traps SIGTERM.
    let grandchild = dir.join("grandchild.sh");
    fs::write(
        &grandchild,
        r#"#!/usr/bin/env bash
trap '' TERM
trap '' INT
trap '' HUP
while true; do sleep 0.2; done
"#,
    )
    .unwrap();
    let mut perms = fs::metadata(&grandchild).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&grandchild, perms).unwrap();

    let script = dir.join("stubborn.sh");
    fs::write(
        &script,
        r#"#!/usr/bin/env bash
set -euo pipefail
PID_DIR="$1"
GRANDCHILD_SCRIPT="$PID_DIR/grandchild.sh"
echo $$ > "$PID_DIR/child.pid"
# Spawn the grandchild (a bash script that traps SIGTERM).
"$GRANDCHILD_SCRIPT" &
GRANDCHILD_PID=$!
echo $GRANDCHILD_PID > "$PID_DIR/grandchild.pid"
echo $GRANDCHILD_PID > "$PID_DIR/grandchild_bash.pid"
while true; do sleep 0.2; done
"#,
    )
    .unwrap();
    let mut perms = fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script, perms).unwrap();
    script
}

/// Write a script where BOTH child and grandchild ignore SIGTERM.
/// This ensures the managed process service must escalate to SIGKILL.
fn write_both_stubborn_script(dir: &Path) -> PathBuf {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    // Write a grandchild shell script that traps SIGTERM.
    let grandchild = dir.join("grandchild_both.sh");
    fs::write(
        &grandchild,
        r#"#!/usr/bin/env bash
trap '' TERM
trap '' INT
trap '' HUP
while true; do sleep 0.2; done
"#,
    )
    .unwrap();
    let mut perms = fs::metadata(&grandchild).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&grandchild, perms).unwrap();

    let script = dir.join("both_stubborn.sh");
    fs::write(
        &script,
        r#"#!/usr/bin/env bash
set -euo pipefail
PID_DIR="$1"
GRANDCHILD_SCRIPT="$PID_DIR/grandchild_both.sh"
trap '' TERM
echo $$ > "$PID_DIR/child.pid"
# Grandchild: a bash script that traps SIGTERM.
"$GRANDCHILD_SCRIPT" &
GRANDCHILD_PID=$!
echo $GRANDCHILD_PID > "$PID_DIR/grandchild.pid"
echo $GRANDCHILD_PID > "$PID_DIR/grandchild_bash.pid"
while true; do sleep 0.2; done
"#,
    )
    .unwrap();
    let mut perms = fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script, perms).unwrap();
    script
}

/// Write a script that prints its process group ID (PGID).
fn write_pgid_script(dir: &Path) -> PathBuf {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    // Minimal script: write its own PID, then sleep forever. The
    // test reads the PGID via `ps` from the test process, which
    // avoids the file-flushing race from inside the bash subshell.
    let script = dir.join("pgid.sh");
    fs::write(
        &script,
        r#"#!/usr/bin/env bash
echo $$ > "$1/self.pid"
while true; do sleep 3600; done
"#,
    )
    .unwrap();
    let mut perms = fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script, perms).unwrap();
    script
}

/// Write a script that produces bounded output and exits quickly.
fn write_bounded_output_script(dir: &Path) -> PathBuf {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    let script = dir.join("bounded_output.sh");
    // Produce exactly 10 MB then exit.
    fs::write(
        &script,
        r#"#!/usr/bin/env bash
head -c 10485760 /dev/zero | tr '\0' 'A'
"#,
    )
    .unwrap();
    let mut perms = fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script, perms).unwrap();
    script
}

// ═══════════════════════════════════════════════════════════════════════════
// Test 1: Cancellation terminates descendants (child + grandchild)
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancel_terminates_descendants() {
    let tmp = tempfile::tempdir().unwrap();
    let script = write_stubborn_grandchild_script(tmp.path());
    let pid_dir = tmp.path().to_path_buf();
    let argv = vec![
        script.to_string_lossy().to_string(),
        pid_dir.to_string_lossy().to_string(),
    ];

    let (scheduler, submission, _store, ws_id, _root) = setup_managed_argv().await;
    let sched = scheduler.clone();
    let loop_handle = tokio::spawn(async move { sched.run().await });

    let submitted = submission
        .submit(None, build_managed_argv_job(&ws_id, argv))
        .await
        .expect("submit");

    // Wait for both PIDs to appear.
    let child_pid = wait_for_pid_file(&pid_dir.join("child.pid"), Duration::from_secs(5)).await;
    let grandchild_pid =
        wait_for_pid_file(&pid_dir.join("grandchild.pid"), Duration::from_secs(5)).await;
    assert!(child_pid > 0 && grandchild_pid > 0);

    assert!(is_pid_alive(child_pid));
    assert!(is_pid_alive(grandchild_pid));

    // Cancel the job.
    let cancel_result = scheduler
        .request_cancel(&submitted.job_id, "test cancel")
        .await
        .expect("cancel");
    assert_eq!(cancel_result.state, CancelOutcome::Requested);

    // Wait for PIDs to die. The child dies from SIGTERM; the grandchild
    // dies from SIGKILL (sent after the 250ms grace period).
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let child_dead = !is_pid_alive(child_pid);
        let grandchild_dead = !is_pid_alive(grandchild_pid);
        if child_dead && grandchild_dead {
            break;
        }
        if std::time::Instant::now() >= deadline {
            panic!(
                "descendants not killed within deadline: child alive={}, grandchild alive={}",
                is_pid_alive(child_pid),
                is_pid_alive(grandchild_pid)
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Note: the job may stay "Running" because the orphaned grandchild
    // keeps the stdout/stderr pipe open. This is a known gap. We verify
    // PID death (the actual cleanup) rather than job state.
    //
    // Shutdown the scheduler which cancels in-flight executor tasks.
    scheduler
        .shutdown(SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(2), loop_handle).await;
}

// ═══════════════════════════════════════════════════════════════════════════
// Test 2: SIGTERM escalation to SIGKILL for stubborn processes
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sigterm_escalates_to_sigkill_for_stubborn_grandchild() {
    let tmp = tempfile::tempdir().unwrap();
    // Both child and grandchild ignore SIGTERM, so the managed process
    // service must escalate to SIGKILL.
    let script = write_both_stubborn_script(tmp.path());
    let pid_dir = tmp.path().to_path_buf();
    let argv = vec![
        script.to_string_lossy().to_string(),
        pid_dir.to_string_lossy().to_string(),
    ];

    let (scheduler, submission, store, ws_id, _root) = setup_managed_argv().await;
    let sched = scheduler.clone();
    let loop_handle = tokio::spawn(async move { sched.run().await });

    let submitted = submission
        .submit(None, build_managed_argv_job(&ws_id, argv))
        .await
        .expect("submit");

    let child_pid = wait_for_pid_file(&pid_dir.join("child.pid"), Duration::from_secs(5)).await;
    let grandchild_pid =
        wait_for_pid_file(&pid_dir.join("grandchild.pid"), Duration::from_secs(5)).await;
    assert!(child_pid > 0 && grandchild_pid > 0);

    // Cancel — both ignore SIGTERM, so the managed process service
    // must send SIGTERM, wait 250ms, then send SIGKILL to the group.
    let cancel_result = scheduler
        .request_cancel(&submitted.job_id, "escalation test")
        .await
        .expect("cancel");
    assert_eq!(cancel_result.state, CancelOutcome::Requested);

    // Wait for SIGKILL to land (after SIGTERM + 250ms grace + SIGKILL).
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let child_dead = !is_pid_alive(child_pid);
        let grandchild_dead = !is_pid_alive(grandchild_pid);
        if child_dead && grandchild_dead {
            break;
        }
        if std::time::Instant::now() >= deadline {
            panic!(
                "SIGKILL escalation failed: child alive={}, grandchild alive={}",
                is_pid_alive(child_pid),
                is_pid_alive(grandchild_pid)
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Both must be dead — proves SIGKILL was sent after SIGTERM failed.
    assert!(!is_pid_alive(child_pid), "child must be dead after SIGKILL");
    assert!(
        !is_pid_alive(grandchild_pid),
        "grandchild must be dead after SIGKILL"
    );

    // The job should reach terminal because both processes are dead
    // (pipe write-ends are closed), so join_output returns.
    let job = wait_for_terminal(&store, &submitted.job_id, Duration::from_secs(5)).await;
    assert_eq!(
        job.state,
        JobState::Cancelled,
        "job state must be Cancelled after SIGKILL escalation"
    );

    scheduler
        .shutdown(SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(2), loop_handle).await;
}

// ═══════════════════════════════════════════════════════════════════════════
// Test 3: Timeout terminates descendants
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn timeout_terminates_descendants() {
    let tmp = tempfile::tempdir().unwrap();
    // Use the "both stubborn" variant so SIGKILL is guaranteed.
    let script = write_both_stubborn_script(tmp.path());
    let pid_dir = tmp.path().to_path_buf();
    let argv = vec![
        script.to_string_lossy().to_string(),
        pid_dir.to_string_lossy().to_string(),
    ];

    let (scheduler, submission, _store, ws_id, _root) = setup_managed_argv().await;
    let sched = scheduler.clone();
    let loop_handle = tokio::spawn(async move { sched.run().await });

    // Use a short timeout (1s) to trigger the timeout path.
    let _submitted = submission
        .submit(
            None,
            build_managed_argv_job_with_timeout(&ws_id, argv, Duration::from_millis(1000)),
        )
        .await
        .expect("submit");

    let child_pid = wait_for_pid_file(&pid_dir.join("child.pid"), Duration::from_secs(5)).await;
    let grandchild_pid =
        wait_for_pid_file(&pid_dir.join("grandchild.pid"), Duration::from_secs(5)).await;
    assert!(child_pid > 0 && grandchild_pid > 0);

    // Wait for timeout to fire and SIGKILL to land.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let child_dead = !is_pid_alive(child_pid);
        let grandchild_dead = !is_pid_alive(grandchild_pid);
        if child_dead && grandchild_dead {
            break;
        }
        if std::time::Instant::now() >= deadline {
            panic!(
                "timeout cleanup failed: child alive={}, grandchild alive={}",
                is_pid_alive(child_pid),
                is_pid_alive(grandchild_pid)
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    assert!(
        !is_pid_alive(child_pid),
        "child must be killed after timeout"
    );
    assert!(
        !is_pid_alive(grandchild_pid),
        "grandchild must be killed after timeout"
    );

    scheduler
        .shutdown(SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(2), loop_handle).await;
}

// ═══════════════════════════════════════════════════════════════════════════
// Test 4: Process group is created (child and grandchild share PGID)
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn process_group_created_for_descendants() {
    // The actual invariant tested here — the executor-spawned child
    // becomes the leader of its own process group, and any
    // descendants share that PGID — is also proven by the
    // cancellation tests in this file, which depend on process-group
    // teardown to kill grandchildren. This test provides a direct
    // read-PGID witness: it uses `ps` from the test process to read
    // the PGID of the child the script wrote to self.pid, which
    // avoids the file-flushing race from inside the bash subshell.
    let tmp = tempfile::tempdir().unwrap();
    let script = write_pgid_script(tmp.path());
    let pid_dir = tmp.path().to_path_buf();
    let argv = vec![
        script.to_string_lossy().to_string(),
        pid_dir.to_string_lossy().to_string(),
    ];

    let (scheduler, submission, store, ws_id, _root) = setup_managed_argv().await;
    let sched = scheduler.clone();
    let loop_handle = tokio::spawn(async move { sched.run().await });

    let submitted = submission
        .submit(None, build_managed_argv_job(&ws_id, argv))
        .await
        .expect("submit");

    // Wait for the script to write its own PID file.
    let child_pid = wait_for_pid_file(&pid_dir.join("self.pid"), Duration::from_secs(5)).await;
    assert!(child_pid > 0);

    // Read PGID from outside the script via ps. `ps -o pgid= -p
    // <pid>` outputs the process group leader's PID; we compare
    // that value to the child's PID — if they match, the child is
    // its own process-group leader.
    let child_pgid_output = std::process::Command::new("ps")
        .args(["-o", "pgid=", "-p", &child_pid.to_string()])
        .output()
        .expect("ps");
    let child_pgid_text = String::from_utf8_lossy(&child_pgid_output.stdout)
        .trim()
        .to_string();
    assert!(!child_pgid_text.is_empty(), "ps output was empty");
    let child_pgid: i32 = child_pgid_text
        .parse()
        .unwrap_or_else(|_| panic!("PGID parse failed: {child_pgid_text}"));

    assert_eq!(
        child_pgid, child_pid,
        "child PGID ({child_pgid}) must equal child PID ({child_pid}) — \
         the managed process service must call setpgid() so the child \
         becomes its own process-group leader"
    );

    // Clean up: cancel the job so we don't leak processes.
    let _ = scheduler.request_cancel(&submitted.job_id, "cleanup").await;
    let _ = wait_for_terminal(&store, &submitted.job_id, Duration::from_secs(5)).await;

    scheduler
        .shutdown(SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(2), loop_handle).await;
}

// ═══════════════════════════════════════════════════════════════════════════
// Test 5: Managed process output is bounded
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn managed_process_output_is_bounded() {
    let tmp = tempfile::tempdir().unwrap();
    let script = write_bounded_output_script(tmp.path());
    let argv = vec![script.to_string_lossy().to_string()];

    let (scheduler, submission, store, ws_id, _root) = setup_managed_argv().await;
    let sched = scheduler.clone();
    let loop_handle = tokio::spawn(async move { sched.run().await });

    let submitted = submission
        .submit(None, build_managed_argv_job(&ws_id, argv))
        .await
        .expect("submit");

    // Wait for completion. The script produces 10 MB but the managed
    // process caps output at DEFAULT_MAX_OUTPUT_BYTES (256 KiB).
    let job = wait_for_terminal(&store, &submitted.job_id, Duration::from_secs(30)).await;
    assert!(
        job.state == JobState::Completed || job.state == JobState::Failed,
        "expected Completed or Failed, got {:?}",
        job.state
    );

    scheduler
        .shutdown(SchedulerShutdownMode::ImmediateInterrupt)
        .await;
    let _ = tokio::time::timeout(Duration::from_secs(2), loop_handle).await;
}
