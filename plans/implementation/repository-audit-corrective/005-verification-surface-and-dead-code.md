# Repository Audit Corrective Milestone C005 — Verification Surface and Dead Code

Status: ready for handoff

Repository baseline: `d5be7acd05dc999888430b7ad23420d554359fec` (`origin/main`)
Audit evidence was gathered at `04536eeeb`; every file this workstream cites is
byte-identical at that commit and at the baseline above, so the findings carry
forward unchanged.

Source roadmap: `plans/subsystems/repository-audit-corrective-roadmap.md#milestone-c005--verification-surface-and-dead-code`

Long-term requirements: `plans/000-long-term-specification.md`, `plans/002-long-term-roadmap.md`, `plans/003-planning-process.md`, `architecture/overview.md`

Applicable ADRs: none required. This milestone changes no contract; it makes existing contracts executable and removes code that cannot run.

Primary class: **infrastructure**

## 1. Objective

Every guard the repository names is executed by the verification command it is named in; the lint gate is exercised on every platform the project ships for, or the asymmetry is stated and owned; and code that can never compile is removed or deliberately retained with a recorded reason.

## 2. Why this milestone is ready

Its hard dependency is that C001–C004 have closed. That is not a formality: the broker-boundary guard reports a genuine finding in the permission reviewer, and re-enabling it before that finding is dispositioned would turn the verification surface red on the same change that is meant to make it green.

Beyond that dependency it has no interface dependency, and it is the milestone that converts this workstream's findings from "found once" into "caught automatically".

## 3. Current implementation evidence

### A guard that documents a contract is not executed

`scripts/check_tool_broker_boundary.py` enforces that production tool calls go through `ToolBroker::execute()`. AGENTS.md states this as an absolute: the broker is the only production tool-call boundary.

The guard is:

- **absent** from `scripts/verify.sh`'s guard set — `run_guards()` does not invoke it;
- **absent** from CI — `.github/workflows/ci.yml` does not invoke it;
- **red** at the baseline, exiting 1 with six findings;
- recorded in closure records as known-red — `plans/closure/memory-skill-promotion/002-status.md` notes it "reports a pre-existing violation" and that the "guard remains red for unrelated reasons".

The six findings are not six defects. The guard's `DIRECT_CALL_PATTERNS` match `\.execute_capture\(`, `\.execute_structured\(` and `\.execute\(&input`, and the last two match **intra-tool delegation** as readily as a boundary violation:

| Site | Code | Verdict |
|---|---|---|
| `src/permission/reviewer.rs` | `self.registry.execute_capture(name, args.clone(), Some(ctx))` | **genuine** — `ToolRegistry::execute_capture` called from outside the broker, in a different subsystem |
| `src/tool/extension.rs` (×2) | `self.execute_structured(input, None)` | false positive — the tool calling its own method |
| `src/tool/git_read.rs` | `self.inner.execute_structured(input, ctx)` | false positive — a wrapper delegating to the inner tool |
| `src/tool/verify.rs` (×2) | `self.execute_structured(..)`, `self.bash.execute_structured(..)` | false positive — the tool calling itself, and one tool calling another |

The guard's own remedy text names `self.tool_broker.execute()`. So the guard has a real finding, five noise findings, and no way to be green — which is precisely why it was parked rather than fixed.

The **consequence** of the parking is the important part: the C001–C004 defect classes in this workstream are of exactly the kind a broker-boundary guard and a lint gate would catch, and neither is running.

### The lint gate is platform-asymmetric

`scripts/verify.sh full` and CI both run:

```
cargo clippy --workspace --all-targets --locked -- -D warnings
```

On the baseline, that command **fails on macOS** with eight errors. Every failing site is production code that exists only on Linux, so the Linux CI runner does not compile it and CI is unaffected:

