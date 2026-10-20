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
- **Native file mutations use authenticated roots.** `write`, `edit`,
  `apply_patch`, and `replace` may write inside the resolved workspace or the
  session's private/shared scratch grants without the general permission
  default, including `permission.default = "ask"`. Explicit tool/path `Ask`
  and `Deny` rules still win. Keep scratch authority session scoped and host
  validated; shell and Git do not inherit it.
- **Carry the accepted decision through execution.** Agent tool calls pass an
  immutable `ExecutionAuthorizationContext` (snapshot, receipt, bounded
  command-preflight result) through the broker and retries. Bash must consume
  that preflight result once and refuse a profile mismatch or unavailable
  required filesystem enforcement before dispatch.
- **Keep audit claims tied to obtained facts.** Command events may carry the
  accepted receipt/source/revision, requested profile, bounded route/risk
  labels, and the actual filesystem/network outcome. Landlock-only network is
  `unrestricted`; a timeout without a returned result is not an enforcement
  claim. Record digests and bounded labels only, never command text or output.
- **Sandbox backend names do not imply parity.** Linux Landlock is a filesystem
  allowlist with `no_new_privs`; macOS Seatbelt confines writes but allows broad
  reads, network access, inherited descriptors, and Apple Events / Launch
  Services channels. Keep backend limits visible in `architecture/security.md`.
- **`Automatic` is never silent Yolo.** With no reviewer model configured,
  `Automatic` defers to the human.
- **Scratch recovery is advisory.** A structured write to a known host temp
  root may suggest the current private scratch path after path authorization
  rejects it. Never rewrite the request, expose an ungranted shared path, or
  let a suggestion override explicit Ask/Deny. Structured reads and shell
  commands keep their normal semantics.
- **The model is answered before the human for a temp-root write.** When the
  pending decision is the *implicit default* `Ask` and the target is a
  structured `write` of a known host temp root outside every authorized root,
  the agent loop refuses the call with that same actionable message and lets
  the model continue. A model that repeats the same path is not offered again
  and reaches the human. The offer replaces only the implicit default: an
  operator tool rule, path rule, stored decision, or hard deny still asks the
  human directly. It is scoped to `write`, because the edit family cannot
  write into a scratch root. See `architecture/permission.md`.

## The Automatic Reviewer

`ApprovalMode::Automatic` routes through
`resolve_automatic_escalation` (`src/permission/reviewer.rs:992`). It has
**two** entry paths, both handled by the same helper:

1. the `Escalate` verdict path directly, from the `Automatic` arm of the
   mode match (`src/agent/tool_batch.rs:450`), and
2. the general policy `Ask` path via `resolve_general_ask_via_human`
   (`src/agent/tool_batch.rs:496`), whose own `Automatic` arm calls the same
   helper at `:516`.

The helper never accepts `Allow`/`Deny` as an input
(`src/permission/reviewer.rs:983`) — it either returns a bounded
allow/deny decision of its own or falls back to the human.

Its bounds are all explicit builder methods on `ReviewerConfig`
(`src/permission/reviewer.rs:103-137`): `with_model`,
`with_max_investigation_calls`, `with_deadline_ms`, `with_max_output_chars`,
`with_headless_deny`, `with_max_equivalent_denials`. Each clamps rather than
trusting the caller — e.g. `with_deadline_ms` clamps to
1_000–120_000 ms, `with_max_output_chars` to 512–16_384, and
`with_max_equivalent_denials` to 1–10. A blank/over-long/NUL-bearing
`with_model` input silently disables `preferred_model` rather than erroring.
The reviewer runs in isolation: it never re-decides a deterministic hard
`Deny`, and `is_reviewer_tool_allowed` (`:71`) restricts it to
`REVIEWER_ALLOWED_TOOLS` (`:53`).

## Guards

Run these after touching approval, reviewer, or sandbox behavior:

```bash
python3 scripts/check_approval_reviewer.py
python3 scripts/check_approval_router.py
python3 scripts/check_sandbox_contract.py
python3 scripts/check_sandbox_policy_wiring.py
```

Only `check_sandbox_contract.py` is wired into `scripts/verify.sh quick` and
CI today; the other three are manual, so run them by hand or a drifted
reviewer/router boundary will pass CI unnoticed.

## Testing

```bash
cargo test -p codegg permission::
cargo test --test approval_reviewer     # reviewer isolation contract (9 tests)
cargo test --test sandbox_landlock
cargo test --test sandbox_policy_wiring
cargo test -p codegg-core approval
```

New `#[tokio::test]`s default to `current_thread`; use
`flavor = "multi_thread", worker_threads = 2` only for real concurrency.

## See Also

- `architecture/permission.md` — authoritative permission contract
- `architecture/approval_reviewer.md` — reviewer contract
- `architecture/authorization.md` — per-operation authority matrix
- `architecture/security.md` — sandbox profiles and containment, command
  classification, and the deterministic security-scanning signals that the
  permission tiering consumes
- `architecture/audit.md` — structural command execution metadata and audit
  evidence boundaries
- `.opencode/skills/bus-projection/SKILL.md` — the registries' bus/event side
- `docs/security-semantics.md` — user-facing escalation semantics

## Source verification

Re-verified 2026-10-06 against `src/permission/{mod,modes,reviewer,approval}.rs`,
`crates/codegg-core/src/approval.rs`,
`crates/codegg-core/src/work_order/model.rs`,
`crates/codegg-core/src/bus/mod.rs`, `src/agent/tool_batch.rs:405-450,496-531`,
and `scripts/verify.sh` + `.github/workflows/ci.yml`. The synchronous
registry signatures and every pinned line number re-confirmed. Corrected the
reviewer builder range (`:103-137`, not `:103-167`), added the per-builder
clamps and the `REVIEWER_ALLOWED_TOOLS` allowlist, recorded that only
`check_sandbox_contract.py` runs in `verify.sh quick`, and added the
`approval_reviewer` / sandbox integration test targets. Claims without a
traceable source were removed rather than guessed.
