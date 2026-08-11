# Agent Runtime Correctness, Autonomy, and Simplification M011 — Typed Tool Outcome and Hosted Closure Corrective Pass

Status: implemented

Source subsystem documents:

- `plans/subsystems/agent-runtime-correctness-autonomy-simplification-roadmap.md`
- `plans/subsystems/agent-runtime-correctness-autonomy-simplification-corrective-closure-addendum.md`

Corrective predecessor plans and records:

- `plans/implementation/agent-runtime-correctness-autonomy-simplification/010-recovery-state-strict-closure-corrective-pass.md`
- `plans/closure/agent-runtime-correctness-autonomy-simplification/010-status.md`
- M009 / PR #74 remains predecessor integration evidence only

Planning governance:

- `plans/003-planning-process.md`, especially section 7 on corrective passes

Relevant architecture and ADRs:

- `architecture/agent.md`
- `architecture/tool.md`
- `architecture/error.md`
- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`

Repository baseline reviewed:

- current `main`: `7d863763f700d936687ad01005e6a0d19b74c991`
- M010 implementation: `ea4136ff2d644a4eaaf3f97872f6efb61bfaed0d`
- M010 stable-Clippy constructor correction: `cbdc01508391e0cd71f74edb8f0c05634d309716`
- M010 closure record commit: `8db1403adeb5659cf4fa19619443f8a983ac2d51`
- current hosted `CI / verify` run: `31521674076`, job `93879950640`, failed at Workspace Clippy on current `main`

Primary class: corrective invariant / closure

Dependencies:

- hard: M001-M010 production corrections remain present and are not reopened beyond the exact defects named here;
- interface: preserve the existing `ToolError` variants and model-facing tool-result text unless a narrower internal result type is required;
- operational: one existing hosted `CI / verify` run must pass on the exact final M011 candidate before strict closure.

Target closure record:

- `plans/closure/agent-runtime-correctness-autonomy-simplification/011-status.md`

## 1. Objective

Finish the agent-runtime correctness/autonomy/simplification workstream with one narrow corrective pass that addresses the two defects exposed after M010:

1. current `main` is not green because the M010 bootstrap deletion left a stale empty unit test that fails `-D warnings` Clippy; and
2. the production tool-execution path still destroys known typed failure information by converting `Result<String, ToolError>` into rendered strings before recovery observes the result.

M011 must remove the stale verification artifact and preserve known execution status through the internal tool-result boundary so recovery consumes typed status wherever the executor already knows it. It then owns the first truthful strict closure decision backed by a green hosted run on the exact final candidate.

This is not another recovery redesign and not a general Tool Broker refactor.

## 2. Why M011 is required

M010 correctly deleted the dormant synthetic bootstrap, dead narration/retry branches, and the unbudgeted repository-specific continuation. Those structural corrections remain present on current `main`.

However, post-M010 evidence invalidates strict closure for two reasons.

### 2.1 Hosted verification is now an observed failure, not missing evidence

`plans/closure/agent-runtime-correctness-autonomy-simplification/010-status.md` conditionally closed M010 because no hosted run existed for the final candidate at the time of authorship.

A later push of current `main` did produce hosted run `31521674076`. The routine guard, formatting, and ownership steps passed, but Workspace Clippy failed before workspace tests ran.

The failure is in `src/agent/progress_recovery.rs`:

```rust
#[test]
fn autonomy_bootstrap_is_explicitly_one_shot() {
    let mut state = AutonomyState::default();
}
```

The bootstrap state it was meant to test was deleted by M010. The test now has no assertion, no behavioral value, and triggers both unused-variable and unused-mut warnings under the canonical hosted Clippy command.

This means the remaining M010 condition is no longer merely external/operational. The current tree contains a small, concrete verification defect.

### 2.2 Typed execution status is still erased before recovery

`ToolExecutionStatus` and `ToolExecutionOutcome` now exist, and typed success is preserved correctly. But the central native execution path still has `Result<String, ToolError>` available and later reduces every failure to:

```text
Err(e) -> "Error: {e}"
```

before returning `Vec<(String, String)>` from `execute_tool_calls()`.

Recovery subsequently reconstructs failures with `ToolExecutionOutcome::legacy(output)`, which classifies the rendered string by substrings such as `permission`, `denied`, `timeout`, and `cancel`.

That does not satisfy M010's acceptance criterion that typed execution status be consumed wherever the executor/broker already knows it.

The repository already has useful typed error distinctions in `ToolError`, including:

- `ToolError::Timeout`;
- `ToolError::Permission`;
- ordinary execution/format/disabled/I/O/network/not-found failures.

No new public error hierarchy is necessary to fix this boundary.

### 2.3 Why prior verification did not catch this

M010 focused tests proved that an explicitly constructed `ToolExecutionOutcome::success(...)` cannot be misclassified by misleading text, and local verification passed before the later merge state.

They did not prove that the ordinary production executor preserves `ToolError` classification all the way to recovery. The stale bootstrap test also survived because behavior-focused tests did not require removal of the obsolete empty test itself; stable hosted Clippy later exposed it.

## 3. Explicit non-goals

Do not:

- redesign `RecoveryController`, `AutonomyState`, provider retry, goal continuation, scheduling, or daemon lifecycle;
- reintroduce bootstrap, narration retry, or repository-specific continuation heuristics deleted by M010;
- change public tool names, schemas, model-facing tool result text, permission prompts, ACP/session protocol, or storage schema;
- create a new public `ToolError` taxonomy solely for this pass;
- refactor the Tool Broker, Tool trait, MCP service, plugin lifecycle, or question subsystem beyond the smallest result-plumbing changes needed to preserve already-known status;
- infer cancellation/denial/timeout from arbitrary prose when a typed branch already identifies the result;
- add a second recovery-state wrapper that duplicates an existing result type without reducing ambiguity;
- suppress the stale test warning with `#[allow(...)]`, underscore variables, or a dummy assertion; delete the obsolete test instead;
- add CI lanes, matrices, scheduled audits, cargo-audit gates, coverage gates, size gates, benchmark gates, release automation, or a fixed release cadence;
- make the unrelated all-features failures recorded by the separate Agent Runtime / Model Adaptation / ACP M017 closure part of this milestone unless one reproduces in the ordinary default hosted `CI / verify` path;
- run a broad all-features campaign solely for M011.