| Site | Why it is macOS-only |
|---|---|
| `src/managed_process.rs` — `sandbox_interactive_entries` | its only production caller, `spawn_sandboxed_interactive_child`, is `#[cfg(target_os = "linux")]` |
| `src/security/sandbox.rs` — `SandboxPathIdentity::capture` | `#[cfg(unix)]`, and its caller's enclosing block is `#[cfg(target_os = "linux")]` |
| `src/interactive_process.rs` — a `needless_return` | inside a `#[cfg(not(target_os = "linux"))]` block |
| `src/lsp/m004.rs` — `M004LspServiceScope::new`, `new_with_network_intent` | `#[cfg(not(target_os = "linux"))]` and `#[cfg(test)]` |
| `src/managed_process.rs` — `spec_with` | `#[cfg(test)]`, and the tests that use it are `#[cfg(target_os = "linux")]` |

macOS is a shipped platform — the repository carries macOS-native containment work and a macOS test family — so the platform asymmetry is real, not hypothetical.

The repository already knows about it. `scripts/verify.sh` carries a comment about "a hosted clippy failure, visible only on a **Linux** runner, because this tree's macOS build fails earlier on unrelated findings", and `scripts/check_test_common_binding.py`'s docstring states the same. The consequence is that a macOS developer running `scripts/verify.sh full` sees failures CI will never reproduce, and cannot tell whether they are pre-existing, environmental, or theirs.

The underlying defect is not that macOS has different code. It is that Linux-only production code is **not marked** as Linux-only, so it is compiled on macOS as dead code and the lint gate reports it there.

### Code that can never compile is retained as history

`src/tui/app/mod.rs` contains:

```rust
/// Legacy app-side dialog router retained only as source history while
/// component dispatch converges. It is intentionally unreachable: the
/// FocusManager-owned component is the sole input authority.
#[cfg(any())]
fn handle_dialog_key_legacy(&mut self, key: KeyEvent) { ... }
```

The function body is **615 lines** — from the `#[cfg(any())]` attribute to the closing brace — and the only reference to it in the entire tree is its own definition. Nothing calls it.

`#[cfg(any())]` is false for every configuration, so the body is never compiled. That is the problem: a body that is never compiled cannot fail to compile when the APIs it calls change. It rots silently, and it costs 615 lines in the module the audit identified as a 20,783-line god module.

The doc comment also records that the convergence is complete — "the FocusManager-owned component is the sole input authority" — which makes the retention a pure cost.

### Other code with no production caller

`src/agent/router.rs` defines `TaskEnvelope`, `classify_envelope` and `route_from_envelope`. The only references to any of them are within the same file's test module. `route_from_envelope` has **no** caller at all.

Unlike the disabled function, this code **does** compile, and a routing abstraction with no caller may be a feature that is planned rather than abandoned. This milestone must therefore **dispose** of it, not assume it is dead: either it is removed, or it is retained with a recorded reason and an owner. The implementer must read the surrounding roadmap and registry to determine which.

### A residual blind spot

`research/` contains five standalone crates with their own lockfiles, outside the workspace. Windows CI builds them, so they are not dead code. But `cargo clippy --workspace` and the workspace's `unsafe_code = "deny"` lint policy do not reach them. This is recorded as a **residual finding**, not a deliverable of this milestone, and it needs an owner rather than a silent fix.

## 4. Invariants that must not regress

- **The broker boundary contract is unchanged.** No tool call is moved on or off the broker. The guard enforces the existing contract; it does not define a new one.
- **The permission reviewer's behaviour is unchanged** except for how its tool call is routed. Its authority, its output ceiling, and the 15-second / read-only / native-only context it constructs are unchanged.
- **CI must not become weaker.** Adding a guard to CI must not remove or relax any existing check.
- **Every guard added must be non-vacuous**, proved by injection, as the repository's existing guards are. A guard that cannot fail is worse than no guard, because it reports a clean tree it did not check.
- **The verification commands the project documents stay accurate.** If `verify.sh quick` gains a guard, the documented set changes, and the documentation changes with it.
- **The `research/` crates keep building.** Nothing in this milestone may break the Windows workflow that builds them.
- **Historical closure records are untouched**, including the ones that document the parked guard's red state. A historical record is evidence, not a bug report.

## 5. Scope

