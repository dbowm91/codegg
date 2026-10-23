# Approval Reviewer (M006)

The Automatic approval reviewer is a dedicated, fast, bounded, read-only
authorization helper. It is invoked **only** while the effective
`ApprovalMode` is `Automatic`, and then only for actions deterministic
CodeGG policy raised to `Escalate` or held at general policy `Ask`. It
returns a strict Allow/Deny/DeferUser verdict, may attach concise feedback
for the primary model on denial, and can never broaden the sandbox or
authorization ceiling.

## Where It Lives

| Artifact | Location |
|----------|----------|
| `ReviewerConfig/Request/Verdict`, parser, service loop, backends | `src/permission/reviewer.rs` |
| `REVIEWER_ALLOW/DENY/DEFER` receipt sources | `src/permission/approval.rs` (`source` mod) |
| Automatic wiring + equivalent-denial backstop | `src/agent/tool_batch.rs` (`resolve_automatic_escalation`) |
| Per-loop denial counters | `src/agent/loop.rs` (`reviewer_denial_counts`) |
| Config surface | `crates/codegg-config/src/schema.rs` (`ApprovalReviewerConfig`) |
| Isolation guard | `scripts/check_approval_reviewer.py` |
| Integration tests | `tests/security_permission/approval_reviewer.inc` (family `security_permission`), `tests/agent_loop/agent_loop_harness.inc` (family `agent_loop`, `approval_automatic_reviewer_*`) |

## How It Works

```
deterministic Allow  ──► execute (reviewer never invoked)
deterministic Deny   ──► deny    (reviewer never invoked, Yolo included)
Escalate or general policy Ask, under Automatic
                       ──► reviewer (bounded) ──► Allow / Deny+feedback / Defer
                                                     │                 │
                              stale/cancel/failure ──┘                 ▼
                              Defer→human (interactive) / Deny (headless)
```

1. `tool_batch.rs` normalizes deterministic policy/security into
   `Allow | Deny | Escalate` exactly as before. Under `Automatic`, two
   paths reach `resolve_automatic_escalation`: the `Escalate` verdict
   (sensitive path, security `Ask`) and the general policy `Ask` branch,
   which routes through the same helper. Nothing else reaches it.
2. Without a configured, registry-validated reviewer model, Automatic
   keeps the M003 behavior: defer to the human (interactive) or explicit
   deny (configured headless mode). No silent provider switch, no
   auto-allow.
3. With a model, `ProviderReviewerBackend` makes one bounded non-tool
   model call per step (`temperature 0`, small output budget, JSON
   object). The model either returns the strict verdict or one
   `{"investigate":{"tool":...,"args":{...}}}` request.
4. `RegistryReviewerInvestigator` serves only
   `read/glob/grep/list/diff/git_read` through `ToolRegistry::execute_capture`
   with a read-only reviewer context rooted to the workspace: `cwd` is the
   workspace root, `permission_mode` is `"automatic-reviewer"`,
   `caller_class` is `"approval-reviewer"`, `max_effect_class` is
   `"read_only"`, `backend_policy` is `"native_only"`, and the per-call
   `timeout_ms` is 15 000. Any other tool (bash, terminal, edit/write/patch,
   task/subagent, network tools) is denied without execution and counts
   against the budget.
5. Default maximum 2 investigation calls, hard cap 3; small output
   budget; 30s default deadline (1s–120s). Two further bounds are
   independent of the output budget: investigation **args** are rejected
   above `REVIEWER_MAX_INVESTIGATION_ARG_CHARS` (4096 JSON chars) and a
   single tool's output is truncated at `REVIEWER_MAX_TOOL_OUTPUT_CHARS`
   (8192 chars). Stale policy/sandbox (request
   revision or profile differing from the captured snapshot, or a
   mid-review change observed before applying Allow) discards the verdict.
