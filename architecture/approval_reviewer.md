# Approval Reviewer (M006)

The Automatic approval reviewer is a dedicated, fast, bounded, read-only
authorization helper. It is invoked **only** for actions deterministic
CodeGG policy already classified as `Escalate` while the effective
`ApprovalMode` is `Automatic`. It returns a strict
Allow/Deny/DeferUser verdict, may attach concise feedback for the primary
model on denial, and can never broaden the sandbox or authorization
ceiling.

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
Escalate x Automatic ──► reviewer (bounded) ──► Allow / Deny+feedback / Defer
                                                     │                 │
                              stale/cancel/failure ──┘                 ▼
                              Defer→human (interactive) / Deny (headless)
```

1. `tool_batch.rs` normalizes deterministic policy/security into
   `Allow | Deny | Escalate` exactly as before. Only `Escalate` with an
   `Automatic` snapshot reaches the reviewer.
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
   verdict, bounded risk/reason, policy revision, investigation count)
   are secret-free and logged; hidden reasoning is never persisted.

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