**In scope**

- The broker-boundary guard: its patterns, the disposition of its one genuine finding, and its wiring into `scripts/verify.sh quick` and CI.
- The platform asymmetry of the lint gate: the Linux-only code that is not marked Linux-only, and the macOS-only lint findings.
- The 615-line `#[cfg(any())]` function body.
- The disposition of `src/agent/router.rs`'s caller-free routing abstraction.
- The documentation of the verification surface that changes.

**Out of scope**

- Adding a macOS lane to CI. The implementer must record the platform matrix that actually runs the lint gate, and the asymmetry's disposition. A new hosted runner is a separate decision with its own cost.
- Changing the workspace lint policy, including `unsafe_code = "deny"`, or extending it to the `research/` crates. That is the residual finding's owner.
- Any change to the tool broker, the tool registry, or the permission reviewer's policy.
- Any change to a historical closure record, including the ones that name the parked guard.

## 6. Required production changes

### Core

- The Linux-only production code listed in §3 must be marked `#[cfg(target_os = "linux")]` — or otherwise scoped so it is not compiled on macOS — so that the lint gate's macOS result reflects the code that actually exists on macOS.
- The macOS-only `needless_return` must be fixed as a lint fix, not suppressed.
- The 615-line `#[cfg(any())]` body must be removed, along with its doc comment. The implementer must first confirm that the `FocusManager`-owned dispatch path covers every branch the legacy function handled — the doc comment says the convergence is complete, but the implementer must verify that against the live dispatch, not against the comment.
- The `src/agent/router.rs` abstraction must be **disposed**: removed, or retained with a recorded reason and an owner. The implementer must read the roadmap and registry before deciding, because a caller-free abstraction may be a planned capability.

### Storage

None.

### Protocol

None.

### Runtime

None.

### Frontend

None, beyond the removal of the disabled dialog router, which is not compiled and so is not observable.

### Security

- The broker-boundary guard's re-enablement is a **security** change: it turns a documented absolute into an enforced one. The genuine finding it reports — the permission reviewer calling `ToolRegistry::execute_capture` directly — must be **dispositioned before** the guard is wired in, and the disposition must be either "routed through the broker, with the same authority ceiling" or "recorded as a deliberate exception with a named owner and a guard allowance that says so".
- The implementer must confirm that routing the reviewer's call through the broker does not change the reviewer's effective authority, and in particular does not change the `read_only` effect class, the `native_only` backend policy, the 15-second timeout, or the output truncation.

### Docs

- The verification surface changes: one guard is added to `verify.sh quick` and to CI, and the platform matrix for the lint gate changes. The documentation that states the guard set and the platform matrix must be updated in the same change.
- `architecture/overview.md` or the equivalent verification-policy section must record the lint gate's platform expectations.

## 7. Ordered work packages

### WP1 — Repair the broker-boundary guard's patterns

Intent: make the guard able to be green.

Required changes: narrow the patterns so that intra-tool delegation is not a finding, while a call to the registry or to a tool from outside the broker remains one. The guard must distinguish "a tool delegating to its own implementation" from "a subsystem invoking a tool directly", and it must recognise the broker's own call shape so the broker is not reported.

Acceptance evidence: the guard is green on the real tree after the genuine finding is dispositioned, and the `--self-test` proves it still rejects a genuine violation.

### WP2 — Disposition the genuine finding

Intent: resolve the one real boundary question.

Required changes: the permission reviewer's direct `ToolRegistry::execute_capture` call must be routed through the broker, **or** recorded as a deliberate exception with a named owner and an explicit allowance in the guard. The implementer must state which, and why.

If it is routed through the broker, the reviewer's authority ceiling must be shown to be unchanged: same effect class, same backend policy, same timeout, same truncation.

Acceptance evidence: the reviewer behaves identically, the guard is green, and the disposition is recorded.

### WP3 — Wire the guard in

Intent: make the contract enforced.

Required changes: add the guard to `scripts/verify.sh`'s `run_guards()` and to CI's guard step, in the position the repository's convention dictates, and update the documented guard set.

