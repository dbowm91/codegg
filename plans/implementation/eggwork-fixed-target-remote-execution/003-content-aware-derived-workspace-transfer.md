# Eggwork Remote Execution M003 — Content-Aware Derived Workspace Transfer

Status: blocked on Eggwork Workspace/Artifact M004 closure

Source roadmap:

- `plans/subsystems/eggwork-fixed-target-remote-execution-roadmap.md`

Closed predecessors:

- M001 fixed-target remote executor + corrective C001;
- M002 target capability/operator policy;
- M002a required-isolation live qualification.

Current CodeGG baseline:

- `841ad117399c9279e3668828d5d9866983603381`

Required upstream contract:

- Eggwork Workspace/Artifact M004 plan:
  `eggstack/eggwork@ed0fc838bd006103021f1fb4749f579cabc2db87`
- `plans/implementation/workspace-artifact-transport/004-reusable-manifest-cas-and-derived-materialization.md` in Eggwork.

M003 becomes ready only after the upstream closure proves `workspace.derive.v1`, principal-scoped base-manifest retention, canonical patch equivalence, and typed `base_manifest_missing` behavior.

Primary class: infrastructure/capability

## 1. Objective

Reduce repeated CodeGG -> Eggwork workspace control/data transfer by reusing an acknowledged prior canonical manifest as a content base and sending only a deterministic manifest patch plus newly required blobs.

This milestone deliberately remains content-aware rather than Git-aware.

CodeGG continues to construct the exact full current `WorkspaceManifest` locally as the correctness/source-provenance authority. The optimization changes transfer planning, not what bytes constitute the remote execution input.

## 2. Current baseline

The current Eggwork executor:

1. walks the whole scheduler-owned workspace;
2. reads every included regular file into an immutable `WorkspaceSnapshot`;
3. builds and validates the full canonical `WorkspaceManifest`;
4. seals execution-source provenance with that manifest digest;
5. calls `find_missing_blobs` for all final file digests;
6. uploads missing blobs;
7. sends the entire manifest through `create_workspace`.

Blob bytes are already content-deduplicated by Eggwork. The remaining repeated-transfer costs are the full digest probe/full manifest request and inability to name a retained server-side manifest as a base.

M003 optimizes those surfaces without weakening steps 1-4.

## 3. Durable invariants

1. The locally constructed full current manifest remains authoritative for execution input and provenance.
2. M003 MUST NOT use metadata/mtime heuristics as a substitute for hashing/reading the exact current snapshot.
3. `ExecutionSubjectProvenance.materialization.manifest_digest` remains the digest of the final full manifest, not the patch or base.
4. A derived server result is accepted only if its returned manifest digest exactly matches CodeGG's local full-manifest digest.
5. Remote base reuse is an optimization hint, never source authority.
6. Missing/expired base may fall back to full materialization; auth, digest, storage, protocol, or internal errors may not.
7. A cache entry for node A is never used for node B.
8. No remote failure causes local execution fallback or alternate-node selection.
9. Scheduler permits, lease identity, isolation policy, and restart semantics remain unchanged.
10. No Git semantics enter Eggwork through this integration.

## 4. Upstream API adoption

After Eggwork M004 closes, pin the exact reviewed immutable Eggwork revision consistently in:

- root production dependencies;
- `crates/eggwork-test-node`;
- any separate lockfiles/fixtures.

Extend `EggworkNodeClient` and the production `NodeClientAdapter` with the upstream derived-workspace method.

Consume only the qualified feature:

```text
workspace.derive.v1
```

Do not infer support from Eggwork version strings.

## 5. Local acknowledged-manifest cache

Add a bounded in-memory cache owned by the Eggwork executor/adapter.

Recommended key:

```text
(node_id, CodeGG workspace_id)
```

Each value contains only:

- the last acknowledged full `WorkspaceManifest`;
- canonical manifest digest;
- observation/acknowledgement time.

Requirements:

- hard cap <= number of configured nodes × a bounded per-node workspace count, with an overall explicit maximum;
- LRU/oldest eviction;
- no file bodies;
- no TLS paths/secrets;
- config replacement/removal invalidates affected node entries;
- daemon restart starts cold and simply uses full materialization until a new base is acknowledged.

Do not persist this optimization cache in `JobRecord` or `JobAttempt`.

## 6. Patch derivation

Given:

- cached acknowledged base manifest B;
- freshly constructed authoritative current manifest C;