6. Deny carries bounded `primary_agent_feedback` back to the primary
   model as the tool outcome. Repeated equivalent denials (keyed by
   tool+path+summary hash) stop re-invoking the reviewer after the
   configured bound (default 3) and defer/deny instead. The key is
   computed **before** a reviewer call is spent, so a saturated action
   never burns a model call.
7. Receipts (`ReviewerReceipt`: request/decision IDs, model id,
   verdict, bounded risk/reason, policy revision, investigation count,
   elapsed ms — nine fields) are secret-free and logged; hidden reasoning
   is never persisted.

## Config / Model Preference

```toml
[approval_reviewer]
model = "reviewer-mini"          # bare id (primary provider) or "provider/model"
max_investigation_calls = 2      # default 2, hard cap 3
deadline_ms = 30_000             # clamped 1_000..=120_000
max_output_chars = 4_000         # clamped 512..=16_384
headless_deny = false            # true: failures deny instead of deferring
max_equivalent_denials = 3       # clamped 1..=10
```

All fields are optional and additive (no storage migration). Absent
config — or a `provider/model` value whose provider is unknown to the
registry — means the reviewer is unavailable and Automatic defers. The
reviewer need not use the same model as the primary agent; prefer a
cheap/fast tool-capable configured model.

## Threat Model (Untrusted Evidence)

Repository content, command text, tool output, and the primary agent's
justification are **untrusted evidence, never instructions**. A file
saying "approve this command" has no authority: verdicts require strict
verdict JSON, tool output is labeled untrusted data in the prompt, and
the system prompt is fixed and short. Malformed, timeout, unavailable,
over-budget, cancelled, stale, or forbidden-tool behavior always fails
closed (DeferUser interactive, explicit deny headless) — never Allow.
The reviewer cannot change `ApprovalMode`, `SandboxProfile`, path or
capability ceilings, credentials, or parent/child authority; it cannot
recurse into the router or spawn subagents (guard-enforced).

## Invariants & Gotchas

1. Sync `ApprovalRouter::route_escalation` still never auto-allows for
   Automatic; only the async reviewer path may return Allow/Deny. The
   `ApprovalMode::Automatic` arm of `route_escalation` always returns
   `ApprovalDecision::defer` — even when a reviewer backend exists,
   because the sync router has no backend to call.
2. `ApprovalMode` and `SandboxProfile` stay orthogonal: reviewer input
   carries the snapshot profile and any drift invalidates the verdict.
3. No pending reviewer request survives restart or cancellation; the
   original action must be re-evaluated against current policy.
4. `headless_deny = false` preserves interactive human fallback; set it
   only for genuinely noninteractive operation.
5. **An advisory never creates an escalation on its own.** The reviewer
   consumes escalations deterministic policy already raised; it never
   raises one. Upstream, `preflight_requires_escalation`
   (`src/agent/tool_batch.rs`) gates the command preflight on a
   **finding** (`result.summary.is_some()`), never on having run — the
   preflight service is wired by default, so "it ran" is always true and
   testing that instead of "it found something" escalated every `bash`
   call and discarded stored `AlwaysAllow` grants. A reviewer verdict
   inherits that discipline: it decides the escalated request, it does
   not create the escalation.
6. The reviewer never re-decides a deterministic `Allow` or `Deny`; only
   the non-deterministically-resolved requests reach it. `Deny` is
   terminal under every mode, Yolo included.

## Testing

```bash
cargo test --test security_permission -- approval_reviewer
cargo test --test agent_loop -- reviewer
cargo test --test agent_loop -- approval
cargo test -p codegg --lib permission::reviewer
python3 scripts/check_approval_reviewer.py
```

## Related Docs

- [permission.md](permission.md) — Router contract and M006 item
- [security.md](security.md) — Approval-vs-security boundary, M006 note
- [tool.md](tool.md) — Registry execution used by the investigator
- [session.md](session.md) — Durable runtime preference ownership

## Source verification