## 4. Invariants that cannot regress

- M010's synthetic bootstrap implementation remains physically absent.
- M010's dead `if false` recovery branches remain absent.
- Primary and follow-up loops retain one bounded post-tool continuation allowance through `AutonomyState`.
- Textual tool-call repair remains M002 adapter/profile-owned and bounded.
- Permission denial cannot cause base-palette restoration that broadens denied authority.
- The M009 broker-principal correction remains intact: broker principal identity matches the grant issuer principal, not the decision/grant ID.
- Workspace identity remains explicit and does not regress to process-global CWD authority.
- Model-facing tool-result text remains compatible even when recovery receives richer internal status.
- Provider transport retry remains separate from semantic recovery.
- Routine CI remains one bounded job and manual release remains unchanged.

## 5. Expected production-code changes

Inspect at minimum:

- `src/agent/loop.rs`;
- `src/agent/progress_recovery.rs`;
- `crates/codegg-core/src/error.rs`;
- the ordinary native/MCP/question execution branches inside `AgentLoop::execute_tool_calls()`;
- `tests/agent_loop_harness.rs` if an integration regression test belongs there;
- `architecture/agent.md` / `architecture/tool.md` only if the internal result contract needs documentation correction;
- `plans/closure/agent-runtime-correctness-autonomy-simplification/010-status.md` as historical predecessor evidence, not as a file to rewrite into success.

Expected implementation shape:

1. Delete the obsolete `autonomy_bootstrap_is_explicitly_one_shot` test entirely.
2. Preserve model-visible result text separately from recovery status at the `execute_tool_calls()` boundary.
3. Reuse `ToolExecutionOutcome` if it remains the simplest correct carrier. A tuple such as `(tool_call_id, ToolExecutionOutcome)` is acceptable if no additional metadata is needed. A tiny private execution-result struct is acceptable only if it avoids duplicated state and has a clear owner.
4. Map already-typed `ToolError` variants to `ToolExecutionStatus` before converting them to display text.
5. Keep `ToolExecutionOutcome::legacy(...)` only at genuinely opaque compatibility seams that expose rendered strings without a reliable typed status.
6. Where MCP timeout/cancellation/question timeout/cancellation branches themselves know the status, construct the corresponding typed outcome there rather than feeding their own generated message back into the substring classifier.
7. Preserve all current model-facing strings unless a test proves an existing string is itself incorrect. This pass is about internal authority/status, not response wording.
8. Make success/event/state-change calculations use typed success when the typed result is available instead of reparsing the rendered text.

## 6. Required internal status mapping

Use the narrowest existing information source.

At minimum, ordinary native execution must satisfy:

| Known execution result | Recovery status |
|---|---|
| `Ok(display)` | `Success` |
| `ToolError::Permission(_)` | `Denied` |
| `ToolError::Timeout(_)` | `Timeout` |
| `ToolError::NotFound(_)` | `ToolError` or `ProtocolError`, consistently documented/tested |
| `ToolError::Execution(_)` | `ToolError` unless the branch has a stronger explicit typed cause |
| `ToolError::Format(_)` | `ProtocolError` or `ToolError`, consistently documented/tested |
| `ToolError::Disabled(_)` | `ToolError` unless existing policy exposes a stronger typed denial contract |
| `ToolError::Io(_)` / `Network(_)` | `ToolError` |

Do not add a `Cancelled` public `ToolError` variant merely to satisfy the table. Use `Cancelled` only where an existing cancellation token/branch actually knows cancellation occurred. Otherwise preserve the existing error type and do not infer cancellation from arbitrary output text.

For MCP/question compatibility branches:

- a branch that itself fired a timeout must yield `Timeout`;
- a branch that itself observed cancellation must yield `Cancelled`;
- an opaque server-returned text failure may use the documented legacy classifier only if no typed MCP error classification is available at that boundary;
- an ordinary successful result containing words like `permission`, `denied`, `timeout`, or `cancelled` must remain `Success`.

## 7. Ordered work packages

### Work package A — Restore a truthful green baseline locally

1. delete the stale empty bootstrap test rather than weakening Clippy;
2. run the exact hosted Clippy command locally:

```bash
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --locked -- -D warnings
```

3. if another warning appears because the deleted bootstrap machinery left additional dead code, remove only the directly obsolete code; do not broaden into generic cleanup;
4. record the prior hosted run `31521674076` as failed predecessor evidence.

### Work package B — Preserve typed native tool outcomes

1. identify the point where `Result<String, ToolError>` is currently converted to a rendered string;
2. map the typed `ToolError` to `ToolExecutionStatus` before rendering;
3. return/carry both `model_text` and typed status to the primary and follow-up recovery paths;
4. use typed success for `AppEvent::ToolResult.success`, state-change/child-progress calculation, and recovery when available;
5. ensure the model still receives the same text it received before M011.

### Work package C — Narrow legacy string classification

1. enumerate every call to `ToolExecutionOutcome::legacy` and `tool_execution_status(rendered)`;
2. remove them from ordinary native typed paths;
3. for MCP/question/other compatibility branches, replace string inference with explicit status wherever the branch already knows timeout/cancel/error/success;
4. retain `legacy` only for a named opaque compatibility seam that truly lacks typed information;
5. document that seam in code and M011 closure evidence.

If no legitimate legacy seam remains, remove `tool_execution_status(rendered)` and `ToolExecutionOutcome::legacy` entirely rather than retaining speculative compatibility code.

### Work package D — Regression tests

Add or adjust deterministic tests proving at minimum:

- the obsolete bootstrap test no longer exists;
- ordinary native success is typed `Success`;
- `ToolError::Permission` reaches recovery as `Denied` without parsing display text;
- `ToolError::Timeout` reaches recovery as `Timeout` without parsing display text;
- ordinary execution failure reaches the chosen non-success typed status;
- a successful display string containing `permission denied`, `timeout`, and `cancelled` remains `Success` end-to-end through the ordinary executor/recovery boundary;
- a known MCP or question timeout/cancellation branch, if covered by this internal result type, emits the corresponding typed status without string inference;
- denied results cannot trigger base-palette restoration;
- primary and follow-up loops retain the single continuation bound;
- broker principal identity remains consistent with the grant issuer.