compute an Eggwork `WorkspaceManifestPatch` whose application to B produces C exactly.

Required semantics:

- compare by normalized relative path;
- unchanged entries omitted;
- removed entries -> `Remove`;
- added/changed directories/files -> corresponding upsert;
- executable-bit or digest/size change counts as a file upsert;
- deterministic path ordering;
- no symlink/special-file synthesis;
- local recomposition test MUST prove `apply(B, patch) == C` before the patch is sent.

Use the upstream core patch type/helper if one is exposed. Do not implement a semantically divergent patch language.

## 7. Transfer-mode selection

For every fresh attempt:

1. perform existing fresh node policy/capability preflight;
2. build the exact full current `WorkspaceSnapshot`;
3. seal execution-source provenance with the full manifest digest;
4. examine `workspace.derive.v1` and the acknowledged local base cache;
5. if no usable base -> full mode;
6. compute the patch and compare its encoded size/cost with the full manifest;
7. use derived mode only when it is strictly smaller under a deterministic rule;
8. otherwise use full mode.

The exact threshold should be simple and testable, e.g. derived serialized request bytes < full serialized manifest request bytes. Do not add adaptive/model-based policy.

An empty patch for an unchanged workspace is valid and should be the cheapest derived case.

## 8. Blob upload behavior

### Full mode

Retain current behavior:

- probe all final file digests;
- upload missing bytes;
- create full workspace.

### Derived mode

The qualified upstream base contract pins blobs referenced by the retained base manifest.

Therefore:

- probe/upload only file digests introduced or changed by patch upserts;
- do not resend unchanged base digests merely as a confidence check;
- if Eggwork reports the base missing, switch to full mode and run the full blob probe/upload before full workspace create.

If upstream closure does not guarantee base blob retention, stop and revise this work package rather than relying on optimistic presence.

## 9. Safe fallback

Fallback to the existing full path is allowed only for the typed upstream `base_manifest_missing` condition.

Required ordering:

- derived miss must create no ready workspace;
- after the miss, full blob probe/upload + existing full create is safe;
- same deterministic attempt workspace id/owner remains in use;
- no execution has been submitted yet.

Do NOT fallback on:

- forbidden/unauthenticated;
- manifest/digest mismatch;
- quota/storage corruption;
- workspace identity conflict;
- protocol/version mismatch;
- timeout/unknown transport outcome after a request might have committed.

For ambiguous transport outcomes, reconcile/retry using upstream idempotency rather than switching representation blindly.

## 10. Response-digest verification

Change workspace creation helpers to retain the upstream `WorkspaceReady` result.

Before `execute_in_workspace`:

```text
ready.manifest_digest == local_current_manifest.digest()
```

must hold.

Mismatch is a hard failure and MUST NOT submit the command.

Apply this verification to both derived and full modes so M003 improves the baseline generally.

## 11. Cancellation/restart semantics

Cancellation:

- before snapshot -> current behavior;
- during snapshot -> fail/cancel locally;
- during blob upload -> current bounded cancellation;
- after a typed derived miss and during full fallback -> cancel without submit;
- after workspace ready but before execute -> no command submit; existing workspace retention/GC cleans up.

Restart:

- optimization cache loss is harmless; resumed/new attempts use full mode;
- accepted execution reconciliation still uses the persisted exact remote handle;
- M003 MUST NOT reconstruct a workspace from a mutable current worktree during restart of an already accepted execution.

## 12. Exact-source provenance interaction

The recently closed execution-subject provenance path is a hard correctness surface.

M003 MUST preserve:

```text
capture S1
  -> build immutable full WorkspaceSnapshot
  -> capture S2
  -> seal full manifest digest/completeness
  -> choose full/derived transport
```

A derived patch is never persisted as the historical source identity.

If `skipped_non_regular > 0` or `skipped_oversize > 0`, existing exact-subject unavailability semantics remain unchanged.

Any optimization that tries to avoid constructing the full manifest locally belongs to a separate future plan and must first redefine/prove source-sealing equivalence.

## 13. Diagnostics and metrics

Expose bounded transfer facts without paths/content:

- mode: full / derived / derived-miss-full;
- base manifest hit/miss;
- full entry count;
- patch entry count;
- full manifest encoded bytes;
- patch encoded bytes;
- blobs probed/uploaded;
- uploaded bytes.

Do not log manifest contents, file paths, file bodies, or secrets by default.

## 14. Tests

Focused patch planning:

