//! Integration test for the M007 scratch retention sweep and the operator
//! reclaim disposition.
//!
//! The sweep and the operator action are operator surfaces, so this test
//! drives the **real** `codegg daemon scratch-reclaim` command against a
//! **real** daemon over the local socket, in an isolated `HOME`,
//! `CODEGG_DAEMON_HOME`, and `CODEGG_DATA_HOME`, so the developer's own
//! daemon and scratch tree are never touched.
//!
//! It asserts the externally observable properties that matter:
//!
//! - the census measures the tree the daemon owns, with no path, no file
//!   name, no file content, and no session identity in the report;
//! - a dry run reports what it would remove and removes nothing;
//! - `--apply` removes the named tree, and the removal is recorded in the
//!   audit trail with bounded metadata;
//! - the operator path removes a tree holding a planted symlink and leaves
//!   the symlink's target intact;
//! - the sweep removes exactly the trees it decided to remove, and nothing
//!   else.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::str::FromStr;
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
///
/// The XDG variables are isolated too. On Linux they are what `dirs` reads
/// for the cache, config, data, and state roots, and a runner that sets
/// them would otherwise give two tests in this binary the **same** config
/// directory — so a config file one test wrote would silently change what
/// another test's daemon does.
struct IsolatedEnv {
    _lock: MutexGuard<'static, ()>,
    old_home: Option<std::ffi::OsString>,
    old_daemon_home: Option<std::ffi::OsString>,
    old_data_home: Option<std::ffi::OsString>,
    old_xdg_cache: Option<std::ffi::OsString>,
    old_xdg_config: Option<std::ffi::OsString>,
    old_xdg_data: Option<std::ffi::OsString>,
    old_xdg_state: Option<std::ffi::OsString>,
}

