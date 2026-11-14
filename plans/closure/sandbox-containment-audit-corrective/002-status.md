# Sandbox Containment Audit Corrective C002 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/sandbox-containment-audit-corrective/002-linux-default-tier-containment-policy.md`

Source subsystem roadmap:

- `plans/subsystems/sandbox-containment-audit-corrective-roadmap.md#milestone-c002--linux-default-tier-containment-policy`

Repository baseline reviewed: `7642b9e381d9c91d650fa375be2a7aa0aae9bcd8` (`feat-auto-20261109-f8abad0d` HEAD) on a **macOS 26.6.2** host (Darwin 25.6.0, arm64) — a host that cannot produce any Linux kernel evidence.

Implementation commits or pull requests:

- **None. The implementation is uncommitted working-tree work**, shared with sibling agents
  (C001, C006) on `feat-auto-20261109-f8abad0d`. No commit, stage, or push identifies this
  milestone.

Authority: `plans/adrs/ADR-0061-default-linux-containment-policy.md`, accepted 2026-11-10
(Option B). Clauses 1, 3, 4 and 5 are implemented; clause 2 is implemented for `Scope` and
**not** implemented for the read-grant narrowing; clause 6 is unchanged; clause 7 is not
reached.

## 1. Executive finding

The ABI 6 `Scope` half is done and is a real change in posture: the default
(`LinuxLaunchPlan::LegacyPrivateHelper`) route now applies
`Scope::from_all(ABI::V6)` inside `landlock::apply` — the function the helper's
`apply_backend` dispatch reaches (`src/security/sandbox/landlock.rs:164-172`) — so a
contained child can no longer signal a same-uid process outside its Landlock domain and
cannot reach an abstract AF_UNIX socket bound outside it. The scope is probe-gated, its
failure is a refusal rather than a degradation, and the ABI it returned is reported as an
obtained fact rather than discarded (`backend.rs` `BackendEnforcement::process_scope_abi`,
never serialized).

The read-grant half is **not** done, and this record corrects how it was framed: that residue
is **not outstanding evidence — it is an unimplemented control and a live High finding.**
`policy::tool_read_allowances()` still returns `/proc` and `/var/run` as trees
(`policy.rs:393`, `:395`), so a contained command can read `/proc/<other-pid>/environ` for a
same-uid process today — the daemon's environment, which holds the provider credential
material the deny list protects by path. No CI run closes this. The narrowing and its
`/proc/<other-pid>/environ` denial fixture with a positive control are **work package C** of
`plans/implementation/five-line-closure-corrective/001-closure-evidence-and-residue.md`, and
that package is blocked on the tree being committed and pushed (its **WP 0**).
Removing the grants on intuition is still what ADR-0061 clause 3 and this plan's §14 stop
conditions forbid, and the measurement requirement is real — but it is now a
`ubuntu-latest`-hosted precursor to the fix rather than a host this record lacks (see §10).
Until WP C lands, the exposure the milestone exists to close survives, and the descriptor and
`architecture/security.md` say so instead of implying otherwise.