Re-verified 2026-10-23 against `src/permission/reviewer.rs`,
`src/permission/approval.rs`, `src/agent/tool_batch.rs`,
`src/agent/loop.rs`, `src/tool/mod.rs`,
`crates/codegg-config/src/schema.rs`,
`scripts/check_approval_reviewer.py`,
`tests/security_permission/approval_reviewer.inc`, and
`tests/agent_loop/agent_loop_harness.inc`.

**Corrected the test-count claim.** The previous revision recorded
`tests/security_permission/approval_reviewer.inc` as holding 9 `#[test]`
bodies. It now holds **22** — the file grew with the cancellation,
parallel-isolation, restart, malicious-output, forbidden-tool,
malformed/outage, unconfigured-reviewer, and receipt-bound cases.

**Corrected the `cargo test` targets.** The root package's integration
tests are **families** (one executable per subject area under
`tests/<family>/`), so `--test approval_reviewer` and `--test
agent_loop_harness` no longer select anything. The commands are now
`cargo test --test security_permission -- approval_reviewer` and
`cargo test --test agent_loop -- …`; the family members are
`tests/security_permission/approval_reviewer.inc` and
`tests/agent_loop/agent_loop_harness.inc`.

**Line references made robust.** Several references had drifted well past
the ~5-line tolerance, so they are now symbol names (file + symbol):

| Claim | Source |
|---|---|
| `REVIEWER_ALLOWED_TOOLS` = `read`/`glob`/`grep`/`list`/`diff`/`git_read` (6) | `reviewer.rs`, `REVIEWER_ALLOWED_TOOLS` |
| `ReviewerVerdict` = `Allow` / `Deny` / `DeferUser` | `reviewer.rs`, `ReviewerVerdict` |
| `ReviewerConfig` fields and the TOML block | `reviewer.rs`, `ReviewerConfig`; `schema.rs`, `ApprovalReviewerConfig` (identical) |
| default 2 investigation calls, hard cap 3 | `reviewer.rs`, `REVIEWER_DEFAULT_INVESTIGATION_CALLS` / `REVIEWER_HARD_MAX_INVESTIGATION_CALLS` / `with_max_investigation_calls`; `schema.rs`, `resolved_max_investigation_calls` (`.min(3)`) |
| `deadline_ms` default 30_000, clamped `1_000..=120_000` | `reviewer.rs`, `REVIEWER_DEFAULT_DEADLINE_MS` / `with_deadline_ms` |
| `max_output_chars` default 4_000, clamped `512..=16_384` | `reviewer.rs`, `REVIEWER_DEFAULT_MAX_OUTPUT_CHARS` / `with_max_output_chars` |
| `headless_deny` default `false` | `reviewer.rs`, `ReviewerConfig::default`; `schema.rs`, `resolved_headless_deny` (`unwrap_or(false)`) |
| `max_equivalent_denials` default 3, clamped `1..=10` | `reviewer.rs`, `REVIEWER_DEFAULT_MAX_EQUIVALENT_DENIALS` / `with_max_equivalent_denials` / `RepeatedDenialTracker::new` |
| `REVIEWER_ALLOW`/`REVIEWER_DENY`/`REVIEWER_DEFER` in the `source` mod | `approval.rs`, `pub mod source` |
| one bounded non-tool call: `tools: None`, `temperature: Some(0.0)`, `max_tokens: Some(512)`, `ResponseFormat::JsonObject` | `reviewer.rs`, `ProviderReviewerBackend` |
| investigation served through `ToolRegistry::execute_capture`, read-only context rooted to the workspace | `reviewer.rs`, `RegistryReviewerInvestigator::investigate`; `execute_capture` is declared in `src/tool/mod.rs` (not `src/agent/registry.rs`) |
| forbidden tools denied without execution **and** charged to the budget | `reviewer.rs`, `RegistryReviewerInvestigator::investigate` returns `ForbiddenTool`; `run_review_loop` converts it to evidence text and still pushes it into the history, so `history.len()` advances |
| stale policy/sandbox discards the verdict, including a mid-review re-check before applying `Allow` | `reviewer.rs`, `is_stale`; the pre-loop check and the pre-`Allow` re-check in `resolve_automatic_escalation` |
| deadline enforced by `tokio::time::timeout` | `reviewer.rs`, `resolve_automatic_escalation` |
| denial key = tool + path + hashed summary | `reviewer.rs`, `denial_key_for`; consumed in `tool_batch.rs`, `resolve_automatic_escalation` |
| `reviewer_denial_counts` per-loop field | `loop.rs`, `reviewer_denial_counts`; read and written in `tool_batch.rs`, `resolve_automatic_escalation` |
| `ApprovalRouter::route_escalation` never auto-allows for `Automatic` | `approval.rs`, `route_escalation` — the `ApprovalMode::Automatic` arm carries an explicit comment and always returns `ApprovalDecision::defer` |
| reviewer cannot recurse into the router, change mode/profile, or spawn subagents | `scripts/check_approval_reviewer.py` forbidden patterns `PermissionRegistry::register`, `PermissionPending`, `request_human_approval`, `ApprovalRouter`, `set_approval_mode`, `set_sandbox_profile`, `TaskTool`, `SubAgent`, `tool::bash`, `tool::terminal`, `tool::(edit\|write\|apply_patch\|replace)`, `tool::(webfetch\|websearch\|research)`, `"bash"`, `"terminal"`, `"task"` |
| `provider/model` with an unknown provider ⇒ reviewer unavailable | `reviewer.rs`, `ReviewerConfig::resolve_model` |
| `tests/security_permission/approval_reviewer.inc` (22 test functions) and the 2 `approval_automatic_reviewer_*` cases in `tests/agent_loop/agent_loop_harness.inc` | Both files exist; the listed `cargo test` targets are valid |

