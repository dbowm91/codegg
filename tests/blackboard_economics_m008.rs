//! M008 — the measured comparison for the agent-workstream blackboard.
//!
//! The functional arms live in `tests/blackboard_trajectory_m008.rs`. This
//! suite is the **measured** half of M008's work package D: it runs the same
//! configurations over a fixed task set, publishes the per-task results with
//! their denominators, and measures the Rust target-directory contention
//! that M007's counted build quota exists to bound.
//!
//! It is **opt-in**. The measurement takes real wall-clock time and spawns
//! real cargo processes, and CI must not pay for a measurement nobody reads.
//! Run it with:
//!
//! ```text
//! CODEGG_RUN_M008_ECONOMICS=1 cargo test --test blackboard_economics_m008 -- --nocapture
//! ```
//!
//! ## What is measured, and what is not
//!
//! **Measured**: the durable cost of the blackboard itself — the rows it
//! writes, the wall-clock its arbitration costs — and the duplicate work it
//! prevents. Every number below comes from real SQLite transactions, real
//! claim arbitration and real Git operations on real repositories.
//!
//! **Not measured**: model quality, token cost, and the wall-clock of a
//! model turn. Those need a live provider and a live model, and no such
//! provider exists in this tree. A report that implied otherwise would be
//! the exact extrapolation the plan forbids, so the report says so in its
//! own `scope` field.
//!
//! ## The Rust target-directory contention
//!
//! M007 admits a counted number of build jobs into a workspace's canonical
//! scope. This suite measures what that bound is worth by running the same
//! pair of cargo builds twice: once **serialized** (one at a time, which is
//! what the quota admits) and once **concurrent** (both admitted, which is
//! what an unbounded workspace would allow). Cargo takes its own lock on the
//! target directory either way, so the question the measurement answers is
//! whether admitting the second build buys anything — and what it costs.
//!
//! The measurement lives in the **isolated** arms on purpose: the shared
//! profile refuses every process-capable surface, so a build inside a shared
//! checkout is refused by contract and could not be measured there at all.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::blackboard::{add_worktree, commit_all, git, init_repository, Blackboard};

/// Write a file inside a checkout, creating its parent directories.
fn write_in(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create the file's parent directory");
    }
    std::fs::write(path, contents).expect("write the file");
}
use codegg_core::workstream_claim::WorkstreamClaimError;

/// The opt-in. Without it the suite reports that it did not run and passes.
const OPT_IN: &str = "CODEGG_RUN_M008_ECONOMICS";

/// Where the report artifacts are written. The measurement writes files, so
/// they go under the build directory rather than into the working tree.
fn report_path(name: &str) -> PathBuf {
    let directory = PathBuf::from("target").join("m008-economics");
    std::fs::create_dir_all(&directory).expect("create the report directory");
    directory.join(name)
}

/// How many times each configuration runs each stratum. The denominator is
/// published with the results, so a reader can see the sample size rather
/// than take a single number on faith.
const REPETITIONS: usize = 3;

/// How long the generated build script holds the target directory's lock, so
/// the contention window is wide enough to observe rather than a race.
const BUILD_LOCK_HOLD_MS: u64 = 1_200;

// ---------------------------------------------------------------------------
// The fixed task set
// ---------------------------------------------------------------------------

/// One stratum of the fixed task set.
#[derive(Clone, Copy)]
struct Stratum {
    name: &'static str,
    /// The paths the two children work on.
    left: &'static str,
    right: &'static str,
    /// Whether the two children want the same path.
    overlapping: bool,
}

/// The fixed task set. Three strata, chosen to cover the three shapes the
/// blackboard can meet: one child, two children on disjoint paths, and two
/// children on the same path.
const STRATA: [Stratum; 3] = [
    Stratum {
        name: "single",
        left: "src/alpha.rs",
        right: "src/alpha.rs",
        overlapping: false,
    },
    Stratum {
        name: "disjoint",
        left: "src/alpha.rs",
        right: "src/beta.rs",
        overlapping: false,
    },
    Stratum {
        name: "overlapping",
        left: "src/shared.rs",
        right: "src/shared.rs",
        overlapping: true,
    },
];