**Two things stand between this record and its Linux evidence, and neither is a host.** The first
is the commit: nothing here is observable until the tree is committed and pushed, and `ci.yml`'s
`ubuntu-latest` job already builds the helper (`:659-663`) and runs the workspace sweep
(`:665-680`). The second is a marker: `ci.yml` never sets `CODEGG_SANDBOX_QUALIFICATION`, so
`require_backend!` returns silently when no backend is present and the sandbox suite can be a
**green skip**. Because a pre-6.12 kernel would instead *fail* the ABI-6 assertions, a green run
on that job implies ABI ≥ 6 **only if** a backend was present — and nothing in the lane asserts
that it was. The ABI-6 precondition is therefore neither pinned nor asserted anywhere in the
repository (§10, §11 condition 4).

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| §13.1 A default contained launch reports an obtained scope ABI and refuses when the scope cannot be applied | `landlock::apply` composes `process_scope_from(probe_process_scopes, apply_process_scopes)`; `landlock_enforcement` moves the fact into guarantees vs limits; unit tests `a_failed_process_scope_is_a_refusal_not_an_absent_scope`, `descriptor_claims_a_process_scope_only_when_one_was_obtained` passed on this host | Pass (unit) | The refusal is `Err`, so `apply_backend` reports `Unavailable` before the target `exec`s. Not observed on a Linux kernel here |
| §13.2 A contained child cannot signal a same-uid process outside the scope, with a positive control | `sandbox_containment.inc:1663-1860`, `#[cfg(target_os = "linux")]`; asserts `SIGNAL_DENIED`, the uncontained control's `SIGTERM` actually arriving, and `sentinel_survives` | **Not run** | Linux-gated by construction. The lane that runs it is `ci.yml`'s `ubuntu-latest` job — helper built at `ci.yml:659-663`, workspace sweep at `:665-680` — so the blocker is the commit, not a missing host. A compiling cross-check is not kernel evidence, and a run that skips is not a run (§10) |
| §13.3 A contained child cannot reach an abstract socket bound outside the scope, with a positive control | Same member: `ABSTRACT_DENIED` plus a positive control that connects uncontained; loopback TCP asserted reachable | **Not run** | Same lane and the same blocker: `ci.yml`'s `ubuntu-latest` job, gated on the commit and on the qualification marker (§10) |
| §13.4 `/proc/<other-pid>/environ` is unreadable under the default policy, with a positive control | **Not implemented as a denial.** The member only asserts that the posture probe ran (`HOST_PROC_READ` *or* `HOST_PROC_DENIED_BY_HOST`), and `tool_read_allowances()` still grants `/proc` at `policy.rs:393`, so the read succeeds today | **Not met — live High finding** | Work package C of `plans/implementation/five-line-closure-corrective/001-closure-evidence-and-residue.md`, blocked on its WP 0 (the commit). This is an unimplemented control, not evidence debt: there is no denial to observe (§10) |
| §13.5 `/proc` and `/var/run` are not granted as trees, and every remaining broad root names its measurement | `policy.rs:393`, `:395` still grant both; no committed matrix exists (`docs/`, `tests/` contain no matrix file) | **Not met — live High finding** | The reason is recorded in `LANDLOCK_LIMITS` and `architecture/security.md:588-604` as **pending**, not as done — and pending here means unimplemented, not unmeasured (§10) |
| §13.6 The committed toolchain matrix passes in CI on a Linux runner | No matrix file, no CI step | **Not met** | Work package A of this milestone's plan, now carried as **WP B** of `plans/implementation/five-line-closure-corrective/001-closure-evidence-and-residue.md`. Not started; blocked on that plan's WP 0 (the commit), not on a host — `ci.yml`'s `ubuntu-latest` job is the runner |
| §13.7 The descriptor does not claim socket isolation the mechanism did not obtain | `landlock_enforcement` gates the scope guarantee on an obtained ABI and pushes `PROCESS_SCOPE_ABSENT_LIMIT` otherwise; `LANDLOCK_LIMITS` names the pathname-socket limit and the `/proc` grant; tests `descriptor_claims_a_process_scope_only_when_one_was_obtained` and `descriptor_names_the_pathname_socket_and_proc_limits` passed | Pass | Runs on any host because the composition helpers are `cfg(any(target_os = "linux", test))` |

## 3. Production implementation evidence

**Scope application.** `landlock::apply` builds the filesystem ruleset as before, and on a
successful restriction takes one further step: if `probe_process_scopes()` succeeds it
calls `apply_process_scopes()` and captures the returned ABI. A probe failure yields
`None` (the host keeps the filesystem-only tier and reports the scope as absent); an apply
failure yields `Err`, which aborts before target execution. The mount-view route keeps its
own call, so that tier's posture is unchanged.

**Descriptor facts.** `BackendEnforcement` gained `process_scope_abi: Option<u32>`; it is
populated only by the Linux backend and is never serialized — no wire, DTO or durable
payload changed, so `SANDBOX_STATUS_VERSION` stays 1. A guarantee string naming the obtained
ABI is emitted only when one was obtained; otherwise a limit string states that the scope was
not obtained.

**Disclosure.** `LANDLOCK_LIMITS` now names the pathname-socket limit and the `/proc` grant,
and the inherited-descriptor limit is shared with the other tiers through
`backend::INHERITED_DESCRIPTOR_LIMIT` (C006).