impl IsolatedEnv {
    fn new(root: &Path) -> Self {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock();
        let old_home = std::env::var_os("HOME");
        let old_daemon_home = std::env::var_os("CODEGG_DAEMON_HOME");
        let old_data_home = std::env::var_os("CODEGG_DATA_HOME");
        let old_xdg_cache = std::env::var_os("XDG_CACHE_HOME");
        let old_xdg_config = std::env::var_os("XDG_CONFIG_HOME");
        let old_xdg_data = std::env::var_os("XDG_DATA_HOME");
        let old_xdg_state = std::env::var_os("XDG_STATE_HOME");
        let home = root.join("home");
        std::env::set_var("HOME", &home);
        std::env::set_var("CODEGG_DAEMON_HOME", root.join("daemon"));
        std::env::set_var("CODEGG_DATA_HOME", root.join("data"));
        // Every XDG root is redirected into the isolated home, so the daemon
        // resolves its config, cache, data, and state roots the same way the
        // test process does — whatever the runner's environment says.
        for (key, leaf) in [
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_DATA_HOME", "data"),
            ("XDG_STATE_HOME", "state"),
        ] {
            let path = home.join(leaf);
            std::fs::create_dir_all(&path).ok();
            std::env::set_var(key, path);
        }
        std::fs::create_dir_all(&home).ok();
        Self {
            _lock: lock,
            old_home,
            old_daemon_home,
            old_data_home,
            old_xdg_cache,
            old_xdg_config,
            old_xdg_data,
            old_xdg_state,
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
            ("XDG_CONFIG_HOME", self.old_xdg_config.take()),
            ("XDG_DATA_HOME", self.old_xdg_data.take()),
            ("XDG_STATE_HOME", self.old_xdg_state.take()),
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

/// The names of every entry in the sessions root, sorted.
///
/// This is the manifest the sweep's before/after is compared against: it
/// shows exactly which trees the sweep removed and which it left alone.
fn manifest() -> Vec<String> {
    let mut names = std::fs::read_dir(sessions_root())
        .expect("read the sessions root")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    names.sort();
    names
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

/// The census measures the daemon's own tree, and the operator disposition
/// removes exactly what it was told to.
#[tokio::test(flavor = "current_thread")]
async fn the_operator_reclaim_disposes_of_one_named_tree_and_records_it() {
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

    // A tree the automatic path accepts, and a tree holding a planted
    // symlink, which the automatic path refuses.
    let plain_id = "scratch-retention-plain-session";
    let plain_digest = codegg::scratch::session_scratch_digest_hex(plain_id);
    let plain_tree = codegg::scratch::session_scratch_dir(plain_id).expect("plant a scratch tree");
    std::fs::write(plain_tree.join("artifact.bin"), b"disposable bytes")
        .expect("plant an artifact");

    let linked_id = "scratch-retention-linked-session";
    let linked_digest = codegg::scratch::session_scratch_digest_hex(linked_id);
    let linked_tree =
        codegg::scratch::session_scratch_dir(linked_id).expect("plant a scratch tree");
    let outside = root.join("outside");
    std::fs::create_dir_all(&outside).expect("create the outside directory");
    std::fs::write(outside.join("precious.txt"), b"outside content").expect("write outside");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, linked_tree.join("link")).expect("plant a symlink");

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
    assert!(outcome.started_pid.is_some());
    assert!(
        wait_for_daemon_ready(&paths, Duration::from_secs(30)).await,
        "daemon did not become ready"
    );

    // The census measures both trees and discloses neither of them.
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
        Some(2),
        "report: {json_text}"
    );
    let trees = report["trees"].as_array().expect("trees array");
    let plain = trees
        .iter()
        .find(|tree| tree["digest"].as_str() == Some(plain_digest.as_str()))
        .expect("the plain tree is reported");
    assert_eq!(
        plain["reclaimable"].as_bool(),
        Some(true),
        "report: {json_text}"
    );
    // The tree holds the daemon's 91-byte owner record and the artifact.
    assert_eq!(
        plain["total_bytes"].as_u64(),
        Some(91 + "disposable bytes".len() as u64),
        "report: {json_text}"
    );
    if cfg!(unix) {
        let linked = trees
            .iter()
            .find(|tree| tree["digest"].as_str() == Some(linked_digest.as_str()))
            .expect("the linked tree is reported");
        assert_eq!(
            linked["reclaimable"].as_bool(),
            Some(false),
            "the automatic path refuses a planted symlink: {json_text}"
        );
        assert_eq!(
            linked["refusal"].as_str(),
            Some("symbolic_link"),
            "report: {json_text}"
        );
    }

    let root_text = root.to_string_lossy().to_string();
    for leak in [
        root_text.as_str(),
        "artifact.bin",
        "disposable bytes",
        "precious.txt",
        plain_id,
        linked_id,
    ] {
        assert!(
            !json_text.contains(leak),
            "the report leaked {leak}: {json_text}"
        );
    }

    // A dry run reports what it would remove and removes nothing.
    let dry = Command::new(&bin)
        .args(["daemon", "scratch-reclaim", "--tree", &plain_digest])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .expect("run scratch-reclaim");
    let dry_text = String::from_utf8_lossy(&dry.stdout).to_string();
    assert!(
        dry.status.success(),
        "scratch-reclaim failed: {dry_text}{}",
        String::from_utf8_lossy(&dry.stderr)
    );
    assert!(dry_text.contains("Dry run"), "output: {dry_text}");
    assert!(plain_tree.exists(), "a dry run must remove nothing");

    // `--apply` removes the named tree.
    let applied = Command::new(&bin)
        .args([
            "daemon",
            "scratch-reclaim",
            "--tree",
            &plain_digest,
            "--apply",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .expect("run scratch-reclaim --apply");
    let applied_text = String::from_utf8_lossy(&applied.stdout).to_string();
    assert!(
        applied.status.success(),
        "scratch-reclaim --apply failed: {applied_text}{}",
        String::from_utf8_lossy(&applied.stderr)
    );
    assert!(applied_text.contains("Removed"), "output: {applied_text}");
    assert!(!plain_tree.exists(), "the tree is removed");

    // The operator path removes a tree the automatic path refuses, and the
    // symlink's target survives it.
    if cfg!(unix) {
        let applied = Command::new(&bin)
            .args([
                "daemon",
                "scratch-reclaim",
                "--tree",
                &linked_digest,
                "--apply",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .expect("run scratch-reclaim --apply");
        let applied_text = String::from_utf8_lossy(&applied.stdout).to_string();
        assert!(
            applied.status.success(),
            "scratch-reclaim --apply failed: {applied_text}{}",
            String::from_utf8_lossy(&applied.stderr)
        );
        assert!(!linked_tree.exists(), "the tree is removed");
        assert!(
            outside.join("precious.txt").exists(),
            "the operator path must not remove the symlink's target"
        );
        assert_eq!(
            std::fs::read(outside.join("precious.txt")).expect("read outside"),
            b"outside content",
            "the operator path must not modify the symlink's target"
        );
    }

    // The removal is recorded in the audit trail, with bounded metadata.
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

    let database = root.join("data").join("codegg").join("codegg.db");
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", database.display()))
        .await
        .expect("open the daemon database");
    let events: Vec<(String, String)> = sqlx::query_as(
        "SELECT action, metadata_json FROM audit_event WHERE action = 'scratch_reclaim' \
         ORDER BY seq",
    )
    .fetch_all(&pool)
    .await
    .expect("read the audit trail");
    assert!(
        !events.is_empty(),
        "the operator reclaim left no audit record"
    );
    for (action, metadata) in &events {
        assert_eq!(action, "scratch_reclaim");
        assert!(
            !metadata.contains(&root_text),
            "the audit record leaked an absolute path: {metadata}"
        );
        assert!(
            !metadata.contains("artifact.bin"),
            "the audit record leaked a file name: {metadata}"
        );
        assert!(
            !metadata.contains("disposable bytes"),
            "the audit record leaked file content: {metadata}"
        );
        assert!(
            !metadata.contains(plain_id) && !metadata.contains(linked_id),
            "the audit record leaked a session identity: {metadata}"
        );
        // The bounded metadata the plan names.
        for key in [
            "scratch.digest",
            "scratch.entry_count",
            "scratch.byte_count",
            "scratch.verdict",
            "scratch.outcome",
        ] {
            assert!(
                metadata.contains(key),
                "the audit record is missing {key}: {metadata}"
            );
        }
    }
    pool.close().await;

    std::fs::remove_dir_all(&root).ok();
}

/// The sweep removes exactly the trees it decided to remove, and nothing
/// else.
///
/// This is the before/after manifest the milestone's closure record shows:
/// the intended removals, and nothing more.
#[tokio::test(flavor = "current_thread")]
async fn the_sweep_removes_the_intended_trees_and_nothing_else() {
    let root = temp_root("sw");
    let _env = IsolatedEnv::new(&root);

    let pool = sweep_pool().await;
    let runs = codegg_core::agent_run::InMemoryAgentRunStore::new();
    let jobs = codegg_core::jobs::InMemoryJobStore::new();

    // One tree the sweep should reclaim: its session is old and idle.
    let idle = create_session(&pool).await;
    let idle_tree = codegg::scratch::session_scratch_dir(&idle).expect("plant a scratch tree");
    std::fs::write(idle_tree.join("artifact.bin"), b"disposable").expect("plant an artifact");
    set_activity(&pool, &idle, 30 * DAY_MS).await;

    // One tree the sweep must leave: its session is still inside the
    // retention window.
    let fresh = create_session(&pool).await;
    let fresh_tree = codegg::scratch::session_scratch_dir(&fresh).expect("plant a scratch tree");
    std::fs::write(fresh_tree.join("artifact.bin"), b"disposable").expect("plant an artifact");
    set_activity(&pool, &fresh, DAY_MS).await;

    // One tree the sweep must leave: a turn is admitted against it.
    let leased = create_session(&pool).await;
    let leased_tree = codegg::scratch::session_scratch_dir(&leased).expect("plant a scratch tree");
    std::fs::write(leased_tree.join("artifact.bin"), b"disposable").expect("plant an artifact");
    set_activity(&pool, &leased, 30 * DAY_MS).await;
    accept_turn(&pool, &leased).await;

    let before = manifest();
    assert_eq!(before.len(), 3, "manifest: {before:?}");

    let report = codegg::core::scratch_sweep::sweep_scratch_once(
        &pool,
        &runs,
        &jobs,
        codegg::core::scratch_sweep::ScratchRetentionPolicy::default(),
        chrono::Utc::now().timestamp_millis(),
    )
    .await;

    assert_eq!(report.reclaimed_idle, 1, "report: {report:?}");
    assert_eq!(report.skipped_leased, 1, "report: {report:?}");
    assert_eq!(report.skipped_active, 1, "report: {report:?}");
    assert_eq!(report.failed, 0, "report: {report:?}");

    let after = manifest();
    let expected: Vec<String> = before
        .iter()
        .filter(|name| !name.contains(&codegg::scratch::session_scratch_digest_hex(&idle)))
        .cloned()
        .collect();
    println!("CODEGG_WP0_MANIFEST_BEFORE_BEGIN");
    for name in &before {
        println!("{name}");
    }
    println!("CODEGG_WP0_MANIFEST_BEFORE_END");
    println!("CODEGG_WP0_MANIFEST_AFTER_BEGIN");
    for name in &after {
        println!("{name}");
    }
    println!("CODEGG_WP0_MANIFEST_AFTER_END");
    assert_eq!(after, expected, "the sweep removed the wrong trees");
    assert!(!idle_tree.exists(), "the idle tree is reclaimed");
    assert!(fresh_tree.exists(), "a fresh session keeps its tree");
    assert!(leased_tree.exists(), "a leased tree is not reclaimed");

    pool.close().await;
    std::fs::remove_dir_all(&root).ok();
}

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// Backdate a tree and everything inside it, so the census sees an age
/// distribution rather than a single instant.
#[cfg(unix)]
fn age_tree(path: &Path, age_ms: i64) {
    let modified = std::time::SystemTime::UNIX_EPOCH
        + std::time::Duration::from_millis(
            (chrono::Utc::now().timestamp_millis() - age_ms).max(0) as u64
        );
    let times = std::fs::FileTimes::new().set_modified(modified);
    let _ = std::fs::File::open(path).and_then(|file| file.set_times(times));
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let child = entry.path();
            if child.is_dir() {
                age_tree(&child, age_ms);
            } else {
                let _ = std::fs::File::open(&child).and_then(|file| file.set_times(times));
            }
        }
    }
}

/// The isolated daemon's configuration directory.
///
/// The daemon reads its configuration from `dirs::config_dir()/codegg/`, so
/// under the isolated `HOME` that is a directory inside the test's own root.
fn isolated_config_dir() -> PathBuf {
    dirs::config_dir().expect("config dir").join("codegg")
}

/// Write the isolated daemon's configuration.
///
/// The test states the policy it depends on rather than inheriting whatever
/// the runner's environment happens to provide.
fn write_isolated_config(body: &str) {
    std::fs::create_dir_all(isolated_config_dir()).expect("create the config directory");
    std::fs::write(isolated_config_dir().join("codegg.jsonc"), body).expect("write the config");
}

/// M007's WP0: the census input.
///
/// The census that M006 produces is what turns the retention default from a
/// *specified* number into a *measured* one. This test builds a population
/// through the daemon's own provisioning path — a real tree, with a real
/// owner record, named by a real digest — measures it with the **real** CLI
/// against a **real** daemon, and prints the report.
///
/// The population is synthetic: no live model turn was driven, so the byte
/// distribution reflects planted files rather than real agent output. The
/// *shape* of the population is what the retention decision depends on — how
/// many trees, how old, how large, and how many the reclaim validator
/// refuses — and that shape is reproduced faithfully here.
///
/// The daemon's **sweep is disabled** for this run. The trees are planted
/// with ages past the orphan grace period and no durable session owns them,
/// so a live sweep would reclaim them as orphans before the census could
/// measure them. The sweep's own behaviour is proved by the unit tests and by
/// the manifest test below.
#[tokio::test(flavor = "current_thread")]
async fn the_census_measures_a_populated_scratch_tree() {
    let root = temp_root("c0");
    let bin = codegg_binary();
    if !bin.exists() {
        eprintln!(
            "skipping: codegg binary not found at {}; set CODEGG_TEST_BIN to run",
            bin.display()
        );
        return;
    }

    let _env = IsolatedEnv::new(&root);

    // Disable the retention sweep for the measurement.
    write_isolated_config("{\n  \"scratch\": {\n    \"enabled\": false\n  }\n}\n");

    // A spread of ages: a day, a week, a month, and a year.
    for (label, age_days) in [("day", 1), ("week", 7), ("month", 30), ("year", 365)] {
        let session_id = format!("scratch-census-{label}");
        let tree = codegg::scratch::session_scratch_dir(&session_id).expect("plant a scratch tree");
        std::fs::write(
            tree.join("artifact.bin"),
            vec![b'x'; 4096 * age_days as usize],
        )
        .expect("plant an artifact");
        #[cfg(unix)]
        age_tree(&tree, age_days as i64 * DAY_MS);
    }

    // A tree holding a planted symlink: the automatic path refuses it, and
    // the census must say so rather than call it reclaimable.
    #[cfg(unix)]
    {
        let session_id = "scratch-census-linked";
        let tree = codegg::scratch::session_scratch_dir(session_id).expect("plant a scratch tree");
        let outside = root.join("census-outside");
        std::fs::create_dir_all(&outside).expect("create the outside directory");
        std::fs::write(outside.join("precious.txt"), b"outside content").expect("write outside");
        std::os::unix::fs::symlink(&outside, tree.join("link")).expect("plant a symlink");
        age_tree(&tree, 3 * DAY_MS);
    }

    // A tree holding a multiply-linked file. After M007's parity fix this
    // is counted but **not** refused: unlinking a name cannot modify the
    // file it names.
    #[cfg(unix)]
    {
        let session_id = "scratch-census-hardlinked";
        let tree = codegg::scratch::session_scratch_dir(session_id).expect("plant a scratch tree");
        let outside = root.join("census-hardlink-source");
        std::fs::write(&outside, b"shared content").expect("write the link source");
        std::fs::hard_link(&outside, tree.join("linked.bin")).expect("plant a hard link");
        age_tree(&tree, 5 * DAY_MS);
    }

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
    assert!(outcome.started_pid.is_some());
    assert!(
        wait_for_daemon_ready(&paths, Duration::from_secs(30)).await,
        "daemon did not become ready"
    );

    let json = Command::new(&bin)
        .args(["daemon", "scratch-usage", "--json", "--refresh"])
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

    // The report is the WP0 evidence. It is printed so it can be quoted in
    // the closure record, and written to `CODEGG_WP0_OUT` when the caller
    // asks for the artifact.
    println!("CODEGG_WP0_CENSUS_BEGIN");
    println!("{}", json_text.trim());
    println!("CODEGG_WP0_CENSUS_END");
    if let Ok(destination) = std::env::var("CODEGG_WP0_OUT") {
        std::fs::write(&destination, json_text.as_bytes()).expect("write the census artifact");
    }

    let report: serde_json::Value =
        serde_json::from_str(&json_text).expect("--json output must parse as JSON");
    assert_eq!(
        report["tree_count"].as_u64(),
        Some(6),
        "the census must reach every planted tree: {json_text}"
    );
    assert_eq!(
        report["truncated"].as_bool(),
        Some(false),
        "report: {json_text}"
    );

    // Exactly one tree is refused, and it is refused for the symlink.
    let trees = report["trees"].as_array().expect("trees array");
    // The age distribution is real, not a single instant: the planted trees
    // span a year, and the census reports each tree's own age.
    let tree_ages = trees
        .iter()
        .filter_map(|tree| tree["oldest_modified_ms"].as_i64())
        .collect::<Vec<_>>();
    assert_eq!(tree_ages.len(), 6, "report: {json_text}");
    let oldest = tree_ages.iter().min().expect("at least one tree");
    let newest = tree_ages.iter().max().expect("at least one tree");
    assert!(
        newest - oldest >= 300 * DAY_MS,
        "the planted age spread is missing from the census: {json_text}"
    );

    let refused: Vec<&serde_json::Value> = trees
        .iter()
        .filter(|tree| tree["reclaimable"].as_bool() == Some(false))
        .collect();
    assert_eq!(refused.len(), 1, "report: {json_text}");
    assert_eq!(
        refused[0]["refusal"].as_str(),
        Some("symbolic_link"),
        "report: {json_text}"
    );

    // The multiply-linked tree is counted but reclaimable: that is M007's
    // parity fix, observed.
    #[cfg(unix)]
    {
        let linked = trees
            .iter()
            .find(|tree| tree["multiply_linked_count"].as_u64() == Some(1))
            .expect("the multiply-linked tree is reported");
        assert_eq!(
            linked["reclaimable"].as_bool(),
            Some(true),
            "report: {json_text}"
        );
    }

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

/// The sweep fires in a **live daemon**, with no human action.
///
/// This is the end-to-end shape of the milestone: the daemon reclaims its
/// own disposable scratch on its own schedule. The population is planted
/// through the daemon's own provisioning path, aged past the orphan grace
/// period, and owns no durable session — so it is exactly what the sweep's
/// orphan rule is for. A fresh tree is planted alongside them and must
/// survive.
#[tokio::test(flavor = "current_thread")]
async fn the_live_daemon_reclaims_its_own_aged_trees() {
    let root = temp_root("lv");
    let bin = codegg_binary();
    if !bin.exists() {
        eprintln!(
            "skipping: codegg binary not found at {}; set CODEGG_TEST_BIN to run",
            bin.display()
        );
        return;
    }

    let _env = IsolatedEnv::new(&root);

    // State the policy this test depends on, so the daemon does not inherit
    // whatever the runner's environment happens to provide.
    write_isolated_config(
        "{\n  \"scratch\": {\n    \"enabled\": true,\n    \"sweep_interval_minutes\": 5,\n    \"orphan_grace_hours\": 1\n  }\n}\n",
    );

    let mut aged = Vec::new();
    for (label, age_days) in [("two", 2), ("ten", 10), ("hundred", 100)] {
        let session_id = format!("scratch-live-{label}");
        let tree = codegg::scratch::session_scratch_dir(&session_id).expect("plant a scratch tree");
        std::fs::write(tree.join("artifact.bin"), vec![b'x'; 1024]).expect("plant an artifact");
        #[cfg(unix)]
        age_tree(&tree, age_days as i64 * DAY_MS);
        // Prove the premise: the sweep reads the tree root's own modification
        // time, so the tree has to actually look old. Without this check a
        // platform whose timestamp setter does not apply to a directory would
        // fail later as a bare timeout instead of naming the cause.
        #[cfg(unix)]
        {
            let modified = std::fs::symlink_metadata(&tree)
                .and_then(|metadata| metadata.modified())
                .expect("read the tree's modification time");
            let age = std::time::SystemTime::now()
                .duration_since(modified)
                .expect("the tree's modification time is not in the future");
            assert!(
                age.as_millis() >= (age_days as i64 * DAY_MS) as u128,
                "the tree root's modification time was not backdated: {age:?}"
            );
        }
        aged.push(tree);
    }
    let fresh_session = "scratch-live-fresh";
    let fresh = codegg::scratch::session_scratch_dir(fresh_session).expect("plant a scratch tree");
    std::fs::write(fresh.join("artifact.bin"), vec![b'x'; 1024]).expect("plant an artifact");

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
    assert!(outcome.started_pid.is_some());
    assert!(
        wait_for_daemon_ready(&paths, Duration::from_secs(30)).await,
        "daemon did not become ready"
    );

    // The sweep runs on its own interval; wait for it to have reclaimed the
    // aged trees rather than assume a timing.
    //
    // The bound is generous on purpose. The sweep's first pass takes the
    // store's writer lock, and the daemon's own startup writes hold it; the
    // store's busy timeout is 30 seconds, so a loaded runner can make the
    // first pass wait that long before it does anything.
    let deadline = std::time::Instant::now() + Duration::from_secs(90);
    while std::time::Instant::now() < deadline && aged.iter().any(|tree| tree.exists()) {
        sleep(Duration::from_millis(100)).await;
    }

    let unreclaimed = aged.iter().any(|tree| tree.exists());

    // Ask the daemon what it sees. A tree that is still there and is not
    // quarantined means the sweep never ran; a quarantined tree means the
    // sweep ran and the reclaim validator refused it. Either way the report
    // names the cause instead of leaving a bare timeout.
    let census = Command::new(&bin)
        .args(["daemon", "scratch-usage", "--json", "--refresh"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .expect("run scratch-usage --json");
    let census_text = String::from_utf8_lossy(&census.stdout).to_string();

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

    assert!(
        !unreclaimed,
        "the live daemon must reclaim an aged tree with no human action; \
         the daemon's census says: {census_text}"
    );
    assert!(
        fresh.exists(),
        "a tree inside the grace period must survive the sweep"
    );

    let _ = std::fs::remove_dir_all(&root);
}

async fn sweep_pool() -> sqlx::SqlitePool {
    let url = format!(
        "file:scratch-retention-sweep-{}?mode=memory&cache=shared",
        uuid::Uuid::new_v4().simple()
    );
    let options = sqlx::sqlite::SqliteConnectOptions::from_str(&url)
        .expect("valid sqlite url")
        .create_if_missing(true)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .expect("sqlite pool");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("session schema");
    pool
}

async fn create_session(pool: &sqlx::SqlitePool) -> String {
    let store = codegg_core::session::SessionStore::new(pool.clone());
    let session = store
        .create(codegg_core::session::CreateSession {
            project_id: format!("scratch-retention-project-{}", uuid::Uuid::new_v4()),
            directory: std::env::temp_dir().to_string_lossy().into_owned(),
            title: Some("scratch retention test".into()),
            ..Default::default()
        })
        .await
        .expect("create session");
    session.id
}

async fn set_activity(pool: &sqlx::SqlitePool, session_id: &str, age_ms: i64) {
    let now = chrono::Utc::now().timestamp_millis();
    sqlx::query("UPDATE session SET time_updated = ? WHERE id = ?")
        .bind(now - age_ms)
        .bind(session_id)
        .execute(pool)
        .await
        .expect("backdate the session");
}

async fn accept_turn(pool: &sqlx::SqlitePool, session_id: &str) {
    let now = chrono::Utc::now().timestamp_millis();
    sqlx::query(
        "INSERT INTO turn_admission_receipt (submission_id, session_id, principal_id, \
         turn_id, request_sha256, status, accepted_at_ms, updated_at_ms) \
         VALUES (?, ?, ?, ?, ?, 'accepted', ?, ?)",
    )
    .bind(format!("submission-{session_id}"))
    .bind(session_id)
    .bind("local-owner")
    .bind(format!("turn-{session_id}"))
    .bind("0".repeat(64))
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .expect("accept a turn");
}