/// The measured result of one configuration on one stratum.
#[derive(Clone)]
struct Measurement {
    configuration: &'static str,
    stratum: &'static str,
    repetitions: usize,
    /// The wall-clock of the whole configuration, summed over repetitions.
    wall_ms_total: u128,
    wall_ms_min: u128,
    wall_ms_max: u128,
    /// The durable blackboard rows the configuration wrote.
    durable_writes: i64,
    /// How many times two children wrote the same path.
    duplicate_edits: u64,
    /// How many times the explicit integration conflicted.
    conflicts: u64,
}

/// Run one configuration over one stratum, once, and return what happened.
async fn run_once(configuration: &'static str, stratum: Stratum) -> (Duration, i64, u64, u64) {
    let started = Instant::now();
    let repository = tempfile::tempdir().expect("the repository");
    init_repository(repository.path());
    let scratch = tempfile::tempdir().expect("the worktree scratch");
    let left_root = scratch.path().join("left");
    let right_root = scratch.path().join("right");
    add_worktree(repository.path(), &left_root, "child/left");
    add_worktree(repository.path(), &right_root, "child/right");

    let mut duplicate_edits = 0u64;
    let mut conflicts = 0u64;
    let durable_writes = match configuration {
        // S0: one agent on an ordinary checkout. No blackboard at all.
        "S0" => {
            let blackboard = Blackboard::solo().await;
            write_in(&left_root, stratum.left, "left\n");
            commit_all(&left_root, "the single agent");
            blackboard.row_count("workstream_claim").await
                + blackboard.row_count("workstream_claim_event").await
                + blackboard.row_count("agent_run_group").await
        }
        // S1: two children, separate worktrees, no board. Both children work
        // the path they were given, so on the overlapping stratum they both
        // write the same path.
        "S1" => {
            let blackboard = Blackboard::new(2).await;
            write_in(&left_root, stratum.left, "left\n");
            commit_all(&left_root, "the left child");
            write_in(&right_root, stratum.right, "right\n");
            commit_all(&right_root, "the right child");
            if stratum.overlapping {
                duplicate_edits += 1;
            }
            // The explicit integration.
            let merge = Command::new("git")
                .args(["merge", "--no-ff", "child/left"])
                .current_dir(&right_root)
                .output()
                .expect("run the explicit merge");
            if !merge.status.success() {
                conflicts += 1;
                let _ = Command::new("git")
                    .args(["merge", "--abort"])
                    .current_dir(&right_root)
                    .output();
            }
            blackboard.row_count("workstream_claim").await
                + blackboard.row_count("workstream_claim_event").await
                + blackboard.row_count("agent_run_group").await
        }
        // S3: two children with a claim board. The child that cannot take
        // the claim does not do the work, so the duplicate edit never
        // happens — at the cost of the claim rows.
        "S3" => {
            let blackboard = Blackboard::new(2).await;
            let (left_run, _) = &blackboard.members[0];
            let (right_run, _) = &blackboard.members[1];

            let left_claim = blackboard
                .claims
                .claim(blackboard.claim_input(left_run, "left", stratum.left))
                .await;
            assert!(left_claim.is_ok(), "the first child takes its claim");

            let right_claim = blackboard
                .claims
                .claim(blackboard.claim_input(right_run, "right", stratum.right))
                .await;
            let right_wrote = match right_claim {
                Ok(_) => {
                    write_in(&right_root, stratum.right, "right\n");
                    commit_all(&right_root, "the right child");
                    true
                }
                Err(WorkstreamClaimError::Overlap) => false,
                Err(error) => panic!("unexpected claim result: {error}"),
            };

            write_in(&left_root, stratum.left, "left\n");
            commit_all(&left_root, "the left child");

            if stratum.overlapping && right_wrote {
                duplicate_edits += 1;
            }
            let merge = Command::new("git")
                .args(["merge", "--no-ff", "child/left"])
                .current_dir(&right_root)
                .output()
                .expect("run the explicit merge");
            if !merge.status.success() {
                conflicts += 1;
                let _ = Command::new("git")
                    .args(["merge", "--abort"])
                    .current_dir(&right_root)
                    .output();
            }
            blackboard.row_count("workstream_claim").await
                + blackboard.row_count("workstream_claim_event").await
                + blackboard.row_count("agent_run_group").await
        }
        other => panic!("unknown configuration {other}"),
    };

    (
        started.elapsed(),
        durable_writes,
        duplicate_edits,
        conflicts,
    )
}