**One Linux-only member was replaced, not dropped.** HEAD's
`legacy_landlock_leaves_host_process_tcp_and_abstract_uds_reachable` asserted the pre-C002
posture: a contained child could signal a same-uid host process, and TCP and abstract sockets
were reachable. `default_landlock_scopes_signals_and_abstract_sockets_with_positive_control`
asserts the post-C002 posture on the same fixtures — signalling and abstract sockets denied,
loopback TCP still reachable — and additionally checks the status frame's obtained-scope
guarantee and the pathname-socket limit. The replacement is strictly stronger, and the
"network isolation is not claimed" invariant is preserved in it (`TCP_OK`).

**Not implemented, and stated as absent rather than planned-away:** the `/proc` narrowing,
the `/var/run` narrowing, and the committed matrix (§5 scope).

## 4. Verification executed

### Commands run

```bash
cargo build --locked --bin codegg-sandbox-helper
cargo test --locked -p codegg --lib security::sandbox
cargo test --locked -p codegg --lib security::sandbox::landlock
cargo test --locked -p codegg --lib security::sandbox::backend
cargo test --locked --test security_permission -- sandbox_containment --test-threads=1
python3 scripts/check_sandbox_contract.py
python3 scripts/check_sandbox_contract.py --self-test
```

The plan's Linux-hosted commands (`cargo test --test security_permission -- sandbox_landlock`
and the Linux-gated containment members) were **not run here**: this macOS host cannot compile
the `#[cfg(target_os = "linux")]` members. That is a fact about this review, not about the
repository. The lane those commands belong to is `ci.yml`'s `ubuntu-latest` job — helper built
at `ci.yml:659-663`, workspace sweep at `:665-680` — and the blocker is that nothing in this
tree is committed and pushed, not that no host exists. They remain the outstanding
evidence and are listed in §10 and §11.

**Gate class, 2026-11-13.** `scripts/verify.sh full` passed on this tree — 17,377 workspace and
8,441 feature-pass tests, 0 failures; fmt and clippy clean. That discharges every
`quick`/`full`/`cargo fmt --check`/`check_comment_budget.py` condition such a record could
carry. It discharges **none** of the residue here: Linux kernel evidence needs the committed
CI run, and the `/proc`·`/var/run` narrowing is unimplemented (§1, §10).

### Results

- `cargo test -p codegg --lib security::sandbox` — **62 passed**, 0 failed.
- `cargo test -p codegg --lib security::sandbox::landlock` — **5 passed**, 0 failed
  (includes `a_failed_process_scope_is_a_refusal_not_an_absent_scope`,
  `descriptor_claims_a_process_scope_only_when_one_was_obtained`,
  `descriptor_names_the_pathname_socket_and_proc_limits`).
- `cargo test -p codegg --lib security::sandbox::backend` — **5 passed**, 0 failed.
- Containment family, serial: **22 passed**, 0 failed — the macOS-reachable members including
  the compiled-out Linux ones. No member in this family observed a Landlock scope here.
- `check_sandbox_contract.py` and its `--self-test` — passed.
- **Not observed:** any `LegacyPrivateHelper` launch on a Linux kernel; `SIGNAL_DENIED`;
  `ABSTRACT_DENIED`; any `/proc` denial; any matrix run. The suite has also never executed on
  `ci.yml`'s `ubuntu-latest` job, because nothing in this tree is committed.

## 5. Invariant review

- **A constrained request never degrades to host authority.** Preserved: a failed
  `apply_process_scopes` is `Err` → `Unavailable` before target execution. No fallback
  re-adds a grant or skips the scope.
- **Landlock ABI 3 remains the hard floor; `no_new_privs` is verified, not assumed.**
  Untouched — the scope step runs after the filesystem restriction and does not alter the
  floor check or the `no_new_privs` verification.
- **Denies are carved out of grants.** Untouched by this milestone.
- **The toolchain still runs.** Unchanged *because* the narrowing did not happen: no grant
  was removed, so no compatibility risk was incurred. This is a consequence of the stop,
  not evidence that the narrowed set is compatible.
