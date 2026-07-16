//! Multi-process integration tests for the singleton daemon lifecycle.
//!
//! These tests spawn the actual `codegg` binary in `daemon start` and
//! `daemon status` modes against a private `CODEGG_DAEMON_HOME` so that
//! parallel runs cannot collide. They verify:
//!
//! - a second `daemon start` invocation against the same lock file exits
//!   cleanly without unbinding the first listener
//! - the live daemon survives the second invocation
//! - SIGTERM-style teardown (we `kill` the child) leaves a recoverable
//!   stale socket that a subsequent `daemon start` can clean up and
//!   take over
//! - a fresh start of the daemon produces a `daemon.json` with a
//!   parseable generation

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use codegg::core::instance::{read_metadata_for_paths, DaemonInstanceGuard, DaemonPaths};
use tokio::process::Command;
use tokio::time::sleep;

fn temp_root(label: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    // Unix-domain sockets on macOS cap path lengths at ~104 bytes (SUN_LEN).
    // Keep the leaf short so the resulting socket path stays under the
    // platform limit. We then put the per-test suffix on a subdirectory
    // name that's well under the limit, while still being unique.
    p.push(format!("cgg-{}", label));
    std::fs::create_dir_all(&p).ok();
    p.push(&uuid::Uuid::new_v4().simple().to_string()[..12]);
    p
}

/// Locate the `codegg` binary built for the current test process. Tests
/// are run via `cargo test`, which puts the binary on the same target
/// directory as the test binary. We probe a few well-known locations.
fn codegg_binary() -> PathBuf {
    if let Ok(explicit) = std::env::var("CODEGG_TEST_BIN") {
        let p = PathBuf::from(explicit);
        if p.exists() {
            return p;
        }
    }
    // cargo sets CARGO_BIN_EXE_codegg for integration tests of the
    // workspace root. Fall back to a sibling "codegg" if needed.
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_codegg") {
        return PathBuf::from(p);
    }
    // Last-resort: search for the binary in target/debug or target/debug-nextest.
    if let Ok(target) = std::env::var("CARGO_TARGET_DIR") {
        let candidate = PathBuf::from(&target).join("debug/codegg");
        if candidate.exists() {
            return candidate;
        }
    }
    PathBuf::from("./target/debug/codegg")
}

async fn wait_for_daemon_ready(paths: &DaemonPaths, timeout: Duration) -> bool {
    let endpoint = paths.endpoint_uri();
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if let Ok(_client) = codegg::core::transport::SocketCoreClient::connect(&endpoint).await {
            return true;
        }
        sleep(Duration::from_millis(50)).await;
    }
    false
}