/// Measure one configuration over one stratum, over the published number of
/// repetitions.
async fn measure(configuration: &'static str, stratum: Stratum) -> Measurement {
    let mut wall_ms_total = 0u128;
    let mut wall_ms_min = u128::MAX;
    let mut wall_ms_max = 0u128;
    let mut durable_writes = 0i64;
    let mut duplicate_edits = 0u64;
    let mut conflicts = 0u64;

    for _ in 0..REPETITIONS {
        let (elapsed, writes, duplicates, conflict) = run_once(configuration, stratum).await;
        let millis = elapsed.as_millis();
        wall_ms_total += millis;
        wall_ms_min = wall_ms_min.min(millis);
        wall_ms_max = wall_ms_max.max(millis);
        durable_writes += writes;
        duplicate_edits += duplicates;
        conflicts += conflict;
    }

    Measurement {
        configuration,
        stratum: stratum.name,
        repetitions: REPETITIONS,
        wall_ms_total,
        wall_ms_min,
        wall_ms_max,
        durable_writes,
        duplicate_edits,
        conflicts,
    }
}

// ---------------------------------------------------------------------------
// The Rust target-directory contention
// ---------------------------------------------------------------------------

/// A generated crate whose build script holds the target directory's lock
/// long enough for a second build to collide with it.
fn generate_contended_crate(root: &Path) {
    std::fs::create_dir_all(root.join("src")).expect("create the crate source directory");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"m008-target-contention\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[dependencies]\n",
    )
    .expect("write the crate manifest");
    std::fs::write(root.join("src/main.rs"), "fn main() {}\n").expect("write the crate source");
    std::fs::write(
        root.join("build.rs"),
        format!(
            "fn main() {{\n    println!(\"cargo:rerun-if-changed=build.rs\");\n    std::thread::sleep(std::time::Duration::from_millis({BUILD_LOCK_HOLD_MS}));\n}}\n"
        ),
    )
    .expect("write the build script");
}

/// The peak resident set of a running process, in kibibytes.
#[cfg(target_os = "linux")]
fn peak_rss_kib(pid: u32) -> Option<u64> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmHWM:") {
            return rest
                .split_whitespace()
                .next()
                .and_then(|value| value.parse::<u64>().ok());
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
fn peak_rss_kib(_pid: u32) -> Option<u64> {
    None
}

/// Run one cargo build and report its wall-clock, its peak resident set and
/// whether it had to wait for the target directory's lock.
struct BuildOutcome {
    wall_ms: u128,
    peak_rss_kib: Option<u64>,
    waited_for_lock: bool,
    stderr: String,
}

/// A cargo build that has been started but not yet waited for.
struct RunningBuild {
    child: std::process::Child,
    stop: Arc<AtomicBool>,
    peak: Arc<AtomicU64>,
    sampler: Option<std::thread::JoinHandle<()>>,
    started: Instant,
}