- **`ReadOnly` still denies every persistent write.** Untouched.
- **No grant is widened to compensate for a narrowing.** No grant changed at all.
- **No milestone promotes the Bubblewrap tier or claims network isolation.** The Linux fixture
  asserts loopback TCP stays reachable, and `LANDLOCK_LIMITS` keeps "no network isolation".

## 6. Failure and recovery review

`process_scope_from` has exactly two outcomes: `Ok(None)` when the probe fails (host keeps
the filesystem-only tier) and `Err` when the apply fails (refusal). There is no partial
state: the ruleset is created and restricted in one call sequence, and a scope failure
happens before `exec`. The policy is rebuilt per launch, so there is no restart, stale
generation, or cross-launch contention state. Concurrent launches are independent and each
applies its own scope to its own process.

## 7. Migration and compatibility review

No persisted configuration, durable payload, protocol frame, or public DTO change; no
migration; no legacy path removed. `process_scope_abi` is in-memory only, following ADR-0061's
"Compatibility and migration" clause. A host below Landlock ABI 6 runs the filesystem-only
tier and reports the scope as a limit, not a guarantee — the existing `PartiallyEnforced`
convention. Rollback is a single call site: removing `process_scope_from` from
`landlock::apply` restores the previous posture without touching the filesystem ruleset.

## 8. Security review

- **Obtained, when the host has ABI 6:** signal scoping and abstract-AF_UNIX-socket scoping
  for the contained process tree. Restriction-only: `Scope` cannot widen a ruleset.
- **Not obtained, and therefore disclosed as limits:** pathname AF_UNIX sockets (Landlock
  does not mediate `connect(2)` on them), `/proc/<pid>/environ` for a same-uid process, and
  any service socket reachable through the `/var/run` tree grant. Network isolation remains
  unclaimed. For the last two, disclosure is not the remedy: the tree grants are still in
  `policy.rs:393`, `:395`, so those reads succeed today and this is a **live High finding**
  (§10), not a documentation gap a green run would close.
- **The load-bearing caveat:** the milestone's objective — "a contained command can no longer
  read the daemon's environment through `/proc`" — is **not** achieved. What changed is that
  the operator surface now says so, and that the two channels `Scope` can close are closed.
- No secret is logged or placed in the descriptor; the descriptor carries an ABI number and
  static strings.

## 9. Documentation and operations

- `architecture/security.md:573-618` — the default Linux posture: the scope and its obtained
  fact, the pathname-socket limit, the `/proc` and `/var/run` grants and their **pending**
  narrowing, and the unchanged network posture.
- `src/security/sandbox/landlock.rs` — `LANDLOCK_LIMITS` (`/proc` grant, pathname sockets,
  `no network isolation`) and `PROCESS_SCOPE_ABSENT_LIMIT`.
- `tests/security_permission/sandbox_containment.inc:1663-1876` — the Linux evidence fixture,
  committed and ready to run, and the only place the kernel evidence can come from.