Acceptance evidence: `verify.sh quick` runs it — demonstrated, not assumed — and CI runs it.

### WP4 — Resolve the lint-gate platform asymmetry

Intent: make the lint gate meaningful on every shipped platform.

Required changes:

- mark the Linux-only production code as Linux-only, so the macOS build does not compile it;
- fix the macOS-only lint finding as a fix;
- record the platform matrix the lint gate actually runs on, and what the expectation is on each platform.

The implementer must decide whether macOS is a **gate** platform or a **reported** platform, and record that decision. A recorded "macOS is reported, not gated" is an acceptable outcome **only** if the report is actually produced and visible. An unrecorded asymmetry is not.

Acceptance evidence: the macOS lint result is clean or explicitly scoped, the Linux result is unchanged and green, and the platform matrix is documented.

### WP5 — Remove the disabled function body

Intent: delete code that cannot run.

Required changes: remove the 615-line `#[cfg(any())]` body and its doc comment, after confirming that the live `FocusManager` dispatch covers the branches the legacy function handled.

Acceptance evidence: the module compiles without it, the dispatch behaviour is unchanged, and the test suite covering dialog key handling still passes.

### WP6 — Dispose the caller-free routing abstraction

Intent: give the abstraction a status.

Required changes: read the agent-routing roadmap and the registry's active and deferred sections. Either the abstraction is removed, or it is retained with a recorded reason, a named owner, and a registry row that says it is pending. The implementer must not delete a planned capability because it has no caller today.

Acceptance evidence: the disposition is recorded, and the registry reflects it.

### WP7 — Record the residual finding

Intent: give the `research/` blind spot an owner.

Required changes: record the finding — five crates outside the workspace's lint and check reach — as a residual finding with a named owner in the registry's deferred section, with the specific evidence.

Acceptance evidence: the finding is registered.

## 8. Failure, cancellation, restart, contention semantics

- **Failure.** A guard that fails is a **build failure**, not a runtime failure. The implementer must not add an `|| true` escape, a failure suppression, or an allowlist entry that silently tolerates the class the guard exists to catch.
- **Cancellation.** No runtime change.
- **Restart.** No durable state change.
- **Contention.** The guards run in CI and in `verify.sh quick`. Their runtime must stay inside the budget the verification policy states, and the implementer must record the added cost of the newly-wired guard.

## 9. Compatibility and migration

- **CI becomes stricter.** That is the intended effect, and it may surface findings in branches other than this one. The implementer must state this in the closure, and the registry's other active lines must be checked for a branch that would newly fail.
- No runtime, protocol, storage, or configuration compatibility impact.
- No migration.

## 10. Required tests

**Focused**

- The broker-boundary guard's `--self-test`, including a case that proves a genuine violation is still rejected and a case that proves intra-tool delegation is not.
- A test that the permission reviewer's tool call is routed as the disposition requires, and that its authority ceiling is unchanged.

**Integration**

- A test that the dialog dispatch behaves identically after the 615-line removal, covering the branches the legacy function handled.

**Security**

- The broker guard's non-vacuity proof is the security evidence. A guard that cannot fail proves nothing.

**Migration**

- None.

**Placement.** The guard's own `--self-test` lives beside the guard, in the repository's established style — the guards in `scripts/` carry their own self-tests, and the shell-policy line's guard is the model. A test for the reviewer's routing belongs in the `security_permission` family, whose members already cover approval and sandbox containment. The dialog-dispatch test belongs in the `tui_render` family.

**Vacuity.** Every guard added must be shown failing on an injected defect. The closure records each injection and its rejection.

## 11. Required verification commands

```bash
# Scoped: quick's guards plus only the tests this change can affect.
scripts/verify.sh scoped HEAD~1

# The affected guards.
python3 scripts/check_tool_broker_boundary.py
python3 scripts/check_tool_broker_boundary.py --self-test

# The affected families.
cargo test --test security_permission
cargo test --test tui_render

# The lint gate, on the platform the implementer is on.
cargo clippy --workspace --all-targets --locked -- -D warnings

# Rust-aware test inventory (ground truth; never regex-scan Rust).
python3 scripts/rust_test_scan.py --validate

# Full workspace, before the closure record is written.
bash scripts/verify.sh full
```