/// Start a cargo build without waiting for it, so two builds can be in
/// flight at the same time — which is the whole point of the concurrent arm.
fn spawn_build(cargo: &str, crate_root: &Path, target_dir: &Path) -> RunningBuild {
    let child = Command::new(cargo)
        .args(["build"])
        .current_dir(crate_root)
        .env("CARGO_TARGET_DIR", target_dir)
        .env("CARGO_TERM_COLOR", "never")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the cargo build");

    let pid = child.id();
    let stop = Arc::new(AtomicBool::new(false));
    let peak = Arc::new(AtomicU64::new(0));
    let sampler = {
        let stop = Arc::clone(&stop);
        let peak = Arc::clone(&peak);
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                if let Some(kib) = peak_rss_kib(pid) {
                    peak.fetch_max(kib, Ordering::Relaxed);
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        })
    };

    RunningBuild {
        child,
        stop,
        peak,
        sampler: Some(sampler),
        started: Instant::now(),
    }
}

/// Wait for a started build and report what it did.
fn finish_build(mut build: RunningBuild) -> BuildOutcome {
    let output = build
        .child
        .wait_with_output()
        .expect("wait for the cargo build");
    build.stop.store(true, Ordering::Relaxed);
    if let Some(sampler) = build.sampler.take() {
        let _ = sampler.join();
    }
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    BuildOutcome {
        wall_ms: build.started.elapsed().as_millis(),
        peak_rss_kib: Some(build.peak.load(Ordering::Relaxed)).filter(|value| *value > 0),
        waited_for_lock: stderr.contains("Blocking waiting for file lock"),
        stderr,
    }
}

/// Run one cargo build to completion.
fn run_build(cargo: &str, crate_root: &Path, target_dir: &Path) -> BuildOutcome {
    finish_build(spawn_build(cargo, crate_root, target_dir))
}

/// The contention measurement: the same pair of builds, serialized and
/// concurrent.
struct Contention {
    serialized_ms: u128,
    concurrent_ms: u128,
    /// The wall-clock of each build in each arm, in order.
    serialized_build_ms: Vec<u128>,
    concurrent_build_ms: Vec<u128>,
    /// How many builds in each arm had to wait for the target directory's
    /// lock. The serialized arm cannot wait — its builds never overlap — so
    /// this is the structural difference between the arms.
    serialized_lock_waits: usize,
    concurrent_lock_waits: usize,
    serialized_peak_rss_kib: Option<u64>,
    concurrent_peak_rss_kib: Option<u64>,
    cargo_version: String,
}

fn measure_contention() -> Contention {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let workspace = tempfile::tempdir().expect("the contention workspace");
    let crate_root = workspace.path().join("crate");
    generate_contended_crate(&crate_root);

    let cargo_version = Command::new(&cargo)
        .arg("--version")
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned());

    // The serialized arm: the first build completes before the second
    // starts. This is what M007's counted quota admits.
    let serialized_target = workspace.path().join("target-serialized");
    let first = run_build(&cargo, &crate_root, &serialized_target);
    let second = run_build(&cargo, &crate_root, &serialized_target);
    let serialized_ms = first.wall_ms + second.wall_ms;
    let serialized_lock_waits = [first.waited_for_lock, second.waited_for_lock]
        .iter()
        .filter(|waited| **waited)
        .count();
    let serialized_build_ms = vec![first.wall_ms, second.wall_ms];

    // The concurrent arm: **both** builds are admitted at once, which is
    // what an unbounded workspace would allow. The two builds are started
    // before either is waited for, so they really are in flight together.
    let concurrent_target = workspace.path().join("target-concurrent");
    let started = Instant::now();
    let running_first = spawn_build(&cargo, &crate_root, &concurrent_target);
    let running_second = spawn_build(&cargo, &crate_root, &concurrent_target);
    let concurrent_first = finish_build(running_first);
    let concurrent_second = finish_build(running_second);
    let concurrent_ms = started.elapsed().as_millis();

    let concurrent_lock_waits = [
        concurrent_first.waited_for_lock,
        concurrent_second.waited_for_lock,
    ]
    .iter()
    .filter(|waited| **waited)
    .count();

    Contention {
        serialized_ms,
        concurrent_ms,
        serialized_build_ms,
        concurrent_build_ms: vec![concurrent_first.wall_ms, concurrent_second.wall_ms],
        serialized_lock_waits,
        concurrent_lock_waits,
        serialized_peak_rss_kib: first.peak_rss_kib.max(second.peak_rss_kib),
        concurrent_peak_rss_kib: concurrent_first
            .peak_rss_kib
            .max(concurrent_second.peak_rss_kib),
        cargo_version,
    }
}

