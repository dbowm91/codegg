# Eggwork Fixed-Target Remote Execution M003 — Closure Status

Status: blocked

Source implementation plan:

- `plans/implementation/eggwork-fixed-target-remote-execution/003-content-aware-derived-workspace-transfer.md`

Source subsystem roadmap:

- `plans/subsystems/eggwork-fixed-target-remote-execution-roadmap.md#m003--workspace-transfer-optimization`

Repository baseline reviewed: `7259292ef371ecd116e8911f9c2ddca7032e2a2e`

Implementation commits or pull requests:

- None — implementation did not begin; upstream hard dependency remains unsatisfied (see §1).

## 1. Executive finding

M003 closes **blocked** without production implementation, by the plan's own
§19 stop conditions. The upstream Eggwork Workspace/Artifact M004 contract
(`workspace.derive.v1`, principal-scoped retained bases, canonical patch
equivalence, typed `base_manifest_missing`) does not exist in the pinned
Eggwork revision or in Eggwork's current planning registry. Attempting
derived transfer against the current upstream would require trusting an
unqualified base, guessing blob retention, or inventing a CodeGG-local patch
language — all explicitly forbidden by the plan (§4, §6, §8, §19).

Current CodeGG behavior is unchanged: full local snapshot construction,
full-manifest provenance sealing, full blob probe/upload, and full
`create_workspace` remain the sole transfer path. M002/M002a qualification
is unaffected. M004 remains deferred.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Upstream M004 closure with `workspace.derive.v1`, principal-scoped base retention, canonical patch equivalence, typed `base_manifest_missing` (plan §4, §19) | Pinned `eggwork-client`/`eggwork-core` rev `6cc813418c3f14740a635fef79208e85219175bb` (`Cargo.toml:282-283`, `Cargo.lock:1896-1914`); Eggwork checkout `6cc8134` has only `001/002/003-workspace-artifact-transport.md`, no `004-*`; `rg derive\.v1\|WorkspaceManifestPatch\|base_manifest_missing` over `6cc8134` crates returns only `#[derive]` noise, zero protocol hits; newer checkout `128f808` likewise zero hits; Eggwork `plans/registry.md` lists Workspace/artifacts as "M001-M003 complete" with no M004 | fail (blocker holds) | Upstream planning commit `ed0fc838bd006103021f1fb4749f579cabc2db87` cited by the plan has no corresponding closed implementation in the pinned or newer Eggwork tree |
| Pin exact reviewed immutable Eggwork revision consistently (plan §4) | Not attempted — no qualified revision exists to pin | not run | Blocked before work-package 1 |
| Extend `EggworkNodeClient` + `NodeClientAdapter` with upstream derived-workspace method; consume only explicitly advertised `workspace.derive.v1` (plan §4) | `src/scheduler/eggwork.rs:195-323` exposes only `capabilities`/`find_missing_blobs`/`create_workspace`/`execute_in_workspace`; no derive method exists upstream to wrap | not run | Would violate "do not infer support from version strings" to stub |
| Bounded acknowledged-manifest cache, LRU, node-scoped, secret-free, non-durable (plan §5) | Not implemented | not run | Cache without a server-side retained-base contract cannot be qualified |
| Deterministic `WorkspaceManifestPatch` with local `apply(B, patch) == C` proof; use upstream type/helper (plan §6) | No upstream patch type/helper exists in pinned crates | not run | Plan forbids divergent local patch language |
| Transfer-mode selection with strictly-smaller rule (plan §7) | Not implemented | not run | No derived mode to select |
| Derived-mode changed-file-only blob probe under qualified retention contract (plan §8) | Upstream closure does not guarantee base blob retention (no closure at all) | fail (stop) | Plan §8: "If upstream closure does not guarantee base blob retention, stop and revise" |
| Typed `base_manifest_missing`-only fallback; no fallback on auth/digest/quota/identity/protocol/ambiguous-transport (plan §9) | No typed miss variant exists upstream | not run | Cannot distinguish typed miss from ambiguous failure |
| `ready.manifest_digest == local_current_manifest.digest()` before execute, both modes (plan §10) | Not implemented | not run | Baseline `create_remote_workspace` (`src/scheduler/eggwork.rs:1667-1677`) discards the ready digest; improvement belongs to the unblocked implementation pass |
| Cancellation/restart/provenance preservation (plan §11, §12) | Unchanged baseline; no new path to review | pass (baseline holds) | Full-mode provenance sealing untouched |
| Diagnostics/metrics, docs, performance evidence (plan §13, §15, §16) | Not produced | not run | Optimization milestone produces no measurements while blocked |

