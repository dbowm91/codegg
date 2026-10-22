#![cfg(target_os = "linux")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;

use codegg::git_service::{GitExecutionService, GitPayload};
use codegg::managed_process::{BoundedOutput, ManagedProcessResult, SandboxExecutionOutcome};

const BENCH_OPT_IN: &str = "CODEGG_RUN_LINUX_GIT_COST_PERF";
const M005_OPT_IN: &str = "CODEGG_RUN_M005_OFFLINE_NATIVE";
const OBJECT_COUNT: usize = 4096;
const BYTES_PER_OBJECT: usize = 25_600;
const COLD_SAMPLES: usize = 30;
const WARM_SAMPLES: usize = 100;
const CONTENTION_BATCHES: usize = 30;

#[derive(Debug, Serialize)]
struct RouteReport {
    cold_us: Vec<u128>,
    cold_p50_us: u128,
    cold_p95_us: u128,
    warm_us: Vec<u128>,
    warm_p50_us: u128,
    warm_p95_us: u128,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct ChildResourceSample {
    user_cpu_us: u64,
    system_cpu_us: u64,
    max_rss_kib: u64,
}

#[derive(Debug, Serialize)]
struct ResourceMetricSummary {
    p50: u64,
    p95: u64,
    max: u64,
}

#[derive(Debug, Serialize)]
struct ChildResourceSummary {
    samples: usize,
    cpu_time_resolution_us: u64,
    user_cpu_us: ResourceMetricSummary,
    system_cpu_us: ResourceMetricSummary,
    max_rss_kib: ResourceMetricSummary,
}

#[derive(Debug, Serialize)]
struct ContentionRouteReport {
    p50_us: u128,
    p95_us: u128,
    max_us: u128,
    samples: usize,
}

#[derive(Debug, Serialize)]
struct ContentionReport {
    concurrent_calls: usize,
    native_git: ContentionRouteReport,
    immutable_snapshot: ContentionRouteReport,
    contained_live_odb: ContentionRouteReport,
}

#[derive(Debug, Serialize)]
struct FixtureReport {
    operation: String,
    shape: String,
    filesystem: String,
    cold_condition: String,
    object_store_bytes: u64,
    snapshot_bytes_to_copy_cold_estimate: u64,
    loose_objects: u64,
    pack_files: u64,
    native_git: RouteReport,
    immutable_snapshot: RouteReport,
    contained_live_odb: RouteReport,
    contained_live_child_resources_cold: ChildResourceSummary,
    contained_live_child_resources_warm: ChildResourceSummary,
    warm_contention: Vec<ContentionReport>,
}

fn git_binary() -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").expect("runner PATH"))
        .map(|directory| directory.join("git"))
        .find(|candidate| candidate.is_file())
        .expect("native Git is installed")
        .canonicalize()
        .expect("canonicalize Git executable")
}

fn git_output(git: &Path, cwd: &Path, args: &[String]) -> std::process::Output {
    Command::new(git)
        .args(args)
        .current_dir(cwd)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("run native Git command")
}

