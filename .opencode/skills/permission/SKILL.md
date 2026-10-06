---
name: permission
description: Permission levels, rules, approval modes, sandbox containment, and the bounded automatic reviewer in codegg
version: 1.0.0
tags:
  - permission
  - approval
  - sandbox
  - security
---

# Permission Module Guide

Operational guide for changing `src/permission/` and the approval/sandbox
surfaces around it. The full contracts live in `architecture/permission.md`,
`architecture/approval_reviewer.md`, and `architecture/security.md`; this
skill covers the ordering and isolation invariants that are easy to break.

## Layout

| Path | Purpose |
|------|---------|
| `src/permission/mod.rs` | `PermissionLevel`, `PermissionRuleset`, `ToolRule`, `PermissionChecker`, `PermissionStore`, `DoomLoopDetector` |
| `src/permission/modes.rs` | `ModeDefinition` + `BuiltinModes` (`review`, `debug`, `docs`) and `mode_ruleset` |
| `src/permission/reviewer.rs` | Bounded automatic reviewer used by `ApprovalMode::Automatic` |
| `src/permission/approval.rs` | Approval plumbing shared with the router |
| `crates/codegg-core/src/approval.rs` | `ApprovalMode` (`Interactive` default, `Automatic`, `Yolo`) |
| `crates/codegg-core/src/work_order/model.rs` | `SandboxRequest` (`ReadOnly`, `WorkspaceWrite`, `FullHost`) |
| `crates/codegg-core/src/bus/mod.rs` | `PermissionRegistry`, `QuestionRegistry` |

## Key Invariants

- **The registries are synchronous.** `PermissionRegistry::register`
  (`crates/codegg-core/src/bus/mod.rs:104`), `respond` (`:132`), and
  `QuestionRegistry::register` (`:268`) / `answer_question` (`:296`) are plain
  `fn`, not `async`. They hand out `oneshot::Sender`s.
- **Register before publishing.** Register the responder FIRST, then publish
  the `Pending` event. Publishing first leaves a window where the human answers
  before anyone is listening and the answer is dropped.
- **Prefer the scoped variants.** `respond_scoped` (`:143`) and
  `answer_question_scoped` (`:305`) key on `(session_id, id)`. Use them
  whenever a session identity is available so a stale answer cannot land in a
  different session.
- **Approval and sandbox are orthogonal axes.** `ApprovalMode` controls who
  answers an escalation; `SandboxRequest` controls what containment the
  process runs under. Never derive one from the other.
- **Ceilings still apply in `Yolo`.** `yolo` auto-allows escalations *within
  the authority ceiling*; an explicit deterministic `Deny` still denies.
- **`Automatic` is never silent Yolo.** With no reviewer model configured,
  `Automatic` defers to the human.

## The Automatic Reviewer

`ApprovalMode::Automatic` routes through
`resolve_automatic_escalation` (`src/permission/reviewer.rs:992`). It has
**two** entry paths, both handled by the same helper:

1. the `Escalate` path directly, and
2. the `Ask` path via `resolve_general_ask_via_human`, reached from the
   `Automatic` arm in `src/agent/tool_batch.rs:516`.

The helper never accepts `Allow`/`Deny` as an input
(`src/permission/reviewer.rs:983`) — it either returns a bounded
allow/deny decision of its own or falls back to the human.

Its bounds are all explicit builder methods
(`src/permission/reviewer.rs:103-167`): `with_model`,
`with_max_investigation_calls`, `with_deadline_ms`, `with_max_output_chars`,
`with_headless_deny`, `with_max_equivalent_denials`. The reviewer runs in
isolation: it never re-decides a deterministic hard `Deny`, and
`is_reviewer_tool_allowed` (`:71`) restricts which tools it may use.

## Guards

Run these after touching approval, reviewer, or sandbox behavior:

```bash
python3 scripts/check_approval_reviewer.py
python3 scripts/check_approval_router.py
python3 scripts/check_sandbox_contract.py
python3 scripts/check_sandbox_policy_wiring.py
```

## Testing

```bash
cargo test -p codegg permission::
cargo test -p codegg-core approval
```

New `#[tokio::test]`s default to `current_thread`; use
`flavor = "multi_thread", worker_threads = 2` only for real concurrency.

## See Also

- `architecture/permission.md` — authoritative permission contract
- `architecture/approval_reviewer.md` — reviewer contract
- `architecture/authorization.md` — per-operation authority matrix
- `architecture/security.md` — sandbox profiles and containment
- `.opencode/skills/bus-projection/SKILL.md` — the registries' bus/event side
- `.opencode/skills/security-semantics/SKILL.md` — deterministic risk signals

## Source verification

Verified 2026-10-06 against `src/permission/{mod,modes,reviewer,approval}.rs`,
`crates/codegg-core/src/approval.rs`,
`crates/codegg-core/src/work_order/model.rs`,
`crates/codegg-core/src/bus/mod.rs`, `src/agent/tool_batch.rs:508-522`, and
`architecture/permission.md`. Corrected the reviewer's entry-path description
(it runs on `Ask` as well as `Escalate`) and pinned the synchronous registry
signatures and their exact lines. Claims without a traceable source were
removed rather than guessed.