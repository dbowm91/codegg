//! Integration test for the M006 scratch usage census.
//!
//! The census is an operator surface, so this test drives the **real**
//! `codegg daemon scratch-usage` command against a **real** daemon over the
//! local socket, in an isolated `HOME` and `CODEGG_DAEMON_HOME` so the
//! developer's own daemon and scratch tree are never touched.
//!
//! It asserts the three externally observable properties that matter:
//!
//! - the report names the tree it owns, by its opaque digest, with its size;
//! - the report carries no absolute path, no file name, no file content, and
//!   no session identity;
//! - `codegg daemon status` shows the same usage in one line.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;

use parking_lot::{Mutex, MutexGuard};
use std::time::Duration;

use codegg::core::instance::{connect_or_start_daemon, ConnectOrStartOptions, DaemonPaths};
use tokio::process::Command;
use tokio::time::sleep;

fn temp_root(label: &str) -> PathBuf {
    // The scratch root refuses a symlinked path component, and on macOS the
    // temp directory is under `/var`, which is a symlink to `/private/var`.
    // The isolated HOME must therefore be a canonical path, and the leaf must
    // stay short: a Unix-domain socket path is capped at ~104 bytes.
    let base = std::fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|_| std::env::temp_dir().to_path_buf());
    let mut path = base;
    path.push(format!(
        "cg{label}{}",
        &uuid::Uuid::new_v4().simple().to_string()[..6]
    ));
    std::fs::create_dir_all(&path).ok();
    path
}

fn codegg_binary() -> PathBuf {
    if let Ok(explicit) = std::env::var("CODEGG_TEST_BIN") {
        let path = PathBuf::from(explicit);
        if path.exists() {
            return path;
        }
    }
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_codegg") {
        return PathBuf::from(path);
    }
    if let Ok(target) = std::env::var("CARGO_TARGET_DIR") {
        let candidate = PathBuf::from(&target).join("debug/codegg");
        if candidate.exists() {
            return candidate;
        }
    }
    PathBuf::from("./target/debug/codegg")
}

/// An isolated environment: its own `HOME` (so the scratch cache root is
/// isolated), its own daemon home, and its own data home.
struct IsolatedEnv {
    _lock: MutexGuard<'static, ()>,
    old_home: Option<std::ffi::OsString>,
    old_daemon_home: Option<std::ffi::OsString>,
    old_data_home: Option<std::ffi::OsString>,
    old_xdg_cache: Option<std::ffi::OsString>,
}

impl IsolatedEnv {
    fn new(root: &Path) -> Self {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock();
        let old_home = std::env::var_os("HOME");
        let old_daemon_home = std::env::var_os("CODEGG_DAEMON_HOME");
        let old_data_home = std::env::var_os("CODEGG_DATA_HOME");
        let old_xdg_cache = std::env::var_os("XDG_CACHE_HOME");
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("CODEGG_DAEMON_HOME", root.join("daemon"));
        std::env::set_var("CODEGG_DATA_HOME", root.join("data"));
        std::env::remove_var("XDG_CACHE_HOME");
        std::fs::create_dir_all(root.join("home")).ok();
        Self {
            _lock: lock,
            old_home,
            old_daemon_home,
            old_data_home,
            old_xdg_cache,
        }
    }
}

