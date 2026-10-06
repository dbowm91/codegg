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
| Integration tests | `tests/approval_reviewer.rs`, `tests/agent_loop_harness.rs` (`approval_automatic_reviewer_*`) |

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
   with a read-only reviewer context rooted to the workspace. Any other
   tool (bash, terminal, edit/write/patch, task/subagent, network tools)
   is denied without execution and counts against the budget.
5. Default maximum 2 investigation calls, hard cap 3; small output
   budget; 30s default deadline (1s–120s). Stale policy/sandbox (request
   revision or profile differing from the captured snapshot, or a
   mid-review change observed before applying Allow) discards the verdict.
6. Deny carries bounded `primary_agent_feedback` back to the primary
   model as the tool outcome. Repeated equivalent denials (keyed by
   tool+path+summary hash) stop re-invoking the reviewer after the
   configured bound (default 3) and defer/deny instead.
7. Receipts (`ReviewerReceipt`: request/decision IDs, model id,
   verdict, bounded risk/reason, policy revision, investigation count,
   elapsed ms) are secret-free and logged; hidden reasoning is never
   persisted.

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
   Automatic; only the async reviewer path may return Allow/Deny.
2. `ApprovalMode` and `SandboxProfile` stay orthogonal: reviewer input
   carries the snapshot profile and any drift invalidates the verdict.
3. No pending reviewer request survives restart or cancellation; the
   original action must be re-evaluated against current policy.
4. `headless_deny = false` preserves interactive human fallback; set it
   only for genuinely noninteractive operation.

## Testing

```bash
cargo test --test approval_reviewer
cargo test --test agent_loop_harness -- reviewer
cargo test --test agent_loop_harness -- approval
cargo test -p codegg --lib permission::reviewer
python3 scripts/check_approval_reviewer.py
```

## Related Docs

- [permission.md](permission.md) — Router contract and M006 item
- [security.md](security.md) — Approval-vs-security boundary, M006 note
- [tool.md](tool.md) — Registry execution used by the investigator
- [session.md](session.md) — Durable runtime preference ownership

## Source verification

Verified 2026-10-06 against `src/permission/reviewer.rs`,
`src/permission/approval.rs`, `src/agent/tool_batch.rs`, `src/agent/loop.rs`,
`src/tool/mod.rs`, `crates/codegg-config/src/schema.rs`, and
`scripts/check_approval_reviewer.py`. No `file.rs:line` references in this doc
to correct.

- **Corrected the invocation trigger.** The doc claimed the reviewer is invoked
  "only" for `Escalate`. It is also reached from the general policy `Ask`
  branch: `PermissionResult::Ask(_)` returns early into
  `resolve_general_ask_via_human` (`src/agent/tool_batch.rs:516`), which under
  `ApprovalMode::Automatic` calls the same
  `resolve_automatic_escalation` helper. The opening, the flow diagram, and
  item 1 now say so; the `Escalate`-only claim is otherwise identical to the
  module header comment in `reviewer.rs:3`-`:8`, which still overstates it.
- Added the omitted `ReviewerReceipt.elapsed_ms` field (item 7). The struct has
  8 fields, not the 7 listed.

Verified accurate as written:

| Claim | Source |
|---|---|
| `REVIEWER_ALLOWED_TOOLS` = `read`/`glob`/`grep`/`list`/`diff`/`git_read` (6) | `reviewer.rs:49` |
| `ReviewerVerdict` = `Allow` / `Deny` / `DeferUser` | `reviewer.rs:313`-`:326` |
| `ReviewerConfig` fields and the TOML block | `reviewer.rs:76`-`:83`; `schema.rs:1255`-`:1277` (identical) |
| default 2 investigation calls, hard cap 3 | `reviewer.rs:52`, `:54`, `:110`; `schema.rs` `.min(3)` |
| `deadline_ms` default 30_000, clamped `1_000..=120_000` | `reviewer.rs:56`, `:115` |
| `max_output_chars` default 4_000, clamped `512..=16_384` | `reviewer.rs:58`, `:120` |
| `headless_deny` default `false` | `reviewer.rs:91`; `schema.rs` `unwrap_or(false)` |
| `max_equivalent_denials` default 3, clamped `1..=10` | `reviewer.rs:60`, `:130`, `:932` |
| `REVIEWER_ALLOW`/`REVIEWER_DENY`/`REVIEWER_DEFER` in the `source` mod | `approval.rs:34`, `:48`-`:50` |
| one bounded non-tool call: `tools: None`, `temperature: Some(0.0)`, `max_tokens: Some(512)`, `ResponseFormat::JsonObject` | `reviewer.rs:854`-`:859` |
| investigation served through `ToolRegistry::execute_capture`, read-only context rooted to the workspace | `reviewer.rs:772`-`:786`; `execute_capture` is declared in `src/tool/mod.rs:1249` (not `src/agent/registry.rs`) |
| forbidden tools denied without execution **and** charged to the budget | `reviewer.rs:757`-`:761` returns `ForbiddenTool`; `run_review_loop` (`reviewer.rs:1115`-`:1126`) converts it to evidence text and still `history.push`es it at `:1122`, so `history.len()` advances |
| stale policy/sandbox discards the verdict, including a mid-review re-check before applying `Allow` | `is_stale` `reviewer.rs:1139`; pre-loop check `:1003`; pre-`Allow` re-check `:1034`-`:1035` |
| deadline enforced by `tokio::time::timeout` | `reviewer.rs:1020` |
| denial key = tool + path + hashed summary | `denial_key_for` `reviewer.rs:950`; consumed `tool_batch.rs:651` |
| `reviewer_denial_counts` per-loop field | `loop.rs:113`, initialized `loop.rs:413`, read `tool_batch.rs:657`, written `tool_batch.rs:782` |
| `ApprovalRouter::route_escalation` never auto-allows for `Automatic` | `approval.rs:252`-`:261` (explicit comment: only the async reviewer may allow/deny) |
| reviewer cannot recurse into the router, change mode/profile, or spawn subagents | `scripts/check_approval_reviewer.py` forbidden patterns `PermissionPending`, `ApprovalRouter`, `set_approval_mode`, `set_sandbox_profile`, `TaskTool`, `SubAgent`, `tool::bash`, … |
| `provider/model` with an unknown provider ⇒ reviewer unavailable | `ReviewerConfig::resolve_model` `reviewer.rs:163`-`:180` |
| `tests/approval_reviewer.rs` (9 `#[test]`) and the 2 `approval_automatic_reviewer_*` cases in `tests/agent_loop_harness.rs` (`:4647`, `:4728`) | Both files exist; the listed `cargo test` targets are valid |