The lint gate must be run on **both** macOS and Linux for the asymmetry to be closed. If only one is available, the closure must name the other as an unexercised condition rather than claiming the asymmetry is resolved.

## 12. Documentation updates

- The documented verification surface: the guard set that `verify.sh quick` runs, and the CI step list.
- The lint gate's platform matrix and per-platform expectation.
- `architecture/overview.md` or the equivalent verification-policy section.
- The registry's deferred section, for the `research/` residual finding.

## 13. Acceptance criteria

Externally observable:

1. `scripts/verify.sh quick` runs the broker-boundary guard, and CI runs it.
2. The broker guard is green on the real tree, and its `--self-test` proves it still rejects a genuine violation.
3. The permission reviewer's tool call is routed through the broker, or is a recorded, owned exception. Its authority ceiling is unchanged either way.
4. The macOS lint result is clean or explicitly scoped, and the Linux lint result is unchanged and green. The platform matrix is documented.
5. The 615-line disabled function body is gone, and dialog dispatch behaviour is unchanged.
6. The caller-free routing abstraction has a recorded disposition.
7. The `research/` blind spot is registered with an owner.

## 14. Stop conditions

The implementer must report rather than improvise if any of the following holds.

- The broker guard cannot be made green without **changing what the broker permits**. Stop: the contract is not this milestone's to change.
- Routing the reviewer's call through the broker changes its authority ceiling, its effect class, its timeout, or its output truncation. Stop: that is an authorization change.
- The live `FocusManager` dispatch does **not** cover a branch the legacy function handled. Stop: the legacy body is then not pure history, and removing it would change behaviour. Record what is missing.
- The caller-free routing abstraction turns out to be a **planned capability** that the registry does not record. Stop and record the gap; do not delete a planned capability.
- Making the macOS lint result clean would require a blanket `allow` on the Linux-only modules. Stop: a blanket allow defeats the gate. Narrow, per-item scoping is acceptable and must be justified per item.
- The newly-wired guard makes `verify.sh quick` exceed the documented time budget. Stop and report the measurement; the budget is a contract.

## 15. Closure evidence required

- The guard's `--self-test` output, and the demonstration that the guard fails on an injected genuine violation.
- The disposition of the permission reviewer's call, with the authority-ceiling comparison stated explicitly.
- The before-and-after lint result **on macOS**, itemised, and the Linux result confirmed unchanged and green.
- The documented platform matrix and the per-platform expectation.
- The confirmation that dialog dispatch behaviour is unchanged, and the branch-coverage analysis of the removed body.
- The disposition record for the routing abstraction.
- The registered residual finding for the `research/` crates.
- The `verify.sh scoped` and `verify.sh full` results, labelled local versus CI.
- The registry update, and the unblock audit of the registry's Blocked work section.

## 16. Handoff notes

- The broker guard has been red and parked since before this audit, and its closure records say so. The parking is the defect as much as the patterns are: a guard that is known-red and unwired is a guard that does not exist. Fix the patterns **and** wire it in, or the change is half a fix.
- Do not solve the noise by adding an allowlist entry per site. The five false positives have one cause — the pattern does not distinguish intra-tool delegation from a boundary violation — and one cause has one fix.
- The macOS lint failures are **not** five unrelated warnings. Four of them are Linux-only production code that is not marked Linux-only. The platform asymmetry is a missing `cfg`, not a macOS-specific problem.
- The 615-line body has to be **verified** against the live dispatch before it is removed, not trusted because its own doc comment says the convergence is done. A comment asserting a migration is complete is exactly the kind of claim this workstream is auditing.
- The routing abstraction may be a planned capability. Read the roadmap and the registry before deleting it.
- This milestone closes no other milestone. C006 and C007 depend on it, because both are measured against a tree whose verification surface is trustworthy.