fn git_ok(git: &Path, cwd: &Path, args: &[&str]) -> std::process::Output {
    let output = git_output(
        git,
        cwd,
        &args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>(),
    );
    assert!(
        output.status.success(),
        "Git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn make_payload(seed: u64) -> Vec<u8> {
    let mut state = seed.max(1);
    let mut bytes = vec![0_u8; BYTES_PER_OBJECT];
    for byte in &mut bytes {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *byte = (state >> 24) as u8;
    }
    bytes
}

fn object_store_stats(object_dir: &Path) -> (u64, u64, u64) {
    fn walk(path: &Path, bytes: &mut u64, loose: &mut u64, packs: &mut u64) {
        for entry in fs::read_dir(path).expect("read Git object directory") {
            let entry = entry.expect("read Git object entry");
            let entry_path = entry.path();
            let metadata = fs::symlink_metadata(&entry_path).expect("stat Git object entry");
            assert!(
                !metadata.file_type().is_symlink(),
                "benchmark fixture unexpectedly contains an object-store symlink"
            );
            if metadata.is_dir() {
                walk(&entry_path, bytes, loose, packs);
            } else if metadata.is_file() {
                *bytes += metadata.len();
                if entry_path
                    .parent()
                    .and_then(Path::file_name)
                    .is_some_and(|name| name == "pack")
                    && entry_path.extension().is_some_and(|ext| ext == "pack")
                {
                    *packs += 1;
                } else if entry_path
                    .parent()
                    .and_then(Path::file_name)
                    .is_some_and(|name| name.to_string_lossy().len() == 2)
                {
                    *loose += 1;
                }
            }
        }
    }

    let (mut bytes, mut loose, mut packs) = (0, 0, 0);
    walk(object_dir, &mut bytes, &mut loose, &mut packs);
    (bytes, loose, packs)
}

fn create_fixture(git: &Path, root: &Path, packed: bool) -> (u64, u64, u64) {
    fs::create_dir_all(root.join("objects-data")).expect("create fixture worktree");
    git_ok(git, root, &["init", "--quiet"]);
    // Keep the source ODB stable while cold copies are repeated. Automatic
    // maintenance is outside the measured routes and could rewrite/remove
    // fixture metadata between an entry's read_dir and stat calls.
    git_ok(git, root, &["config", "gc.auto", "0"]);
    git_ok(git, root, &["config", "maintenance.auto", "false"]);
    for index in 0..OBJECT_COUNT {
        let path = root
            .join("objects-data")
            .join(format!("object-{index:04}.bin"));
        fs::write(&path, make_payload(index as u64 + 0x9e37_79b9))
            .expect("write incompressible Git object payload");
    }
    git_ok(git, root, &["add", "--", "objects-data"]);
    let commit = git_output(
        git,
        root,
        &[
            "-c".into(),
            "user.name=Perf Fixture".into(),
            "-c".into(),
            "user.email=perf@example.invalid".into(),
            "commit".into(),
            "--quiet".into(),
            "-m".into(),
            "performance fixture".into(),
        ],
    );
    assert!(
        commit.status.success(),
        "fixture commit failed: {}",
        String::from_utf8_lossy(&commit.stderr)
    );
    if packed {
        git_ok(git, root, &["gc", "--aggressive", "--prune=now"]);
    }
    let object_dir = fs::canonicalize(root.join(".git/objects")).expect("resolve fixture ODB");
    let (bytes, loose, packs) = object_store_stats(&object_dir);
    assert!(
        bytes >= 95 * 1024 * 1024,
        "fixture ODB is too small: {bytes}"
    );
    if packed {
        assert!(packs > 0, "packed fixture has no pack file");
    } else {
        assert!(
            loose >= OBJECT_COUNT as u64,
            "fixture lacks 4,096 loose objects"
        );
    }
    (bytes, loose, packs)
}

fn copy_metadata_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("create cold repository root");
    for entry in fs::read_dir(source).expect("read source Git metadata") {
        let entry = entry.expect("read source metadata entry");
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path).expect("stat source Git metadata");
        assert!(
            !metadata.file_type().is_symlink(),
            "benchmark source metadata unexpectedly contains a symlink"
        );
        if metadata.is_dir() {
            copy_metadata_tree(&source_path, &destination_path);
        } else {
            fs::copy(&source_path, &destination_path).expect("copy cold Git metadata");
        }
    }
}

fn copy_gitdir(source_repository: &Path, destination_repository: &Path) {
    fs::create_dir_all(destination_repository).expect("create cold sample repository");
    copy_metadata_tree(
        &source_repository.join(".git"),
        &destination_repository.join(".git"),
    );
}

fn advise_drop_object_cache(object_dir: &Path) {
    use std::os::fd::AsRawFd;

    let mut pending = vec![object_dir.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).expect("read object-store directory") {
            let entry = entry.expect("read object-store entry");
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).expect("stat object-store entry");
            assert!(
                !metadata.file_type().is_symlink(),
                "benchmark fixture unexpectedly contains an object-store symlink"
            );
            if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file() {
                let file = fs::File::open(path).expect("open object-store file");
                // SAFETY: the descriptor refers to this open regular file.
                // DONTNEED is advisory and does not change repository content.
                let result = unsafe {
                    libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED)
                };
                assert_eq!(result, 0, "advise object-file page-cache eviction");
            }
        }
    }
}

