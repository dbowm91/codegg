# Eggwork Fixed-Target Remote Execution M003 Closure — Content-Aware Derived Workspace Transfer

Source plan: `plans/implementation/eggwork-fixed-target-remote-execution/003-content-aware-derived-workspace-transfer.md`
Subsystem roadmap: `plans/subsystems/eggwork-fixed-target-remote-execution-roadmap.md`
Implementation commit: `1ce377ce` (CodeGG `main`)
Review baseline: upstream `e6a5d82e1ea392bb505aef1315c0e8c91d8baa0c` (Eggwork HEAD at implementation)

## Disposition

M003 is closed positively. All 14 acceptance criteria are met with the
evidence below. No stop condition fired.

## Upstream contract and pin

- Upstream Workspace M004 implementation `1e89dacd...`, closure
  `5b989c96...` (`plans/closure/workspace-artifact-transport/004-status.md`
  in the Eggwork repo), reviewed head `e6a5d82e...` — verified to contain
  both Workspace M004 and the later Security M004 Landlock hardening.
- Pinned consistently in root `Cargo.toml`/`Cargo.lock`
  (`eggwork-client`, `eggwork-core`) and
  `crates/eggwork-test-node/Cargo.toml`/`Cargo.lock` (`eggwork-server`,
  `eggwork-runner`, `eggwork-core`): every entry resolves to
  `rev=e6a5d82e1ea392bb505aef1315c0e8c91d8baa0c`.
- Consumed surface only: `workspace.derive.v1` capability string,
  `NodeClient::create_workspace_derived`, `WorkspaceManifestPatch`
  (`schema_version 1`, `Remove`/`Directory`/`File`), `apply_to` through
  the shared validator, `WorkspaceReady.manifest_digest`, typed `409
  base_manifest_missing`. Support is read from the fresh preflight
  capability intersection; nothing is inferred from version strings.

## Implementation summary

- `EggworkNodeClient::create_workspace_derived` seam plus production
  `NodeClientAdapter` delegation (`src/scheduler/eggwork.rs`).
- Acknowledged-base optimization cache on the executor keyed by
  `(node_id, workspace)`: 8 bases per node, 64 total, oldest-first
  eviction, manifests only (no file bodies, TLS paths, or secrets),
  never persisted in `JobRecord`/`JobAttempt`, cold after restart,
  cleared on config replacement.
- `derive_patch`: path-compared diff (unchanged omitted, removed →
  `Remove`, added/changed → upsert, executable-bit and digest/size
  changes count as file upserts), deterministic path ordering, upstream
  `validate()`, then a mandatory recomposition proof
  (`apply(base, patch)` digest-equals current) before the patch may be
  sent. Symlink content fails closed.
- Transfer-mode selection per fresh attempt after snapshot and
  provenance sealing: derived only with an acknowledged base, an
  advertised capability, and a strictly smaller patch; otherwise full.
  Derived mode probes/uploads only patch-introduced digests.
- Safe fallback: only typed `409 base_manifest_missing` returns to full
  mode with the same deterministic workspace id/owner before any submit
  (the server creates no workspace on a miss). Auth, digest, quota,
  identity, protocol, timeout, and transport errors fail closed.
- `ready.manifest_digest == local_current_manifest.digest()` is
  verified in both modes before `execute_in_workspace`; mismatch is a
  hard failure with no submit.
- Bounded transfer facts on the progress sink (mode, base hit/miss,
  entry counts, manifest/patch bytes, blobs probed/uploaded, uploaded
  bytes; no paths, contents, or secrets).

## Acceptance evidence (criterion order, plan §18)

1. Full mode compatible: all pre-existing executor/lease/restart tests
   unchanged and green; full path exercised by every live run.
2. `workspace.derive.v1` gated on the fresh preflight intersection
   (`preflight_reports_derive_support_from_intersection`).
3. Full current manifest remains the source/provenance authority:
   snapshot construction and S1/S2 sealing order untouched
   (`architecture/jobs.md` provenance note).
4. Patch recomposition proven inside `derive_patch` on every use; 8
   focused patch tests (empty, content, exec-bit, add, delete,
   directory-descendant, ordering, symlink rejection).
5. Digest verification in both modes; `transfer_digest_mismatch_refuses_submit`
   proves no submit on mismatch.
6. Same-node bases enable derived transfer: live test below.
7. `transfer_base_miss_falls_back_to_full` (server-side base loss →
   one derived attempt, then full success, same identity).
8. `transfer_non_miss_derived_error_never_falls_back` (403 → Failed,
   one derived call, zero submits).
9. Changed-only probe: `transfer_derived_uploads_only_changed_blobs`
   (3-file workspace, 1 changed → exactly that digest probed/uploaded)
   under the upstream retention contract (retained manifests pin blobs;
   reaped bases report missing, never resurrect authority).
10. Cache bounds/secret-freedom/node-scope/non-durability:
    `derived_cache_is_node_and_workspace_scoped`,
    `derived_cache_evicts_oldest_entries_under_bounds`,
    `derived_cache_config_replacement_invalidates`.
11. Required Landlock/live lease/restart/no-fallback invariants green in
    hosted run `36745285774` (success, see below).
12. Measured savings (deterministic 50-file fixture, distinct 1 KiB
    files, local `build_snapshot` + `derive_patch` measurement):
    full manifest 7532 bytes / 50 probes; unchanged → 123-byte patch
    (−98.4%), 0 probes; small edit (1 file) → 272-byte patch (−96.4%),
    1 probe; moderate edit (10 changed + 1 added + 1 removed) →
    1808-byte patch (−76.0%), 11 probes. Derive compute ≤1 ms;
    snapshot cost unchanged by design (full local construction kept).
13. No Git semantics added; patch language is the upstream path-map
    contract.
14. No unresolved high/medium findings.

## Verification actually run

- `cargo test -p codegg --lib -- scheduler::eggwork`: 33 passed
  (19 new M003 tests).
- `cargo test --test eggwork_remote_execution`: 25 passed.
- `scripts/verify.sh quick`: passed (fmt, guards incl.
  `check_eggwork_target_routing.py` + `check_scheduler_bypass.py` +
  execution-ownership, workspace check).
- `cargo clippy --workspace --all-targets --locked -- -D warnings`:
  clean. `cargo fmt --all -- --check` and `git diff --check`: clean.
- Hosted `CI / verify` run `36745285774` on `1ce377ce`: **success**
  (~19 min, 11,785 tests, live Eggwork included on the main push).
  New `live_derived_workspace_reuse_under_required_isolation` PASSED:
  full-then-derived against the real node under required Landlock
  isolation (derived counter 1, both executions submitted and
  completed). Pre-existing live lease/restart/escape suites green.
- Full-mode control-byte rule also covered:
  `transfer_patch_not_smaller_stays_full` and
  `transfer_without_derive_feature_stays_full`.

## Residual findings

- None blocking. The `eggpool` cancellation-registration flakes seen
  once on the unrelated lint-fix run `36740881458` did not reproduce on
  this run; they are owned by the CI timing-flake corrective C002, not
  by this milestone (no M003 code touches that path).

## M004 disposition

M003 satisfied its half of the M004 gate (optimized materializer
contract). M004 (whole-AgentRun derived workspace) remains deferred on
the stable AgentRun worker-entry contract, which is outside this
milestone's scope.
