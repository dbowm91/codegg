# Phase 07: Persistent Command and Script Run Index

## Objective

Create a durable run-record and artifact-index layer for commands, tests, native tool routes, and Python scripts. The current subsystem can classify, plan, execute, project, and produce bounded metadata, but raw outputs and Python pseudo-local labels are not yet unified into a persistent, inspectable run model.

This phase should establish `.codegg/runs/` as the canonical durable record for executable activity and make run artifacts retrievable by stable identifiers. It should not yet add the full TUI experience; Phase 08 will consume this substrate.

## Scope

This phase covers:

- stable run identifiers;
- run manifest schema;
- raw stdout/stderr/diff/script/test artifact storage;
- atomic persistence;
- run indexing and lookup;
- retention and cleanup policy;
- redaction and integrity metadata;
- replay/rerun descriptors;
- adapters for shell, test runner, native routes, and Python.

## Existing substrate to reuse

Reuse:

- `.codegg/test-runs/` indexing and log conventions;
- shell bounded output store and command run bridge;
- `ProjectionResult` and raw artifact handle concepts;
- Python run result/script hash/change diff data;
- session and worktree persistence in `codegg-core`;
- existing snapshot IDs and git/worktree state primitives;
- current redaction/security hooks.

## Design principles

1. Raw artifacts are durable; projections are derived views.
2. Manifests are small, versioned, and append-safe.
3. A run record must state exactly what executed and under which policy.
4. Secrets should not be duplicated unnecessarily into metadata.
5. Run persistence failure must not corrupt execution results.
6. Rerun descriptors must not silently bypass current permissions.

## Workstream A: Define the run domain model

Add a shared domain type, preferably in `codegg-core` if it is frontend-independent:

```rust
pub struct RunId(String);

pub enum RunKind {
    RawShell,
    ManagedProcess,
    Test,
    GitRead,
    GitMutation,
    Search,
    Python,
    NativeTool,
}

pub struct RunManifest {
    pub schema_version: u32,
    pub run_id: RunId,
    pub session_id: Option<String>,
    pub parent_run_id: Option<RunId>,
    pub kind: RunKind,
    pub command_or_intent: RunInvocation,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub status: RunStatus,
    pub workspace_root: PathBuf,
    pub cwd: PathBuf,
    pub backend: ExecutionBackendRecord,
    pub risk: RiskRecord,
    pub permissions: Vec<PermissionDecisionRecord>,
    pub sandbox: Option<SandboxRecord>,
    pub artifacts: Vec<ArtifactRecord>,
    pub projection: Option<ProjectionRecord>,
    pub changes: Vec<ChangedPathRecord>,
    pub rerun: Option<RerunDescriptor>,
}
```

Use a ULID/UUIDv7-like sortable run ID or a repo-standard identifier type.

## Workstream B: Define artifact records

Support artifact kinds:

- stdout;
- stderr;
- combined log;
- command/script source;
- test report;
- raw test log;
- unified diff;
- changed-file manifest;
- projection text;
- RTK-compressed projection;
- structured JSON result;
- policy/sandbox evidence.

Each artifact record should include:

- artifact ID/handle;
- relative path under the run directory;
- MIME/logical type;
- byte length;
- SHA-256 digest;
- truncation state;
- redaction state;
- created timestamp;
- whether it is safe for direct model promotion.

## Workstream C: Directory layout and atomic writes

Suggested layout:

```text
.codegg/runs/
  index.jsonl or index.sqlite
  2026-07-10/
    <run-id>/
      manifest.json
      stdout.log
      stderr.log
      invocation.json
      projection.txt
      diff.patch
      changes.json
      policy.json
```

Requirements:

- write artifacts to temporary names and atomically rename;
- write manifest last or use status transitions (`running` -> `complete`);
- recover incomplete runs after process crash;
- never use user-controlled filenames directly;
- enforce maximum artifact sizes and bounded disk use;
- store paths relative to the run root.

Choose JSONL plus filesystem for MVP unless SQLite already offers a clearly simpler shared index. Avoid building two competing indexes.

## Workstream D: Add a run store API

Define an async-safe API:

```rust
pub trait RunStore {
    async fn begin_run(&self, draft: RunDraft) -> Result<RunHandle>;
    async fn write_artifact(&self, run: &RunHandle, artifact: ArtifactInput) -> Result<ArtifactRef>;
    async fn complete_run(&self, run: RunHandle, completion: RunCompletion) -> Result<RunManifest>;
    async fn get_run(&self, id: &RunId) -> Result<Option<RunManifest>>;
    async fn read_artifact(&self, id: &ArtifactId, range: Option<ByteRange>) -> Result<ArtifactChunk>;
    async fn list_runs(&self, query: RunQuery) -> Result<Vec<RunSummary>>;
}
```

Provide a filesystem implementation and an in-memory implementation for tests.

## Workstream E: Integrate execution families

### Python

Replace pseudo-local labels with real artifact references when a run store is available:

- script source/hash;
- stdout;
- stderr;
- diff;
- changed files;
- policy/sandbox evidence.

Maintain compatibility for direct unit execution without a store, but make the production path persistent.

### Test runner

Adapt existing `.codegg/test-runs/` artifacts into the shared run store or add a compatibility bridge. Avoid duplicating full logs in two stores long-term.

### Bash/managed command

Persist invocation, output, exit status, timeout state, classifier/planner metadata, and projection.

### Native git/search tools

Persist normalized invocation and structured result alongside model-facing projection.

## Workstream F: Rerun descriptors

Store a rerun descriptor that captures:

- normalized argv or script source reference;
- backend family;
- cwd/workspace root;
- requested mode;
- relevant config/profile name;
- parent run relationship.

Do not store permission grants as automatically reusable authorization. A rerun must pass current classification, policy, and permissions again.

## Workstream G: Retention and cleanup

Add configurable retention:

- maximum total bytes;
- maximum run count;
- maximum age;
- preserve failed runs longer than successful runs if desired;
- pin/protect runs referenced by sessions or user action;
- cleanup only completed/unpinned runs;
- never follow symlinks outside `.codegg/runs/`.

Provide a deterministic cleanup planner and dry-run tests.

## Workstream H: Security and redaction

- apply existing secret redaction before model-facing projection;
- decide whether raw artifacts retain unredacted output; default should be local-only with explicit security metadata;
- restrict artifact handles to workspace/session authorization boundaries;
- validate all paths and artifact IDs;
- hash artifacts for integrity and corruption detection;
- avoid embedding raw environment variables in manifests.

## Workstream I: Tests

Add tests for:

- run ID generation and ordering;
- manifest serde/versioning;
- atomic begin/write/complete flow;
- crash/incomplete-run recovery;
- artifact digest verification;
- ranged artifact reads;
- path traversal rejection;
- retention planning;
- pinned run preservation;
- Python real artifact handles;
- test-runner adapter;
- rerun descriptor does not persist permission approval;
- concurrent independent run writes.

## Validation commands

```bash
cargo test -p codegg-core run_store
cargo test -p codegg --lib python_script
cargo test -p codegg --lib test_runner
cargo test -p codegg --lib shell
cargo test -p codegg --lib command_intent
```

Full capped suite:

```bash
CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=1
```

## Acceptance criteria

- every production command/script execution can receive a stable RunId;
- raw artifacts are retrievable by real handles;
- manifests identify backend, policy, permissions, workspace, status, projection, and changes;
- writes are atomic and crash-tolerant;
- retention is bounded and tested;
- reruns re-evaluate policy and permissions;
- Python pseudo-labels are replaced by real references in the run-store path;
- the store is frontend-independent and ready for TUI/protocol consumption.