impl Drop for IsolatedEnv {
    fn drop(&mut self) {
        for (key, value) in [
            ("HOME", self.old_home.take()),
            ("CODEGG_DAEMON_HOME", self.old_daemon_home.take()),
            ("CODEGG_DATA_HOME", self.old_data_home.take()),
            ("XDG_CACHE_HOME", self.old_xdg_cache.take()),
        ] {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

/// The sessions root the daemon will census, under the isolated `HOME`.
fn sessions_root() -> PathBuf {
    dirs::cache_dir()
        .expect("cache dir")
        .join("codegg")
        .join("scratch")
        .join("sessions")
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
async fn scratch_usage_reports_the_daemon_s_own_tree_without_leaking_it() {
    let root = temp_root("su");
    let bin = codegg_binary();
    if !bin.exists() {
        eprintln!(
            "skipping: codegg binary not found at {}; set CODEGG_TEST_BIN to run",
            bin.display()
        );
        return;
    }

    let _env = IsolatedEnv::new(&root);

    // Plant one scratch tree, named by its digest, with one file in it.
    let session_id = "scratch-usage-integration-session";
    let digest = codegg::scratch::session_scratch_digest_hex(session_id);
    let tree = sessions_root().join(&digest);
    std::fs::create_dir_all(&tree).expect("plant scratch tree");
    std::fs::write(tree.join("artifact.bin"), b"census-secret-content").expect("plant artifact");

    let paths = DaemonPaths::resolve();
    let result = connect_or_start_daemon(ConnectOrStartOptions {
        paths: paths.clone(),
        autostart: true,
        startup_timeout: Duration::from_secs(30),
        poll_interval: Duration::from_millis(50),
        executable: Some(bin.clone()),
    })
    .await
    .expect("connect-or-start should autostart a daemon");
    assert!(result.started_pid.is_some());
    assert!(
        wait_for_daemon_ready(&paths, Duration::from_secs(30)).await,
        "daemon did not become ready"
    );

    let json = Command::new(&bin)
        .args(["daemon", "scratch-usage", "--json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .expect("run scratch-usage --json");
    let json_text = String::from_utf8_lossy(&json.stdout).to_string();
    assert!(
        json.status.success(),
        "scratch-usage --json failed: {}{}",
        json_text,
        String::from_utf8_lossy(&json.stderr)
    );

    let report: serde_json::Value =
        serde_json::from_str(&json_text).expect("--json output must parse as JSON");
    assert_eq!(
        report["tree_count"].as_u64(),
        Some(1),
        "report: {json_text}"
    );
    let trees = report["trees"].as_array().expect("trees array");
    assert_eq!(trees.len(), 1, "report: {json_text}");
    let entry = &trees[0];
    assert_eq!(
        entry["digest"].as_str(),
        Some(digest.as_str()),
        "report: {json_text}"
    );
    assert_eq!(
        entry["total_bytes"].as_u64(),
        Some("census-secret-content".len() as u64),
        "report: {json_text}"
    );
    assert_eq!(
        entry["reclaimable"].as_bool(),
        Some(true),
        "report: {json_text}"
    );

    // The report discloses counts, sizes, and the opaque digest only. No
    // absolute path, no file name, no file content, and no session identity.
    let root_text = root.to_string_lossy().to_string();
    assert!(
        !json_text.contains(&root_text),
        "the report leaked an absolute path: {json_text}"
    );
    assert!(
        !json_text.contains("census-secret-content"),
        "the report leaked file content: {json_text}"
    );
    assert!(
        !json_text.contains("artifact.bin"),
        "the report leaked a file name: {json_text}"
    );
    assert!(
        !json_text.contains(session_id),
        "the report leaked a session identity: {json_text}"
    );

    // The human form names the same tree.
    let human = Command::new(&bin)
        .args(["daemon", "scratch-usage"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .expect("run scratch-usage");
    let human_text = String::from_utf8_lossy(&human.stdout).to_string();
    assert!(
        human.status.success(),
        "scratch-usage failed: {}{}",
        human_text,
        String::from_utf8_lossy(&human.stderr)
    );
    assert!(
        human_text.contains("Scratch usage"),
        "human report missing its header: {human_text}"
    );
    assert!(
        human_text.contains(&digest[..12]),
        "human report missing the tree digest: {human_text}"
    );
    assert!(
        !human_text.contains("artifact.bin"),
        "the human report leaked a file name: {human_text}"
    );

    // `daemon status` shows the same usage in one line.
    let status = Command::new(&bin)
        .args(["daemon", "status"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .expect("run daemon status");
    let status_text = String::from_utf8_lossy(&status.stdout).to_string();
    let scratch_line = status_text
        .lines()
        .find(|line| line.contains("Scratch:"))
        .expect("daemon status must carry a scratch summary line");
    assert!(
        !scratch_line.contains(&root_text),
        "the daemon status scratch line leaked an absolute path: {scratch_line}"
    );
    assert!(
        !scratch_line.contains("artifact.bin"),
        "the daemon status scratch line leaked a file name: {scratch_line}"
    );

    let stop = Command::new(&bin)
        .args(["daemon", "stop"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await
        .expect("stop the daemon");
    assert!(
        stop.status.success(),
        "daemon stop failed: {}",
        String::from_utf8_lossy(&stop.stderr)
    );

    std::fs::remove_dir_all(&root).ok();
}

/// The census must not reclaim: a full census leaves the tree exactly as it
/// was, and no tombstone appears.
#[tokio::test(flavor = "current_thread")]
async fn the_census_reclaims_nothing() {
    let root = temp_root("sr");
    let bin = codegg_binary();
    if !bin.exists() {
        eprintln!(
            "skipping: codegg binary not found at {}; set CODEGG_TEST_BIN to run",
            bin.display()
        );
        return;
    }

    let _env = IsolatedEnv::new(&root);

    let session_id = "scratch-census-readonly-session";
    let digest = codegg::scratch::session_scratch_digest_hex(session_id);
    let tree = sessions_root().join(&digest);
    std::fs::create_dir_all(&tree).expect("plant scratch tree");
    std::fs::write(tree.join("artifact.bin"), b"keep-me").expect("plant artifact");
    let before = std::fs::read_dir(&tree).expect("read tree").count();

    let paths = DaemonPaths::resolve();
    let outcome = connect_or_start_daemon(ConnectOrStartOptions {
        paths: paths.clone(),
        autostart: true,
        startup_timeout: Duration::from_secs(30),
        poll_interval: Duration::from_millis(50),
        executable: Some(bin.clone()),
    })
    .await
    .expect("connect-or-start should autostart a daemon");
    assert!(
        wait_for_daemon_ready(&paths, Duration::from_secs(30)).await,
        "daemon did not become ready"
    );

    for _ in 0..3 {
        let output = Command::new(&bin)
            .args(["daemon", "scratch-usage", "--refresh"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .await
            .expect("run scratch-usage --refresh");
        assert!(output.status.success(), "scratch-usage --refresh failed");
    }

    assert!(tree.exists(), "the census removed the tree");
    assert_eq!(
        std::fs::read_dir(&tree).expect("read tree").count(),
        before,
        "the census changed the tree's entries"
    );
    assert_eq!(
        std::fs::read(tree.join("artifact.bin")).expect("read artifact"),
        b"keep-me",
        "the census changed the tree's content"
    );
    let tombstones = std::fs::read_dir(sessions_root())
        .expect("read sessions root")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(".reclaim-"))
        .count();
    assert_eq!(tombstones, 0, "the census created a reclaim tombstone");

    drop(outcome);
    std::fs::remove_dir_all(&root).ok();
}
