# Managed Scratch Native Qualification C001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/managed-agent-scratch-native-qualification-corrective/001-linux-native-scratch-containment-qualification.md`

Source subsystem roadmap:

- `plans/subsystems/managed-agent-scratch-native-qualification-corrective-addendum.md#7-milestones`

Repository baseline reviewed: `e1e8dd341e1f416ff7c713174c3144dd21da8c66` on `plan/managed-scratch-spaces`
(the source plan was written against `fb84bee63dda0b1c3407f377c695db99ec78324b`; the two commits in
between are planning-only reconciliations, so the production baseline is unchanged)

Implementation commits or pull requests:

- `2597ec9e67f3c08df71c53b32e446be241719ca8` — `test(sandbox): qualify delegated scratch
  containment on native Linux` — adds the native Linux qualification fixture
  `tests/linux_native_scratch_containment.rs`. No production code changed.
- `ea400ff7` — `test(sandbox): correct the observation probe value vocabulary` — fixes a
  defect in the fixture's own assertions, found before the qualifying run (see §4).
- PR #128 (`https://github.com/dbowm91/codegg/pull/128`) carries both commits on
  `plan/managed-scratch-spaces`. It was **not** merged as part of this handoff.

## 1. Executive finding

The native Linux scratch containment boundary is **complete and qualified on
the filesystem dimension**. The delegated shared-scratch grant path is now
proven end to end on a real Linux kernel with a real Landlock enforcement:
an authenticated, admitted delegated run receives exactly the write roots its
frozen grant snapshot authorizes, and the kernel refuses every other scratch
root, the scratch parent, an unrelated host temporary directory, a
cross-boundary hard link or rename, and a symlink planted inside the
workspace that points out of it.

The milestone is closed because the evidence is obtained rather than
requested. The fixture asserts the **obtained** backend status — Landlock,
ABI >= 3, `no_new_privs` — from the same launch whose child behaviour it
observes, and it carries a defect-injection control that grants the
ungranted sibling artificially and observes the difference. The denials are
therefore not vacuous, and the "host cannot enforce" case is a *failed*
qualification rather than a skipped one.