## 3. Production implementation evidence

No production implementation landed, by design for a blocked closure.

Ownership/codebase state at review:

- `src/scheduler/eggwork.rs` retains the M002/M002a-qualified full-mode path:
  `build_snapshot` (bounded walk, no symlink following), `upload_snapshot`
  (full digest probe + missing upload), `create_remote_workspace` (full
  `create_workspace`), deterministic attempt workspace id, dual-view
  capability preflight with required `isolation.landlock.workspace-rw.v1`.
- `EggworkNodeClient` trait (`src/scheduler/eggwork.rs:195-223`) has no
  derived-workspace method; `NodeClientAdapter` forwards only the four
  upstream methods above.
- Workspace pins remain `6cc813418c3f14740a635fef79208e85219175bb` in root
  `Cargo.toml` and `crates/eggwork-test-node/Cargo.toml`; no revision bump
  was attempted because there is no qualified revision to adopt.
- No cache, patch, mode-selection, digest-verification, or metrics code was
  added. `git status` shows only this closure batch as pending.

Distinguished clearly: planned-but-absent behavior is the entire M003
derived path (§5-§10, §13). Implemented-and-retained behavior is the
existing full-mode transfer with M002a live qualification intact.

## 4. Verification executed

### Commands run

```bash
rg -l "derive\.v1|WorkspaceManifestPatch|base_manifest_missing" ~/.cargo/git/checkouts/eggwork-082d4652b6e18fe8/6cc8134/
rg -l "derive\.v1|WorkspaceManifestPatch|base_manifest_missing" ~/.cargo/git/checkouts/eggwork-082d4652b6e18fe8/128f808/
rg -n "M004|derive" ~/.cargo/git/checkouts/eggwork-082d4652b6e18fe8/6cc8134/plans/subsystems/workspace-artifact-transport-roadmap.md
cargo test --locked -p codegg --lib scheduler::eggwork
scripts/verify.sh quick
cargo fmt --all -- --check
git diff --check
```

Plan-literal live/hosted commands (`cargo test --test
eggwork_remote_execution_live`, `cargo clippy --workspace --all-targets`,
`scripts/verify.sh full`, hosted Linux live) were **not run**: no
implementation exists to qualify, and hosted live is closure evidence only
for an unblocked implementation pass.

### Results

- Upstream absence (local truth): both Eggwork checkouts return zero
  protocol hits for `derive.v1` / `WorkspaceManifestPatch` /
  `base_manifest_missing`; workspace-artifact-transport holds only
  `001/002/003-*.md`; Eggwork registry lists Workspace/artifacts as
  M001-M003 complete with no M004. Pinned CodeGG rev `6cc8134` matches
  `Cargo.lock`.
- `cargo test --locked -p codegg --lib scheduler::eggwork`: 14 passed /
  0 failed (full-mode regression baseline holds; includes lease identity,
  posture projection, preflight, upload/create paths).
- `scripts/verify.sh quick`: passed (fmt, agent schema, core-boundary,
  sandbox, execution-ownership, tui-authority, http-route-disposition,
  audit-coverage, scheduler-bypass, eggwork target-routing, workspace
  check).
- `cargo fmt --all -- --check` and `git diff --check`: clean (this closure
  batch is formatting-clean; diff check covers pending planning files).
- Live/hosted scope: not run, truthfully labeled; no live claim is made.

## 5. Invariant review

Per plan §3 durable invariants — all hold vacuously because no derived
path was added, and the full-mode baseline is unchanged:

1. Full current manifest remains authoritative — holds; no patch path exists.
2. No mtime heuristics — holds; `build_snapshot` still hashes/reads every file.
3. `materialization.manifest_digest` is the full-manifest digest — holds; no
   patch digest is persisted.
4. Derived acceptance requires exact digest match — vacuously holds; no
   derived result is accepted.
5. Base reuse is a hint, never authority — vacuously holds.
6. Missing base falls back; other errors do not — vacuously holds; only full
   mode runs.
7. Cache entry never crosses nodes — vacuously holds; no cache exists.
8. No local fallback or alternate-node selection — holds; adapter still fails
   typed preflight without fallback.