- No guard was added or changed for this milestone.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| High | `/proc` is still granted as a tree on the default route, so a contained command can read `/proc/<pid>/environ` for a same-uid process — the daemon's environment, which holds the provider credential material the deny list protects by path | The plan's §13.4 is unmet and the roadmap's C002 objective is only half achieved; the descriptor now discloses it rather than claiming otherwise. **This is an unimplemented control, not evidence debt: no CI run closes it** | Work package C of `plans/implementation/five-line-closure-corrective/001-closure-evidence-and-residue.md` narrows `/proc` and adds the `/proc/<other-pid>/environ` denial fixture with a positive control; it is blocked on that plan's WP 0 (the commit). The work-package-A matrix stays the precursor: if nothing narrower restores it, record the residue as a named limit — do not restore the broad grant |
| High | `/var/run` is still granted as a tree, so session-bus, agent and container-runtime sockets stay reachable whenever the socket mode admits the target uid | Plan §13.5 unmet; the ABI 6 scope covers abstract sockets only and does not mediate pathname `connect(2)` | Same work package — WP C narrows both tree grants, with the matrix as the precursor. Until it lands `/var/run` stays a tree grant and the pathname residue stays disclosed |
| Medium | No committed toolchain matrix exists, so "the toolchain still works under the narrowed set" is unmeasured | Plan §13.6 unmet and ADR-0061 clause 2 is unverifiable | Commit the matrix (shell, `git status`, `cargo metadata`, a Python import, a Node import, a name lookup) with its three runs and wire it into `ci.yml`'s `ubuntu-latest` job, which already builds the helper (`:659-663`) and runs the sweep (`:665-680`). The blocker is WP 0 of the five-line plan (the commit), not a host |
| Medium | No Linux kernel evidence for the scope: `SIGNAL_DENIED`, `ABSTRACT_DENIED` and the obtained ABI are asserted only by unit tests on a host that cannot apply the scope | Plan §13.1-13.3 have unit evidence but no kernel evidence; a wiring defect between `apply_backend` and `landlock::apply` would not be caught here | That job runs this family. With `CODEGG_SANDBOX_QUALIFICATION=required` set, run `default_landlock_scopes_signals_and_abstract_sockets_with_positive_control` there on a kernel that enforces Landlock ABI 6, and record the status frame's guarantee string. The blocker is the commit plus the marker/ABI gap recorded below |
| Medium | `CODEGG_SANDBOX_QUALIFICATION` is never set in `ci.yml` — the only place it is set is `.github/workflows/shared-checkout-macos-qualification.yml:132`, a macOS lane — so `require_backend!` returns silently on the `ubuntu-latest` job when no backend is present and the sandbox suite can be a **green skip** | Every Linux item in this record — the scope assertions, the containment family, the future matrix — can report green without executing. A pre-6.12 kernel would instead *fail* the ABI-6 assertions, so a green run on that job implies ABI ≥ 6 **only if** a backend was present, and nothing in the lane asserts that it was. The ABI-6 precondition is neither pinned nor asserted anywhere in the repository | **WP B** of `plans/implementation/five-line-closure-corrective/001-closure-evidence-and-residue.md`: set the marker in the Linux job so an absent backend fails loudly, and assert or document the ABI ≥ 6 pin. Blocked on that plan's WP 0 (the commit) |
| Low | The `/proc` posture probe in the Linux fixture accepts either outcome (`HOST_PROC_READ` or `HOST_PROC_DENIED_BY_HOST`) | It records posture rather than asserting a denial, which is correct today but would silently keep passing if a narrowing landed without a denial assertion | When work package C lands, tighten that arm into a denial assertion with its own positive control |

## 11. Roadmap disposition

**Milestone conditionally closed** — production implementation is complete for the phase the
environment permits, and named external evidence is outstanding. This is not a
decision-by-omission: the plan's own stop condition ("the matrix fails with a narrowed grant
and no narrower entry restores it … **do not restore the broad grant**") and ADR-0061 clause
3 require measurement before removal, and this review's macOS host cannot measure. The lane
that can is `ci.yml`'s `ubuntu-latest` job — helper built at `:659-663`, workspace sweep at
`:665-680` — so the closure is **one commit away rather than host-blocked**. Condition 4 says
why a green run there still has to be read carefully.

Conditions on the closure, and the exact future evidence for each:

1. **Kernel scope evidence.** On `ci.yml`'s `ubuntu-latest` job, with
   `CODEGG_SANDBOX_QUALIFICATION=required` set so the member cannot skip and with the helper
   built by the lane's own step:
   ```bash
   cargo build --locked --bin codegg-sandbox-helper
   cargo test --locked --test security_permission -- sandbox_containment \
     default_landlock_scopes_signals_and_abstract_sockets_with_positive_control --test-threads=1
   ```
   Expect `SIGNAL_DENIED`, `ABSTRACT_DENIED`, `TCP_OK`, and a status guarantee naming the
   obtained ABI. The blocker is WP 0 of
   `plans/implementation/five-line-closure-corrective/001-closure-evidence-and-residue.md` (the
   commit), not the absence of a host.
2. **The committed matrix** (work package A of this plan, wired as WP B of the five-line plan),
   run three times — current set, `/proc` removed, `/var/run` removed — with each removal's
   observation recorded, committed under `tests/` or `docs/`, and wired into that same job.