// ---------------------------------------------------------------------------
// The report
// ---------------------------------------------------------------------------

/// The measured comparison across the configurations.
#[tokio::test]
async fn the_configuration_comparison_publishes_its_denominators() {
    if std::env::var(OPT_IN).as_deref() != Ok("1") {
        eprintln!("M008 economics is UNRUN: set {OPT_IN}=1 to run the measured comparison");
        return;
    }

    let mut measurements = Vec::new();
    for configuration in ["S0", "S1", "S3"] {
        for stratum in STRATA {
            measurements.push(measure(configuration, stratum).await);
        }
    }

    let contention = measure_contention();

    let report = serde_json::json!({
        "scope": {
            "measured": [
                "the durable blackboard rows a configuration writes",
                "the wall-clock of the blackboard's own arbitration over real SQLite transactions",
                "the duplicate edits two children perform on the same path",
                "the conflicts the explicit integration reports",
                "the Rust target-directory contention between two cargo builds"
            ],
            "not_measured": [
                "model quality",
                "token cost",
                "the wall-clock of a model turn",
                "any live-provider behaviour"
            ],
            "why": "no live provider and no live model exist in this tree; a report implying otherwise would be the extrapolation the plan forbids"
        },
        "denominators": {
            "repetitions_per_configuration_per_stratum": REPETITIONS,
            "strata": STRATA.iter().map(|stratum| stratum.name).collect::<Vec<_>>(),
            "configurations": ["S0", "S1", "S3"],
            "build_lock_hold_ms": BUILD_LOCK_HOLD_MS
        },
        "results": measurements.iter().map(|measurement| serde_json::json!({
            "configuration": measurement.configuration,
            "stratum": measurement.stratum,
            "repetitions": measurement.repetitions,
            "wall_ms_total": measurement.wall_ms_total,
            "wall_ms_min": measurement.wall_ms_min,
            "wall_ms_max": measurement.wall_ms_max,
            "wall_ms_per_repetition_mean": measurement.wall_ms_total / measurement.repetitions as u128,
            "durable_blackboard_writes_total": measurement.durable_writes,
            "durable_blackboard_writes_per_repetition": measurement.durable_writes / measurement.repetitions as i64,
            "duplicate_edits_total": measurement.duplicate_edits,
            "conflicts_total": measurement.conflicts
        })).collect::<Vec<_>>(),
        "rust_target_dir_contention": {
            "cargo": contention.cargo_version,
            "builds": 2,
            "serialized_ms": contention.serialized_ms,
            "concurrent_ms": contention.concurrent_ms,
            "serialized_build_ms": contention.serialized_build_ms,
            "concurrent_build_ms": contention.concurrent_build_ms,
            "serialized_lock_waits": contention.serialized_lock_waits,
            "concurrent_lock_waits": contention.concurrent_lock_waits,
            "serialized_peak_rss_kib": contention.serialized_peak_rss_kib,
            "concurrent_peak_rss_kib": contention.concurrent_peak_rss_kib,
            "note": "cargo takes its own target-directory lock either way; the measurement reports what admitting the second build concurrently costs rather than assuming it helps"
        },
        "limitations": [
            "the sample is small by construction; the denominators are published with every number",
            "wall-clock on a shared, contended host is not a per-machine benchmark",
            "the configuration arms measure the blackboard's own mechanics, not model behaviour"
        ]
    });

    let path = report_path("blackboard-m008-economics.json");
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&report).expect("serialize the economics report"),
    )
    .expect("write the economics report artifact");
    println!("M008_ECONOMICS_REPORT={report}");
}

