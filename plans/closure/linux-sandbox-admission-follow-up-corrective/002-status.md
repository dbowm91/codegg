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
| WP2: thread bound policy unchanged through durable storage/deserialization, child request and backend choice | §3.2–§3.4; `ToolProgramExecutionContext` carries both axes, is covered by the replay-safety digest, and survives a JSON round-trip (§4.2) | met for the policy thread; the native obtained-enforcement matrix has now run on a qualified hosted Linux runner (§4.7a, §10.1) |
| WP2: a real Tool Program child-job integration showing denied host TCP/UDP/DNS and host-process/IPC access | The proof's first execution failed and found the second defect in §3.6; that is fixed, and the proof **passed** on the hosted M004 runner `39685717425` (18 passed / 0 failed in the native binary, with the host positive control intact). The residual cargo-specific attribution stays outstanding (§10.1 item 5) | **met**, with the cargo attribution outstanding (§10.1) |
| WP2: the bound policy must survive into the *backend choice*, including the required process/Filesystem tier | §3.6 — the durable route built the launch spec before requesting the M004 tier, so the owner's forced offline flag left the spec with no captured path identity and the launch failed closed. Fixed in both durable routes the delegated child uses; the guard now checks the ordering | met |
| WP2: verify denial survives FullHost filesystem preference plus OfflineRequired intent; do not conflate profile with independent network authority | §4.2 `the_two_axes_are_independent`; the two axes are narrowed independently in `narrow_child_ceiling`; the launch-level refusal was observed on the qualified hosted runner (§4.7a) | met |
| WP3: legacy jobs (without persisted intent) stay explicitly `LegacyUnrestricted`; current principal policy governs newly admitted jobs | §3.2 additive fields with `#[serde(default)]`; §4.2 `a_pre_m005_payload_still_decodes` and `a_missing_authorization_keeps_the_historical_meaning` | met |
| WP3: added wire/persistence fields need additive versioning/migration and older-client negative tests | Additive `Option<SandboxProfile>` + `NetworkIntent` with `#[serde(default)]`; no storage-layout migration, so `STORAGE_LAYOUT_VERSION` is unchanged; the pre-M005 payload is the older-client negative test (§4.2) | met |
| WP3: check parent/child ceilings on resume, restart and agent handoff; test stale revisions and nonexistent or revoked principals | The ceiling travels inside the durable execution context, so a restart or replay re-reads it rather than re-deriving it (§3.2); a stale/invalid authorization is refused at the tool boundary (§4.2 `an_invalid_authorization_is_refused`) | met for the policy thread; a live restart/handoff trace is **not** covered and is not claimed |
| WP4: machine-readable host-admission manifest with `discovered` / `can_launch` / `native_negative_proven` / `maintainer_admitted` kept distinct | §3.5; `src/security/sandbox/host_admission.rs`; §4.3 | met |
| WP4: the manifest alone can never turn a requested boundary into `Enforced` | `HostAdmissionManifest::contains_enforcement_claim`; §4.3 `the_manifest_makes_no_enforcement_claim`; the module has no path that grants an admission from a probe | met |
| WP4: Ubuntu positive and restricted/no-userns negative | Produced on the hosted runner `39685717425`: the Ubuntu 24.04 positive is the native binary's 18 passing tests, and the `restricted-userns-fails-closed` job on the same run **passed** | met |
| WP5: report which dimension failed, pre-target failure reason and safe manual remedy; cap/redact sensitive host paths/socket/credentials | §3.5; every capability carries an evidence string, and `can_launch` records the failing dimension with the underlying reason | met for the manifest; the runtime-side remedy text for a failed launch is part of the outstanding native matrix |
| WP5: extend `scripts/check_network_intent_owner_wiring.py` | The script did **not** exist on any branch in this repository's history. It was created, not extended (§3.7) | met, with a recorded premise correction |
| WP5: prove the guard fails by defect injection | §4.4 — eleven injected defects, each rejected | met |
| WP5: native workflow path triggers for changed Tool Program, job, policy and protocol paths | §3.8 — `src/tool/tool_program.rs` and `src/tool/tool_program_context.rs` added to **both** trigger blocks; the guard enforces the coverage | met |
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

### 3.6 A second defect the native proof found: the durable route built the spec too late

