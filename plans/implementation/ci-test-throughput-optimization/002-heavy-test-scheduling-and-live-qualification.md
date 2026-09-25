# CI/Test Throughput M002 — Heavy-Test Scheduling and Live Qualification

Status: implemented (closed; see
`plans/closure/ci-test-throughput-optimization/002-status.md`)

Repository planning baseline: `0c896db32d2325da39b129acde7dd406ce472dc4`

Source roadmap:

- `plans/subsystems/ci-test-throughput-optimization-roadmap.md`

Primary class: polish / development infrastructure.

Hard dependency:

- M001 closed with a stable measured hosted-CI baseline and retained compiler/profile settings.

## 1. Objective

Remove avoidable heavyweight setup and overly coarse serialization from the ordinary test critical path while preserving authoritative Linux qualification for real Eggwork/Landlock behavior.

This milestone owns two related costs:

1. the Linux live Eggwork integration target, including its workspace-excluded helper builds;
2. whole-binary Nextest exclusivity currently applied to five heavy binaries containing 53 tests in aggregate.

## 2. Current evidence

`tests/eggwork_remote_execution_live.rs`:

- is Linux-only;
- historical hosted evidence records ~144.28 seconds for the live target;
- builds `crates/eggwork-test-node` on demand when `CODEGG_EGGWORK_TEST_NODE` is absent;
- locates that helper under `crates/eggwork-test-node/target/debug/`;
- also resolves/builds the pinned Eggwork sandbox helper;
- uses a process-local `OnceCell`, which does not provide suite-wide initialization across separate Nextest test processes.

The Nextest `ci` profile currently assigns `threads-required = "num-cpus"` to entire binaries:

- `eggwork_remote_execution_live` — 7 tests;
- `interactive_process_attach_resume` — 19 tests;
- `interactive_process_sessions` — 11 tests;
- `interactive_terminal_tui` — 6 tests;
- `scheduler_cancellation` — 10 tests.

The existing scheduler-cancellation flake proves that at least some tests require strict isolation; it does not prove that every test colocated in those binaries does.

## 3. Invariants and non-goals

Preserve:

- real Linux Eggwork/Landlock qualification;
- current lease, restart, cancellation, materialization, and sandbox assertions;
- current PTY/process cleanup semantics;
- no public-network dependency;
- a single routine CI job;
- deterministic bounded test resources.

Do not:

- mark live tests ignored without an explicit authoritative replacement execution path;
- move live qualification to an unowned/manual-only state;
- remove assertions;
- increase global Nextest slots above the M001-stable value;
- weaken timeouts merely to make parallel execution look faster;
- create a second permanent workflow or CI matrix.

## 4. Work packages

### WP1 — Make Eggwork fixture construction explicit

Create or reuse one small deterministic build entry point that can prebuild:

- `codegg-eggwork-test-node`;
- the pinned `eggwork-sandbox-helper`.

CI should build these once before live execution and export the explicit fixture path(s) consumed by the tests. Test-side fallback may remain for local direct invocation, but CI must not depend on each test process discovering/building the fixture independently.

The helper build must stay `--locked` and pinned to the same immutable Eggwork revision already required by the live test.

### WP2 — Cache the excluded helper target correctly

Configure the existing Rust/Cargo cache so the workspace-excluded helper target is represented explicitly, or use a shared target directory if that is safe with the helper's separate dependency graph.

Do not merge incompatible SQLite link graphs into one Cargo workspace resolve.

Record cold and warm helper-build behavior. A cache change that produces no useful hit or increases complexity may be rejected.

### WP3 — Audit machine-wide exclusivity at test granularity

For each of the five heavy binaries, classify tests as:

- must run alone on the machine;
- needs a weighted resource cost smaller than all CPUs;
- safe under ordinary `ci` concurrency.

Use current test semantics: fixed/global process state, PTYs, real subprocess trees, timing assertions, ports, and external helper lifecycles.

Prefer Nextest per-test filters or named test groups over whole-binary `num-cpus` reservations where sound.

The known `scheduler_cancellation` timing-sensitive cases must remain protected. Do not generalize a successful local parallel run into hosted safety without hosted evidence.

### WP4 — Define ordinary-PR versus authoritative live qualification

The preferred policy is:

- ordinary PR workspace tests exclude the real live-Eggwork target unless changed paths can affect Eggwork remote execution, scheduler integration, sandbox/helper behavior, or its fixture;
- relevant PRs run the live target explicitly in the same `verify` job;
- pushes to `main` run the live target explicitly on Linux regardless of changed path, preserving an authoritative integration signal.

Implement change detection with repository-owned deterministic shell/Git logic or an already-present mechanism. Do not add a broad third-party path-filter dependency solely for this.

If reliable changed-file determination cannot be achieved without making the workflow fragile, keep live qualification unconditional and close WP4 negatively; WP1–WP3 may still land.

The explicit live step must make its coverage visible in logs. It must never silently disappear because a selector matched zero tests.

### WP5 — Re-measure execution tail

Compare:

- current binary-wide exclusivity;
- narrowed resource groups;
- live target with explicit prebuilt helper;
- ordinary PR path with live target omitted when legitimately unrelated.

Record test counts and durations. Any flake increase invalidates the candidate.

## 5. Required verification

Focused:

```bash
cargo nextest run --workspace --locked --profile ci
cargo test --test scheduler_cancellation --locked -- --test-threads=1
cargo test --test interactive_process_attach_resume --locked -- --test-threads=1
cargo test --test interactive_process_sessions --locked -- --test-threads=1
cargo test --test interactive_terminal_tui --locked -- --test-threads=1
```

On supported Linux with prebuilt fixture:

```bash
cargo test --test eggwork_remote_execution_live --locked -- --test-threads=1
```

Run the final resource policy through hosted CI at least twice when practical, because contention flakes are the defect class being modified.

## 6. Acceptance criteria

- CI no longer relies on repeated in-test nested Cargo setup for the Eggwork live fixture.
- The excluded helper target has an explicit cache/build policy.
- Each machine-wide Nextest reservation is justified at the smallest practical test/group boundary.
- Previously protected timing-sensitive tests remain deterministic.
- Ordinary PR live-Eggwork omission, if adopted, is path-sensitive and paired with explicit relevant-PR + unconditional-main Linux qualification.
- No test assertion or supported-Linux qualification is lost.
- Comparable hosted test-execution wall time improves, or non-beneficial subchanges are reverted.

## 7. Stop conditions

Stop and report if:

- fixture prebuild requires changing the pinned Eggwork production dependency;
- the separate helper workspace cannot share/cache artifacts without SQLite linkage conflicts;
- changed-file selection is ambiguous enough to risk missing relevant Eggwork changes;
- narrowing exclusivity causes a hosted timing/process flake;
- a test uses hidden global state that cannot be expressed safely with current Nextest grouping.

## 8. Closure evidence

Create `plans/closure/ci-test-throughput-optimization/002-status.md` with:

- helper build/cache design;
- cold/warm timing;
- per-test resource-class audit;
- exact Nextest filter/group changes;
- ordinary-PR/main live qualification policy;
- hosted repeated-run evidence;
- before/after test-step timing;
- residual heavy tests;
- M003 readiness disposition.