Scope of the claim is deliberately narrow and is stated exactly: the
**filesystem** dimension of Required containment on a native Linux host.
This record makes no claim about process, IPC, or network isolation, none
about read confidentiality (the read policy deliberately exposes curated
toolchain roots), and none about Windows, where the Required backend is
still absent and is recorded as blocked in the sibling C002 closure.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| WP A: a native Linux host with available Landlock ABI 3+ | `tests/linux_native_scratch_containment.rs::native_linux_host_can_enforce_landlock`; CI run `39291133332` on `ubuntu-latest` | pass | The fixture asserts the backend is Landlock and that a host without it **fails** rather than skips. |
| WP B: host fixture — direct child writes under its private root and under a granted shared root | `required_launch_confines_scratch_to_the_granted_roots`; `model_owned_bash_tool_confines_scratch_to_the_granted_roots` | pass | `PRIVATE_WRITE=allowed`, `GRANTED_SHARED_WRITE=allowed`; the host-side view agrees (`private.txt`, `granted.txt` exist). |
| WP B: host fixture — cannot modify sibling scratch, scratch parent, unrelated host temp, credential roots, explicitly denied paths | same two tests | pass | `UNGRANTED_SIBLING_WRITE`, `FOREIGN_SESSION_WRITE`, `SCRATCH_PARENT_WRITE`, `HOST_TEMP_WRITE`, `CREDENTIAL_WRITE`, `HOST_TEMP_TRUNCATE`, `HARDLINK_OUT`, `RENAME_OUT`, `SYMLINK_ESCAPE` are all `denied`. |
| WP B: helper status `Enforced`, Landlock ABI >= 3, `no_new_privs` | `required_launch_confines_scratch_to_the_granted_roots` | pass | Asserted on `SandboxExecutionOutcome::Enforced { backend, abi, guarantees }` returned by the same launch. |
| WP C: real admitted integration through `JobSubmissionService` and `ManagedProcessService`, not a hand-written helper spec | `admit_delegated_run` drives the real `TaskTool` → `JobSubmissionService` → `JobScheduler`; the launch spec handed to the managed process names **only the workspace**, so every scratch write root is composed by the production owner from the authorization scope | pass | This is the load-bearing claim: the test cannot have written the scratch roots itself. |
| WP C: private temp vars and shared alias scope | observation `PRIVATE_TEMP_VARS=correct` (`CODEGG_SCRATCH_DIR`, `TMPDIR`, `TMP`, `TEMP` all equal the run-private root) and `SHARED_ALIAS_VAR=correct` (`CODEGG_SHARED_SCRATCH_HANDOFF` equals the granted root) | pass | Checked inside the contained child. |
| WP C: the running job owns its lease across the parent return, and releases/reclaims only after reaping | `the_attempt_lease_outlives_its_parent_and_blocks_reclaim_until_release` | pass | The lease is held after the admission call returned; a revoke while leased does **not** reclaim the directory; after the lease is released, the revoked root **is** reclaimed. |
| WP C: foreign-child replay / unknown grant denial | `an_unowned_or_unknown_scratch_grant_is_refused_at_admission` | pass | An unknown typed id, another session's scratch, and a path-like id are each refused at admission. |
| WP C: one sandbox setup failure that leaves the target unexecuted | `a_sandbox_setup_failure_leaves_the_target_unexecuted` | pass | A hard-linked denied path makes the exclusion unrepresentable; the launch fails with `SandboxFailed` and the sentinel file the target would have created does not exist. |
| WP C: no silent FullHost downgrade | `a_required_policy_refuses_a_containment_free_launch` | pass | A `WorkspaceWrite` authorization with `SandboxRequest::Disabled` fails with `SandboxFailed` and the target never runs. |
| Cancellation / no orphaned side effect | `a_cancelled_attempt_never_executes_the_target` | pass | A pre-cancelled attempt returns `CancelledBeforeSpawn` and creates no marker. |
| Non-vacuity: a test that would fail if a sibling write incorrectly succeeded | `an_over_broad_grant_is_observable_by_the_same_probe` | pass | An artificially broadened grant makes the *same* ungranted sibling writable, so the denials above are discriminating. |
| Frozen effective grant snapshot is immutable | `admitted_run_freezes_only_the_granted_scratch` | pass | A retry of the accepted invocation returns the same run and snapshot and creates no second run; widening the grant set on the same accepted invocation is refused. |
| §11 focused: `cargo fmt --all -- --check` | CI run `39291133332` step "Formatting" | pass | Also verified locally. |
| §11 focused: `cargo test -p codegg --test managed_scratch_job_admission --locked` | CI "Workspace tests" + local macOS run | pass | Local: 1 passed, 0 failed. |
| §11 focused: `cargo test -p codegg --lib scratch::tests --locked` | CI "Workspace tests" + local macOS run | pass | Local: 12 passed, 0 failed. |
| §11 focused: `cargo test -p codegg --test sandbox_containment --locked` | CI "Workspace tests" | pass | Existing macOS/Linux Landlock fixture suite remains green. |
| §11 focused: `cargo clippy --workspace --all-targets --locked -- -D warnings` | CI run `39291133332` step "Workspace Clippy" | pass | Also verified locally against the fixture. |
| §11 focused: `python3 scripts/check_sandbox_policy_wiring.py` | local | pass | "sandbox-policy-wiring guard passed" |
| §11 focused: `python3 scripts/check_execution_ownership.py` | local + CI step | pass | "execution-ownership guard ok" |
| §11 focused: `python3 scripts/check_scheduler_bypass.py` | local + CI step | pass | "scheduler-bypass guard ok" |
| §11 focused: `bash scripts/check-core-boundary.sh` | local + CI step | pass | "codegg-core boundary check passed" |
| §11 focused: `scripts/verify.sh quick` | local macOS | pass | Exit 0 on the source commit, including the workspace `cargo check --workspace --all-targets`. The M004 closure's pre-existing macOS failures are in the `server,plugins,lsp-test-support` feature tier, which `verify.sh quick` does not exercise, and they are unrelated to this milestone. |
| §11: "Reuse existing CI only" | CI run `39291133332` (the repository's own `CI` workflow, `workflow_dispatch` on `plan/managed-scratch-spaces`) | pass | No new workflow, benchmark, or hosted research infrastructure was added. |

## 3. Production implementation evidence

**No production code changed.** C001 is a qualification milestone, and the
gap it closes was an *evidence* gap, not a behavior gap: the delegated
shared-grcratch grant path was already implemented and already fail-closed,
but it had only ever been exercised on a macOS host, and the existing Linux
Landlock fixtures drove a hand-written helper launch spec rather than the
real admitted path.

The landed change is one new integration test target,
`tests/linux_native_scratch_containment.rs`, which exercises the existing
production seams:

- **Ownership / admission** — `TaskTool` (`src/tool/task.rs`) →
  `JobSubmissionService` → `JobScheduler` → durable `AgentRunRecord` in
  `codegg-core`'s `AgentRunStore`. The shared-grant snapshot is frozen at
  acceptance; the effective scope is rebuilt from that record with
  `AgentScratchScope::from_authorized_grants`, exactly the way
  `src/agent/worker.rs` rebuilds it for a delegated run.
- **Execution** — `ExecutionAuthorizationContext` carries the rebuilt scope
  into the real `BashTool` (`src/tool/bash.rs`) and the real
  `ManagedProcessService` (`src/managed_process.rs`), which launches the real
  `codegg-sandbox-helper` and returns the **obtained**
  `SandboxExecutionOutcome`.
- **Enforcement** — `src/security/sandbox/landlock.rs` compiles the effective
  `SandboxLaunchSpec` into a Landlock ruleset. The production owner, not the
  test, adds the run-private root and each granted shared root to the spec's
  read/write roots, and sets `CODEGG_SCRATCH_DIR` / `TMPDIR` / `TMP` / `TEMP`
  and `CODEGG_SHARED_SCRATCH_<ALIAS>`.
- **Lifecycle** — attempt leases (`AgentRunStore::acquire_scratch_attempt_leases`
  / `release_scratch_attempt_leases`) and `scratch::reclaim_shared_scratch_dir`
  decide whether a revoked shared root may be removed.

The fixture therefore **cannot** pass by writing the answer into a launch
spec: the spec it hands over names only the workspace, and the private and
granted scratch roots only become writable because the production owner
composed them from the durable grant snapshot. If the owner stopped
composing them, the child could not write its private root; if it composed
too many, the ungranted sibling would become writable and the defect-
injection control would show the difference.

## 4. Verification executed

### Commands run

Native Linux evidence — CI run `39291133332`
(`https://github.com/dbowm91/codegg/actions/runs/39291133332`), the
repository's existing `CI` workflow dispatched on `plan/managed-scratch-spaces`
at `ea400ff7`, runner `ubuntu-latest`:

```bash
cargo fmt --check --all
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --locked --bin codegg-sandbox-helper
cargo nextest run --workspace --locked --profile ci
```

Local macOS verification (the development host; **not** native Linux, so it
cannot stand in for the C001 evidence):

```bash
cargo fmt --all -- --check
cargo check --tests -p codegg --test zz_tmp_linux_check   # platform gate lifted, then removed
cargo clippy --tests -p codegg --test zz_tmp_linux_check -- -D warnings
cargo test -p codegg --test managed_scratch_job_admission --locked
cargo test -p codegg --lib scratch::tests --locked
python3 scripts/check_sandbox_contract.py
python3 scripts/check_sandbox_policy_wiring.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
bash scripts/check-core-boundary.sh
scripts/verify.sh quick
```

### Results

| Scope | Result | Environment |
|---|---|---|
| `cargo fmt --check --all` | pass | CI (native Linux) |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | pass | CI (native Linux) |
| `cargo nextest run --workspace --locked --profile ci` | pass — the new `linux_native_scratch_containment` target ran and every one of its cases passed on the Linux runner | CI (native Linux, `ubuntu-latest`) |
| Sandbox helper build for integration tests | pass | CI (native Linux) |
| `cargo test -p codegg --test managed_scratch_job_admission --locked` | 1 passed, 0 failed | local macOS |
| `cargo test -p codegg --lib scratch::tests --locked` | 12 passed, 0 failed | local macOS |
| `cargo check` / `cargo clippy` of the new fixture with the `target_os = "linux"` gate lifted | pass, no warnings | local macOS |
| `check_sandbox_contract.py`, `check_sandbox_policy_wiring.py`, `check_execution_ownership.py`, `check_scheduler_bypass.py`, `check-core-boundary.sh` | all pass | local macOS and CI |
| `scripts/verify.sh quick` | pass | local macOS |

**Not run, and why.** The C001 fixture's own assertions cannot run on the
macOS development host: the file is gated on `target_os = "linux"`, so it
does not exist there at all, and no macOS result may be read as C001
evidence. The native Linux run above is the evidence. No test in the new
target was skipped on the Linux runner: a Linux host that cannot obtain
Landlock fails the fixture by design rather than skipping it, so the
"unavailable" case cannot be reported as a pass.

**The native Linux run was dispatched, not pulled.** The `pull_request`
workflow did not start for the source commit during this session, so the
same existing `CI` workflow was dispatched explicitly on the branch ref.
A dispatch checks out the branch head rather than a synthetic merge commit,
which is the stricter placement for this evidence: the tree that ran is
exactly the tree that was reviewed, with no base-branch merge mixed in.

**A defect in the fixture's own assertions was found and fixed before the
qualifying run.** The first dispatched run (`39289232898`, at
`2597ec9e`) was superseded when a static cross-check of the observation
script against its assertions found that the two temporary-variable probes
emit `correct`/`wrong` while the assertions used the `allowed`/`denied`
helper — a value-vocabulary mismatch that could never pass. The defect was
in the fixture, not in the product: it was found by comparing every emitted
probe name and value vocabulary against every assertion, and it is fixed by
`ea400ff7`, which adds a dedicated `assert_correct` helper. Every emitted
probe is now asserted exactly once, with a matching value vocabulary. The
qualifying run is `39291133332` at `ea400ff7`. The superseded run's own
result is not cited as evidence for anything, and the fix is disclosed here
rather than folded silently into the record.

**Why the mismatch was not caught by the local macOS work.** The fixture is
gated on `target_os = "linux"`, so the development host cannot execute it;
the local pass was a type-check and a Clippy pass with the platform gate
lifted, plus a shell-level smoke run of the observation script that
confirmed the script executes and emits all thirteen probe keys. A
type-check cannot see a mismatch between a string a script prints and a
string a test asserts, and only a native run can execute the fixture. That
is the reason the plan requires native evidence, and it is recorded as a
process observation about this milestone, not as a product defect.

**Unrelated pre-existing failure, recorded not concealed.** The repository's
`main` branch push run `39282749528` is red for a reason unrelated to this
milestone: `crates/codegg-core/src/workspace_services.rs:156` trips
`clippy::unnecessary_sort_by` on the CI toolchain's newer clippy. That code
is not on this branch, and the Clippy step of run `39291133332` passed. The
finding is owned by the `main` line, not by C001.

**Toolchain drift.** The local toolchain is rustc 1.89.0; the CI runner
uses the current stable toolchain. The fixture was type-checked and linted
locally with the platform gate lifted, and natively on the CI runner; the
Linux-specific code path was therefore never executed on a non-Linux host.

## 5. Invariant review

- **`ScratchId`, session/run owner and persisted immutable grants remain
  authority; paths and aliases do not create grants.** Holds. The fixture
  builds its scope from the durable `AgentRunRecord`, refuses a path-like
  grant id, refuses another session's scratch, and refuses an unknown id.
- **Private `TMPDIR`/`TMP`/`TEMP`/`CODEGG_SCRATCH_DIR` never become globally
  writable shared temp.** Holds, and is now observed rather than asserted:
  the contained child reports all four variables equal to its own
  run-private root, while the granted shared root is exposed only under
  `CODEGG_SHARED_SCRATCH_HANDOFF`.
- **Required process fails before target launch if the backend cannot
  enforce the requested scope; no implicit FullHost downgrade.** Holds.
  `a_required_policy_refuses_a_containment_free_launch` proves the
  `ManagedProcessService` refuses a `WorkspaceWrite` authorization whose
  request disables containment, and that the target does not run.
- **A job/attempt lease must outlive all child I/O and prohibit cleanup
  during execution.** Holds.
  `the_attempt_lease_outlives_its_parent_and_blocks_reclaim_until_release`
  shows the lease surviving the parent's return, blocking reclamation of a
  revoked shared root, and permitting it only after the attempt is reaped.
- **Shared roots are explicit sandbox/tool roots and never replace the
  child's private temp variables.** Holds; see the temp-variable observation.
- **Claimed sandbox guarantees must match obtained restrictions.** Holds.
  The assertions are made against `SandboxExecutionOutcome::Enforced` with
  the reported ABI and guarantees, not against the requested profile.
- **Denial/error strings alone cannot establish obtained confinement.**
  Honored. Every containment claim in this record rests on the child's
  observed filesystem outcome plus the obtained backend status, never on an
  error message.
- **A missing capable host means blocked for evidence, not passed.** Holds.
  The fixture fails on a host without Landlock, and the file does not exist
  off Linux, so there is no path by which this milestone could be satisfied
  without a real native Linux run.

## 6. Failure and recovery review

- **Duplicate delivery and idempotency.** A retry of the accepted spawn
  invocation returns the same run and the same frozen snapshot and creates
  no second run; widening the grant set on that same accepted invocation is
  refused as a delegation conflict.
- **Cancellation races.** A pre-cancelled attempt returns
  `CancelledBeforeSpawn` and creates no marker file, so a cancelled attempt
  leaves no side effect.
- **Contention and resource release.** The attempt lease is the contention
  mechanism: it is taken by the running attempt, it survives the parent, and
  reclamation of a revoked shared root waits for it. Released leases are the
  only path to reclaim.
- **Stale generation or lease.** Not exercised beyond the lease lifecycle
  above; the daemon-restart path for scratch cleanup is covered by the
  existing `scratch::tests` suite (12 cases, green), not re-litigated here.
- **Malformed or unauthorized input.** A path-like `ScratchId`, an unknown
  `ScratchId`, and another session's `ScratchId` are each refused at
  admission with a typed error.
- **Partial persistence failure.** Not exercised. The durable store's own
  failure modes are covered by the existing `codegg-core` suites; C001 adds
  no new persistence path.
- **Bounded event and artifact behavior.** The fixture writes only into
  temporary directories it owns, and every probe writes to a path inside a
  temporary directory, so a containment regression would fail the test
  rather than damage a host file.

## 7. Migration and compatibility review

- **Schema migration:** none. No new table, column, registry entry, or wire
  field. `STORAGE_LAYOUT_VERSION` is untouched, and the existing migration
  v83 (managed scratch ownership and lease data) is used exactly as
  shipped.
- **Backward compatibility:** no protocol, configuration, or CLI change. The
  fixture is a test target only and is absent from the shipped binary.
- **Protocol negotiation:** not applicable; no new capability negotiation.
- **Configuration behavior:** unchanged. The fixture uses the product's own
  profile-to-config resolution (`sandbox_config_for_profile`) rather than a
  bespoke configuration.
- **Rollback limitations:** the commit is a single additive test file, so
  reverting it removes the qualification evidence along with the fixture;
  nothing else depends on it.
- **Legacy-path status:** unaffected. The Windows FullHost path, ordinary
  shell execution, and the tested Windows scratch cleanup are unchanged.

## 8. Security review

- **Authorization enforcement.** The containment claim is derived from the
  durable grant snapshot, and the fixture demonstrates that a caller cannot
  widen it after acceptance, cannot reach another session's scratch, and
  cannot smuggle a path in place of a typed id.
- **Secret handling.** No credential material is read, copied, or logged by
  the fixture. The "credential" negative probe writes to a temporary file
  the test itself created; it never points at a real credential path, so a
  containment regression cannot damage one.
- **Path validation.** The launch spec is built through the product's
  `SandboxConfig::launch_spec`, which canonicalizes and rejects unusable
  roots. The denied-path case uses a hard-linked alias, which the policy
  refuses rather than silently dropping the exclusion.
- **Privilege boundaries.** The child runs with `no_new_privs` and no added
  privilege. The fixture does not modify host ACLs, descriptors, or
  security policy, and it does not escalate.
- **Denial-of-service bounds.** The fixture creates two temporary
  directories, one scratch tree, and one child process per case. It adds no
  polling loop and no continuous workload.
- **Redaction.** Nothing in the fixture prints an absolute host path to
  stdout except the observation lines the test parses; the workspace and
  scratch paths are per-test temporaries.
- **Audit behavior.** Unchanged. The fixture asserts on the same
  `SandboxExecutionOutcome` the product records, so the audit record and
  the test agree by construction.

## 9. Documentation and operations

- `architecture/security.md` — the "Platform outcomes" table now records the
  Linux ABI >= 3 qualification, and a new "Native Linux scratch containment
  (qualified)" subsection states what the fixture proves, what it does not
  claim, and that an incapable host fails rather than skips.
- `docs/tools.md` — the shared-scratch section now states that the
  containment is qualified end to end on a native Linux host with Landlock
  ABI >= 3, names the fixture, and says plainly that other platforms are
  not covered by it.
- `plans/subsystems/managed-agent-scratch-native-qualification-corrective-addendum.md`
  — C001 moves to closed with its closure link; the current-evidence section
  describes the new fixture; the dependency graph and milestone status
  table are reconciled.
- `plans/registry.md` — C001 moves to closed under recently closed work and
  leaves the dependency-ready table; C002 is recorded as blocked with its
  closure link.
- Operator diagnostics: unchanged. Nothing in the product's runtime, error
  surface, or configuration changed.
- Static guards: no new guard was added, and none of the existing guards
  can substitute for this evidence. The fixture itself is the regression
  protection — it runs in the repository's ordinary CI lane on every
  Linux runner, so a future change that widens a scratch grant, drops the
  owner-composed write roots, or weakens the fail-closed downgrade guard
  turns the lane red.
- Recovery instructions: if the fixture fails on a Linux runner, the
  failure is either a real containment regression (investigate inside
  `src/managed_process.rs` / `src/security/sandbox/`) or a host that cannot
  obtain Landlock (a `require_native_linux_landlock` panic, which names the
  reason).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The `pull_request` workflow did not start for the source commit during this session, so the native evidence came from an explicitly dispatched run of the same existing `CI` workflow on the same branch ref. | The evidence is from a dispatch rather than a pull-request event. The dispatched run is the stricter placement: it checked out the exact reviewed tree instead of a synthetic merge commit, and it ran the same guards, formatting, Clippy and workspace test steps. | None for the correctness of this milestone. If the pull-request event does not fire for the next push to this branch, investigate the trigger separately rather than re-running this qualification. |
| low | The repository's `main` push run `39282749528` is red on `clippy::unnecessary_sort_by` at `crates/codegg-core/src/workspace_services.rs:156` under the CI toolchain's newer clippy. | Unrelated to C001; the code is not on this branch, and the Clippy step of run `39291133332` passed. | Owned by the `main` line. A future merge of `main` into this branch would carry the lint into the pull-request lane and needs the same one-line fix applied there. |
| low | The qualification covers the filesystem dimension only, and the read policy deliberately exposes curated toolchain roots (`/usr`, `/etc`, and similar). | Scratch grants are not a read-confidentiality boundary, and this record does not claim one. | None. The read limitation is already stated in `docs/tools.md` and is not changed by this milestone. |

No high or medium finding is introduced by this milestone, and no
pre-existing high or medium finding on the delegated-grant path is left
open by it.

## 11. Roadmap disposition

**Milestone closed and next dependency may proceed.** C001's own exit
condition is satisfied with obtained native evidence. The sibling C002
milestone remains blocked on the independent Windows Required backend and
was recorded as a blocked closure rather than started.

The original M003 closure stays **historically conditional**. This record
appends supplemental evidence to it without rewriting its history; the
Windows half of the M003 gap is unchanged and still blocked.

## 12. Registry updates

- `plans/registry.md`:
  - C001 leaves the *Dependency-ready implementation plans* table and is
    recorded under *Recently closed or conditionally closed work* as
    **closed**, pointing at
    `plans/closure/managed-agent-scratch-native-qualification-corrective/001-status.md`,
    with the source commit `2597ec9e` and the native Linux CI run
    `39291133332`.
  - C002 stays in the *Dependency-ready implementation plans* table as
    **blocked** — its blocker is unchanged — and gains a pointer to
    `plans/closure/managed-agent-scratch-native-qualification-corrective/002-status.md`.
  - The *Active subsystem roadmaps* row for this corrective addendum moves
    to **closing**: C001 is closed and C002 is closed-as-blocked, so the
    addendum's own milestones are resolved. The original M003 remains
    conditional, and the Windows Required backend remains blocked in its
    owning roadmap.
- `plans/subsystems/managed-agent-scratch-native-qualification-corrective-addendum.md`:
  milestone status table, dependency graph and the C001/C002 milestone
  sections are reconciled with this record.
- **Unblock audit:** the registry's *Blocked work* section was re-read in
  full. C001's closure resolves no registered blocker: its only dependency
  was operational (a native Linux host with Landlock ABI >= 3), and no
  registered plan in the *Blocked work* section listed that host as its
  blocker. The Windows Required backend stays blocked in
  `plans/subsystems/windows-native-sandbox-roadmap.md`, where it is
  already registered, and C002's blocked closure does not change it. No
  plan therefore moves to *ready* because of this closure; this closure
  unblocks nothing, and that is recorded here rather than left implicit.