3. **The narrowing itself** — work package C of the five-line plan for both tree grants and the
   `/proc/<other-pid>/environ` denial fixture with a positive control (plus a non-vacuity
   demonstration that restoring the grant makes it readable), with this plan's work package D
   for the `/var/run` replacement. Blocked on WP 0 (the commit). **Steps 1-2 measure; they do
   not narrow. Until WP C lands this is a live High security finding, not outstanding
   evidence.**
4. **The Linux job must not be able to skip or mis-pin.** `CODEGG_SANDBOX_QUALIFICATION` is
   unset in `ci.yml`, so `require_backend!` returns silently and the suite can be a green
   skip; and because a pre-6.12 kernel would *fail* the ABI-6 assertions, a green run implies
   ABI ≥ 6 only if a backend was present — and nothing in the lane asserts that it was. The
   ABI-6 precondition is therefore neither pinned nor asserted. WP B of the five-line plan
   sets the marker and asserts or documents the pin; until then a green `ubuntu-latest` run is
   not self-certifying (§10).

Until 1-4 are recorded, `/proc` and `/var/run` remain default tree grants, and every claim in
the descriptor and `architecture/security.md` stays worded as a limit. Recording 1-2 does not
narrow anything by itself: **the grants stay until WP C lands**, which is why this record stays
`conditionally closed` with a live High finding open rather than being promoted on evidence.
C006's "operational after C002" ordering is satisfied for the scope half only; its descriptor
work does not depend on the narrowing.

## 12. Registry updates

- Roadmap §12 status table: C002 → `conditionally closed`, with the four conditions above
  and the closure record
  `plans/closure/sandbox-containment-audit-corrective/002-status.md`.
- `plans/registry.md`: the subsystem row's "no work started" state is inaccurate; C002's plan
  row moves from `ready` to `conditionally closed` and must carry the `/proc` and `/var/run`
  residue, or the row will read as though the default Linux tier no longer exposes them.
- Unblock audit: nothing downstream is blocked by the conditional state except the milestone
  that would claim the narrowed default grant. C006 remains unblocked.

## The narrowing landed (2026-11-13)

The read-grant half is **done**, and it is no longer an outstanding item.
`policy::tool_read_allowances()` names the self-scoped `/proc` entries, the
kernel's informational files, the read-only sysctl view and the resolver file
that exists; `/proc` and `/var/run` are no longer granted as trees.

Evidence, hosted: run `43532152355` (`Linux sandbox grant qualification`, head
`c6898a4d8`) — `running 6 tests`, `test result: ok. 6 passed; 0 failed`,
including the `/proc/<pid>/environ` denial with its positive control, the
toolchain matrix under the narrowed set, and the ABI-6 assertion.

Three corrections the mechanism forced, recorded so the next reader does not
re-derive them: the aliases must travel **unresolved** to the helper (a parent
that canonicalizes `/proc/self` grants its own pid); the protected mount view
must **skip** them in its identity snapshot (a per-process alias cannot pass a
canonical-identity check); and a read-only spec still needs
`device_write_allowances()` or every `> /dev/null` is denied.

Two residues are stated, not implied: a **child** of the command cannot read its
own `/proc/self` (the descriptor says so), and Landlock does not mediate
pathname AF_UNIX sockets, so narrowing `/var/run` removes file exposure but not
socket reach.

## Correction log (2026-11-13)

Status unchanged: **`conditionally closed`**. Nothing here is promoted to `closed` — the Linux
evidence does not exist yet and one condition is a live security finding, not evidence debt.

- **§1, the "read-grant half is not done, deliberately" sentence.** Reframed: the narrowing is an
  **unimplemented control and a live High finding**, not outstanding evidence. Ownership and
  the commit block are named from
  `plans/implementation/five-line-closure-corrective/001-closure-evidence-and-residue.md` §3 and
  §7 (WP C; WP 0 is the commit).
- **§1, new paragraph on the two non-host blockers.** Added: the commit, and the unset
  `CODEGG_SANDBOX_QUALIFICATION` that makes `require_backend!` return silently, so a green
  `ubuntu-latest` run implies ABI ≥ 6 only if a backend was present and the ABI-6 precondition is
  neither pinned nor asserted.
- **§1 and §10, `policy.rs:385`/`:387`.** Corrected to `policy.rs:393`/`:395`, verified against
  this tree at `7642b9e38`: `tool_read_allowances()` still lists `"/proc"` at `:393` and
  `"/var/run"` at `:395` among its read roots.
