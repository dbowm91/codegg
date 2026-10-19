# C014 — M005 Production Owner Qualification Corrective

Status: implemented on branch; hosted exact-source qualification pending

Source subsystem roadmap:

- `plans/subsystems/linux-execution-isolation-roadmap.md`

Closure record:

- `plans/closure/linux-execution-isolation/022-status.md`

Repository baseline reviewed: `6efd5ebdf90f91fc24dcac7f2827f4ca9e355135`

## 1. Problem

M005 implements a revisioned per-principal `NetworkIntent` (`LegacyUnrestricted` |
`OfflineRequired`), a `/network` command, immutable snapshot propagation, and
finite/Test/Python/PTY/LSP owner wiring. The M005 plan records the milestone as
"active; internal candidate only" and names the remaining work as exact-source
hosted owner qualification, negative owner-path qualification, supported-host
qualification, and the absence of a production `NetworkEnforcement::Enforced`
claim.

A review of the owner wiring against the source found that the remaining work was
**not only qualification**. Two production owners did not carry the intent at
all, and one of them is the primary command route.

### 1.1 The durable command route dropped the intent entirely

`JobPayload::Shell` and `JobPayload::ManagedArgv` had no network intent and no
profile ceiling. The scheduler's shell executor built a `ManagedProcessRequest`
with `SandboxRequest::Disabled` and no `execution_authorization`, so the
managed-process owner saw an unconstrained request.

This is the path the **active routing** mode takes. `src/tool/bash.rs:667-671`
disables active routing when the sandbox is enabled, so:

- a **constrained** profile with a workspace root takes the direct route
  (`execute_via_raw_shell_with_authorization`), which is M005-wired;
- a **FullHost** profile, or a constrained profile with **no workspace root**,
  takes the durable route, which carried neither the intent nor the profile.

Under a FullHost profile the principal's `OfflineRequired` intent was therefore
never seen by any owner, and the command ran with host networking. The
managed-process owner's guard ("`OfflineRequired` currently requires a
constrained filesystem profile") could not fire, because the owner was never
told the intent.

The same gap removed containment: a constrained profile with no workspace root
was accepted and then executed with no sandbox at all.

### 1.2 Producers with no authenticated snapshot in scope

`src/tui/commands/test.rs`, `src/run_rerun.rs` and
`src/scheduler/tool_program_executor.rs` construct payloads with
`network_intent: LegacyUnrestricted` and `sandbox_profile: None`, because the
producer has no authenticated snapshot. These are legitimate *defaults* only if
the daemon applies the principal's own policy before the job runs. The daemon
did not.

### 1.3 The hosted qualification job could not see the work

`.github/workflows/linux-m004-native.yml` triggered on `src/managed_process.rs`,
`src/interactive_process.rs`, `src/lsp/**`, `src/security/sandbox/**` and a few
others. It did **not** trigger on `crates/codegg-core/src/approval.rs`,
`crates/codegg-core/src/jobs/**`, `crates/codegg-protocol/src/core.rs`,
`src/scheduler/**`, `src/python_script/**`, `src/tool/bash*`, `src/core/**` or
`src/interactive_process_attach.rs`. The M005 owner wiring is in exactly those
files, so a change to the intent, the payload that carries it, or any owner that
consumes it could not trigger its own qualification job.

### 1.4 A CI step was vacuously green

The `restricted-userns-fails-closed` job ran

```bash
cargo test --locked --test linux_m004_process_ipc --features linux-git-test-support \
  required_process_tier_fails_closed_when_user_namespaces_are_restricted
```

but no such test existed. The filter matched zero tests, libtest exited zero,
and the step reported success without proving anything. The sibling steps at the
same place in the workflow point at real tests in `src/lsp/m004.rs` and
`tests/interactive_process_attach_resume.rs`.

### 1.5 The obtained network fact was never recorded

`LinuxObtainedEnforcement` has a `network: ObtainedState` field, but
`ObtainedState::Applied` was only ever assigned to the *filesystem* dimension;
`network` was always `NotRequested`. The audit-facing
`SandboxEnforcement::for_constrained_enforced` sets
`network: NetworkEnforcement::Unrestricted` unconditionally, and no code path
constructs `NetworkEnforcement::Enforced`. An offline-contained child was
therefore reported as "network unrestricted".