- unchanged workspace -> empty patch;
- one file content change;
- executable-bit change;
- add file/directory;
- delete file;
- delete directory with descendants represented explicitly;
- deterministic ordering;
- local recomposition equals current full manifest.

Cache:

- node/workspace isolation;
- bound/eviction;
- config replacement invalidation;
- cold restart -> full mode.

Transfer:

- feature absent -> full;
- base absent locally -> full;
- derived hit;
- upstream `base_manifest_missing` -> full fallback;
- any other derived error -> no fallback;
- patch not smaller -> full;
- response digest mismatch -> no execute;
- changed-file-only blob probe in derived mode;
- no second submit/retry regression.

Provenance:

- sealed manifest digest identical between full and derived modes for the same snapshot;
- skipped-file completeness semantics unchanged;
- source drift still refuses submit.

Live:

- production mTLS full workspace establishes base;
- next attempt derives unchanged/small-change workspace against real Eggwork;
- required Landlock execution still succeeds on the derived workspace;
- filesystem escape remains denied;
- lease/restart/no-duplicate suites remain green.

## 15. Performance evidence

M003 is an optimization milestone; closure needs measured evidence, not only correctness.

Use a deterministic fixture with at least:

- unchanged workspace;
- small edit (1-5 files);
- moderate edit;
- no reusable base.

Record:

- request/control bytes for workspace creation;
- blob probe count;
- blob upload bytes;
- elapsed snapshot + transfer time separately.

Acceptance does not require a universal wall-clock speedup on localhost, but unchanged/small-edit cases MUST demonstrate materially lower workspace-control bytes and no increase in uploaded blob bytes.

Do not claim reduced local snapshot/hash cost; M003 intentionally preserves full local snapshot construction.

## 16. Documentation

Update:

- `architecture/jobs.md`;
- `architecture/scheduler.md`;
- execution-source provenance documentation;
- Eggwork integration roadmap/registry;
- operator diagnostics where transfer mode is surfaced.

Document that blob dedup existed before M003 and that this milestone adds manifest-level reuse.

## 17. Verification

At minimum:

```bash
cargo test -p codegg --lib scheduler::eggwork --locked
cargo test --test eggwork_remote_execution --locked
cargo test --test eggwork_remote_execution_live --locked
python3 scripts/check_eggwork_target_routing.py
python3 scripts/check_scheduler_bypass.py
bash scripts/check-core-boundary.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
scripts/verify.sh full
git diff --check
```

Hosted Linux live tests are required for closure because the current qualified remote execution path includes required Landlock.

## 18. Acceptance criteria

1. Existing full workspace mode remains compatible.
2. CodeGG uses `workspace.derive.v1` only when explicitly advertised.
3. Current full manifest remains the exact source/provenance authority.
4. Derived patch locally recomposes to that full manifest exactly.
5. Server-ready digest must equal the local current digest before execute.
6. Same-node acknowledged bases enable derived transfer.
7. Missing/expired base falls back safely to full mode.
8. Other derived errors never silently fallback.
9. Derived mode probes/uploads only changed/new blob content under the qualified upstream retention contract.
10. Cache is bounded, secret-free, node-scoped, and non-durable.
11. Required Landlock/live lease/restart/no-fallback invariants remain green.
12. Measured unchanged/small-edit workspace-control bytes improve materially.
13. No Git semantics are added to Eggwork.
14. No unresolved high/medium finding remains.

## 19. Stop conditions

Stop and record a blocker if:

- Eggwork M004 closes without guaranteeing canonical final digest equivalence;
- base retention does not pin referenced blobs long enough for derived use;
- safe fallback cannot distinguish typed base miss from ambiguous failure;
- CodeGG would need to trust the patch instead of constructing the current full manifest;
- the optimization conflicts with execution-subject sealing;
- a shared writable remote tree is required;
- implementing this milestone would require Git semantics in Eggwork;
- a new scheduler/placement path becomes necessary.

## 20. Closure evidence

Create:

- `plans/closure/eggwork-fixed-target-remote-execution/003-status.md`

Record:

- exact upstream Eggwork M004 closure/revision;
- immutable dependency pin;
- cache bounds;
- full-vs-derived selection rule;
- patch equivalence evidence;
- digest-verification evidence;
- base-miss fallback evidence;
- provenance preservation;
- live required-isolation/restart evidence;
- deterministic performance measurements;
- hosted CI;
- residual findings;
- M004 whole-AgentRun disposition.