Prefer behavior tests over static source-regex guards. Do not add a new script merely to assert the obsolete test name is absent.

### Work package E — Verification and closure reconciliation

Run only the checks needed for this boundary:

```bash
cargo test -p codegg --lib agent::progress_recovery -- --nocapture
cargo test -p codegg --lib agent::r#loop::tests -- --nocapture
cargo test --test agent_loop_harness -- --test-threads=1
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
git diff --check
```

Run additional MCP/question focused tests only if their production branches are modified.

Then obtain one normal existing hosted `CI / verify` run on the exact final M011 candidate.

Do not add a workflow dispatch trigger or any new CI mechanism merely to manufacture evidence. The existing push/PR path is the evidence contract.

## 8. Concurrency, cancellation, restart, and failure semantics

Concurrency:

- keep existing parallel tool execution and semaphore limits unchanged;
- preserve result ordering by original tool-call index;
- adding typed status must not reorder results or serialize currently parallel calls.

Cancellation:

- if an existing cancellation branch can identify cancellation before rendering, propagate `Cancelled`;
- do not infer cancellation from successful model-facing output text;
- existing turn cancellation/steering checks remain authoritative.

Restart:

- no durable state is added;
- no database or replay migration is required.

Failure:

- typed failure classification is recovery metadata, not a replacement for existing user/model-visible error text;
- provider transport failure remains outside tool recovery;
- permission denial remains distinguishable from ordinary execution failure;
- timeout remains distinguishable where the executor already has a timeout branch.

## 9. Security and authorization review

Verify explicitly that:

- `ToolError::Permission` becomes `Denied` before any display-text conversion;
- recovery never converts a known denial into `Success` or generic retryable progress;
- `RestoreBasePalette` remains suppressed for typed denial;
- textual tool-call repair still routes through ordinary permission and broker execution;
- broker `principal_ref` remains bound to the actual principal used by the authority grant;
- workspace path/policy identity remains unchanged;
- no new tool authority or retry permission is introduced by the richer result type.

## 10. Storage, protocol, migration, compatibility, and observability

Storage:

- no schema change.

Protocol:

- no provider, ACP, daemon, session, Tool Program, or external Tool Broker protocol change is expected.

Migration:

- no user/operator action.

Compatibility:

- model-visible tool-result text should remain byte-for-byte compatible for the same execution result where practical;
- the internal result type may change because it is not a user-facing protocol;
- legacy string classification remains only if a concrete opaque compatibility seam requires it.

Observability:

- existing logs/events may continue to expose model-facing text;
- do not add a new telemetry subsystem;
- if an existing event has a success/error field, derive it from typed status where available.

## 11. Verification posture

M011 exists partly because hosted Clippy exposed a defect missed by the M010 closure process. Verification is therefore explicit but still minimal.

Required:

- focused recovery/loop tests;
- `agent_loop_harness`;
- exact workspace Clippy command used by CI;
- `scripts/verify.sh quick`;
- one existing hosted `CI / verify` run on the exact final candidate.

Not required:

- a new CI lane;
- a local all-features workspace test;
- the unrelated all-features M017 reproduction suite;
- cargo-audit, binary-size, coverage, fuzz, benchmark, scheduled, artifact, or release gates.

If the ordinary hosted `CI / verify` run fails on an unrelated pre-existing issue, record exact evidence and classify ownership. Do not mark M011 strictly closed unless the M011 acceptance criteria and the ordinary hosted gate are green on the accepted candidate.

## 12. Explicit acceptance criteria

M011 is complete only when every item below is true.

### Verification defect closure