## 2. Scope

This corrective qualifies M005's **owner coverage**. It does not change the
M005 enforcement mechanism, the `/network` command, the persistence model, or
the sandbox backend, and it does not authorize a production
`NetworkEnforcement::Enforced` claim.

Out of scope, deliberately:

- The M005 enforcement mechanism itself (`with_linux_offline_network`).
- Any change to the obtained-enforcement report. Section 1.5 is recorded as a
  finding with its remediation, and left for the corrective that owns the
  obtained-facts contract, because emitting an `Enforced` network claim is
  exactly what the M005 plan defers.
- The tool-program child-job route (§5.3).
- Any new persistent setting, public DTO, or production-default switch.

## 3. Changes

### 3.1 The durable payload carries the intent and the profile ceiling

`JobPayload::Shell` and `JobPayload::ManagedArgv` in
`crates/codegg-core/src/jobs/mod.rs` gain

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
sandbox_profile: Option<crate::approval::SandboxProfile>,
#[serde(default)]
network_intent: crate::approval::NetworkIntent,
```

matching the fields `JobPayload::Test` and `JobPayload::Python` already
carried. `#[serde(default)]` keeps every persisted legacy payload readable;
those decode as the historical unrestricted intent and no profile ceiling.

### 3.2 The bash dispatch threads the authenticated intent

`src/tool/bash/process.rs` computes the intent and the profile ceiling from the
authorization snapshot — the same source the direct route uses — and passes both
into the durable payload:

```rust
let authorization = audit_ctx.and_then(|context| context.execution_authorization.as_ref());
let network_intent = authorization
    .map(|authorization| authorization.snapshot.network_intent())
    .unwrap_or_default();
let sandbox_profile = authorization.map(|authorization| authorization.snapshot.sandbox_profile());
```

The background route already carried both on `BashBackgroundCtx`; it now
forwards them into the payload as well.

### 3.3 The shell executor applies the intent and rebuilds the containment

`src/scheduler/executors.rs` reads the ceiling and the intent from the payload,
sets them on the request, and — when the ceiling requires filesystem
containment — rebuilds the same sandbox spec the direct route would have used:

```rust
let config = crate::security::sandbox::sandbox_config_for_profile(profile, &ctx.workspace_root)
    .ok_or_else(...)?;   // fails closed
let spec = config.launch_spec(target, &args, Some(&cwd))?;
request.sandbox = crate::managed_process::SandboxRequest::Required(spec);
```

A constrained job whose host cannot provide containment now **fails closed**
instead of running uncontained.

### 3.4 The daemon stamps the principal's policy at admission

`CoreDaemon::stamp_principal_execution_policy` in `src/core/daemon_jobs.rs`
reads the persisted per-principal preference and applies it to any payload that
arrives without one. It is applied at `CoreRequest::JobSubmit` and at the rerun
child-job submission, which bypasses `JobSubmit`.

The stamp only ever **narrows**:

- the effective intent is offline when either the payload or the principal asks
  for offline;
- the effective profile is the more contained of the payload's and the
  principal's, via the new `SandboxProfile::narrow` helper in
  `crates/codegg-core/src/approval.rs`.

### 3.5 Negative owner-path qualification

Three unit tests in `src/managed_process.rs` (which run in ordinary CI, not
only on the native runner):

- `offline_required_refuses_unconstrained_profile_before_spawn` — an
  `OfflineRequired` authorization under a `FullHost` profile is refused before
  the target runs.
- `offline_required_scheduler_carried_intent_refuses_disabled_sandbox_before_spawn`
  — a scheduler-carried offline intent with no sandbox is refused.
- `request_carried_network_intent_cannot_widen_an_offline_authorization` — a
  request-carried `LegacyUnrestricted` cannot widen an `OfflineRequired`
  authorization, because the snapshot is the ceiling.

One native test in `tests/linux_m004_process_ipc.rs`:

- `required_process_tier_fails_closed_when_user_namespaces_are_restricted` —
  the test the hosted job has been filtering on. It asserts the host really has
  `user.max_user_namespaces=0` before it claims anything, requires a process
  tier, and asserts the target did not run.

### 3.6 The hosted job can see the work

`.github/workflows/linux-m004-native.yml` now triggers on the M005 owner
surface: `crates/codegg-core/src/approval.rs`,
`crates/codegg-core/src/authorization/**`, `crates/codegg-core/src/jobs/**`,
`crates/codegg-core/src/session/schema.rs`,
`crates/codegg-protocol/src/core.rs`, `src/agent/**`, `src/core/**`,
`src/interactive_process_attach.rs`, `src/python_script/**`, `src/scheduler/**`,
`src/tool/bash.rs`, `src/tool/bash/**`, `src/tool/test.rs` and
`src/tool/python_script.rs`.

### 3.7 A guard protects the wiring

`scripts/check_sandbox_policy_wiring.py` gains
`check_network_intent_owner_wiring`, which asserts that

- the intent type and the monotonic ceiling exist in core;
- the process owner reads the intent from the authorization snapshot and has
  all three refusal messages;
- `JobPayload::Shell`, `ManagedArgv`, `Test` and `Python` all carry the intent
  and the ceiling;
- the bash dispatch and the shell executor thread them through;
- the PTY, LSP, Test and Python owners apply the intent;
- no production owner hardcodes the unrestricted intent into a payload or spawn
  spec, except at the four documented "no authenticated intent in scope" sites,
  each of which prints the reason it is allowed.

The guard was proven non-vacuous by injecting a widening into
`src/tool/bash/process.rs` and observing the failure.

## 4. Verification

```bash
cargo check --workspace --locked --all-targets
cargo test -p codegg --lib managed_process::tests::offline
cargo test -p codegg --lib managed_process::tests::request_carried_network_intent
python3 scripts/check_sandbox_policy_wiring.py
```

The hosted exact-source qualification is the `process-ipc` job of
`.github/workflows/linux-m004-native.yml` on a push to this branch. **A
workflow dispatch does not run that job**: the workflow's dispatch condition is
`github.event_name != 'workflow_dispatch' || github.ref_name != '<this branch>'`,
so dispatching on this branch runs only the C002 cost pilot. The qualification
job runs on a **push** whose changed paths match the trigger list.

## 5. Remaining gaps

### 5.1 The obtained network fact is still not recorded

Section 1.5 is unchanged by this corrective. The M005 netns is obtained in
reality; nothing reports it. `SandboxEnforcement::for_constrained_enforced`
still sets `network: NetworkEnforcement::Unrestricted`, and
`NetworkEnforcement::Enforced` still has no producer. This is deliberate here
— the M005 plan defers the production `Enforced` claim — but it means the
audit record understates what was obtained, and ADR-0044 clause 2's M005
precondition is vacuous until a corrective owns the obtained-facts contract.

### 5.2 The tool-program child-job route is not qualified

`src/scheduler/tool_program_executor.rs` builds `ManagedArgv` and `Test`
payloads for a tool program's child jobs. `ChildJobRequest` carries no network
intent and no profile ceiling, so those payloads default to unrestricted and
are **not** covered by the daemon stamp (they are submitted straight to the
scheduler). The guard allowlists this site with that reason. Closing it
requires `ChildJobRequest` to carry the intent.

### 5.3 The supported-host policy is still informal

M005 requires a Linux host that can provide the network namespace. The
workstation used for development is Linux 6.8, whose Landlock ABI is below the
6 that the M004/M005 scope requires, so native execution is unsupported there
and the qualification evidence has to come from the hosted runner. There is no
machine-readable supported-host policy; the probe result is a runtime refusal
rather than a declared capability set.

### 5.4 The M005 netns has no hosted end-to-end proof yet

The hosted `process-ipc` job has never run the M005 owner probe
(`managed_process_owner_applies_authenticated_offline_network_intent`) against
this branch's source. Section 4 explains how to run it.
