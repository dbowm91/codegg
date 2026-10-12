# Desktop Frontend/IDE Post-Closure Corrective C003 — Diagnostics NotReady Contract

Status: implemented

Repository baseline: `d34354c37` (M006-C planning and ADR-0017 audit registered)

Source corrective addendum:

- `plans/subsystems/desktop-frontend-ide-foundation-post-closure-corrective-addendum.md`

Affected predecessor:

- `plans/implementation/desktop-frontend-ide-foundation/008-native-lsp-read-surface.md`
- `plans/closure/desktop-frontend-ide-foundation/009-status.md` (immutable; do not edit)

Applicable ADRs:

- `plans/adrs/ADR-0012-native-lsp-read-surface-delivery-and-authorization.md` (accepted)

Primary class: invariant/capability

## 1. Objective

Repair the diagnostics pull contract so an unavailable or cold LSP service has a typed `NotReady` outcome distinct from an authoritative empty diagnostic set. Preserve project-scoped `file.read` authorization and additive protocol compatibility.

## 2. Why this corrective is required

M006-C's WP-A audit found `CoreDaemon::lsp_diagnostics` returns an empty `LspDiagnosticsResultDto` if the request project id is empty, the daemon has no LSP service, or the diagnostic store has no row. ADR-0012 explicitly requires a cold/unavailable read to return `NotReady`, because empty would falsely tell users that a project is clean. The current result DTO has no readiness field. M006-C cannot render diagnostics truthfully until this is corrected.

This is a missed contract in the M006-B implementation, not a change to the accepted ADR. Preserve M006-B's closure record as historical evidence.

## 3. Current implementation evidence

- `LspDiagnosticsGet` is authorized at `file.read` and routed through `src/core/daemon_lsp.rs`.
- `LspDiagnosticsResultDto` currently contains `project_id`, `files`, and `truncated`, with no readiness status.
- The handler manufactures an empty result when `lsp_service()` is unavailable and when the diagnostic store has no project entry.
- Existing `DiagnosticsReconciler` already distinguishes a complete empty set from a missing result; it cannot distinguish unavailable service from empty because the DTO has erased that state.
- `LspReadResultDto` already establishes the desired `Ready` / `NotReady` vocabulary for point reads.

## 4. Invariants that must not regress

1. `NotReady` never carries or implies an empty complete result.
2. A complete `Ready` result with zero files/diagnostics is authoritative emptiness.
3. Missing, stale, unsupported, or malformed readiness fields fail closed to `NotReady`, never `Ready`.
4. Existing `file.read` authorization, request family, projection behavior, and warm-server-only rule remain unchanged.
5. Older daemon responses that omit the additive status field deserialize as `NotReady`.
6. Diagnostics payload bounds and raw disclosure rules remain unchanged.

## 5. Scope

### In scope

- Add a diagnostics readiness enum (`Ready` / `NotReady`) to the existing result DTO with fail-closed deserialization default.
- Return `NotReady` when the LSP service is absent/cold or no authoritative store snapshot exists.
- Return `Ready` with a complete file set, including an empty set, only when the diagnostics store confirms an authoritative snapshot for the project.
- Update client reconciliation and all DTO constructors/tests to preserve the distinction.
- Add a request-level trajectory proving unavailable is not empty and a ready empty set clears previously displayed diagnostics.
- Update authoritative LSP architecture/skill documentation and registry state; preserve M006-B closure immutability.

### Out of scope

- New LSP operations, authorization capabilities, storage tables, migrations, publisher scheduling, or push behavior.
- TUI presentation implementation (M006-C remains separately registered and blocked until this closes).
- Changing ADR-0012, M006-B's immutable closure record, or the M006-E apply path.

## 6. Required production changes

### Protocol and compatibility

Add a serialized readiness enum and `status` field to `LspDiagnosticsResultDto`. Use `#[serde(default)]` with a default of `NotReady` so an older daemon's response cannot be mistaken for a clean result. Do not use a boolean whose missing-value default is ambiguous.

### Daemon behavior

Check the service and store before constructing `Ready`. The store API must distinguish “authoritative empty snapshot” from “no snapshot available”; if it cannot, extend the narrow in-memory diagnostics store contract without adding persistence or inventing a timestamp heuristic. `NotReady` carries no file payload. Invalid request, authorization denial, and transport failure remain their existing distinct errors.

