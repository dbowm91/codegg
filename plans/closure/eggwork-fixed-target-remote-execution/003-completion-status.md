# Eggwork Fixed-Target Remote Execution M003 — Completion Status

Status: closed

Source implementation plan:

- `plans/implementation/eggwork-fixed-target-remote-execution/003-content-aware-derived-workspace-transfer.md`

Source subsystem roadmap:

- `plans/subsystems/eggwork-fixed-target-remote-execution-roadmap.md#m003--workspace-transfer-optimization`

Historical blocker record (retained unchanged):

- `plans/closure/eggwork-fixed-target-remote-execution/003-status.md`

Repository baseline reviewed: `b88a73cad5380d10b11d409363893539a9809d80`

Implementation commits:

- `d64253e7` — derived workspace transfer, immutable Eggwork pin, cache, docs
- `7dfc9730` — canonical manifest digest comparison
- `80363a15` — live derived-transfer fixture with a stable base
- `8b1291a3` — deterministic transfer-cost measurements
- `2cc93483` — fallback, changed-blob, and ready-digest regression coverage
- `729e20fc` — derived-workspace Landlock escape coverage

## 1. Executive finding

M003 is implemented and closed. CodeGG still constructs and seals the exact
full current workspace snapshot locally, then uses the upstream
`workspace.derive.v1` contract only when authenticated capability and status
views both advertise it and the fully serialized patch request is smaller
than the full request. A bounded node-and-workspace-scoped cache holds only
acknowledged manifests. Derived ready responses are digest-checked before
execution. Only typed `base_manifest_missing` permits full fallback.

The earlier blocked record remains as a true record of the earlier review.
Eggwork Workspace/Artifact M004 subsequently closed upstream and unblocked
this implementation; that contract was rechecked before pinning.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Qualified upstream API and immutable pin | Eggwork Workspace/Artifact M004 closure at `e6a5d82e1ea392bb505aef1315c0e8c91d8baa0c`; root and standalone test-node manifests and lockfiles use this revision | pass | Contract includes `workspace.derive.v1`, principal-scoped retained manifests/blobs, canonical `apply_to`, and side-effect-free typed `base_manifest_missing`. |
| Full local snapshot remains provenance authority | Snapshot and source-subject sealing remain before transfer selection in `src/scheduler/eggwork.rs`; full manifest digest is retained in execution provenance | pass | No mtime shortcut or Git semantics. |
| Bounded, node-scoped acknowledged base cache | `ManifestCache`, hard cap 256, oldest-entry eviction, configuration replacement invalidation; unit tests cover node isolation/bounds/invalidation | pass | In-memory only; stores manifests/digests, no file bodies, paths, or credentials. |
| Canonical patch and recomposition | Upstream `WorkspaceManifestPatch`; `make_manifest_patch` recomposes and compares canonical full-manifest digests; unit coverage includes additions, changes, executable bits, removals, and deterministic order | pass | The full manifest remains the execution and provenance authority. |
| Deterministic full/derived selection | Serialized request comparison uses `derived_bytes < full_bytes`; cost fixture covers cold base, unchanged, small, and moderate edits | pass | No adaptive policy. |
| Changed-content-only probe/upload in derived mode | Scripted test records 33 blob probes for cold full mode and one for a one-file derived change; unchanged mode records zero | pass | The live server retains the acknowledged base blobs per the qualified upstream contract. |
| Typed miss fallback and fail-closed other errors | Scripted tests prove `base_manifest_missing` falls back after full probing, while an internal API error does not full-fallback or submit | pass | Fallback reuses the same deterministic workspace identity. |
| Ready digest verification in both modes | Scripted full and derived mismatch cases fail without command submission | pass | Checks the returned full manifest digest before execute. |
| Required isolation remains enforced for derived workspaces | Hosted Linux live mTLS test performs a full transfer, changes one file, then completes a derived execution with required Landlock; derived command's outside read/write attempts are denied | pass | No local execution fallback. |
| Restart, lease, and duplicate-submit invariants | Existing scheduler/restart/fencing suites run in hosted workspace suite; live M003 flow observes the terminal generation and submits exactly once per attempt | pass | Scheduler remains admission/retry authority. |
| Measured control bytes, blob operations, and elapsed phases | Deterministic fixture results in §15 | pass | Local controlled fixture; no universal wall-clock speedup is claimed. |

## 3. Production implementation evidence

- Root production and standalone live-test dependencies pin Eggwork client,
  core, server, and runner to `e6a5d82e1ea392bb505aef1315c0e8c91d8baa0c`.