9. Scheduler/lease/isolation/restart unchanged — holds; `scheduler::eggwork`
   suite green.
10. No Git semantics — holds; no Git code added.

## 6. Failure and recovery review

No new failure modes introduced. Baseline full-mode semantics reviewed and
unchanged:

- Duplicate delivery/idempotency: one CodeGG attempt maps to one Eggwork
  execution identity; transport retries reuse the tuple (M001/C001 evidence).
- Cancellation: before snapshot / during snapshot / during upload / after
  ready-but-before-execute paths unchanged; no derived-miss fallback path
  exists to cancel through.
- Restart: no optimization cache exists, so restart is trivially cold and
  full-mode; accepted-execution reconciliation still uses the persisted exact
  remote handle; no worktree recapture added.
- Malformed/unauthorized input: path/count/size bounds, symlink skipping,
  digest validation, and fail-closed capability preflight unchanged.
- Ambiguous transport after a request might have committed: no
  representation switch exists, so no blind-switch hazard was added.

## 7. Migration and compatibility review

No migration, no protocol negotiation change, no config change, no
dependency bump. Root and test-node Eggwork pins stay at `6cc8134`. Full
workspace mode remains the sole compatible path; no legacy path was removed.
Rollback is a revert of this planning-only batch with zero data implications.

## 8. Security review

No authorization, secret, network, or privilege surface touched. No TLS
paths/secrets cached (no cache exists). Node credentials remain
daemon-owned references, never in `JobRecord`/labels/logs/Debug output.
`#![deny(unsafe_code)]` holds for the lib. Execution-ownership,
scheduler-bypass, and eggwork target-routing guards pass under
`verify.sh quick`.

## 9. Documentation and operations

- This closure record is the sole new artifact for M003 in this batch
  (plus registry/roadmap/plan status updates listed in §12).
- `architecture/jobs.md`, `architecture/scheduler.md`, provenance docs, and
  operator diagnostics are **not** updated: there is no new transfer mode to
  document. The existing full-mode + blob-dedup documentation remains
  accurate; the M003 "manifest-level reuse" note must not be added until an
  unblocked implementation lands.
- No new static guards or metrics; no operator action required.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | Upstream Eggwork Workspace/Artifact M004 has not closed; `workspace.derive.v1`, retained-base, patch-equivalence, and typed miss semantics are unavailable | M003 cannot be implemented without violating its own §§4/6/8/19 | Wait for upstream M004 closure at the cited planning path, then re-verify the four contract points before activating the handoff |
| medium | `create_remote_workspace` still discards the upstream ready digest, so the plan §10 general hardening (verify both modes) is outstanding | Baseline misses a defense-in-depth check even in full mode | Carry the §10 verification into the unblocked M003 implementation pass |
| low | Newer Eggwork checkout `128f808` also lacks M004, confirming the gap is not a stale-pin artifact | No pin-only fix exists | Do not bump the Eggwork rev for M003 until M004 lands |

No critical findings. No findings indicate a defect in the shipped full-mode code.

## 11. Roadmap disposition

- M003: **blocked** — upstream hard dependency unsatisfied; no downstream
  plan is unblocked by this closure.
- M004 (whole remote AgentRun worker): remains **deferred** on M003 plus the
  stable AgentRun worker-entry contract; unchanged.
- No subsystem roadmap revision required beyond recording this blocked
  closure; no corrective plan is registered because the blocker is external,
  not a CodeGG defect.

## 12. Registry updates

- `plans/registry.md` Blocked-work row for Eggwork M003: blocker text
  retained, closure link added to this record.
- `plans/registry.md` Eggwork remote-execution gate paragraph: M003
  "blocked specifically on Eggwork Workspace/Artifact M004" retained with
  this closure as the current evidence.
- `plans/subsystems/eggwork-fixed-target-remote-execution-roadmap.md`: M003
  section `blocked` retained with closure link; header status unchanged
  (active roadmap, M003 blocked).
- `plans/implementation/eggwork-fixed-target-remote-execution/003-content-aware-derived-workspace-transfer.md`:
  status line `blocked` retained with closure link.
- Unblock audit: no registered plan lists M003 as a satisfied hard/interface
  dependency while remaining blocked on anything else; M004 stays deferred.
  Nothing is moved to `ready` by this closure.
