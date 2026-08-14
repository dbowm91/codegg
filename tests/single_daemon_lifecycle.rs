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

use codegg::core::instance::{
    connect_or_start_daemon, read_metadata_for_paths, ConnectOrStartOptions, DaemonInstanceGuard,
    DaemonPaths,
};
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
async fn connect_or_start_keeps_autostarted_daemon_alive_after_return() {
    let root = temp_root("autostart");
    let data_root = root.join("data");
    let bin = codegg_binary();
    if !bin.exists() {
        eprintln!(
            "skipping: codegg binary not found at {}; set CODEGG_TEST_BIN to run",
            bin.display()
        );
        return;
    }

    let old_runtime = std::env::var_os("CODEGG_DAEMON_HOME");
    let old_data = std::env::var_os("CODEGG_DATA_HOME");
    std::env::set_var("CODEGG_DAEMON_HOME", &root);
    std::env::set_var("CODEGG_DATA_HOME", &data_root);
    let result = connect_or_start_daemon(ConnectOrStartOptions {
        paths: DaemonPaths::with_root(root.clone()),
        autostart: true,
        startup_timeout: Duration::from_secs(10),
        poll_interval: Duration::from_millis(50),
        executable: Some(bin.clone()),
    })
    .await
    .expect("connect-or-start should autostart a daemon");
    assert!(result.started_pid.is_some());
    drop(result.client);

    let second = codegg::core::transport::SocketCoreClient::connect(
        &DaemonPaths::with_root(root.clone()).endpoint_uri(),
    )
    .await
    .expect("autostarted daemon must survive helper return");
    assert!(!second
        .daemon_id()
        .await
        .expect("live daemon identity")
        .is_empty());

    let stop = Command::new(&bin)
        .env("CODEGG_DAEMON_HOME", &root)
        .env("CODEGG_DATA_HOME", &data_root)
        .env_remove("CODEGG_CORE_ENDPOINT")
        .args(["daemon", "stop"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await
        .expect("stop autostarted daemon");
    assert!(
        stop.status.success(),
        "autostarted daemon stop failed: {}",
        String::from_utf8_lossy(&stop.stderr)
    );

    match old_runtime {
        Some(value) => std::env::set_var("CODEGG_DAEMON_HOME", value),
        None => std::env::remove_var("CODEGG_DAEMON_HOME"),
    }
    match old_data {
        Some(value) => std::env::set_var("CODEGG_DATA_HOME", value),
        None => std::env::remove_var("CODEGG_DATA_HOME"),
    }
    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test(flavor = "current_thread")]
async fn concurrent_connect_or_start_calls_converge_on_one_daemon() {
    let root = temp_root("autostart-race");
    let data_root = root.join("data");
    let bin = codegg_binary();
    if !bin.exists() {
        eprintln!(
            "skipping: codegg binary not found at {}; set CODEGG_TEST_BIN to run",
            bin.display()
        );
        return;
    }

    let old_runtime = std::env::var_os("CODEGG_DAEMON_HOME");
    let old_data = std::env::var_os("CODEGG_DATA_HOME");
    std::env::set_var("CODEGG_DAEMON_HOME", &root);
    std::env::set_var("CODEGG_DATA_HOME", &data_root);
    let options = || ConnectOrStartOptions {
        paths: DaemonPaths::with_root(root.clone()),
        autostart: true,
        startup_timeout: Duration::from_secs(10),
        poll_interval: Duration::from_millis(50),
        executable: Some(bin.clone()),
    };
    let (left, right) = tokio::join!(
        connect_or_start_daemon(options()),
        connect_or_start_daemon(options())
    );
    assert!(
        left.is_ok(),
        "left starter failed: {:?}",
        left.as_ref().err()
    );
    assert!(
        right.is_ok(),
        "right starter failed: {:?}",
        right.as_ref().err()
    );
    let left = left.expect("left connection");
    let right = right.expect("right connection");
    assert_eq!(left.daemon_id, right.daemon_id);
    drop(left.client);
    drop(right.client);

    let stop = Command::new(&bin)
        .env("CODEGG_DAEMON_HOME", &root)
        .env("CODEGG_DATA_HOME", &data_root)
        .args(["daemon", "stop"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await
        .expect("stop raced daemon");
    assert!(stop.status.success(), "race daemon stop failed");

    match old_runtime {
        Some(value) => std::env::set_var("CODEGG_DAEMON_HOME", value),
        None => std::env::remove_var("CODEGG_DAEMON_HOME"),
    }
    match old_data {
        Some(value) => std::env::set_var("CODEGG_DATA_HOME", value),
        None => std::env::remove_var("CODEGG_DATA_HOME"),
    }
    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test(flavor = "current_thread")]
async fn plain_entrypoint_reaches_tui_startup_boundary_without_prestarted_daemon() {
    let root = temp_root("plain-startup");
    let data_root = root.join("data");
    let bin = codegg_binary();
    if !bin.exists() {
        eprintln!(
            "skipping: codegg binary not found at {}; set CODEGG_TEST_BIN to run",
            bin.display()
        );
        return;
    }

    let output = Command::new(&bin)
        .env("CODEGG_DAEMON_HOME", &root)
        .env("CODEGG_DATA_HOME", &data_root)
        .env("CODEGG_TUI_STARTUP_PROBE", "1")
        .env_remove("CODEGG_CORE_ENDPOINT")
        .args(["--no-session"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .expect("plain codegg startup probe");
    assert!(
        output.status.success(),
        "plain startup probe failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("event-loop boundary"));
    assert!(
        DaemonPaths::with_root(root.clone()).log_path.exists(),
        "plain startup should leave diagnostics at the canonical daemon log path"
    );

    let stop = Command::new(&bin)
        .env("CODEGG_DAEMON_HOME", &root)
        .env("CODEGG_DATA_HOME", &data_root)
        .args(["daemon", "stop"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await
        .expect("stop plain-startup daemon");
    assert!(stop.status.success(), "plain-startup daemon stop failed");
    std::fs::remove_dir_all(&root).ok();
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
        .env("CODEGG_DATA_HOME", root.join("data"))
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
        .env("CODEGG_DATA_HOME", root.join("data"))
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
        .env("CODEGG_DATA_HOME", root.join("data"))
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
        .env("CODEGG_DATA_HOME", root.join("data"))
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
        .env("CODEGG_DATA_HOME", root.join("data"))
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
        .env("CODEGG_DATA_HOME", root.join("data"))
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

#[tokio::test(flavor = "current_thread")]
async fn stop_requires_matching_live_daemon_identity() {
    let root = temp_root("stop-mismatch");
    let paths = DaemonPaths::with_root(root.clone());

    let bin = codegg_binary();
    if !bin.exists() {
        eprintln!(
            "skipping: codegg binary not found at {}; set CODEGG_TEST_BIN to run",
            bin.display()
        );
        return;
    }

    let mut daemon = Command::new(&bin)
        .env("CODEGG_DAEMON_HOME", &root)
        .env("CODEGG_DATA_HOME", root.join("data"))
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

    let mut metadata = read_metadata_for_paths(&paths).expect("daemon metadata");
    let original_id = metadata.daemon_id.clone();
    metadata.daemon_id = "codegg-stale-metadata".to_string();
    std::fs::write(
        &paths.metadata_path,
        serde_json::to_string_pretty(&metadata).expect("serialize metadata"),
    )
    .expect("replace metadata");

    let stop_out = Command::new(&bin)
        .env("CODEGG_DAEMON_HOME", &root)
        .env("CODEGG_DATA_HOME", root.join("data"))
        .env_remove("CODEGG_CORE_ENDPOINT")
        .args(["daemon", "stop"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .expect("daemon stop");
    assert!(
        !stop_out.status.success(),
        "mismatched stop unexpectedly succeeded"
    );
    let stderr = String::from_utf8_lossy(&stop_out.stderr);
    assert!(
        stderr.contains("does not match metadata identity"),
        "{stderr}"
    );
    assert!(stderr.contains("no signal sent"), "{stderr}");

    assert!(
        wait_for_daemon_ready(&paths, Duration::from_secs(2)).await,
        "identity mismatch must not terminate the live daemon"
    );

    // Restore the metadata so the daemon's normal ownership cleanup remains
    // representative of the production path before terminating the fixture.
    metadata.daemon_id = original_id;
    std::fs::write(
        &paths.metadata_path,
        serde_json::to_string_pretty(&metadata).expect("restore metadata"),
    )
    .expect("restore metadata");
    let _ = daemon.kill().await;
    let _ = daemon.wait().await;
    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stop_signals_the_current_daemon_after_identity_match() {
    let root = temp_root("stop-current");
    let paths = DaemonPaths::with_root(root.clone());

    let bin = codegg_binary();
    if !bin.exists() {
        eprintln!(
            "skipping: codegg binary not found at {}; set CODEGG_TEST_BIN to run",
            bin.display()
        );
        return;
    }

    let mut daemon = Command::new(&bin)
        .env("CODEGG_DAEMON_HOME", &root)
        .env("CODEGG_DATA_HOME", root.join("data"))
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

    let stop_out = Command::new(&bin)
        .env("CODEGG_DAEMON_HOME", &root)
        .env("CODEGG_DATA_HOME", root.join("data"))
        .env_remove("CODEGG_CORE_ENDPOINT")
        .args(["daemon", "stop"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .expect("daemon stop");
    assert!(
        stop_out.status.success(),
        "current daemon stop failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&stop_out.stdout),
        String::from_utf8_lossy(&stop_out.stderr)
    );
    assert!(String::from_utf8_lossy(&stop_out.stdout).contains("Sent SIGTERM"));
    assert!(
        !paths.socket_path.exists(),
        "SIGTERM left the socket behind"
    );
    assert!(
        !paths.metadata_path.exists(),
        "SIGTERM left daemon metadata behind"
    );
    assert!(
        !paths.socket_path.with_extension("pid").exists(),
        "SIGTERM left the legacy PID file behind"
    );

    let exit = tokio::time::timeout(Duration::from_secs(10), daemon.wait())
        .await
        .expect("daemon did not exit after SIGTERM")
        .expect("wait for daemon");
    assert!(
        exit.success(),
        "SIGTERM should enter the graceful daemon shutdown path"
    );

    // The singleton lock must be released as part of the same graceful
    // lifecycle; a fresh daemon can start without manual stale-state cleanup.
    let mut restarted = Command::new(&bin)
        .env("CODEGG_DAEMON_HOME", &root)
        .env("CODEGG_DATA_HOME", root.join("data"))
        .env_remove("CODEGG_CORE_ENDPOINT")
        .args(["daemon", "start"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("restart daemon after graceful stop");
    assert!(
        wait_for_daemon_ready(&paths, Duration::from_secs(10)).await,
        "daemon could not restart after graceful stop"
    );
    let _ = restarted.kill().await;
    let _ = restarted.wait().await;
    std::fs::remove_dir_all(&root).ok();
}