- `EggworkNodeClient` and the production `NodeClientAdapter` expose derived
  workspace creation. Fresh authenticated capability/status agreement gates
  use of the exact `workspace.derive.v1` feature.
- Every attempt snapshots and hashes the full local workspace and seals the
  full manifest digest before selecting transfer mode.
- A 256-entry in-memory cache keys acknowledgements by node ID and CodeGG
  workspace ID, evicts oldest entries, and clears on node configuration
  replacement.
- Upstream patch application must recompose to the local full-manifest
  digest. Derived mode is selected only when its serialized request is
  strictly smaller than the full request.
- Full mode retains full-manifest blob probing. Derived mode probes only
  file digests from patch upserts. A typed base miss performs a full probe,
  upload, and create before any command is submitted.
- Both full and derived `WorkspaceReady` digests are compared with the local
  full-manifest digest before execution.
- Bounded transfer diagnostics report mode, base status, request bytes,
  snapshot and transfer elapsed time, blob probes/uploads, and uploaded
  bytes, without file paths or contents.

## 4. Verification executed

### Local

```bash
PKG_CONFIG_LIBDIR=/usr/local/lib/pkgconfig cargo test --locked -p codegg --lib scheduler::eggwork::tests:: -- --nocapture
cargo clippy --locked -p codegg --all-targets --features server,plugins,lsp-test-support -- -D warnings
scripts/verify.sh quick
cargo fmt --all -- --check
git diff --check
```

The focused scheduler suite passed 22 tests, including the four cost shapes,
typed miss fallback, non-miss failure, changed-only probing, and full/derived
ready-digest mismatches. Clippy, quick verification, formatting, and diff
checks passed. `PKG_CONFIG_LIBDIR` selects the host's x86_64 liblzma for the
local test link; it does not affect repository configuration.

A local feature-enabled `nextest` sweep also built successfully, but two tests
in the unrelated `projection_transport_real` binary returned HTTP 503 because
those hand-built server fixtures omitted `CODEGG_SERVER_AUTH_DISABLED=1`.
One affected test passed when rerun with that variable set. The sweep was not
counted as a clean full-feature result; the exact-head hosted workspace suite
below passed. No Eggwork code or M003 test was implicated.

### Hosted Linux

Hosted CI run `36664384276` on `729e20fc9d2f407aa1b802435b8361e4269f3d70`
passed. It included formatting, workspace Clippy, static guards, the
workspace test suite, and the Linux-only live Eggwork fixture. The live test
qualified production mTLS full-then-derived transfer under required
Landlock, including workspace access and filesystem escape denial.

The local macOS host does not execute the Linux-only live fixture; hosted
Linux is the source of that evidence.

## 5. Invariant review

1. The local current full manifest remains authoritative for source and
   execution input.
2. Every included file is freshly read and hashed; no metadata substitutes
   for content.
3. Provenance records the full current manifest digest, never the patch or
   base digest.
4. A remote ready response is accepted only when its full digest matches the
   local full manifest.
5. Retained bases are optimization hints, not source authority.
6. Only a typed base-missing result can change representation to full mode.
7. Cache keys include node identity and do not cross nodes.
8. Remote failure cannot switch to local execution or another node.
9. Scheduler permit, target, lease, retry, and restart semantics remain in
   CodeGG.
10. No Git semantics were introduced.

## 6. Failure and recovery review

The typed miss occurs before a ready workspace or command submission and is
safe to retry as a full materialization under the same attempt identity.
Other API errors fail closed. Ambiguous transport outcomes are not
reinterpreted as a base miss. Cancellation checks remain before fallback
upload/create and before execute. Cache loss on daemon restart simply makes
the next attempt cold; accepted execution recovery continues to reconcile
the persisted remote handle and does not reconstruct historical input from
the mutable worktree.

## 7. Migration and compatibility review

No schema migration, durable job/attempt change, or configuration change was
needed. Nodes without the advertised derived feature and cold caches use the
existing full mode. The dependency update is immutable and lockfile-pinned.

## 8. Security review

Derived execution is gated by authenticated capability and status
observations. The base namespace remains principal-scoped by Eggwork.
Landlock stays required for the live derived execution; outside workspace
read/write attempts are denied. No credential material, file body, or path
is added to cache or transfer diagnostics. Static target-routing,
scheduler-bypass, execution-ownership, and core-boundary guards passed.

## 9. Documentation and operations