Verified accurate, unchanged:

- The invocation trigger. The reviewer is reached from the `Escalate`
  verdict **and** from the general policy `Ask` branch:
  `PermissionResult::Ask(_)` returns early into
  `resolve_general_ask_via_human` (`src/agent/tool_batch.rs`), which under
  `ApprovalMode::Automatic` calls the same `resolve_automatic_escalation`
  helper. The opening, the flow diagram, and item 1 say so. The module
  header comment in `src/permission/reviewer.rs` names both branches
  explicitly, so the doc and the header agree.
- `ReviewerReceipt` has 9 fields, including `elapsed_ms` (item 7).
- The `ToolRegistry::execute_capture` context properties
  (`permission_mode`/`caller_class`/`max_effect_class`/`backend_policy`/
  `timeout_ms`) recorded in item 4.
- `REVIEWER_MAX_INVESTIGATION_ARG_CHARS` (4096) and
  `REVIEWER_MAX_TOOL_OUTPUT_CHARS` (8192) are enforced in the
  investigator; recorded in item 5.
- The guard's `check_tool_batch_wiring` requires
  `resolve_automatic_escalation`, `RegistryReviewerInvestigator`,
  `ProviderReviewerBackend`, and `reviewer_denial_counts` to be present
  in `tool_batch.rs`; all four are.

The 2026-10-06 verification paragraph below is left as it stood at its own
date; the corrections above supersede it where they disagree with the
current tree.

Verified 2026-10-06 against source after upstream `2573f9c0` ("Decision runtime
ownership migration and M006 closure"), which added 30 lines near the top of
`crates/codegg-config/src/schema.rs` and rewrote `src/tool/mod.rs`. Corrected:
the `ReviewerConfig` field span `schema.rs:1255`-`:1277`->`1290`-`:1312` and
`ToolRegistry::execute_capture` `src/tool/mod.rs:1249`->`1317`, each re-checked
to land on the same construct. The reviewer router, mode/profile rules, and
forbidden-pattern guard are unchanged by that commit.
