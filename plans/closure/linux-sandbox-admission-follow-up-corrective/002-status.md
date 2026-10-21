# Linux Sandbox Admission Follow-up Corrective — Milestone C002 — Closure Status

Status: **conditionally closed**
Source implementation plan: `plans/implementation/linux-sandbox-admission-follow-up-corrective/002-m005-delegated-owner-and-host-admission.md`
Source subsystem roadmap: `plans/subsystems/linux-sandbox-admission-follow-up-corrective-addendum.md` (C002)
Repository baseline reviewed: `2c1953347896848c2f52c2dfbd9f04029d1c2cc2` (`main`, the merge of PR #149)
Implementation commits: `ef3083ab38a5f216a67813090f67b2e62a73d124` — the delegated-owner policy thread, the host-admission manifest, the static guard and its workflow triggers

## 1. Executive finding

The delegated **Tool Program** child-job route was the one production path
that still dropped the accepted principal's execution policy. C014 fixed the
durable `Shell`/`ManagedArgv` route; the Tool Program route was missed.

The defect was structural, not a one-line slip:

1. The `tool_program` tool submits its job from the model-facing tool
   boundary, so it never reaches `CoreRequest::JobSubmit` — which is where
   `stamp_principal_execution_policy` narrows the payload to the principal's
   persisted preference.
2. The delegated child job is submitted straight to the scheduler by
   `ToolProgramExecutor`, so it never reaches daemon admission at all.
3. The child payload therefore hard-coded `sandbox_profile: None` and
   `network_intent: LegacyUnrestricted`, regardless of what the principal had
   accepted.

The consequence: a principal that had accepted offline execution still got a
delegated `cargo test` / `cargo build` / `cargo clippy` / `cargo fmt` child
job with host network and no filesystem ceiling.

The repository already contained the correct pattern for the *same* job kind:
`src/tool/test.rs` inherits `snapshot.sandbox_profile()` and
`snapshot.network_intent()` into the `JobPayload::Test` it submits. C002 makes
the Tool Program route carry the same ceiling, with the same narrowing
discipline, and adds a guard that fails if any link in the chain is broken.

**This is not by itself an M005 production admission.** No Linux host is
admitted, no backend is registered, and C010's no-admission decision is
unchanged. See §10 for the one named outstanding condition.

## 2. Requirement-to-evidence matrix

| Plan requirement | Evidence | Status |
|---|---|---|
| WP1: enumerate each producer/consumer of `NetworkIntent::OfflineRequired` and the sandbox ceiling; producers without a principal snapshot receive daemon-stamped authenticated intent (narrowing only) | §3.1; `src/core/daemon_jobs.rs` stamps `Shell`/`ManagedArgv`/`Test`/`Python`; the Tool Program route bypassed both stamping points and now carries the accepted snapshot instead (§3.2) | met |
| WP2: thread bound policy unchanged through durable storage/deserialization, child request and backend choice | §3.2–§3.4; `ToolProgramExecutionContext` carries both axes, is covered by the replay-safety digest, and survives a JSON round-trip (§4.2) | met for the policy thread; the native obtained-enforcement matrix is outstanding (§10) |
| WP2: a real Tool Program child-job integration showing denied host TCP/UDP/DNS and host-process/IPC access | Not produced. Requires a qualified Linux host this environment does not provide | **outstanding** (§10) |
| WP2: verify denial survives FullHost filesystem preference plus OfflineRequired intent; do not conflate profile with independent network authority | §4.2 `the_two_axes_are_independent`; the two axes are narrowed independently in `narrow_child_ceiling` | met at the policy level; the launch-level refusal is part of the outstanding native matrix |
| WP3: legacy jobs (without persisted intent) stay explicitly `LegacyUnrestricted`; current principal policy governs newly admitted jobs | §3.2 additive fields with `#[serde(default)]`; §4.2 `a_pre_m005_payload_still_decodes` and `a_missing_authorization_keeps_the_historical_meaning` | met |
| WP3: added wire/persistence fields need additive versioning/migration and older-client negative tests | Additive `Option<SandboxProfile>` + `NetworkIntent` with `#[serde(default)]`; no storage-layout migration, so `STORAGE_LAYOUT_VERSION` is unchanged; the pre-M005 payload is the older-client negative test (§4.2) | met |
| WP3: check parent/child ceilings on resume, restart and agent handoff; test stale revisions and nonexistent or revoked principals | The ceiling travels inside the durable execution context, so a restart or replay re-reads it rather than re-deriving it (§3.2); a stale/invalid authorization is refused at the tool boundary (§4.2 `an_invalid_authorization_is_refused`) | met for the policy thread; a live restart/handoff trace is part of the outstanding native matrix |
| WP4: machine-readable host-admission manifest with `discovered` / `can_launch` / `native_negative_proven` / `maintainer_admitted` kept distinct | §3.5; `src/security/sandbox/host_admission.rs`; §4.3 | met |
| WP4: the manifest alone can never turn a requested boundary into `Enforced` | `HostAdmissionManifest::contains_enforcement_claim`; §4.3 `the_manifest_makes_no_enforcement_claim`; the module has no path that grants an admission from a probe | met |
| WP4: Ubuntu positive and restricted/no-userns negative | Not produced. Requires the qualified Linux host | **outstanding** (§10) |
| WP5: report which dimension failed, pre-target failure reason and safe manual remedy; cap/redact sensitive host paths/socket/credentials | §3.5; every capability carries an evidence string, and `can_launch` records the failing dimension with the underlying reason | met for the manifest; the runtime-side remedy text for a failed launch is part of the outstanding native matrix |
| WP5: extend `scripts/check_network_intent_owner_wiring.py` | The script did **not** exist on any branch in this repository's history. It was created, not extended (§3.6) | met, with a recorded premise correction |
| WP5: prove the guard fails by defect injection | §4.4 — seven injected defects, each rejected | met |
| WP5: native workflow path triggers for changed Tool Program, job, policy and protocol paths | §3.7 — `src/tool/tool_program.rs` and `src/tool/tool_program_context.rs` added to **both** trigger blocks; the guard enforces the coverage | met |
| WP5: no false claims in `docs/security-semantics.md` / `architecture/security.md` | No new enforcement claim is made anywhere; the manifest is explicitly capability, not enforcement (§3.5) | met |
| Verification: `check_network_intent_owner_wiring.py`, `check_sandbox_contract.py`, `check_execution_ownership.py`, formatting, Clippy, `scripts/verify.sh quick` | §4 | met |
| Verification: branch-wide CI on the reconciled head | Not run on this head; the branch-wide evidence for the merged baseline is in `plans/closure/branch-and-ci-stabilization/001-status.md` | **outstanding** (§10) |

## 3. Production implementation evidence

### 3.1 The gap, as found

- `src/core/daemon_jobs.rs` — `stamp_principal_execution_policy` narrows the
  payload's `sandbox_profile` and `network_intent` against the principal's
  persisted preference. It matches `Shell | ManagedArgv | Test | Python`.
- The `tool_program` tool submits its job directly to the scheduler, so it
  never passes through `CoreRequest::JobSubmit`, where that stamping lives.
- `ToolProgramExecutor` submitted the delegated child job directly to the
  scheduler, so the child never passed through daemon admission either.
- The child payload hard-coded `sandbox_profile: None` and
  `network_intent: NetworkIntent::LegacyUnrestricted`.
- `src/scheduler/executors.rs` forwards the payload's two axes into the
  managed-process request, and `src/managed_process.rs` refuses an offline
  intent that is not accompanied by containment — so the *owner* was already
  correct; only the payload was wrong.

### 3.2 The durable carrier

`ToolProgramExecutionContext` (in `crates/codegg-core/src/jobs/mod.rs`) gained
two additive fields:

- `sandbox_profile: Option<SandboxProfile>` — `#[serde(default)]`
- `network_intent: NetworkIntent` — `#[serde(default)]`

Both are covered by `compute_digest`, so a context whose ceiling changed
cannot share a replay-safety digest with the one it replaced. A payload
persisted before M005 has neither field and still decodes, with the
historical meaning preserved.

### 3.3 The tool boundary

`src/tool/tool_program_context.rs` gained `accepted_execution_ceiling`, which
reads the accepted authorization snapshot:

- the profile and the intent come from the snapshot, which is the only place
  a principal's offline preference is expressed;
- a caller with **no** authorization object keeps the historical meaning (no
  profile ceiling, unrestricted network) — the absence of a decision is never
  invented into an offline one, and the legacy path is preserved for
  non-production callers;
- an authorization object that is **present but invalid** is an integrity
  failure and is refused, not downgraded.

### 3.4 The executor

`ToolProgramExecutor` binds the ceiling onto the broker adapter, and
`child_job_payload` — extracted so the production builder is directly
testable — narrows it into every delegated child operation (`Test`, `Build`,
`Lint`, `Format`). `narrow_child_ceiling` narrows the two axes independently:
a child may narrow the parent's intent or profile, never widen it.

### 3.5 The host-admission manifest (WP4)

`src/security/sandbox/host_admission.rs` produces a machine-readable manifest
with four sections that are deliberately separate: `discovered` (facts, with
evidence), `can_launch` (derived request capability), `native_negative_proven`
(empty until a maintainer records refusal evidence) and `maintainer_admitted`
(a maintainer decision, never granted by a probe).

The manifest reports capability, never enforcement. `contains_enforcement_claim`
exists so that an enforcement claim can be rejected mechanically rather than by
review. The kernel release is read from `/proc` rather than by spawning
`uname`, so a read-only capability probe stays a file read and does not
become a process-execution surface.

### 3.6 The static guard (WP5)

`scripts/check_network_intent_owner_wiring.py` checks seven links: the durable
carrier and its digest coverage, the tool-boundary stamp, the executor binding
and the narrowed child payload, the durable payload variants, the scheduler
forwarding, the owner's refusal, and the native workflow trigger coverage.

**Premise correction:** the C002 plan says to *extend*
`scripts/check_network_intent_owner_wiring.py`. That script did not exist in
this repository's history — not on `main`, and not on any branch — so it was
created rather than extended.

### 3.7 The native workflow triggers

`.github/workflows/linux-m004-native.yml` did not trigger on the Tool Program
tool or context files; only `src/scheduler/**` covered the executor. Both
`pull_request` and `push` path lists gained `src/tool/tool_program.rs` and
`src/tool/tool_program_context.rs`. The guard enforces the coverage in both
blocks, because a path that qualifies only a pull request would leave a direct
push to `main` unqualified.

## 4. Verification executed

All commands below were run in the linked worktree
`~/.codex/worktrees/032b/codegg` under `umask 0022` with an isolated
`XDG_CACHE_HOME`, which is the GitHub runner's default umask. The local
workspace's default umask is `0002`, under which `core::scratch_sweep::tests`
fails for an environmental reason that is unrelated to this change (those
fixtures use `create_dir_all`, so their root lands group-writable and the
scratch allocator refuses it with `WrongOwner`; the same binary passes when
only the umask changes).

**Local scope was deliberately narrowed** at the user's direction: this
environment is contended and previously cancelled rust processes. No
full-workspace sweep was run, no Windows job was awaited, and no macOS
compile was run. The commands below are the changed surface plus the gates
this plan names.

### 4.1 Compile and static gates

| Command | Result |
|---|---|
| `cargo check --workspace --all-targets --locked` | **exit 0** |
| `cargo fmt --all -- --check` | pass (via `scripts/verify.sh quick`) |
| `python3 scripts/check_network_intent_owner_wiring.py` | pass |
| `python3 scripts/check_network_intent_owner_wiring.py --self-test` | pass — 7 injected defects, each rejected |
| `scripts/verify.sh quick` | see §4.5 |

### 4.2 Focused C002 tests

`cargo test --locked --test tool_program_c002_delegated_ceiling` — 6 passed,
0 failed:

- `delegated_child_job_inherits_the_parents_offline_intent` — the accepted
  offline intent reaches the delegated child payload through the durable
  round-trip;
- `every_delegated_child_operation_carries_the_ceiling` — `Test`, `Build`,
  `Lint` and `Format` all carry it;
- `the_two_axes_are_independent` — an offline intent is not satisfied by a
  narrower filesystem profile, and a narrower profile does not imply an
  offline intent;
- `a_missing_authorization_keeps_the_historical_meaning`;
- `a_pre_m005_payload_still_decodes`;
- `an_invalid_authorization_is_refused`.

**Non-vacuity.** The pre-fix behaviour was simulated in the production
builder — the ceiling ignored, the legacy values hard-coded — and the suite
was re-run against the same tree. Three tests failed
(`delegated_child_job_inherits_the_parents_offline_intent`,
`every_delegated_child_operation_carries_the_ceiling`,
`the_two_axes_are_independent`), each with `left: LegacyUnrestricted,
right: OfflineRequired`. The three legacy-behaviour tests passed, as they
should: they assert that the legacy path is unchanged. The production file
was restored from a byte-exact copy afterwards and the restore was confirmed
by re-counting the narrowed payload sites.

### 4.3 Host-admission manifest tests

`cargo test --locked --test host_admission_manifest` — 7 passed, 0 failed:
discovery never grants an admission, discovery never records native
evidence, the manifest makes no enforcement claim, the four sections stay
distinct, every dimension carries evidence, `can_launch` states that it is
not enforcement, and only a maintainer record can set an admission.

### 4.4 Guard self-test

`python3 scripts/check_network_intent_owner_wiring.py --self-test` — pass.
Seven defects were injected one at a time into a copy of the tree, and the
guard rejected each: the child payload dropping the narrowed intent; the
child payload dropping the narrowed profile; the carrier losing its
`network_intent` field; the digest stopping covering the ceiling; the tool
boundary stopping stamping the ceiling; the executor stopping binding the
ceiling; and the native workflow stopping triggering on the Tool Program
paths. The guard also reports failures on an unmodified tree, which is
checked first so a vacuous guard cannot pass.

### 4.5 `scripts/verify.sh quick`

`scripts/verify.sh quick` — **"Quick verification passed."**, exit 0. The new
`check_network_intent_owner_wiring.py` guard runs inside it, at
`scripts/verify.sh:79`.

### 4.6 Changed-surface regression slice

`cargo nextest run --locked --profile ci --no-fail-fast -p codegg` with
`--lib` and the eleven changed-surface targets:

```
--lib
--test tool_program_c002_delegated_ceiling
--test host_admission_manifest
--test git_execution_origin_matrix
--test identity_m002_live_audit_hooks
--test command_routing_execution_ownership
--test tool_program_child_jobs
--test tool_program_m012_child_ownership
--test tool_program_m013_authority
--test tool_program_m014_authority_pipeline
--test tool_program_runtime
--test tool_program_fault_injection
--test tool_program_m014_artifacts
```

**7522 tests run, 7522 passed, 4 skipped** — `Summary [ 164.410s]`, exit 0.

The `command_routing_execution_ownership` and `security::workflow` targets
are included because this plan names them; `security::workflow` lives in the
library target, which the `--lib` selection covers. The two Git fixtures this
milestone changed are included because their paths moved. The eight
Tool Program targets are included because they exercise the tool boundary and
the executor that this milestone changed.

The slice was **not** the workspace sweep: no other target was built or run.

## 5. Invariant review

- **Never trust model/child-supplied payload, env or serialized defaults to
  widen intent.** Held. The ceiling originates in the accepted authorization
  snapshot, not in the payload, the program, the model or the environment. A
  delegated child narrows; it cannot widen. A pre-M005 payload keeps the
  legacy meaning, which is the *narrower* of the two in the sense that it
  grants nothing new.
- **Network intent and filesystem profile are independent axes.** Held, and
  now pinned by a test. `narrow_child_ceiling` narrows them separately, and
  the snapshot's own `with_network_intent` already refuses to widen an
  offline intent.
- **An advisory must never create an escalation.** Unaffected.
- **The manifest is capability, not enforcement.** Held. `discover()` cannot
  grant an admission and cannot record native evidence; the enforcement
  verdict is still owned solely by the per-launch receipt.
- **Fail-closed paths were not weakened.** No assertion was relaxed, no
  timeout lowered, no filter widened. The one new refusal (present-but-
  invalid authorization) is a refusal, not a relaxation.
- **C010's no-admission decision.** Unchanged. No host is admitted by this
  milestone.

## 6. Failure and recovery review

- The delegated child inherits the ceiling from the durable context, so a
  restart or replay re-reads it rather than re-deriving it. There is no
  "retry" path that could re-derive a wider policy.
- A delegated child that asks for a wider policy than its parent keeps the
  parent's narrower one. There is no implicit legacy retry.
- An authorization that fails validation is refused at the tool boundary, so
  a tampered or stale decision never reaches a child job.

## 7. Migration and compatibility review

- No storage-layout migration: the two new fields live in a serialized JSON
  context inside an existing column, and both are `#[serde(default)]`.
  `STORAGE_LAYOUT_VERSION` is unchanged.
- Older payloads decode unchanged. The negative test
  `a_pre_m005_payload_still_decodes` asserts the historical meaning.
- Older clients are unaffected: no protocol DTO changed.
- The replay-safety digest now covers two more fields. A context persisted
  before this change and one persisted after it differ in digest, which is
  the intended effect: a changed ceiling must not share a fingerprint with
  the one it replaced.

## 8. Security review

- The closed hole: an accepted offline intent previously did not reach a
  delegated Tool Program child job. It now does.
- The child payload is built by a single extracted function, so the ceiling
  cannot be dropped by one of four construction sites.
- The guard makes each link in the chain a mechanically checked property, and
  the self-test proves the guard is not vacuous.
- The workflow trigger gap is closed in both trigger blocks, so a future
  change to the Tool Program policy path reaches the native qualification job.
- No secret is logged. The manifest records host paths and kernel facts that
  are already visible to the operator running the tool; the evidence strings
  come from the existing probes, which do not carry credentials.

## 9. Documentation and operations

- `plans/registry.md` — the subsystem row and the milestone rows were
  updated in the same commit as this record.
- `plans/subsystems/linux-sandbox-admission-follow-up-corrective-addendum.md`
  — the milestone status table was updated.
- `docs/security-semantics.md` and `architecture/security.md` — no change was
  made, and none is required: this milestone adds no new enforcement claim,
  only a policy that now reaches the owner it was always meant to reach. The
  existing text already describes the per-launch receipt as the source of an
  enforcement verdict.
- `scripts/check_network_intent_owner_wiring.py` is wired into
  `scripts/verify.sh quick`, so the guard runs on every local quick gate.

## 10. Unresolved findings

### 10.1 The native adversarial matrix (severity: high, owner: M005 owner qualification)

**Condition.** C002 WP2 requires one real Tool Program delegated child
showing permitted workspace/scratch action **and** denied host TCP/UDP/DNS and
host-process/IPC access, at an exact SHA, with a deliberately uncontained
positive control so the refusals are non-vacuous. WP4 requires the same
matrix to be recorded as native negative evidence, including an Ubuntu
positive and a restricted/no-userns negative.

**Why it is outstanding.** The matrix must run on a Linux host whose Landlock
ABI 6 signal and abstract-socket scopes qualify. This workstation does not
provide them: `landlock::probe_process_scopes()` fails there, so the M004
process/IPC scopes cannot be established. The plan is explicit that this
case is recorded **blocked**, not green-from-skip — so it is recorded here as
the condition of a conditional closure, and no skip has been reported as a
pass anywhere in this record.

**Risk.** The policy thread is proven by unit and integration evidence at the
payload boundary. What is *not* yet proven is that a real delegated child on a
qualified host is actually denied host TCP/UDP/DNS and host-process/IPC, and
that the launch receipt the runtime observes is truthful. The M005 production
owner qualification remains open, and C010's no-admission decision stands
regardless.

**Exact future evidence required.** On a Linux host with Landlock ABI 6
qualified, at an exact revision:

1. `CODEGG_RUN_M005_OFFLINE_NATIVE=1 cargo test --locked --test linux_m004_process_ipc --features linux-git-test-support`
   — with the host-positive IPv4/IPv6 TCP/UDP, DNS/rebinding, inherited
   connected FD, pathname and abstract UDS, host proc/signal, nested child and
   concurrent-worker cases, each denied;
2. the restricted/no-userns negative case, where the setup must fail **before**
   the target marker, with a typed refusal;
3. a deliberately uncontained positive control in the same run, so the
   refusals are non-vacuous;
4. the captured kernel/LSM/Bubblewrap/Landlock facts, the requested and
   obtained launch receipt, and the actual denied effects, recorded against
   that exact revision in the `native_negative_proven` section of the
   manifest;
5. `native_negative_proven` non-empty and `maintainer_admitted` set by a
   maintainer decision — never by the probe.

**Stop conditions carried forward.** If any nested route silently loses
intent, `OfflineRequired` executes with host network, a status receipt is
forged from policy or probe alone, or a socket/FD crosses isolation, this
condition escalates and C002 does not close.

### 10.2 Branch-wide hosted CI on this head (severity: medium, owner: this milestone)

The hosted branch-wide sweep and the hosted M004/M005 native job were not run
on this head; the local scope was narrowed at the user's direction because the
environment is contended. The branch-wide evidence for the merged baseline is
in `plans/closure/branch-and-ci-stabilization/001-status.md`
(hosted CI run `39649501713`: 15,841 run, 15,841 passed, 8 skipped, on
`4c08f2cc5`, which is content-identical to `main` for every source file).

**Exact future evidence required.** A hosted `CI` run on this milestone's
head, and a hosted native M004/M005 job, with the run links recorded here.

### 10.3 macOS target compile (severity: low, owner: this milestone)

Not run; the host is Linux and the plan's macOS compile is a hosted job.
**Exact future evidence required.** A hosted macOS target-compile run on this
head.

## 11. Roadmap disposition

- **C002** — production changes, the guard and the manifest are landed and
  verified locally. The closure is **conditional** on §10.1, which needs a
  qualified Linux host.
- **Closing C002 does not admit M005.** No host is admitted, no backend is
  registered, and C010's no-admission decision is unchanged. A separately
  reviewed host-admission disposition is still required, and it must be
  backed by the §10.1 evidence.
- **C001** — this milestone did not change C001's scope. C001's own record is
  `plans/closure/linux-sandbox-admission-follow-up-corrective/001-status.md`.
- **C003** — untouched. Its dependencies are unchanged by this milestone.

## 12. Registry updates

- The subsystem row for "Linux sandbox admission post-review corrective" in
  `plans/registry.md` records C002 as **conditionally closed** with this
  record's path and the §10.1 condition.
- The dependency-ready plan list records C002's plan as `closing`/`conditionally
  closed` and points at this record.
- **Unblock audit:** C003 (typed Git consistency / ADR packet) was audited.
  Its hard dependencies are ADR-0044 acceptance, the C004 production
  vertical and the M005 production owner qualification. This milestone does
  **not** satisfy the last of those — §10.1 is open — so C003 stays as it
  was and is **not** unblocked by this closure.
- No other registered plan's blocker is resolved by this milestone.

## 13. Verification policy note

The local scope was narrowed deliberately and is recorded here rather than
hidden. The reasons were the contended host and the user's explicit
instruction to prune testing that was not needed. The narrowing means:

- the full workspace sweep was **not** re-run locally;
- the Windows and macOS hosted jobs were **not** awaited;
- the native Linux matrix was **not** attempted, because the host does not
  qualify (§10.1).

What was run covers the changed surface, the two fixtures this milestone
changed, and the gates this plan names. The one gap that matters for the
milestone's own acceptance — the native refusal matrix — is named, owned, and
stated with the exact evidence that would close it.