- `autonomy_bootstrap_is_explicitly_one_shot` is deleted, not silenced or converted into a dummy test;
- `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --locked -- -D warnings` passes on the final candidate;
- the ordinary hosted `CI / verify` job passes on the exact final candidate and proceeds through Workspace tests rather than stopping at Clippy;
- the M011 closure record cites both failed predecessor run `31521674076` and the final green hosted run so the evidence transition is explicit.

### Typed outcome closure

- ordinary native execution no longer discards known `ToolError` classification before recovery;
- `ToolError::Permission` is observed by recovery as `Denied` without rendered-string parsing;
- `ToolError::Timeout` is observed by recovery as `Timeout` without rendered-string parsing;
- ordinary native non-permission/non-timeout failures are observed as an explicit non-success status chosen and documented consistently;
- known MCP/question timeout or cancellation branches use typed status when those branches are modified or already feed the common result boundary;
- successful model-facing output containing denial/timeout/cancellation words remains `Success` end-to-end;
- `ToolExecutionOutcome::legacy` / `tool_execution_status(rendered)` is absent from the ordinary typed native path;
- any remaining legacy classifier has a named, concrete opaque compatibility owner and a regression test; if no such owner exists, the classifier is deleted;
- model-facing tool-result text remains compatible;
- typed success, denial, and timeout are used consistently by recovery and any existing success/error event flag touched by this pass.

### M010 invariant preservation

- no synthetic bootstrap execution returns;
- no disabled `if false` recovery branch returns;
- no repository-specific second continuation returns;
- primary and follow-up loops retain the one bounded `AutonomyState` continuation allowance;
- textual tool-call repair remains bounded and adapter-owned;
- denied authority cannot be restored;
- broker principal and explicit workspace corrections remain intact.

### Planning and closure truthfulness

- `plans/closure/agent-runtime-correctness-autonomy-simplification/010-status.md` remains historical conditional evidence and is not rewritten to conceal the later hosted failure;
- `plans/closure/agent-runtime-correctness-autonomy-simplification/011-status.md` records the failed current-main run, implementation commits, focused checks, exact final hosted run, and unresolved findings by severity;
- the corrective addendum and registry identify M011 as the controlling strict closure milestone until its evidence is accepted;
- the workstream moves to `closed` only if no critical/high/medium finding remains in M011 scope and the exact final hosted run is green.

## 13. Stop conditions

Stop and report rather than broadening M011 if:

- preserving typed status requires a public protocol/schema migration rather than a private execution-result adjustment;
- a supported executor genuinely exposes only opaque rendered failure text and changing it would require redesigning that subsystem;
- the canonical hosted CI fails solely on an unrelated defect outside this workstream after all M011 checks pass;
- another branch merge materially changes the ordinary tool-execution result boundary while implementation is underway.

For an opaque executor, keep a clearly named compatibility fallback and document why typed information is unavailable. Do not redesign the entire executor to eliminate one legacy seam.

For an unrelated hosted failure, do not conceal it as green and do not expand M011 into the unrelated subsystem. Record ownership and leave strict closure conditional/blocked as appropriate.

## 14. Required closure evidence

`plans/closure/agent-runtime-correctness-autonomy-simplification/011-status.md` must include:

- baseline and implementation commit SHAs;
- failed predecessor hosted run `31521674076` / job `93879950640` and exact failure cause;
- final green hosted run ID/job ID on the exact accepted M011 candidate;
- before/after data-flow for native tool result: executor `Result<String, ToolError>` -> internal typed outcome -> model text + recovery status;
- mapping table for each `ToolError` variant relevant to recovery;
- inventory of every remaining `ToolExecutionOutcome::legacy` / `tool_execution_status(rendered)` call and justification for each, or evidence that none remain;
- focused test, workspace Clippy, `scripts/verify.sh quick`, and `git diff --check` outcomes;
- M010 structural-invariant regression evidence;
- broker-principal/workspace regression evidence;
- compatibility statement confirming model-facing result text/protocol did not intentionally change;
- unresolved findings classified by severity and owner;
- recommendation: `closed`, `conditionally closed`, `corrective pass required`, or `blocked`.

A green Clippy run alone is insufficient. Strict closure requires both the typed-result acceptance criteria and the exact hosted `CI / verify` success.