The native delegated-child proof was run on the hosted M004 qualification
runner, and it **failed** — with a typed refusal from Bubblewrap setup:

```
the offline delegated child should launch:
  SandboxFailed("Bubblewrap setup failed: sandbox grant has no captured path identity")
```

That is not a fixture problem. It is a second defect in the same chain, and
it is what "thread the bound policy through ... backend choice" actually
means.

The owner turns the offline flag on when it accepts an offline intent:

```rust
// src/managed_process.rs
spec.linux_offline_network = true;
spec.linux_process_isolation = true;
spec.linux_protected_mount_view = true;
```

The protected mount view is what **captures** the path identities the helper
re-verifies at launch, and `SandboxConfig::launch_spec` captures them only
when the protected mount view is already requested **at spec-build time**.
The direct route gets this right — its caller sets
`with_linux_offline_network(true)` before building the spec, and the passing
sibling proof asserts `!spec.linux_offline_network` to show the intent comes
from the authority rather than the spec.

The two **durable** routes did not. They built the config from the profile
alone, so the spec carried `path_identities: Vec::new()`, and the owner then
turned the protected mount view on *after* the spec existed — too late for the
capture. The launch then failed closed with the refusal above.

So before this milestone, a durable job carrying `OfflineRequired` with a
constrained profile could not launch at all: it failed closed rather than
running uncontained, which is the safe direction, but it was a functional
dead end. The delegated Tool Program child inherits that route, so C002's fix
alone would have turned "runs with host network" into "cannot run".

**The fix.** The decision belongs in one place, so that every durable route
requests the same tier for the same accepted policy and a route cannot
quietly reintroduce the ordering defect. `sandbox_config_for_durable_payload`
in `src/security/sandbox.rs` builds the launch configuration for a durable
payload and requests the M004 process/filesystem tier for an offline intent
**while the config is still being built**, so the identities are captured
before the spec exists. Both durable routes the delegated child uses go
through it:

- `src/scheduler/executors.rs` — the managed-argv and shell route;
- `src/test_runner/runner.rs` — the `Test` route, which a delegated
  `cargo test` child uses.

The owner's forcing of the flags stays as it was: it is defence in depth. If
a route ever forgets to request the tier, the launch still fails closed.

**Why the proof has to use that helper.** The first version of the proof
built its own configuration from the profile, which meant it reproduced the
*broken* behaviour and failed even after the routes were fixed — it was
testing a copy of the logic rather than the logic. It now calls
`sandbox_config_for_durable_payload` directly, so the proof exercises the
production decision: if the helper stops requesting the tier, the proof fails.

The guard checks the same thing statically: that the helper exists, that it
considers the intent and requests the tier, and that **both** durable routes
build their configuration through it rather than through the profile-only
helper.

### 3.7 The static guard (WP5)
`scripts/check_network_intent_owner_wiring.py` checks nine links: the durable
carrier and its digest coverage, the tool-boundary stamp, the executor binding
and the narrowed child payload, the durable payload variants, the scheduler
forwarding, the M004 tier's ordering against the launch spec, the owner's
refusal, and the native workflow trigger coverage.

**Premise correction:** the C002 plan says to *extend*
`scripts/check_network_intent_owner_wiring.py`. That script did not exist in
this repository's history — not on `main`, and not on any branch — so it was
created rather than extended.

### 3.8 The native workflow triggers

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
| `python3 scripts/check_network_intent_owner_wiring.py --self-test` | pass — 11 injected defects, each rejected |
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
**Eleven** defects were injected one at a time into a copy of the tree, and
the guard rejected each:

1. the delegated child payload dropping the narrowed network intent;
2. the delegated child payload dropping the narrowed sandbox profile;
3. the durable carrier losing its `network_intent` field;
4. the replay-safety digest stopping covering the ceiling;
5. the tool boundary stopping stamping the ceiling;
6. the executor stopping binding the ceiling;
7. the test runner stopping forwarding the payload's network intent;
8. the shared durable-payload helper stopping requesting the M004 tier;
9. the managed executor stopping using the shared durable-payload helper;
10. the test runner stopping using the shared durable-payload helper;
11. the native workflow stopping triggering on the Tool Program paths.

Cases 8–10 are the ones the shared-helper refactor added: before it, a
durable route could go back to building its own configuration from the
profile and quietly reintroduce the tier-ordering defect of §3.6.

