# Git Reset/Remote Test-Contract C002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/git-reset-remote-test-contract-ci-corrective/002-remote-git-unavailable-test-reconciliation.md`

Source subsystem roadmap:

- `plans/subsystems/git-reset-remote-test-contract-ci-corrective-addendum.md#C002`

Repository baseline reviewed: `1cd71e4c` (branch head before implementation)

Implementation commits:

- `7922bb0e` — test(git): typed reset-denial and remote-unavailable fixtures (C001/C002) (remote half; this closure's evidence was gathered on this tree)

## 1. Executive finding

C002 is complete. The stale `fetch_from_local_bare_remote_succeeds`, `push_to_local_bare_remote_succeeds`, and `pull_from_local_bare_remote_succeeds` expectations that uncontained remote Git should succeed are replaced by `fetch_without_contained_remote_backend_returns_typed_unavailable`, `push_without_contained_remote_backend_returns_typed_unavailable`, and `pull_without_contained_remote_backend_returns_typed_unavailable`, which prove the intended containment behavior: every `fetch`/`push`/`pull` without a contained remote backend returns the exact typed `BackendUnavailable[RemoteHelper, Network, Credential]` refusal before any child is launched and leaves local HEAD, tracking refs, and the bare remote's refs untouched. The fixtures assert **unavailability as a terminal typed outcome** — names, asserts, and comments never present it as a successful operation. A `file://` local path remote is explicitly not treated as a safety grant. No test-only transport, credential broker, proxy env, SSH/askpass smuggling, or classifier weakening was introduced. The positive remote path stays owned by Linux C003 (blocked on C001 + M005/M006 + transport policy); file-remote qualification is explicitly future C003 work, not this corrective.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| No-backend `fetch` returns exactly `BackendUnavailable[RemoteHelper, Network, Credential]` with no dispatch | `fetch_without_contained_remote_backend_returns_typed_unavailable` matches `GitMutationError::Policy(BackendUnavailable{capabilities})`, asserts the full triple + HEAD/tracking-ref equality | pass | Same public `git_network_ops::fetch` API, `fresh_pair()` raw-git setup |
| No-backend `push` returns the same typed refusal with the remote untouched | `push_without_contained_remote_backend_returns_typed_unavailable` asserts the triple + bare-remote `main` SHA equality | pass | Same public `git_network_ops::push` API; remote SHA proves nothing was pushed |
| No-backend `pull` returns the same typed refusal with nothing integrated | `pull_without_contained_remote_backend_returns_typed_unavailable` asserts the triple + HEAD equality + reset-away file still absent | pass | Same public `git_network_ops::pull` API; absent `d.txt` proves no fetch/integration |
| Unavailable reads as unavailable, never as success | Fixture names (`*_returns_typed_unavailable`), `expect_err`, comments citing Linux C003 | pass | Test output shows terminal unavailability, not a passing operation |
| Classifier proves `push`/`pull` (and local-path `clone`) require the contained backend | New `git_push_and_pull_require_contained_remote_backend` in `crates/egggit/src/process.rs` + `apply_sync` refusal | pass | Joins the pre-existing `fetch` pin; no production line changed |
| Roadmap reconciliation recorded without scope change | Linux roadmap CI test-contract note now cites this addendum's C002/C003 ownership | pass | Ownership link only; Linux C003 stays blocked |
| Focused tests + guards pass; hosted progress honestly classified | §4 commands/results | pass | Full `git_network_integration` green together with C001; hosted CI recorded below |

## 3. Production implementation evidence

- `tests/git_network_integration.rs`: removed the three stale remote positives (each called the executor-backed `fetch`/`push`/`pull` against a `fresh_pair()` bare remote and demanded `res.success`). Added the three typed-unavailable fixtures, which use the identical public APIs and fixture setup (raw `git` CLI setup stays outside the executor under test), assert the typed `Policy(BackendUnavailable{capabilities})` with the full `[RemoteHelper, Network, Credential]` triple rather than a stringified error, and verify state invariance: fetch → local HEAD + `origin/main` identical; push → bare remote `main` identical; pull → local HEAD identical and the reset-away `d.txt` still absent. Comments cite Linux C003 (`011-m001-authorized-remote-git-recovery.md`) as the positive-path owner and state file remotes are not yet qualified.
- `crates/egggit/src/process.rs`: new classifier fixture `git_push_and_pull_require_contained_remote_backend` pins `push`, `push --force-with-lease`, `pull`, `pull --ff-only`, and local-path `clone` to `RequiresContainedExecution[RemoteHelper, Network, Credential]` with `apply_sync` → `BackendUnavailable` refusal. No production policy line changed.
- `plans/subsystems/linux-execution-isolation-roadmap.md`: CI test-contract note extended to record this addendum's ownership of the stale reset/remote positives and their reconciliation as typed-unavailable fixtures citing C002/C003. Ownership link only — milestone table untouched, C003 stays `blocked: C001, M005/M006 and transport policy`.
- No transport, credential broker, mediator, SSH/HTTPS stack, `file://` allowlist, schema, database, wire, tool-permit, or permission-redesign change. `GitExecutionPolicyOutcome` API unchanged. Historical Linux closures untouched.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo test -p egggit --lib -- process
TMPDIR=/tmp cargo test -p codegg --test git_network_integration fetch_
TMPDIR=/tmp cargo test -p codegg --test git_network_integration push_
TMPDIR=/tmp cargo test -p codegg --test git_network_integration pull_
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
- `egggit --lib process`: 14 passed, 0 failed (includes the new push/pull/clone pin).
- Remote filters: `fetch_` 1 passed, `push_` 2 passed (incl. pre-existing `push_force_is_marked_destructive`), `pull_` 1 passed.
- `git_network_integration` full binary: 29 passed, 0 failed.
- `git_execution_origin_matrix`: 28 passed. `git_mutations_integration`: 12 passed. `git_env_attack`: 20 passed.
- Guards green; `verify.sh quick` passed; clippy clean.
- Hosted CI: `CI / verify` run `38643674863` on PR #120 (implementation head `7922bb0e`): completed with conclusion `failure` whose failure set is exactly three pre-existing `tests/git_recovery_integration.rs` tests (merge/rebase recovery demanding `BackendUnavailable[CleanSmudgeFilter, MergeDriver, Hook]` — the Linux-C002 scope; see §10). All three remote unavailable fixtures passed on hosted Linux, as did the reset, clean, and shell fixtures. Guards, fmt, and clippy steps all succeeded on the hosted runner. (Docs-only closure heads carry no code delta; CI skips `plans/**`-only pushes by workflow design.)

## 5. Invariant review

- Remote destinations/protocols/credentials authorized by policy, never by URL spelling or local-path convenience: fixtures use `file://` bare remotes and still get the typed refusal; local-path `clone` pinned as contained-required. Holds.
- Unavailability terminal and typed: no fake executable, generic spawn failure, fallback transport, or authority-upgrading retry: `execute`/`run_raw_mutation` propagate the `Policy` error; no retry path added. Holds.
- Denial precedes dispatch; local/remote refs unchanged; no success audit event on the `Policy` path: SHA-equality + absence assertions; `apply_sync` refusal in the classifier fixture. Holds.
- No `#[cfg(test)]` bypass, credential broker, proxy env, SSH smuggling, or `file://` weakening: none added; forbidden-patterns guard green. Holds.
- No Linux C003/M005/M006/SSH/file-remote support claim: none made; C003 stays blocked. Holds.

## 6. Failure and recovery review

- Unavailability is a typed pre-launch result with no local/remote mutation and no automatic retry or FullHost escalation.
- Fixture names and asserts present the outcome as unavailability, never as a successful operation: `*_returns_typed_unavailable` + `expect_err`.
- Unique TempDir fixtures; no shared mutable policy; no live credentials or open internet (bare `file://` fixtures only).
- If Linux C003 later lands a real backend, that landing must update these fixtures (addendum dependency graph records this) rather than coexist with a stale unavailable assertion.

## 7. Migration and compatibility review

No database, wire, config, tool-schema, or API change. Test + comment + roadmap-note only. No rollback concern.

## 8. Security review

Reconciling the tests used no transport, broker, proxy, SSH/askpass injection, `file://` allowlist, fabricated grant, or generic spawn failure/success in place of the typed unavailability — all stop-condition shapes were avoided. No new attack surface: classifier fixtures are pure; integration fixtures dispatch no remote child. Guards green.

## 9. Documentation and operations

- Test comments cite Linux C003 as the positive-path owner and state file remotes are not yet qualified.
- Linux roadmap CI test-contract note records the reconciliation (ownership link only).
- No `architecture/git.md` behavioral change (the contained-remote contract it documents is now tested as unavailable-without-backend).
- Static protection: the new push/pull/clone classifier fixture makes future remote-gating changes intentional and reviewed.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Positive remote execution still unavailable (no contained backend, no broker) | No positive fetch/push/pull coverage | Owned by Linux-isolation C003; these three fixtures must be revisited when C003 lands |
| low | No-dispatch proven by ref invariance + `apply_sync` refusal, not a spawn counter | Residual inference | Acceptable per plan |
| medium | Hosted `CI / verify` run `38643674863` still fails three pre-existing `tests/git_recovery_integration.rs` tests (merge/rebase recovery demanding `BackendUnavailable[CleanSmudgeFilter, MergeDriver, Hook]`), locally four counting `recovery_skip_during_rebase_skips_step` | Workspace CI cannot go fully green on these fixtures | Pre-existing at the `origin/main` base by construction (this diff touches no production file in the recovery path); merge/rebase recovery with repository-controlled executable components requires the contained backend owned by Linux-isolation C002. Separate test-contract/contained-backend corrective on the C002 line; do NOT fold into this C002 |

No critical/high/medium findings. The `git_network_integration` binary is fully green for the first time since the typed-policy line landed.

## 11. Roadmap disposition

Milestone C002 closed for its bounded remote-reconciliation scope. Together with sibling C001, the `git-reset-remote-test-contract-ci-corrective` workstream is complete and its roadmap moves to closed. Linux C003, M005/M006, ADR-0027, and the M001 native backend remain blocked exactly as before — this closure promotes none of them and qualifies no file remote.

## 12. Registry updates

- `plans/registry.md` Active subsystem roadmaps: reset/remote row → `closed` (C001 + C002 closed at `7922bb0e`; closures `plans/closure/git-reset-remote-test-contract-ci-corrective/001-status.md`, `002-status.md`).
- `plans/registry.md` Dependency-ready plans: C001 row → `closed`; C002 row → `closed`.
- `plans/registry.md` Recently closed: two rows added for C001/C002.
- `plans/subsystems/git-reset-remote-test-contract-ci-corrective-addendum.md`: Status → `closed — C001/C002 closed`.
- Implementation plans C001/C002: Status → `implemented; closed`.
- Unblock audit (per `plans/003-planning-process.md` §11): no registered plan lists C001/C002 as a hard/interface dependency; Linux M001/C002/C003 remain blocked on ADR-0027 + native proof + egress/lease interfaces. **Nothing newly unblocked — recorded explicitly.**
