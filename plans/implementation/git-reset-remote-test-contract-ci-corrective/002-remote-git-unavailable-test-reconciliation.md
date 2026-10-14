# Git Reset/Remote Test-Contract C002 — Remote Git Unavailable Fixtures

Status: implemented; closed (implementation `7922bb0e`; closure `plans/closure/git-reset-remote-test-contract-ci-corrective/002-status.md`; hosted `CI / verify` run `38643674863` — scope green, residual recovery red classified as pre-existing Linux-C002 scope)

Baseline: `1cd71e4c` (branch head before implementation).

Source: `plans/subsystems/git-reset-remote-test-contract-ci-corrective-addendum.md` (C002); remote recovery contract `plans/implementation/linux-execution-isolation/011-m001-authorized-remote-git-recovery.md` (Linux C003, blocked — owns the positive path); test-contract precedent `plans/implementation/git-clean-authorization-ci-corrective/001-typed-git-clean-test-authorization.md`. Accepted ADR-0028; `plans/000-long-term-specification.md` §§4,17,27–29; `plans/003-planning-process.md` §§2,7.
Primary class: **invariant**.

## 1. Objective

Reconcile the three stale remote expectations in `tests/git_network_integration.rs` with the existing Linux containment roadmap: `fetch`/`push`/`pull` against a local bare remote without a contained remote backend must return the exact typed `BackendUnavailable[RemoteHelper, Network, Credential]` outcome before any child is launched — asserted as the **unavailable-capability truth**, never as a successful operation and never as a pass-through. The positive remote path stays owned by Linux C003.

## 2. Why this milestone is ready

No hard dependencies: the typed remote classification (`classify_git_request` → `RequiresContainedExecution[RemoteHelper, Network, Credential]` for `fetch`/`pull`/`push`/`clone` regardless of URL scheme → `BackendUnavailable` via `ensure_trusted_git_request`) is closed under Linux C001, and the local bare-remote fixtures exist on the baseline. **Interface** dependency on Linux C003 for the positive-path contract only: C003 is blocked (C001, M005/M006, transport policy), so this C002 asserts current unavailability and cites C003 without consuming any capability from it. Operational dependency: final hosted Linux CI result.

## 3. Current implementation evidence

- `tests/git_network_integration.rs::executor()` threads no authorization and no contained backend.
- `fetch_from_local_bare_remote_succeeds`, `push_to_local_bare_remote_succeeds`, `pull_from_local_bare_remote_succeeds` use `fresh_pair()` (local bare remote via raw `git` CLI setup — fine, outside the executor) then call `git_network_ops::fetch` / `push` / `pull` through the executor and assert `res.success`. Locally each fails with `Policy(BackendUnavailable { capabilities: [RemoteHelper, Network, Credential] })`.
- `classify_git_request` gates `fetch|pull|push|clone|ls-remote` before any scheme check: a `file://` local path is **not** an allowlist. Linux C003 §6 explicitly reserves file-remote qualification as future work ("File/local remotes can be qualified independently of networked transports" — not yet qualified). Repository-controlled remote helpers, `url.*.insteadOf` rewrites, `core.sshCommand`, and credential helpers apply to local remotes too, so treating a local path as safe would be a policy weakening.
- `crates/egggit/src/process.rs` pins `fetch origin` → `RequiresContainedExecution[RemoteHelper, Network, Credential]` but has no `push`/`pull` pins. Related tests (`push_force_is_marked_destructive`, redaction/classifier tests) cover request shapes and URL hygiene, not execution, and are unaffected.

## 4. Invariants that must not regress

- Remote Git destinations, protocols, and credentials are authorized by principal/workspace/run policy, never by URL spelling or local-path convenience.
- Unavailability is terminal and typed: no fake executable, no generic spawn failure, no automatic direct-network fallback, no retry that upgrades authority.
- Denial precedes child launch: local HEAD, tracking refs, and the bare remote's refs byte-identical before/after; no audit success event on the `Policy` path.
- No `#[cfg(test)]` bypass, fabricated credential broker, proxy env injection, `GIT_SSH_COMMAND` smuggling, or classifier weakening for `file://` remotes.
- No claim that Linux C003, M005/M006, SSH brokerage, or file-remote qualification has become supported.

## 5. Scope (in / out)

In: replace the three stale remote positives with typed-unavailable fixtures through the same public `git_network_ops::fetch` / `push` / `pull` API; `push`/`pull` classifier pins beside the existing `fetch` pin; test comments citing Linux C003 as the positive-path owner.

Out: no production policy change; no credential broker, mediator, SSH/HTTPS transport, or file-remote allowlist; no Linux C003 scope (that plan owns positive remote execution and stays blocked).

## 6. Required production changes

None. Test-only plus pure classifier unit fixtures (no process).

### A. Fixture reconciliation (required)