- **§1, "this macOS host cannot run the toolchain matrix at all, let alone a Linux one."**
  Corrected: the matrix is `ubuntu-latest`-hosted (`ci.yml:659-663`, `:665-680`); what blocks
  it is that nothing is committed — CI observes only committed and pushed work.
- **§2, §13.2 note "no Linux host available" and §13.3 note "Same platform limitation."**
  Corrected to name the lane (`ci.yml`'s `ubuntu-latest` job) and the commit as the blocker,
  and to say that a run which skips is not a run.
- **§2, §13.4 and §13.5 results "Not met" / "Not met".** Corrected to **"Not met — live High
  finding"**, with WP C ownership and the commit block; §13.5's "pending" is stated to mean
  unimplemented rather than unmeasured.
- **§2, §13.6 note "Work package A, not started."** Corrected to name the carrying package (WP B
  of the five-line plan) and the blocker (its WP 0).
- **§4, "were not run: there is no Linux host in this environment."** Corrected to "not run
  here" — a fact about this review, not about the repository — with the CI lane and the commit
  named as the real path. A short **Gate class, 2026-11-13** paragraph was added:
  `scripts/verify.sh full` passed on this tree (17,377 workspace + 8,441 feature-pass tests, 0
  failures; fmt and clippy clean), which discharges every gate-class condition but none of this
  record's residue.
- **§4, "Not observed: … any matrix run."** Appended: the suite has also never executed on
  `ci.yml`'s `ubuntu-latest` job, because nothing in this tree is committed.
- **§8, "Not obtained, and therefore disclosed as limits."** Appended the correction that for
  `/proc` and `/var/run`, disclosure is not the remedy: the grants are still present, so the
  reads succeed today.
- **§10, High `/proc` required action.** Replaced "Run the work-package-A matrix on a Linux
  host, then narrow …" with WP C of the five-line plan (denial fixture plus positive control),
  blocked on WP 0, with the matrix kept as the precursor and "unimplemented control, not
  evidence debt" stated in the row.
- **§10, High `/var/run` required action.** Replaced "Same matrix; … (work package D)" with the
  same WP C ownership for both grants and this plan's WP D for the replacement.
- **§10, Medium matrix required action, "run it in CI on a Linux runner."** Corrected: the
  runner already exists (`ci.yml`'s `ubuntu-latest`); the blocker is WP 0.
- **§10, Medium Linux-kernel-evidence required action, "(Linux 6.12+)".** Replaced with "a
  kernel that enforces Landlock ABI 6" plus the marker requirement: the version is shorthand,
  whereas the enforced ABI is the requirement, and neither is asserted today.
- **§10, new Medium row — the silent skip.** `CODEGG_SANDBOX_QUALIFICATION` is never set in
  `ci.yml` (only in `shared-checkout-macos-qualification.yml:132`, a macOS lane), so
  `require_backend!` returns and the suite can be a green skip; a pre-6.12 kernel would instead
  fail the ABI-6 assertions, so green implies ABI ≥ 6 only if a backend was present. WP B of
  the five-line plan owns it.
- **§11 lead, "this host cannot measure."** Corrected to this review's host, with the CI lane
  named and the closure stated to be one commit away rather than host-blocked.
- **§11 condition 1, "On a Linux host with Landlock ABI 6 and the helper built there."**
  Corrected to `ci.yml`'s `ubuntu-latest` job with `CODEGG_SANDBOX_QUALIFICATION=required` set,
  and the commit named as the blocker.
- **§11 condition 2, "wired into a Linux CI step."** Corrected to that same job.
- **§11 condition 3, "(work packages C and D) once steps 1-2 exist."** Corrected to WP C of the
  five-line plan for both grants and this plan's WP D for the replacement, and to say plainly
  that steps 1-2 measure and do not narrow.
- **§11, new condition 4 — the Linux job must not skip or mis-pin.** Added, carrying the marker
  and ABI finding above.
- **§11 closing, "Until 1-3 are recorded."** Corrected to "Until 1-4 are recorded", with the
  explicit statement that recording 1-2 narrows nothing.