// ---------------------------------------------------------------------------
// The contention arm, on its own
// ---------------------------------------------------------------------------

/// The Rust target-directory contention measurement, published on its own so
/// it can be read without the rest of the report.
///
/// The claim M007 makes is that a counted build quota in a workspace does not
/// silently amplify CPU or disk. This measurement is the evidence for the
/// half of that claim a benchmark can answer: what does admitting a second
/// build into one target directory actually cost?
#[test]
fn the_rust_target_dir_contention_is_measured_and_published() {
    if std::env::var(OPT_IN).as_deref() != Ok("1") {
        eprintln!(
            "M008 economics is UNRUN: set {OPT_IN}=1 to run the target-dir contention measurement"
        );
        return;
    }

    let contention = measure_contention();
    let report = serde_json::json!({
        "cargo": contention.cargo_version,
        "builds": 2,
        "build_lock_hold_ms": BUILD_LOCK_HOLD_MS,
        "serialized_ms": contention.serialized_ms,
        "concurrent_ms": contention.concurrent_ms,
        "serialized_build_ms": contention.serialized_build_ms,
        "concurrent_build_ms": contention.concurrent_build_ms,
        "serialized_lock_waits": contention.serialized_lock_waits,
        "concurrent_lock_waits": contention.concurrent_lock_waits,
        "serialized_peak_rss_kib": contention.serialized_peak_rss_kib,
        "concurrent_peak_rss_kib": contention.concurrent_peak_rss_kib,
        "scope": "the isolated arms only: the shared profile refuses every process-capable surface, so a build inside a shared checkout could not be measured there"
    });
    let path = report_path("blackboard-m008-target-contention.json");
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&report).expect("serialize the contention report"),
    )
    .expect("write the contention report artifact");
    println!("M008_TARGET_CONTENTION_REPORT={report}");

    // The measurement is only meaningful if it actually observed the
    // contention it is about. This is a **structural** claim, not a
    // wall-clock one: the serialized arm's builds never overlap, so they
    // cannot wait for the target directory's lock, while the concurrent
    // arm's second build must. Asserting a wall-clock ordering instead
    // would be asserting that a contended host did not reorder two
    // millisecond-scale measurements.
    assert_eq!(
        contention.serialized_lock_waits, 0,
        "the serialized arm cannot wait for a lock it never contends for"
    );
    assert!(
        contention.concurrent_lock_waits > 0,
        "the concurrent arm must actually collide on the target directory's lock"
    );
}

/// The generated crate is buildable at all. A contention measurement over a
/// crate that does not compile measures nothing.
#[test]
fn the_contended_crate_compiles() {
    if std::env::var(OPT_IN).as_deref() != Ok("1") {
        eprintln!("M008 economics is UNRUN: set {OPT_IN}=1 to build the contended crate");
        return;
    }
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let workspace = tempfile::tempdir().expect("the contention workspace");
    let crate_root = workspace.path().join("crate");
    generate_contended_crate(&crate_root);
    let target = workspace.path().join("target");
    let outcome = run_build(&cargo, &crate_root, &target);
    assert!(
        outcome.stderr.is_empty() || !outcome.stderr.contains("error["),
        "the contended crate must compile: {}",
        outcome.stderr
    );
}

/// The Git fixture is a real repository, so the measurements above are
/// measuring Git operations and not a directory that looks like one.
#[test]
fn the_measurement_fixture_is_a_real_repository() {
    let repository = tempfile::tempdir().expect("the repository");
    init_repository(repository.path());
    let log = git(&["log", "--oneline"], repository.path());
    assert!(
        String::from_utf8_lossy(&log.stdout).contains("base"),
        "the fixture repository has a real commit"
    );
    let status = git(&["status", "--porcelain"], repository.path());
    assert!(
        status.stdout.is_empty(),
        "the fixture repository starts clean"
    );
}