The guard also reports failures on an unmodified tree, which is checked first
so a vacuous guard cannot pass.

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

The slice was re-run on the tree that carries the §3.6 tier-ordering fix —
**7522 tests run, 7522 passed, 4 skipped**, `Summary [ 203.060s]`, exit 0 —
because that fix changes the durable job route the library target covers.

### 4.7 Hosted evidence

The hosted runs are the authoritative evidence for the parts a local sweep
cannot cover. Every run below is on an exact head of this branch.

| Run | Head | Workflow | Result |
|---|---|---|---|
| `39677678144` | `a74c7dbbe` | CI (branch-wide) | **success** — 15,854 tests run, 15,854 passed, 8 skipped, `Summary [ 472.488s]` |
| `39677678125` | `a74c7dbbe` | Linux M004 process and IPC qualification | **failure** — the new delegated-child proof found the second defect in §3.6 |
| `39677671427` | `a74c7dbbe` | Linux M004 process and IPC qualification | **failure** — same cause, the other trigger event |
| `39681302386` | `9a40e983e` | Linux M004 process and IPC qualification | **failure** — the proof still rebuilt its own configuration and so reproduced the defect the fix had removed (§3.6) |
| `39682689275` | `9a40e983e` | CI (branch-wide), `workflow_dispatch` | **cancelled** — superseded by the next push on the branch |
| `39685731938` | `7be59bd46` | CI (branch-wide), `workflow_dispatch` | **success** — 15,854 tests run, 15,854 passed, 8 skipped, `Summary [ 451.904s]` |
| `39685717425` | `7be59bd46` | Linux M004 process and IPC qualification | **success** — the delegated-child proof passes on a qualified host (§10.1) |
| `39685699753` | `7be59bd46` | Linux M004 process and IPC qualification (`push`) | the same qualification from the `push` trigger; superseded by the merge push |
| `39685699680` | `7be59bd46` | Linux C006 protected mount qualification | **success** |
| `39689988865` | `b87f15860` | CI (branch-wide), `workflow_dispatch` | **failure** — one timing-sensitive test, diagnosed in §4.7b |
| `39691145379` | `b9648609c` | CI (branch-wide) | **success** — 16,029 tests run, 16,029 passed, 8 skipped, `Summary [ 376.832s]` |
| `39691145352` | `b9648609c` | Linux M004 process and IPC qualification | **success** — the delegated-child proof passes on the shipped tree (§4.7a) |

Heads `b87f15860`, `96b150dc8` and `b9648609c` differ only in `plans/` — the
merge commit `0a77cfa37` carries every code change on the branch, and the two
later commits are records. The hosted qualification of the shipped tree is
therefore the one on `b9648609c`, where CI and the native qualification are
both green.

The `pull_request` event dispatch for `9a40e983e` produced **no** runs at all
— GitHub created none, for any workflow, after the push. The CI run was
therefore dispatched explicitly on that exact ref with `workflow_dispatch`,
which checks out the same head and runs the same job. The M004 run for the
same head was created by the `push` event.

The three M004 failures are **evidence, not regressions**: they are the hosted
native run reporting the durable route's ordering defect, which is what the
proof was added to find. The 17 sibling tests in that suite passed in every
failing run; only the new proof failed, and it failed with a typed refusal
rather than a silent pass.

The CI run on `7be59bd46` is the branch-wide hosted sweep for this line: the
identical command over the identical workspace on a dedicated runner.

### 4.7a The native qualification that passed

Run `39685717425` on head `7be59bd46` is the hosted native qualification that
C002 WP2 asked for. Its `process-ipc` job ran the whole
`linux_m004_process_ipc` binary with `CODEGG_RUN_M005_OFFLINE_NATIVE=1` set at
the job level, so the proof was not skipped:

```
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 510.17s
...
test delegated_tool_program_child_carries_the_offline_ceiling_to_the_owner ... ok
```

The 18 include the sibling proofs the C002 matrix is composed of —
`offline_network_blocks_host_network_and_inherited_socket_access`,
`process_tier_hides_host_proc_and_blocks_host_signal_and_abstract_socket`,
`managed_process_owner_applies_authenticated_offline_network_intent`,
`offline_network_runs_cargo_metadata_and_check_without_network`, and the
contained-Git and private-index proofs. The `restricted-userns-fails-closed`
job on the same run **passed**, which is the no-userns negative WP4 asks for.