- Replace `fetch_from_local_bare_remote_succeeds` with `fetch_without_contained_remote_backend_returns_typed_unavailable` (or equivalent): identical `fresh_pair()` setup via raw git; capture local `HEAD` and `origin/main` SHAs; call the identical public API; assert `expect_err` with **typed** `GitMutationError::Policy(BackendUnavailable { capabilities: [RemoteHelper, Network, Credential] })`; assert both SHAs byte-identical after (no fetch occurred, no tracking-ref movement).
- Replace `push_to_local_bare_remote_succeeds` with `push_without_contained_remote_backend_returns_typed_unavailable`: identical setup + local commit via raw git; capture the bare remote's `main` SHA (`git --git-dir=<remote> rev-parse main`); assert the same typed refusal with the full capability triple; assert the remote SHA identical after (nothing pushed).
- Replace `pull_from_local_bare_remote_succeeds` with `pull_without_contained_remote_backend_returns_typed_unavailable`: identical setup (push + local reset via raw git); capture local `HEAD`; assert the same typed refusal; assert `HEAD` identical after (nothing fetched or integrated).
- Name and comment each fixture as asserting **unavailability**, not success: an unavailable capability must read as a terminal typed outcome in test output, never as `ok` masking a skipped operation. Keep TempDir fixtures unique; keep env hardening.

### B. Honest positive disposition (strictly gated)

The positive path (mediated HTTPS/SSH/file-remote Git with credential brokerage) is owned by Linux C003, blocked on C001 + M005/M006 + transport policy. Do **not** qualify file remotes here, do not add a test-only transport, and do not downgrade the classifier for local paths. Record the positive as blocked on Linux C003 in comments and the closure. If C003 later lands a real backend, that landing must update these fixtures rather than coexist with a stale unavailable assertion (addendum dependency graph records this).

### C. Regression pins

Extend the `egggit::process` classifier tests: pin `push origin main` and `pull origin main` (and representative flag variants) to `RequiresContainedExecution[RemoteHelper, Network, Credential]` alongside the existing `fetch` pin, plus `apply_sync` refusal for one remote argv. Changing remote gating is a policy change, not a test update.

## 7. Ordered work packages

1. **Rebaseline** the three tests, the typed `BackendUnavailable` outcome, and the no-backend seam; confirm file remotes are unqualified on this branch.
2. **Unavailable fixtures**: replace all three positives with typed-unavailable + state-invariance assertions.
3. **Classifier pins**: add `push`/`pull` containment pins.
4. **Roadmap reconciliation**: record the three fixtures' ownership under Linux C003 in the Linux roadmap's CI test-contract note (link, not scope change) and cite it from test comments.
5. **Qualify**: focused remote tests + full `git_network_integration` (must be fully green together with C001) + origin/guard suites + quick + Clippy; hosted CI on the final SHA.
6. **Close** in `plans/closure/git-reset-remote-test-contract-ci-corrective/002-status.md` without touching Linux C003's blocked status.

## 8. Failure, cancellation, restart, contention semantics

Unavailability is a typed pre-launch result with no local/remote state mutation and no automatic retry, fallback transport, or FullHost escalation. Unique TempDir fixtures; no shared mutable policy; no live credentials, no open internet (bare `file://` fixtures only).

## 9. Compatibility and migration

No database, wire, config, tool-schema, or API change. `GitExecutionPolicyOutcome` unchanged. Do not rewrite historical closures; do not mark Linux C003 (or any M005/M006 gate) unblocked.

## 10. Required tests and commands

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

## 11. Documentation updates

Test comments citing Linux C003 (`011-m001-authorized-remote-git-recovery.md`) as the positive-path owner and stating file remotes are not yet qualified. One short note in the Linux roadmap's CI test-contract section recording the reconciliation (ownership link only — no milestone status change). No `architecture/git.md` behavioral change.

## 12. Acceptance criteria

- No-backend `fetch`/`push`/`pull` return exactly `BackendUnavailable[RemoteHelper, Network, Credential]`; no child dispatch; local and remote refs unchanged.
- Unavailable reads as unavailable: fixture names, asserts, and comments never present the outcome as a successful operation.
- No test-only transport, credential, or `file://` bypass; positive remote execution explicitly deferred to Linux C003.
- `git_network_integration` fully green together with C001; guards pass; hosted CI recorded.

## 13. Stop conditions

Stop if reconciling the tests requires a transport, credential broker, proxy env, SSH/askpass smuggling, `file://` allowlist, or replacing the typed unavailability with a generic spawn failure or a faked success. Those belong to Linux C003, not this contract corrective.

## 14. Closure evidence required

Write `plans/closure/git-reset-remote-test-contract-ci-corrective/002-status.md` with exact SHA, before/after fixture matrix, unavailability + invariance evidence, positive-path disposition (blocked on Linux C003), roadmap-reconciliation diff, focused commands/outcomes, hosted CI link, unrelated blockers, and final recommendation. Preserve historic closures.
