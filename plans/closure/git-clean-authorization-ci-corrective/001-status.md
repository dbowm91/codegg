# Git Clean Authorization CI C001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/git-clean-authorization-ci-corrective/001-typed-git-clean-test-authorization.md`

Source subsystem roadmap:

- `plans/subsystems/git-clean-authorization-ci-corrective-addendum.md#C001`

Repository baseline reviewed: `8ffb3cce` (planning branch head before implementation)

Implementation commits:

- `b0392431` — test(shell,git): ADR-0028 advisory shell fixtures and typed git-clean denial (git half; this closure's evidence was gathered on this tree)

## 1. Executive finding

C001 is complete. The stale `clean_removes_untracked_files` expectation that
an unauthenticated destructive `git clean` should succeed is replaced by
`clean_without_destructive_capability_returns_typed_denial`, which proves the
intended authorization behavior: real clean without an explicit
`DestructiveRepositoryChange` capability returns the exact typed
`RequiresAdditionalAuthorization` refusal before any child is launched and
leaves the temporary repo untouched, while dry-run preview stays usable
without a grant. No test-only privilege bypass, blanket allowlist, fake
executable, `#[cfg(test)]` bypass, or policy weakening was introduced. The
genuine positive destructive-clean path is honestly recorded as unavailable:
`GitMutationExecutor::run_subprocess` does not consume
`execution_authorization` to promote the typed refusal (it calls
`GitEnvPolicy::apply`, which returns the refusal as a `Policy` error), so
there is currently no trusted production seam for a test-authorized confined
cleanup — that capability stays blocked on the already-registered Linux
containment work (C002/contained Git), not this CI corrective.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| No-grant `git clean -f` returns exactly `RequiresAdditionalAuthorization[DestructiveRepositoryChange]` with no dispatch/side effect | `clean_without_destructive_capability_returns_typed_denial` matches `GitMutationError::Policy(RequiresAdditionalAuthorization{capabilities})` and asserts the capability + file/HEAD/status invariance | pass | Same public `git_network_ops::clean` API, `fresh_repo()` + unique untracked file |
| Dry-run preview stays functional without grant; broad ignored cleanup not silently allowed | `clean_preview_lists_untracked_files` (entries observed) + `clean_broad_ignored_request_is_rejected_by_policy` (`is_broad`) unchanged and green | pass | Preview immutability covered by existing test; classifier adds `-n`/`--dry-run` allowed proof |
| Classifier proves `clean -n` allowed and `clean -f` gated even in a temp repo | New `git_clean_preview_is_allowed_but_real_clean_requires_destructive_authorization` in `crates/egggit/src/process.rs` | pass | `-n`, `-n -d`, `--dry-run` → `AllowedTrustedNonExecutable`; three real-clean argv shapes → `RequiresAdditionalAuthorization[DestructiveRepositoryChange]` + `apply_sync` refusal |
| No test-only bypass or privilege promotion; positive path honestly dispositioned | `run_subprocess` rebaseline (§3); no `with_execution_authorization` fabrication in tests | pass | Positive contained clean recorded as blocked on Linux-isolation C002, not asserted |
| Focused tests + guards pass; hosted progress honestly classified | §4 commands/results | pass | Clean binary's 3/3 green; full `git_network_integration` 24/29 with 5 pre-existing stale positives classified in §10; hosted CI not run |
| Full-suite green only after shell hazards removed; conditional on unrelated red | Shell C002 landed in the same implementation commit; remaining red is reset/network, not clean | pass | Sibling shell closure discharges the safety gate for this head |

## 3. Production implementation evidence

- `tests/git_network_integration.rs`: removed `clean_removes_untracked_files`
  (called `git_network_ops::clean` with `GitMutationExecutor::new()` minus
  authorization and demanded deletion). Added
  `clean_without_destructive_capability_returns_typed_denial`, which invokes
  the identical public API, asserts the typed
  `Policy(RequiresAdditionalAuthorization{DestructiveRepositoryChange})`
  rather than a stringified error, and verifies the untracked fixture
  (`garbage-no-grant.txt`) still exists, `README.md` still exists, `HEAD`
  is byte-identical before/after, and `status --porcelain` still shows the
  untracked entry. Comments distinguish TempDir containment (not a grant)
  from actual authorization.
- `crates/egggit/src/process.rs`: new classifier fixture
  `git_clean_preview_is_allowed_but_real_clean_requires_destructive_authorization`
  pins the policy so future changes to real-clean gating are intentional and
  reviewed. No production policy line changed (`classify_git_request` for
  `reset|clean` untouched).
- Trusted-authorization rebaseline: `GitMutationExecutor` owns
  `with_execution_authorization`, but `run_subprocess`
  (`src/git_mutations.rs:704-759`) clones only `env_policy` and calls
  `env.apply(argv, root)`, which fails through
  `ensure_trusted_git_request` → `classify_git_request` with the typed
  refusal. Attaching a fabricated authorization snapshot in a test would not
  promote the outcome, so no such fixture was added — correctly, per the
  plan's stop condition. A genuine positive requires the contained-backend
  authority integration owned by Linux-isolation C002/C003.
- No Git operation, schema, database, wire, tool-permit, remote-transport,
  symlink, or permission-redesign change. `GitExecutionPolicyOutcome` API
  unchanged. Historical `plans/closure/linux-execution-isolation/009-status.md`
  untouched; Linux C002/C003 not claimed closed.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo test -p egggit --lib -- process
TMPDIR=/tmp cargo test -p codegg --test git_network_integration clean_preview_lists_untracked_files
TMPDIR=/tmp cargo test -p codegg --test git_network_integration clean_without_destructive_capability_returns_typed_denial
TMPDIR=/tmp cargo test -p codegg --test git_network_integration
TMPDIR=/tmp cargo test -p codegg --test git_execution_origin_matrix
TMPDIR=/tmp cargo test -p codegg --test git_mutations_integration
TMPDIR=/tmp cargo test -p codegg --test git_env_attack
python3 scripts/check_git_forbidden_patterns.py
python3 scripts/check_execution_ownership.py
TMPDIR=/tmp bash scripts/verify.sh quick
TMPDIR=/tmp cargo clippy --workspace --all-targets --locked -- -D warnings
```

### Results

- `cargo fmt --check`: clean.
- `egggit --lib process`: 12 passed, 0 failed (includes the new clean classifier fixture).
- `git_network_integration clean_*` filter: 3 passed (`clean_preview_lists_untracked_files`, `clean_without_destructive_capability_returns_typed_denial`, `clean_broad_ignored_request_is_rejected_by_policy`).
- `git_network_integration` full binary: 24 passed, 5 failed. The 5 failures are pre-existing stale positives that also demand unauthenticated/contained execution and are **outside this C001's clean scope**: `reset_hard_moves_head_and_drops_changes` (`Policy(RequiresAdditionalAuthorization[DestructiveRepositoryChange])`), `reset_soft_keeps_working_tree`, `push_to_local_bare_remote_succeeds`, `pull_from_local_bare_remote_succeeds`, `fetch_from_local_bare_remote_succeeds` (contained/remote authority → `BackendUnavailable`/policy refusal). They existed before this change (previously masked by fail-fast stopping at the clean failure: main runs `38606475897`/`38598782570` stopped after 7027 passed with 6614 skipped). Fixing them is a separate test-contract/contained-backend milestone, not a silent expansion of this C001.
- `git_execution_origin_matrix`: 28 passed. `git_mutations_integration`: 12 passed. `git_env_attack`: 20 passed.
- `check_git_forbidden_patterns.py`: PASS (0 findings). `check_execution_ownership.py`: ok.
- `verify.sh quick`: passed (same scope as shell closure).
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: clean.
- Hosted Linux CI on the exact implementation SHA: **not run** (local-only verification). No hosted run is claimed. The prior hosted first-failure (`clean_removes_untracked_files` on runs `38606475897`/`38598782570`) is discharged: the offending expectation no longer exists and its replacement passes locally.

## 5. Invariant review

- Destructive cleanup requires explicit `DestructiveRepositoryChange` plus any kernel/process/credential constraints; a TempDir grants nothing: new denial test uses `fresh_repo()` + no authorization and gets the typed refusal. Holds.
- Denial precedes child launch; fixture/index/HEAD unchanged; no raw Git/Bash fallback: file-exists + HEAD-equality + status assertions; `run_subprocess` maps the policy refusal before `cmd.output()`. Holds (no-dispatch inferred from state invariance + `apply_sync` refusal in the classifier fixture; no success tombstone/audit event is produced on the `Policy` path).
- Preview `git clean -n` needs no grant and changes nothing; broad `-x` unscoped cleanup stays rejected: existing preview/broad tests green + classifier `-n` allowed assertions. Holds.
- No blanket allowlist, fake executable, `#[cfg(test)]` bypass, FullHost execution, disabled policy checks, injected approval receipt, or temp-path grant magic: none added; `rg` for `DENIED_GIT_EXECUTABLE|trusted_git_request_denied` stays absent (forbidden-patterns guard). Holds.
- No storage/protocol migration, public `git` tool schema change, or contained/remote support claim: none made. Holds.

## 6. Failure and recovery review

- Permission refusal is a typed pre-launch result with no Git state mutation and no automatic retry through raw Bash/helper/fallback/scheduler: `run_raw_mutation` propagates the `Policy` error; no retry path added.
- Success claims require scoped capability + verified containment, not exit-zero: no positive clean asserted; the unavailable positive is recorded, not faked.
- TempDir fixtures unique per test (`garbage-no-grant.txt` in a fresh repo); Git global/credential env stripped by the test helper (`GIT_CONFIG_GLOBAL/SYSTEM=/dev/null`, `GIT_TERMINAL_PROMPT=0`). Holds.
- Cancellation/restart never reinterprets missing authority as approval: no mutable shared global policy added for tests; no real remote/host-global clean or network credential in tests.

## 7. Migration and compatibility review

No database, wire, config, tool-schema, or API change. `CleanRequest`/`CleanPreview` unchanged. Test comments only. Historical Linux C001 closure and command-authorization closures unchanged. No rollback concern: the change is test-only plus a new unit fixture.

## 8. Security review

Resolving the test did not use an unconditional bypass, host-global clean,
unowned execution-service migration, fabricated grant, or generic spawn
failure in place of the typed refusal — all stop-condition shapes were
avoided. No new attack surface: the classifier fixture is pure (no process),
the integration denial test performs no deletion. `check_git_forbidden_patterns`
and `check_execution_ownership` both green.

## 9. Documentation and operations

- Updated test comments in `tests/git_network_integration.rs` (containment ≠ authorization).
- No `architecture/git.md` behavioral claim changed (no genuinely changed claim beyond the test contract; the typed `DestructiveRepositoryChange` contract it already documents is now tested).
- Static protection: the new `egggit::process` classifier fixture makes future real-clean gating changes intentional and reviewed, alongside the existing `check_git_forbidden_patterns.py` trusted-policy guard.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | `git_network_integration` still has 5 stale positives outside clean scope: `reset_hard`, `reset_soft` (both now `RequiresAdditionalAuthorization`), `push/pull/fetch` to local bare remotes (contained/remote authority) | Full integration binary red; workspace CI cannot go green on these fixtures | Separate test-contract/contained-backend corrective (reset typed-denial fixtures + network containment qualification) owned by the Linux-isolation C002/C003 line; do NOT fold into this C001 |
| low | Hosted exact-SHA Linux CI not run for this closure | No hosted proof on CI runners | Future hosted `CI / verify` run on the closure head; local evidence complete and honestly labeled |
| low | No-dispatch proven by state invariance + `apply_sync` refusal, not a child-spawn counter | Residual inference (no direct dispatch count) | Acceptable per plan ("if feasible without production churn"); a spawn counter would require production adapter churn |

No critical/high findings. The medium finding is a pre-existing, separately owned scope — not a defect in the clean contract this milestone owns.

## 11. Roadmap disposition

Milestone C001 closed for its bounded clean test-contract scope. The
`git-clean-authorization-ci-corrective` workstream has no further milestone
(it defines C001 as the single test-reconciliation milestone), so its roadmap
moves to closed. Linux contained Git (C002), mediated remote Git (C003),
ADR-0027 acceptance, and the M001 native backend remain blocked exactly as
before — this closure promotes none of them. The medium finding above should
be picked up by a focused reset/network test-contract successor or by the
already-registered containment line, not by reopening this C001.

## 12. Registry updates

- `plans/registry.md` Active subsystem roadmaps: Git-clean row → `closed`, current milestone `C001 closed at b0392431 (closure plans/closure/git-clean-authorization-ci-corrective/001-status.md)`, blockers `— (Linux contained/remote Git stays separately blocked)`.
- `plans/registry.md` Dependency-ready plans: Git-clean C001 row → `closed`, implementation `b0392431`.
- `plans/registry.md` execution-gate note: the `clean_removes_untracked_files` first-failure half of the gate is discharged (expectation replaced, replacement green locally). The remaining `git_network_integration` red (reset/network) is classified above, not hidden.
- `plans/subsystems/git-clean-authorization-ci-corrective-addendum.md`: Status → `closed — C001 closed`; C001 plan link marked implemented/closed.
- `plans/implementation/git-clean-authorization-ci-corrective/001-typed-git-clean-test-authorization.md`: Status → `implemented; closed`.
- Unblock audit (per `plans/003-planning-process.md` §11): reviewed the registry Blocked work section and affected subsystem dependency graphs. No registered plan lists Git-clean C001 as a hard/interface dependency; Linux M001/C002/C003 remain blocked on ADR-0027 + native proof + egress/lease interfaces, not on this test fixture. **Nothing newly unblocked — recorded explicitly.**