The **host facts** the run captured, for the record:

| Fact | Value |
|---|---|
| Runner image | `ubuntu-24.04` (GitHub-hosted, `ubuntu24/20261018.339`) |
| Kernel | `7.0.0-1017-azure #17~24.04.1-Ubuntu SMP PREEMPT Thu Sep 24 18:46:57 UTC 2026 x86_64` |
| Bubblewrap | `0.9.0` (`0.9.0-1ubuntu0.3`), `/usr/bin/bwrap` mode `755`, owner root |
| `max_user_namespaces` | `58121` |
| Seccomp actions available | `kill_process kill_thread trap errno user_notif trace log allow` |
| AppArmor profile | `unconfined`; `kernel.apparmor_restrict_unprivileged_userns` was `1` and was relaxed to `0` on that ephemeral VM |
| Landlock | ABI 6 process scopes qualify — `probe_process_scopes()` succeeded, and the proof panics rather than skips when it does not |

The **denied effects** the delegated-child proof observed: the probe's TCP
connect to the host listener was refused (it exits `10` on success, so a
reachable host would have failed the step), the host UDP listener received
nothing, and the child still completed its permitted workspace write. The
host **positive control** — the same host reaching its own loopback listener —
passed, so a denial could not be a broken fixture.

### 4.7b A hosted CI failure that was diagnosed as a timing flake

CI run `39689988865` on head `b87f15860` **failed** with one test:

```
FAIL [   0.158s] ( 8417/16029) codegg::git_credential_cross_path f2_sanitize_large_argv_is_linear
thread 'f2_sanitize_large_argv_is_linear' panicked at tests/git_credential_cross_path.rs:419:5:
sanitize_argv_for_run_store took 148.190758ms on 10k tokens — likely quadratic
```

The nextest run stopped there (8,420 of 16,029 tests had run), so the
remaining tests in that run were never executed — this run is **not** counted
as evidence for anything.

**The diagnosis.** The assertion is a wall-clock bound — `elapsed < 100 ms`
for sanitizing a 10,000-token argv — and it failed at 148 ms, 48% over. The
evidence that this is runner timing and not a regression:

- **The function is linear by inspection.**
  `sanitize_argv_for_run_store` is a single `map` over the argv with one
  `contains` check per token and a per-token redaction. There is no nested
  scan and no quadratic path.
- **The file is untouched.** `tests/git_credential_cross_path.rs` is not in
  this branch's diff against `main`, and `main`'s PR #157 did not change it
  either. It is in the Git-credential path, which none of this milestone's
  changes touch.
- **It passes locally, including under load.** The test binary passes in
  0.01 s here, and still passes in 0.01–0.02 s with sixteen CPU burners
  running.
- **The same test passes in the next hosted run.** In CI run `39691145379`
  on the same code, the same test at the same position (8,418 of 16,029)
  **passed in 0.010 s**.

The test was added in `527ad616c` with its 100 ms budget and is not a test
this branch changed. **Nothing was weakened to make it pass**: the budget was
not raised, the test was not excluded, and no assertion was relaxed. The
re-run on `b9648609c` is the green signal; this run is recorded because a
failed run is a fact and not something to be quietly dropped.

This is recorded as an **observation about the hosted runner**, not as a
defect in this milestone. A wall-clock bound in a shared test suite is
inherently sensitive to runner load, and the load in the failing run was
raised by the 16,029-test workspace sweep it was part of.


### 4.8 The reconciliation with `main` (`53d9fc9fd`)

`main` advanced while this milestone was in flight — PR #157, the M005
writer-containment corrective, merged as `53d9fc9fd`. The branch was
reconciled with it as a **merge**, not a rebase, so no commit was rewritten
and the earlier heads' evidence stayed valid.

The merge conflicted in **one file**, `src/scheduler/tool_program_executor.rs`,
in a single hunk. Both sides added a field to the same `BrokerAdapter` struct
and a builder method to the same `impl` block, and each side's addition was
kept:

| Side | Field | Axis |
|---|---|---|
| this branch | `execution_ceiling_profile` / `execution_ceiling_intent` | the delegated child's **sandbox profile and network intent** ceiling (C002) |
| `main` (PR #157) | `process_admission` | which **processes** a Tool Program's nested call may reach (C005) |

The two are complementary rather than competing: one bounds the *launch
policy* of a delegated child job, the other bounds the *process reach* of a
nested call. The resolution keeps both fields, both initializers, and both
builder methods; `BrokerAdapter::new` initializes all three.

**One interaction to note.** `main`'s PR #157 also added a **process-admission**
gate at the final executor (`admit_process_class` in
`src/scheduler/executors.rs`), which refuses a process-class job for a
reserved checkout. A delegated Tool Program child is a process-class job, so
from this merge forward the delegated child's effective policy is the
**intersection** of two independent authorities: the accepted sandbox/network
ceiling this milestone delivers, and `main`'s process-denial authority. The
gate fails closed — it returns `Ok` only when no process-admission service is
attached — so it can refuse a delegated child that the ceiling alone would
have allowed, never the reverse. This is `main`'s C005 contract, not a change
to this milestone's; it is recorded because it changes what a delegated child
may do, and because it is the reason the hosted evidence had to be re-run on
the merged tree rather than carried over.

The reconciliation was proved at both levels the earlier ones were:

- **file level** — `git ls-files -u` is empty, `git diff --check` is clean,
  and `git diff origin/main` shows this branch's changes to
  `crates/codegg-core/src/jobs/mod.rs` (the durable carrier and its digest),
  `src/scheduler/executors.rs`, `src/test_runner/runner.rs`,
  `src/security/sandbox.rs` and `scripts/verify.sh` intact — the auto-merged
  files kept both sides' content;
- **compile level** — `cargo check --workspace --all-targets --locked` exits 0
  on the merged tree.

Two **no-collision** checks were made explicitly, because the merge brought a
second ADR-numbered artifact onto the branch:

- **ADR numbering.** `main` published `ADR-0047-restricted-agent-shared-checkout-writer-contract.md`
  (renumbered from an earlier `ADR-0044`), while `ADR-0044-contained-git-live-objects-and-metadata-authority.md`
  is still the contained-Git live-read authority this milestone's records
  reference. `ADR-0044`'s status is unchanged — *proposed, not accepted, no
  production authority granted* — so no record in this milestone needed
  correcting and the ADR numbering stayed consistent across the merge.
- **registry rows.** The registry union kept one row per milestone: this
  milestone's C001 and C002 rows and `main`'s M005 writer-containment rows all
  survive. One factual correction was made to this milestone's row, which
  still described the guard's self-test as injecting seven defects when it now
  injects eleven.

  A per-table duplicate scan of the merged registry found **three** duplicated
  rows in the `Agent tool-use operability corrective` block (C001, C004 and
  C006 each appear twice). These are **pre-existing on `main`** — the same
  three duplicates are present at `2c1953347`, the baseline this branch
  started from, so they were not introduced by this merge and are not this
  milestone's rows. They are left untouched: fixing another subsystem's
  registry hygiene is outside C001/C002's scope, and the rows are recorded
  here so the observation is not lost.

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

**Where it stands.** The proof now exists and has been **executed on a
qualified Linux host** — the hosted M004 qualification runner, which is the
Ubuntu 24.04 runner whose Landlock ABI 6 process scopes qualify and which
already sets `CODEGG_RUN_M005_OFFLINE_NATIVE=1`:

- `delegated_tool_program_child_carries_the_offline_ceiling_to_the_owner`
  in `tests/linux_m004_process_ipc.rs` builds the delegated child's payload
  with the production builder, reads its axes back the way the scheduler
  executor reads them, launches through the owner with the production
  profile-derived launcher, and asserts the child is denied host IPv4 TCP and
  UDP against a host positive control.

Its **first hosted execution failed**, and the failure found the second
defect described in §3.6: the durable route built the launch spec before it
requested the M004 tier, so the owner had no captured path identity to
re-verify. That defect is fixed in both durable routes the delegated child
uses, and the guard now checks the ordering.

**The proof has since passed on a qualified Linux host.** Run `39685717425`
on head `7be59bd46` is the re-execution §10.1's first condition asked for:
the `process-ipc` job ran the whole `linux_m004_process_ipc` binary with
`CODEGG_RUN_M005_OFFLINE_NATIVE=1` set at the job level, and the delegated
proof is reported as

```
test delegated_tool_program_child_carries_the_offline_ceiling_to_the_owner ... ok
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 510.17s
```

The 18 include the sibling proofs the C002 matrix is composed of — the
offline-network denial, the host-PID/IPC/abstract-socket denial, the
authenticated offline owner, the offline `cargo metadata`/`cargo check`
run, and the contained-Git and private-index proofs. The
`restricted-userns-fails-closed` job on the same run **passed**, which is the
no-userns negative WP4 asks for. The host facts and the denied effects are
recorded in §4.7a.

**What the proof does and does not establish.** It proves the delegated
route's *policy* reaches the owner and is enforced there. It does **not**
prove that a `cargo` delegated child's own network fetch is denied, because
the Tool Program contract restricts a delegated child's argv to `cargo`
(`ChildJobConfig::validate` rejects any other executable). A delegated child
can therefore never be an arbitrary network probe, and the executable in this
proof is a probe instead. The cargo-specific attribution is recorded as
outstanding below rather than claimed.

**Risk.** The policy thread is proven by unit and integration evidence at the
payload boundary. The owner's enforcement of it is proven by the hosted
native run. The M005 production owner qualification remains open, and C010's
no-admission decision stands regardless.

**Exact future evidence required.**

1. ~~A hosted M004 run on the fixed head, in which
   `delegated_tool_program_child_carries_the_offline_ceiling_to_the_owner`
   passes, with its per-test result recorded.~~ — **satisfied** by run
   `39685717425` on `7be59bd46`; the per-test result is quoted above and the
   host facts in §4.7a.
2. ~~The hosted `restricted-userns-fails-closed` job passing on the same
   head, which is the no-userns negative C002 WP4 asks for.~~ — **satisfied**
   by the same run.
3. The captured kernel/LSM/Bubblewrap/Landlock facts, the requested and
   obtained launch receipt, and the actual denied effects, recorded against
   that exact revision in the `native_negative_proven` section of the
   manifest. The facts and effects are captured in §4.7a; what remains is
   the manifest entry itself, which is **maintainer-owned** (§10.1b).
4. `native_negative_proven` non-empty and `maintainer_admitted` set by a
   maintainer decision — never by the probe.
5. The residual cargo-specific attribution: evidence that a delegated
   `cargo` child's own network attempt is refused rather than merely
   unexercised.

**Stop conditions carried forward.** If any nested route silently loses
intent, `OfflineRequired` executes with host network, a status receipt is
forged from policy or probe alone, or a socket/FD crosses isolation, this
condition escalates and C002 does not close.

### 10.1b The admission record is maintainer-owned (severity: medium, owner: the maintainer)

The host-admission manifest deliberately separates what a host *offers* from
what a maintainer *records*. `discover()` cannot write `native_negative_proven`
and cannot write `maintainer_admitted`; `contains_enforcement_claim()` exists
so a manifest that asserts enforcement is rejected mechanically rather than by
review. The consequence is that the native evidence this milestone produced
**cannot be promoted into the manifest by the agent that produced it** — the
`NativeNegativeEvidence` entry for the revision above is a maintainer
decision, and `maintainer_admitted.admitted` stays `false` until a maintainer
sets it.

The evidence a maintainer needs is already captured: the revision, the run
id, the refusal cases, and the host facts in §4.7a. The exact shape is:

```
NativeNegativeEvidence {
    revision: "<the exact head that was qualified>",
    run_id:   Some("39685717425"),
    cases:    ["ipv4_tcp_denied", "ipv4_udp_denied", "host_process_ipc_denied"],
    recorded_at: "<the maintainer's recording time>",
}
```

This is recorded as an **open maintainer action**, not as a gap in the
implementation. C002 stays *conditionally closed* until it is recorded, and
**no admission is claimed by this milestone either way**: C010's no-admission
decision stands, no backend is registered, and ADR-0044 remains proposed.


### 10.1a The Python durable route has the same ordering defect (severity: medium, owner: the Python tool's owner)

`src/python_script/executor.rs` builds its own `SandboxLaunchSpec` for the
`JobPayload::Python` route with `linux_protected_mount_view: false` and
`path_identities: Vec::new()`, and it forwards the payload's intent to the
managed process request exactly as the other durable routes do. The owner
therefore turns the protected mount view on after the spec exists, with the
same consequence: a durable Python job carrying `OfflineRequired` and a
constrained profile fails closed with "sandbox grant has no captured path
identity".

This is the **same class** of defect that §3.6 fixed for the delegated
child's two routes, and it was found by reading the owner's forcing logic
rather than by a failing run, because the delegated child's proof is what
exercised the owner's forcing path.

It fails **closed**, so it is not a confidentiality hole: the job cannot run
with host network. It is a functional dead end for an offline Python job.

It is **not** fixed here, because the Python durable route is outside C002's
delegated-child scope and its path set is built differently (the Python route
computes its own read/write roots rather than going through
`sandbox_config_for_profile`), so the fix needs the Python route's own owner
to reason about its path set. The finding is recorded here so it is not lost.

**Exact future evidence required.** The Python route's owner either requests
the M004 tier before building its spec, capturing the identities for its own
path set, or records why the Python route cannot take the offline tier and
makes the payload's intent and the route's capability agree.

### 10.2 Branch-wide hosted CI on this head (severity: medium, owner: this milestone)

**Satisfied for `7be59bd46`.** The hosted branch-wide sweep ran on that head:
CI run `39685731938`, **15,854 tests run, 15,854 passed, 8 skipped**,
`Summary [ 451.904s]`, exit 0. The same head also carries the hosted native
qualification in §10.1 and the C006 protected-mount qualification
(`39685699680`, success).

The head has since moved to the merge commit that reconciles this line with
`main` (§4.8), which brings `main`'s PR #157 production changes into the two
durable routes and into the owner that enforces the ceiling. The hosted
evidence was therefore re-run on the merged tree, and it is **green there**:
CI run `39691145379` on `b9648609c` — **16,029 tests run, 16,029 passed,
8 skipped**, `Summary [ 376.832s]` — and the native qualification run
`39691145352`, also green, with the delegated-child proof passing (§4.7a).
The count grew from 15,854 to 16,029 because `main`'s PR #157 added tests.

The local scope stays narrowed at the owner's direction: the local
changed-surface slice on the merged tree is **7546 tests run, 7546 passed,
4 skipped**, `Summary [ 190.297s]` — 24 more tests than the pre-merge slice,
from `main`'s PR #157.

The branch-wide evidence for the merged baseline is also in
`plans/closure/branch-and-ci-stabilization/001-status.md`
(hosted CI run `39649501713`: 15,841 run, 15,841 passed, 8 skipped, on
`4c08f2cc5`, which is content-identical to `main` for every source file).


### 10.3 macOS target compile (severity: low, owner: this milestone)

Not run; the host is Linux and the plan's macOS compile is a hosted job.
**Exact future evidence required.** A hosted macOS target-compile run on this
head.

## 11. Roadmap disposition

- **C002** — production changes, the guard and the manifest are landed and
  verified locally and on a hosted native runner. The closure is **conditional**
  on §10.1b (the maintainer-owned admission record) and on the residual
  cargo-specific attribution in §10.1 item 5. The qualified-Linux-host
  condition that §10.1 originally named is **met**: run `39685717425` ran the
  delegated-child proof on a host whose Landlock ABI 6 process scopes
  qualify, and it passed.
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
  **not** satisfy the last of those — §10.1b is open, because the
  admission record is maintainer-owned and has not been made — so C003 stays
  as it is and is **not** unblocked by this closure.
- No other registered plan's blocker is resolved by this milestone.

## 13. Verification policy note

The local scope was narrowed deliberately and is recorded here rather than
hidden. The reasons were the contended host and the user's explicit
instruction to prune testing that was not needed. The narrowing means:

- the full workspace sweep was **not** re-run locally;
- the Windows and macOS hosted jobs were **not** awaited.

What was run covers the changed surface, the two fixtures this milestone
changed, and the gates this plan names.

**The native matrix was not attempted on this workstation, because the host
does not qualify** (`landlock::probe_process_scopes()` fails here). It was run
on a **hosted** qualified Linux runner instead — run `39685717425` — and the
result is recorded in §4.7a and §10.1. The gap that remains for the
milestone's own acceptance is no longer the qualified host; it is the
maintainer-owned admission record (§10.1b) and the residual cargo-specific
attribution, both named with the exact evidence that would close them.

