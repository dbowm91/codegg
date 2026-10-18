#![cfg(target_os = "linux")]

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use serde::Serialize;

use codegg::git_service::{GitExecutionService, GitPayload};
use codegg::managed_process::{
    BoundedOutput, ManagedProcessRequest, ManagedProcessResult, ManagedProcessService,
    ProcessProvenance, SandboxExecutionOutcome, SandboxRequest,
};
use codegg::security::sandbox::{SandboxConfig, SandboxMode};

const BENCH_OPT_IN: &str = "CODEGG_RUN_LINUX_GIT_COST_PERF";
const M005_OPT_IN: &str = "CODEGG_RUN_M005_OFFLINE_NATIVE";
const OBJECT_COUNT: usize = 4096;
const BYTES_PER_OBJECT: usize = 25_600;
const COLD_SAMPLES: usize = 30;
const WARM_SAMPLES: usize = 100;

#[derive(Debug, Serialize)]
struct RouteReport {
    cold_us: Vec<u128>,
    cold_p50_us: u128,
    cold_p95_us: u128,
    warm_us: Vec<u128>,
    warm_p50_us: u128,
    warm_p95_us: u128,
}

#[derive(Debug, Serialize)]
struct FixtureReport {
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
) -> String {
    let result = service
        .execute(operation, repository)
        .await
        .expect("typed snapshot Git read");
    assert!(result.success, "typed snapshot Git read returned failure");
    assert!(matches!(result.payload, Some(GitPayload::Log(_))));
    result.stdout
}

async fn contained_live_git(
    git: &Path,
    repository: &Path,
    argv: &[String],
    label: &str,
) -> ManagedProcessResult {
    let args = argv.iter().skip(1).cloned().collect::<Vec<_>>();
    let spec = SandboxConfig::new()
        .with_enabled(true)
        .with_mode(SandboxMode::WorkspaceWrite)
        .with_linux_offline_network(true)
        .launch_spec(git, &args, Some(repository))
        .expect("construct required contained-Git launch spec");
    let command = std::iter::once(spec.target.as_os_str().to_owned())
        .chain(spec.args.iter().map(OsString::from))
        .collect();
    let mut request = ManagedProcessRequest::new(
        command,
        repository.to_path_buf(),
        ProcessProvenance::new("linux-git-c002-performance", label),
    );
    request.timeout = Some(Duration::from_secs(20));
    request.environment_policy = codegg::managed_process::EnvironmentPolicy::sanitized()
        .with_var("GIT_CONFIG_NOSYSTEM", "1")
        .with_var("GIT_CONFIG_GLOBAL", "/dev/null")
        .with_var("GIT_TERMINAL_PROMPT", "0")
        .with_var("HOME", "/tmp/codegg-empty-home");
    request.sandbox = SandboxRequest::Required(spec);
    let result = ManagedProcessService::run(request)
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

async fn benchmark_fixture(git: &Path, packed: bool) -> FixtureReport {
    let fixture = tempfile::tempdir().expect("benchmark fixture directory");
    let repository = fixture.path().join("repository");
    let (object_store_bytes, loose_objects, pack_files) = create_fixture(git, &repository, packed);
    let op = operation();
    let argv = codegg_git::render_argv(&op);
    let typed_service = GitExecutionService::new();

    let mut native_cold = Vec::with_capacity(COLD_SAMPLES);
    let mut snapshot_cold = Vec::with_capacity(COLD_SAMPLES);
    let mut live_cold = Vec::with_capacity(COLD_SAMPLES);
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
        let snapshot = immutable_snapshot(&typed_service, &op, &cold_repository).await;
        snapshot_cold.push(elapsed_us(started));
        assert_eq!(snapshot.as_bytes(), native.stdout.as_slice());

        advise_drop_object_cache(&cold_objects);
        let started = Instant::now();
        let live = contained_live_git(git, &cold_repository, &argv, &format!("cold-{index}")).await;
        live_cold.push(elapsed_us(started));
        assert_eq!(output_bytes(&live.stdout), native.stdout);
    }

    // Prime process and cache state outside the measured warm set.
    let expected = native_git(git, &repository, &argv).stdout;
    assert_eq!(
        immutable_snapshot(&typed_service, &op, &repository)
            .await
            .as_bytes(),
        expected.as_slice()
    );
    assert_eq!(
        output_bytes(
            &contained_live_git(git, &repository, &argv, "warm-prime")
                .await
                .stdout,
        ),
        expected.as_slice()
    );

    let mut native_warm = Vec::with_capacity(WARM_SAMPLES);
    let mut snapshot_warm = Vec::with_capacity(WARM_SAMPLES);
    let mut live_warm = Vec::with_capacity(WARM_SAMPLES);
    for index in 0..WARM_SAMPLES {
        let started = Instant::now();
        let native = native_git(git, &repository, &argv);
        native_warm.push(elapsed_us(started));
        assert_eq!(native.stdout.as_slice(), expected.as_slice());

        let started = Instant::now();
        let snapshot = immutable_snapshot(&typed_service, &op, &repository).await;
        snapshot_warm.push(elapsed_us(started));
        assert_eq!(snapshot.as_bytes(), expected.as_slice());

        let started = Instant::now();
        let live = contained_live_git(git, &repository, &argv, &format!("warm-{index}")).await;
        live_warm.push(elapsed_us(started));
        assert_eq!(output_bytes(&live.stdout), expected);
    }

    let filesystem =
        std::env::var("CODEGG_GIT_COST_FILESYSTEM").unwrap_or_else(|_| "unspecified".into());
    FixtureReport {
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
    let reports = vec![
        benchmark_fixture(&git, true).await,
        benchmark_fixture(&git, false).await,
    ];
    let git_version = git_output(&git, Path::new("."), &["--version".into()]);
    let git_version = String::from_utf8_lossy(&git_version.stdout)
        .trim()
        .to_owned();
    let report = serde_json::json!({
        "git": git_version,
        "cold_samples_per_shape": COLD_SAMPLES,
        "warm_samples_per_shape": WARM_SAMPLES,
        "results": reports,
        "limits": [
            "wall time includes canonical service/launcher overhead",
            "CPU time and peak RSS are not measured by this pilot",
            "POSIX_FADV_DONTNEED is advisory and does not prove true cold cache",
            "concurrent-agent contention is not measured by this pilot",
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