`architecture/jobs.md`, `architecture/scheduler.md`, and
`docs/execution-ownership.md` document the transfer behavior and ownership
boundary. The operator diagnostics identify full, derived, and typed-miss
fallback modes and expose bounded transfer measurements. Blob deduplication
predated M003; this milestone adds retained-manifest reuse.

## 10. Unresolved findings

| Severity | Finding | Impact | Disposition |
|---|---|---|---|
| low | Wall-clock values in the deterministic local fixture vary by host; the recorded elapsed phases are local snapshot and transfer planning, while production separately records actual transfer elapsed time | Local timings are directional and are not a remote-node latency guarantee | Accepted; request bytes and blob operation counts are deterministic and satisfy optimization acceptance without claiming universal wall-clock improvement. |

No unresolved high- or medium-severity finding remains.

## 11. Roadmap disposition

- M003 is closed.
- M004 (whole remote AgentRun worker) remains deferred. M003 was its
  workspace-transfer prerequisite, but the stable AgentRun worker-entry
  contract remains a separate unresolved interface dependency; no registered
  M004 handoff is ready.
- Registry audit found no other registered plan whose hard or interface
  dependency is newly satisfied by M003. No plan was moved to `ready`.

## 12. Registry updates

- The earlier blocked `003-status.md` remains unchanged as historical
  evidence. This completion record is the current M003 closure gate.
- The implementation plan and M003 roadmap row move to `closed`.
- The Eggwork subsystem remains active because M004 is deferred; the registry
  records the remaining worker-entry contract dependency.
- The M003 closure-review row moves to recently closed work. M004 remains
  deferred and is not registered as dependency-ready.

## 13. Upstream and dependency evidence

Eggwork Workspace/Artifact M004 closed at
`e6a5d82e1ea392bb505aef1315c0e8c91d8baa0c`; its closure is
`plans/closure/workspace-artifact-transport/004-status.md` in Eggwork. CodeGG
rechecked these exact behaviors at that revision before adoption:

- explicit `workspace.derive.v1` advertisement;
- principal-scoped retained manifests and referenced blobs;
- canonical `WorkspaceManifestPatch::apply_to` final digest semantics;
- typed `base_manifest_missing` with no ready-workspace side effect.

The same full revision is used by production and the live test-node
dependencies.

## 14. Transfer selection and cache bounds

The cache cap is 256 acknowledged manifests globally, keyed by `(node_id,
CodeGG workspace_id)`, with oldest-entry eviction and configuration-reload
invalidation. It is transient and begins empty after restart.

The selection rule is deterministic: select derived mode iff the serialized
derived workspace request is strictly smaller than the serialized full
workspace request. Otherwise use full mode. A typed base miss is the only
derived-to-full fallback.

## 15. Deterministic performance measurements

Command:

```bash
PKG_CONFIG_LIBDIR=/usr/local/lib/pkgconfig cargo test --locked -p codegg --lib scheduler::eggwork::tests::derived_transfer_cost_fixture_covers_cold_unchanged_small_and_moderate_edits -- --nocapture
```

One measured run on the local x86_64 macOS host (fixture: 32 stable 256-ish
byte files plus `build.sh`) produced:

| Case | Mode | Full request bytes | Selected control bytes | Reduction | Blob probes | Uploads | Uploaded bytes | Snapshot µs | Transfer planning µs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| No reusable base | full | 5,171 | 5,171 | 0% | 33 | 33 | 8,280 | 2,560 | 1,153 |
| Unchanged | derived | 5,171 | 282 | 94.55% | 0 | 0 | 0 | 1,375 | 3,529 |
| Small edit (one file) | derived | 5,171 | 426 | 91.76% | 1 | 1 | 26 | 1,338 | 3,537 |
| Moderate edit (eight files) | derived | 5,171 | 1,489 | 71.20% | 8 | 8 | 2,056 | 1,909 | 3,467 |

Blob counts/bytes use the deterministic fixture's all-missing cold assumption;
the unchanged and edit rows count only digests introduced by patch upserts.
The recorded elapsed values split full local snapshot construction from
transfer planning; no local snapshot/hash cost reduction is claimed. The
executor separately records elapsed remote transfer in its bounded
diagnostics. Every reusable-base scenario materially reduced workspace
control bytes and uploaded no more bytes than the cold full case.

## 16. M004 whole-AgentRun disposition

M004 is not unblocked to `ready`. Its M003 dependency is satisfied, but the
stable AgentRun worker-entry contract called out by the subsystem roadmap is
not recorded as closed in the planning registry. M004 remains deferred until
that interface is independently established and reviewed.