#[tokio::test(flavor = "current_thread")]
async fn second_daemon_start_against_live_daemon_does_not_steal_lock() {
    let root = temp_root("second");
    let paths = DaemonPaths::with_root(root.clone());

    let bin = codegg_binary();
    if !bin.exists() {
        eprintln!(
            "skipping: codegg binary not found at {}; set CODEGG_TEST_BIN to run",
            bin.display()
        );
        return;
    }

    // Start daemon A.
    let mut a = Command::new(&bin)
        .env("CODEGG_DAEMON_HOME", &root)
        .env_remove("CODEGG_CORE_ENDPOINT")
        .args(["daemon", "start"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn daemon A");
    if !wait_for_daemon_ready(&paths, Duration::from_secs(10)).await {
        // Capture stderr for diagnostics, then fail.
        let out = a.wait_with_output().await;
        panic!(
            "daemon A never became ready; output={:?}",
            out.as_ref().map(|o| (
                String::from_utf8_lossy(&o.stdout).to_string(),
                String::from_utf8_lossy(&o.stderr).to_string(),
                o.status
            ))
        );
    }

    // Start daemon B against the same lock/endpoint.
    let b_out = Command::new(&bin)
        .env("CODEGG_DAEMON_HOME", &root)
        .env_remove("CODEGG_CORE_ENDPOINT")
        .args(["daemon", "start"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .expect("spawn daemon B");
    // B should exit 0 (we treat "already running" as success) and not
    // unlink the live socket.
    assert!(
        b_out.status.success(),
        "daemon B unexpectedly failed: stderr={}",
        String::from_utf8_lossy(&b_out.stderr)
    );
    assert!(paths.socket_path.exists(), "daemon B removed A's socket");

    // A is still reachable.
    assert!(wait_for_daemon_ready(&paths, Duration::from_secs(2)).await);

    // Cleanup.
    let _ = a.kill().await;
    let _ = a.wait().await;
    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test(flavor = "current_thread")]
async fn stale_socket_after_ungraceful_exit_is_recoverable() {
    let root = temp_root("stale");
    let paths = DaemonPaths::with_root(root.clone());

    let bin = codegg_binary();
    if !bin.exists() {
        eprintln!(
            "skipping: codegg binary not found at {}; set CODEGG_TEST_BIN to run",
            bin.display()
        );
        return;
    }

    // Start daemon X.
    let mut x = Command::new(&bin)
        .env("CODEGG_DAEMON_HOME", &root)
        .env_remove("CODEGG_CORE_ENDPOINT")
        .args(["daemon", "start"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn daemon X");
    assert!(
        wait_for_daemon_ready(&paths, Duration::from_secs(10)).await,
        "daemon X never became ready"
    );

    // Read metadata record from X.
    let md_before = read_metadata_for_paths(&paths).expect("daemon X metadata");
    assert!(!md_before.generation.is_empty());

    // Kill -9 to simulate an ungraceful exit. The lock file is closed
    // (the file descriptor is released) and the OS releases the flock.
    // We expect to leave behind a stale socket path and metadata file.
    #[cfg(unix)]
    unsafe {
        let pid = x.id().expect("x pid") as i32;
        libc::kill(pid, libc::SIGKILL);
    }
    let _ = x.wait().await;
    // Give the OS a brief moment to fully reap the process.
    sleep(Duration::from_millis(200)).await;

    // Lock should be free now.
    {
        let _guard = DaemonInstanceGuard::try_acquire(&paths)
            .expect("try_acquire")
            .expect("lock should be free after SIGKILL");
        // Drop the guard explicitly so the child daemon can acquire the lock.
        _guard.release();
    }

    // Start daemon Y in the same home. It should take over successfully
    // (it sees the stale socket, fails to connect, and removes it).
    let mut y = Command::new(&bin)
        .env("CODEGG_DAEMON_HOME", &root)
        .env_remove("CODEGG_CORE_ENDPOINT")
        .args(["daemon", "start"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn daemon Y");
    assert!(
        wait_for_daemon_ready(&paths, Duration::from_secs(10)).await,
        "daemon Y never became ready after recovery"
    );

    // Y has a different generation than X.
    let md_after = read_metadata_for_paths(&paths).expect("daemon Y metadata");
    assert_ne!(
        md_before.generation, md_after.generation,
        "fresh daemon should produce a new generation"
    );

    // Cleanup.
    let _ = y.kill().await;
    let _ = y.wait().await;
    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test(flavor = "current_thread")]
async fn status_reports_daemon_identity_with_metadata() {
    let root = temp_root("status");
    let paths = DaemonPaths::with_root(root.clone());

    let bin = codegg_binary();
    if !bin.exists() {
        eprintln!(
            "skipping: codegg binary not found at {}; set CODEGG_TEST_BIN to run",
            bin.display()
        );
        return;
    }

    let mut d = Command::new(&bin)
        .env("CODEGG_DAEMON_HOME", &root)
        .env_remove("CODEGG_CORE_ENDPOINT")
        .args(["daemon", "start"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn daemon");

    assert!(
        wait_for_daemon_ready(&paths, Duration::from_secs(10)).await,
        "daemon never became ready"
    );

    let status_out = Command::new(&bin)
        .env("CODEGG_DAEMON_HOME", &root)
        .env_remove("CODEGG_CORE_ENDPOINT")
        .args(["daemon", "status"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .expect("daemon status");
    let stdout = String::from_utf8_lossy(&status_out.stdout);
    assert!(status_out.status.success(), "status failed: {stdout}");
    assert!(stdout.contains("Daemon is running"));
    assert!(stdout.contains("Generation:"));

    // Cleanup.
    let _ = d.kill().await;
    let _ = d.wait().await;
    std::fs::remove_dir_all(&root).ok();
}