### Client behavior

Update `DiagnosticsReconciler` so it applies only `Ready` results. `NotReady` preserves the last known set and reports unavailable/freshness state. A complete `Ready` empty set clears stale diagnostics.

## 7. Ordered work packages

1. Trace diagnostics store empty-vs-missing semantics and record a tiny state table.
2. Add the fail-closed status field and daemon mapping.
3. Update client reconciler and every DTO construction site.
4. Add/falsify request-level tests and compatibility deserialization coverage.
5. Update architecture/skill/plan registry and collect verification evidence.

## 8. Failure and compatibility semantics

- LSP absent, cold, or lacking a complete diagnostic snapshot returns `NotReady`.
- LSP warm with a complete zero-file result returns `Ready` plus an empty set.
- Older daemon response without `status` deserializes as `NotReady`.
- A rejected/failed request leaves previously displayed diagnostics intact and marked stale.

## 9. Required tests

- Daemon without an LSP service returns `NotReady`.
- Warm LSP with a complete empty store returns `Ready` and no files.
- Missing project snapshot returns `NotReady`.
- Reconciler preserves its previous set on `NotReady` and clears only on `Ready` + empty.
- JSON response without the new status field defaults to `NotReady`.
- Existing LSP authorization, read-surface trajectory, and diagnostics reconciliation tests pass.

## 10. Required verification

```bash
cargo fmt --all -- --check
cargo test -p codegg-client lsp
cargo test -p codegg --test lsp_read_surface_trajectory
cargo test -p codegg --lib daemon_lsp
cargo check -p codegg --features server --locked
scripts/verify.sh quick
```

Run the hosted root CI workflow on the final revision. Space workflow dispatches and confirm they are queued/running rather than paused or indefinitely pending before relying on them.

Hosted attempt `38228419611` was active and reached Workspace Clippy, then
failed on Clippy's `derivable_impls` finding for the new status default. The
enum now derives `Default` and marks `NotReady` as the default variant; retain
that first result as a recorded failed attempt and rerun on the corrected SHA.

Hosted attempt `38228801296` was active and reached Workspace Clippy, then
failed on two findings: an existing `TurnCompleted` pattern named fields
already covered by `..`, and the new diagnostics readiness test module sat
before later module items. The pattern now names only `turn_id`, and the test
module is at end of file. Focused local Clippy (`cargo clippy -p codegg --lib
--tests --locked -- -D warnings`) completed with no issues; dispatch hosted CI
again after pushing these corrections.

Hosted attempt `38229457314` passed Formatting and Workspace Clippy, then
failed in Workspace tests when two `egggit` timeout tests were themselves
terminated by `SIGKILL`. The timeout code signaled the child's process group
without first creating a separate group; on Linux this selected the caller's
group and killed the test processes. Timeout-bound children now become their
own Unix process-group leaders before spawn. The 84 `egggit` tests and
`scripts/check_git_forbidden_patterns.py` pass locally after the fix. This
Git-process isolation correction is documented in `architecture/git.md` and
the Git skill; rerun hosted CI before closure.

## 11. Documentation and closure evidence

Update `architecture/lsp.md`, `.opencode/skills/lsp-ide/SKILL.md`, and `plans/registry.md`. Closure requires the empty-vs-missing state table, compatibility test, focused trajectories, relevant authorization checks, quick verification, and hosted CI result. Do not edit `plans/closure/desktop-frontend-ide-foundation/009-status.md`.

## 12. Stop conditions

- The diagnostics owner cannot distinguish no snapshot from a complete empty snapshot without a new persistence or lifecycle contract.
- Preserving older-daemon compatibility would require treating an omitted status as `Ready`.
- The change requires weakening `file.read`, introducing implicit LSP startup, or claiming push publication is active.

## 13. Downstream disposition

M006-C (`plans/implementation/desktop-frontend-ide-foundation/009-lsp-editor-presentation.md`) may return to `ready for handoff` only after C003 closes. The M006-C plan must continue using explicit pulls until a separate plan closes the undriven publisher gap.