fn operation() -> codegg_git::GitOperation {
    codegg_git::GitOperation::Log {
        oneline: true,
        max_count: Some(1),
        paths: Vec::new(),
    }
}

fn status_operation() -> codegg_git::GitOperation {
    codegg_git::GitOperation::Status { short: true }
}

fn native_git(git: &Path, repository: &Path, argv: &[String]) -> std::process::Output {
    let output = git_output(git, repository, &argv[1..]);
    assert!(
        output.status.success(),
        "native Git read failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

async fn immutable_snapshot(
    service: &GitExecutionService,
    operation: &codegg_git::GitOperation,
    repository: &Path,
) -> codegg::git_service::GitExecutionResult {
    let result = service
        .execute(operation, repository)
        .await
        .expect("typed snapshot Git read");
    assert!(result.success, "typed snapshot Git read returned failure");
    match (operation, result.payload.as_ref()) {
        (codegg_git::GitOperation::Log { .. }, Some(GitPayload::Log(_)))
        | (codegg_git::GitOperation::Status { .. }, Some(GitPayload::Status(_))) => {}
        (operation, payload) => panic!("unexpected payload for {operation:?}: {payload:?}"),
    }
    result
}

async fn concurrent_native_batch(
    git: &Path,
    repository: &Path,
    argv: &[String],
    expected: &[u8],
    concurrency: usize,
) -> Vec<u128> {
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..concurrency {
        let git = git.to_path_buf();
        let repository = repository.to_path_buf();
        let argv = argv.to_vec();
        let expected = expected.to_vec();
        tasks.spawn_blocking(move || {
            let started = Instant::now();
            let output = native_git(&git, &repository, &argv);
            assert_eq!(output.stdout, expected, "concurrent native Git parity");
            elapsed_us(started)
        });
    }
    let mut samples = Vec::with_capacity(concurrency);
    while let Some(result) = tasks.join_next().await {
        samples.push(result.expect("join concurrent native Git read"));
    }
    samples
}

async fn concurrent_snapshot_batch(
    service: Arc<GitExecutionService>,
    operation: &codegg_git::GitOperation,
    repository: &Path,
    expected: &[u8],
    concurrency: usize,
) -> Vec<u128> {
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..concurrency {
        let service = Arc::clone(&service);
        let operation = operation.clone();
        let repository = repository.to_path_buf();
        let expected = expected.to_vec();
        tasks.spawn(async move {
            let started = Instant::now();
            let result = service
                .execute(&operation, &repository)
                .await
                .expect("concurrent immutable-snapshot Git read");
            assert!(result.success, "concurrent snapshot read failed");
            match (&operation, result.payload.as_ref()) {
                (codegg_git::GitOperation::Log { .. }, Some(GitPayload::Log(_)))
                | (codegg_git::GitOperation::Status { .. }, Some(GitPayload::Status(_))) => {}
                (operation, payload) => panic!("unexpected payload for {operation:?}: {payload:?}"),
            }
            assert_eq!(result.stdout.as_bytes(), expected);
            elapsed_us(started)
        });
    }
    let mut samples = Vec::with_capacity(concurrency);
    while let Some(result) = tasks.join_next().await {
        samples.push(result.expect("join concurrent immutable-snapshot read"));
    }
    samples
}

async fn concurrent_live_batch(
    service: Arc<GitExecutionService>,
    operation: codegg_git::GitOperation,
    git: &Path,
    repository: &Path,
    expected: &[u8],
    concurrency: usize,
) -> Vec<u128> {
    let mut tasks = tokio::task::JoinSet::new();
    for index in 0..concurrency {
        let service = Arc::clone(&service);
        let operation = operation.clone();
        let git = git.to_path_buf();
        let repository = repository.to_path_buf();
        let expected = expected.to_vec();
        tasks.spawn(async move {
            let started = Instant::now();
            let result = contained_live_git(
                &service,
                &operation,
                &git,
                &repository,
                &format!("contention-{concurrency}-{index}"),
            )
            .await;
            assert_eq!(output_bytes(&result.stdout), expected);
            elapsed_us(started)
        });
    }
    let mut samples = Vec::with_capacity(concurrency);
    while let Some(result) = tasks.join_next().await {
        samples.push(result.expect("join concurrent contained Git read"));
    }
    samples
}

async fn contained_live_git(
    service: &GitExecutionService,
    operation: &codegg_git::GitOperation,
    git: &Path,
    repository: &Path,
    label: &str,
) -> ManagedProcessResult {
    let result = service
        .execute_contained_live_read_test_only(operation, repository, git, label, true)
        .await
        .expect("required contained Git must run or fail closed");
    assert!(
        result.exit_status.success(),
        "contained Git read failed: {}",
        result.stderr.to_string_lossy()
    );
    assert!(
        matches!(result.sandbox, SandboxExecutionOutcome::Enforced { .. }),
        "contained Git read lacks obtained sandbox enforcement"
    );
    result
}

fn child_resource_sample(output: &BoundedOutput) -> ChildResourceSample {
    let stderr = output.to_string_lossy();
    let fields = stderr
        .lines()
        .find_map(|line| line.strip_prefix("CODEGG_TIME\t"))
        .expect("GNU time resource marker is present")
        .split('\t')
        .collect::<Vec<_>>();
    assert_eq!(fields.len(), 3, "GNU time resource marker has three fields");
    let cpu_us = |value: &str| {
        (value
            .parse::<f64>()
            .expect("GNU time CPU seconds are numeric")
            * 1_000_000.0) as u64
    };
    ChildResourceSample {
        user_cpu_us: cpu_us(fields[0]),
        system_cpu_us: cpu_us(fields[1]),
        max_rss_kib: fields[2].parse().expect("GNU time peak RSS is numeric"),
    }
}

fn percentile(samples: &[u128], percent: usize) -> u128 {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let rank = (sorted.len() * percent).div_ceil(100).saturating_sub(1);
    sorted[rank]
}

fn route_report(cold: Vec<u128>, warm: Vec<u128>) -> RouteReport {
    RouteReport {
        cold_p50_us: percentile(&cold, 50),
        cold_p95_us: percentile(&cold, 95),
        warm_p50_us: percentile(&warm, 50),
        warm_p95_us: percentile(&warm, 95),
        cold_us: cold,
        warm_us: warm,
    }
}

fn contention_route_report(samples: Vec<u128>) -> ContentionRouteReport {
    ContentionRouteReport {
        p50_us: percentile(&samples, 50),
        p95_us: percentile(&samples, 95),
        max_us: samples.iter().copied().max().unwrap_or_default(),
        samples: samples.len(),
    }
}

fn resource_metric_summary(samples: &[u64]) -> ResourceMetricSummary {
    ResourceMetricSummary {
        p50: percentile_u64(samples, 50),
        p95: percentile_u64(samples, 95),
        max: samples.iter().copied().max().unwrap_or_default(),
    }
}

fn percentile_u64(samples: &[u64], percent: usize) -> u64 {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let rank = (sorted.len() * percent).div_ceil(100).saturating_sub(1);
    sorted[rank]
}

fn child_resource_summary(samples: &[ChildResourceSample]) -> ChildResourceSummary {
    ChildResourceSummary {
        samples: samples.len(),
        // GNU time formats %U/%S to centiseconds; zero means below this
        // resolution, not that the Git process consumed no CPU.
        cpu_time_resolution_us: 10_000,
        user_cpu_us: resource_metric_summary(
            &samples
                .iter()
                .map(|sample| sample.user_cpu_us)
                .collect::<Vec<_>>(),
        ),
        system_cpu_us: resource_metric_summary(
            &samples
                .iter()
                .map(|sample| sample.system_cpu_us)
                .collect::<Vec<_>>(),
        ),
        max_rss_kib: resource_metric_summary(
            &samples
                .iter()
                .map(|sample| sample.max_rss_kib)
                .collect::<Vec<_>>(),
        ),
    }
}

fn elapsed_us(started: Instant) -> u128 {
    started.elapsed().as_micros()
}

fn output_bytes(output: &BoundedOutput) -> Vec<u8> {
    assert_eq!(
        output.omitted_bytes, 0,
        "Git output exceeded its capture limit"
    );
    output
        .head
        .iter()
        .copied()
        .chain(output.tail.iter().copied())
        .collect()
}

async fn benchmark_fixture(
    git: &Path,
    packed: bool,
    operation_label: &str,
    op: &codegg_git::GitOperation,
) -> FixtureReport {
    let fixture = tempfile::tempdir().expect("benchmark fixture directory");
    let repository = fixture.path().join("repository");
    let (object_store_bytes, loose_objects, pack_files) = create_fixture(git, &repository, packed);
    let argv = codegg_git::render_argv(op);
    let typed_service = GitExecutionService::new();

    let mut native_cold = Vec::with_capacity(COLD_SAMPLES);
    let mut snapshot_cold = Vec::with_capacity(COLD_SAMPLES);
    let mut live_cold = Vec::with_capacity(COLD_SAMPLES);
    let mut live_cold_resources = Vec::with_capacity(COLD_SAMPLES);
    for index in 0..COLD_SAMPLES {
        let sample = tempfile::tempdir().expect("cold sample directory");
        let cold_repository = sample.path().join("repository");
        copy_gitdir(&repository, &cold_repository);
        let cold_repository = fs::canonicalize(cold_repository).expect("canonical cold repository");
        let cold_objects = cold_repository.join(".git/objects");

        advise_drop_object_cache(&cold_objects);
        let started = Instant::now();
        let native = native_git(git, &cold_repository, &argv);
        native_cold.push(elapsed_us(started));

        advise_drop_object_cache(&cold_objects);
        let started = Instant::now();
        let snapshot = immutable_snapshot(&typed_service, op, &cold_repository).await;
        snapshot_cold.push(elapsed_us(started));
        assert_eq!(snapshot.stdout.as_bytes(), native.stdout.as_slice());

        advise_drop_object_cache(&cold_objects);
        let started = Instant::now();
        let live = contained_live_git(
            &typed_service,
            op,
            git,
            &cold_repository,
            &format!("cold-{index}"),
        )
        .await;
        live_cold.push(elapsed_us(started));
        live_cold_resources.push(child_resource_sample(&live.stderr));
        assert_eq!(output_bytes(&live.stdout), native.stdout);
    }

    // Prime process and cache state outside the measured warm set.
    let expected = native_git(git, &repository, &argv).stdout;
    assert_eq!(
        immutable_snapshot(&typed_service, &op, &repository)
            .await
            .stdout
            .as_bytes(),
        expected.as_slice()
    );
    assert_eq!(
        output_bytes(
            &contained_live_git(&typed_service, &op, git, &repository, "warm-prime")
                .await
                .stdout,
        ),
        expected.as_slice()
    );

    let mut native_warm = Vec::with_capacity(WARM_SAMPLES);
    let mut snapshot_warm = Vec::with_capacity(WARM_SAMPLES);
    let mut live_warm = Vec::with_capacity(WARM_SAMPLES);
    let mut live_warm_resources = Vec::with_capacity(WARM_SAMPLES);
    for index in 0..WARM_SAMPLES {
        let started = Instant::now();
        let native = native_git(git, &repository, &argv);
        native_warm.push(elapsed_us(started));
        assert_eq!(native.stdout.as_slice(), expected.as_slice());

        let started = Instant::now();
        let snapshot = immutable_snapshot(&typed_service, &op, &repository).await;
        snapshot_warm.push(elapsed_us(started));
        assert_eq!(snapshot.stdout.as_bytes(), expected.as_slice());

        let started = Instant::now();
        let live = contained_live_git(
            &typed_service,
            &op,
            git,
            &repository,
            &format!("warm-{index}"),
        )
        .await;
        live_warm.push(elapsed_us(started));
        live_warm_resources.push(child_resource_sample(&live.stderr));
        assert_eq!(output_bytes(&live.stdout), expected);
    }

    // Measure warm-path contention with concurrent calls against the same
    // repository. Each percentile uses 30 batches at the selected fan-out;
    // per-call latency includes queueing, launcher setup and child execution.
    let typed_service = Arc::new(typed_service);
    let mut warm_contention = Vec::with_capacity(3);
    for concurrent_calls in [1, 4, 8] {
        let mut native = Vec::with_capacity(CONTENTION_BATCHES * concurrent_calls);
        let mut snapshot = Vec::with_capacity(CONTENTION_BATCHES * concurrent_calls);
        let mut live = Vec::with_capacity(CONTENTION_BATCHES * concurrent_calls);
        for _ in 0..CONTENTION_BATCHES {
            native.extend(
                concurrent_native_batch(git, &repository, &argv, &expected, concurrent_calls).await,
            );
            snapshot.extend(
                concurrent_snapshot_batch(
                    Arc::clone(&typed_service),
                    &op,
                    &repository,
                    &expected,
                    concurrent_calls,
                )
                .await,
            );
            live.extend(
                concurrent_live_batch(
                    Arc::clone(&typed_service),
                    op.clone(),
                    git,
                    &repository,
                    &expected,
                    concurrent_calls,
                )
                .await,
            );
        }
        warm_contention.push(ContentionReport {
            concurrent_calls,
            native_git: contention_route_report(native),
            immutable_snapshot: contention_route_report(snapshot),
            contained_live_odb: contention_route_report(live),
        });
    }

    let filesystem =
        std::env::var("CODEGG_GIT_COST_FILESYSTEM").unwrap_or_else(|_| "unspecified".into());
    FixtureReport {
        operation: operation_label.into(),
        shape: if packed { "packed" } else { "fragmented-loose" }.into(),
        filesystem,
        cold_condition: "unique repository path; POSIX_FADV_DONTNEED advised on each route, but eviction is not guaranteed".into(),
        object_store_bytes,
        snapshot_bytes_to_copy_cold_estimate: object_store_bytes,
        loose_objects,
        pack_files,
        native_git: route_report(native_cold, native_warm),
        immutable_snapshot: route_report(snapshot_cold, snapshot_warm),
        contained_live_odb: route_report(live_cold, live_warm),
        contained_live_child_resources_cold: child_resource_summary(&live_cold_resources),
        contained_live_child_resources_warm: child_resource_summary(&live_warm_resources),
        warm_contention,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn c002_packed_and_fragmented_cold_warm_git_cost_qualification() {
    if std::env::var(BENCH_OPT_IN).as_deref() != Ok("1") {
        eprintln!("skipped: set {BENCH_OPT_IN}=1 on a qualified Linux runner");
        return;
    }
    assert_eq!(
        std::env::var(M005_OPT_IN).as_deref(),
        Ok("1"),
        "C002 live reads must only be benchmarked under the M005 offline proof"
    );

    let git = git_binary();
    assert!(
        Path::new("/usr/bin/time").is_file(),
        "qualified runner must provide GNU time at /usr/bin/time"
    );
    let operations = [("log-1", operation()), ("status-short", status_operation())];
    let mut reports = Vec::with_capacity(operations.len() * 2);
    for (label, operation) in &operations {
        reports.push(benchmark_fixture(&git, true, label, operation).await);
        reports.push(benchmark_fixture(&git, false, label, operation).await);
    }
    let git_version = git_output(&git, Path::new("."), &["--version".into()]);
    let git_version = String::from_utf8_lossy(&git_version.stdout)
        .trim()
        .to_owned();
    let report = serde_json::json!({
        "git": git_version,
        "cold_samples_per_shape": COLD_SAMPLES,
        "warm_samples_per_shape": WARM_SAMPLES,
        "operations": operations.iter().map(|(label, _)| label).collect::<Vec<_>>(),
        "results": reports,
        "limits": [
            "wall time includes canonical service/launcher overhead",
            "CPU time and peak RSS are not measured comparatively for native Git or the typed-snapshot route",
            "GNU time reports child CPU and max RSS only for the contained-live route; native and typed-snapshot child resources are not instrumented",
            "GNU time reports user/system CPU to centisecond precision; CPU values below 10 ms appear as zero",
            "POSIX_FADV_DONTNEED is advisory and does not prove true cold cache",
            "warm contention measures 1, 4, and 8 simultaneous calls using 30 batches per level; it does not model separate agent identities",
            "setup-only and child-only time are not separated",
            "snapshot bytes copied are approximated by the fixture ODB size per cold sample"
        ]
    });
    let path = Path::new("linux-git-c002-performance.json");
    fs::write(
        path,
        serde_json::to_vec_pretty(&report).expect("serialize performance report"),
    )
    .expect("write performance report artifact");
    println!("C002_PERFORMANCE_REPORT={}", report);
}
