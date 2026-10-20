# Managed Agent Scratch Milestone M005 — Temp-root redirect offer before human approval

Status: closed — see `plans/closure/managed-agent-scratch/005-status.md` (implementation `39e1445b`, merged `e52695b9`; live observation recorded in closure §4 and §13)
Repository baseline: `fe2c1306` on `main`

Source roadmap: `plans/subsystems/managed-agent-scratch-roadmap.md#7-milestones`
Source predecessor: `plans/implementation/managed-agent-scratch/003-permission-guidance-and-lifecycle-qualification.md`
Source predecessor closure: `plans/closure/managed-agent-scratch/003-status.md`
Long-term requirements: `plans/000-long-term-specification.md` (agent operability and authority), `plans/002-long-term-roadmap.md`
Applicable ADRs: None required — this milestone changes the *order* of an
existing escalation, not the authority model. The human remains the only
authority for the original location; nothing is auto-approved.
Primary class: capability (model-visible behaviour) with an invariant
component (the original path stays unauthorized).

## 1. Objective

When a structured write targets a recognized host temporary root that is not
an authorized root, give the model one actionable redirect offer **before**
the human is asked. If the model follows the suggestion, no human is
involved. If the model insists on the same restricted path, the normal human
approval path runs exactly as before.

## 2. Why this milestone is ready

M003 shipped the advisory temp-write recovery hint, but the hint lives
*inside* the write tool, so it is only reachable after the permission layer
has already said Allow. Under the default policy
(`permission.default = "ask"`, no explicit rule) the permission layer asks
the human first, so the model never receives the suggestion. This was
observed live: the write to `/tmp/codegg-recovery-probe.txt` produced a
`permission_pending` event and the turn stayed parked for 11 minutes before
failing. The remedy is a bounded, in-module change; no dependency is
outstanding.

## 3. Current implementation evidence

- `src/permission/mod.rs` — `check_trusted_root_mutation` relaxes the
  *general default* `Ask` for mutations whose target canonicalizes under a
  trusted root (workspace, private scratch, granted shared scratch). A target
  under none of them keeps the ordinary `Ask`.
- `src/agent/tool_batch.rs` — the general-`Ask` arm routes to
  `resolve_general_ask_via_human`, which publishes `PermissionPending` and
  waits without a bound by default.
- `src/tool/write.rs` — `execute_structured` refuses an out-of-root target
  and, when the target is under a known host temp root, returns the
  non-authoritative recovery hint.
- `src/agent/tool_batch.rs` — `ToolPermissionOutcome::Denied { message }`
  becomes a `ToolExecutionOutcome` whose `model_text` reaches the model, so
  a model-visible guidance message needs no new transport.

## 4. Invariants that must not regress

- The restricted path is never written. The offer is a refusal with
  guidance; it is not an approval and it is not a grant.
- No path is rewritten. The suggestion names a path; it never becomes the
  requested path.
- An explicit operator `Deny`, a stored decision, an explicit tool rule, and
  an explicit path rule all keep their authority. The offer may only
  replace the *implicit default* `Ask`.
- The human remains the sole authority for the original location. The offer
  changes when the human is asked, never whether the original path is
  allowed.
- Scratch access never applies to another session, the human shell,
  plugins, or arbitrary absolute paths.
- Shared scratch roots are never disclosed in the offer; only the caller's
  own private scratch root is named.

## 5. Scope

In scope: the structured `write` tool, the default-policy `Ask` path, one
offer per (tool, canonical path) per session.

Out of scope: `edit`, `apply_patch`, `replace` (they are rooted at the
workspace only and cannot honor a scratch suggestion today — offering one
would be a broken promise); shell commands (the hint never intercepted shell
writes); any change to who may authorize the original location; any change
to the sandbox, scratch ownership, or reclamation.

## 6. Required production changes

- `src/tool/util.rs` — one shared helper that renders the redirect message
  for a known-temp-root target and a caller's private scratch root, so the
  permission layer and the write tool emit identical wording.
- `src/permission/mod.rs` — `PermissionChecker` gains per-session state for
  the paths already offered, and a method that decides whether the current
  `Ask` is the implicit default for an out-of-root temp target and returns
  the offer text.
- `src/agent/tool_batch.rs` — the general-`Ask` arm consults the offer
  before the human path; the offer is returned as a model-visible denial,
  the human path is unchanged when the offer does not apply or was already
  given.
- `architecture/permission.md`, `.opencode/skills/permission/SKILL.md`,
  `docs/tools.md` — the contract, in one place per surface.

## 7. Ordered work packages

WP1 — shared message helper. Extract the redirect wording from the write
tool into `src/tool/util.rs`; the write tool keeps its current behavior
(byte-identical message) and the permission layer uses the same helper.

WP2 — offer decision. `PermissionChecker::temp_root_redirect_offer` returns
`Some(message)` only when all of the following hold: the tool is `write`;
the target is absolute and under a known host temp root
(`relative_to_known_temp_root`); the target is under none of the authorized
roots; the caller's private scratch root exists and canonicalizes; no stored
decision, explicit tool rule, or matching path rule governs the call; and
this (tool, canonical path) was not already offered in this session.

WP3 — agent-loop wiring. In the general-`Ask` arm, try the offer first. On
`Some(message)`, return `ToolPermissionOutcome::Denied { message }` so the
model receives the guidance and the turn continues. On `None`, fall through
to the existing human path unchanged.

WP4 — verification evidence. Focused tests, the local gate, and hosted CI.

## 8. Failure, cancellation, restart, contention semantics

The offer state is in-memory and per session. A daemon restart clears it,
so the next attempt offers again — the conservative direction. A cancelled
or crashed turn leaves no persistent state. The offer is computed from the
checker's own rules, so it cannot race with a policy change: the decision
is re-derived on every call.

## 9. Compatibility and migration

No schema, DTO, configuration key, or protocol change. The operator-visible
behavior changes for one case: a temp-root write under the default policy
now reaches the model before it reaches a human. Operators who want the
human asked first can say so explicitly (`permission.tools.write = "ask"`),
which the offer honors.

## 10. Required tests

- The offer is produced for a temp-root target outside every authorized root.
- No offer for a target inside an authorized root.
- No offer for a target outside every root that is not a known temp root.
- No offer when an explicit tool rule asks, when a path rule matches, or when
  a stored decision exists.
- No offer when the private scratch root does not exist.
- The second identical attempt is not offered again, so the human path runs.
- The agent loop returns the guidance as a model-visible result and does not
  publish `PermissionPending` for the offered attempt.
- The existing write-tool hint test still passes with the shared wording.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=2 cargo nextest run --locked --profile ci -p codegg -E 'test(/permission/) or test(/scratch/)'
scripts/verify.sh quick
```

## 12. Documentation updates

`architecture/permission.md` (the Check Flow contract and the temp-root
paragraph), `.opencode/skills/permission/SKILL.md`, `docs/tools.md` if it
describes the hint.

## 13. Acceptance criteria

1. Under the default policy, a structured write to a known host temp root
   outside every authorized root returns the actionable guidance to the
   model and publishes no `PermissionPending`.
2. A model that retries at the suggested scratch path succeeds with no human
   involved.
3. A model that repeats the same restricted path reaches the unchanged human
   approval path.
4. No path is rewritten, no grant is created, and the restricted path is
   never written.
5. Explicit operator rules, stored decisions, and hard denials keep their
   authority.

## 14. Stop conditions

Report rather than improvise if: the offer would require changing the
authority model (for example, auto-approving the original path); the edit
family is found to need the same treatment for the offer to be honest;
implementing this requires a new protocol field or a storage migration; or
the guidance text would need to disclose a shared scratch root.

## 15. Closure evidence required

The focused suite, the local gate, hosted CI on the merged head, and a
recorded live observation that the model receives the guidance before any
human is asked, with the durable `core_event_log` showing no
`permission_pending` for the offered attempt.

## 16. Handoff notes

Start from `src/agent/tool_batch.rs` — the general-`Ask` arm is the seam.
`src/tool/util.rs` already owns the temp-root classification, so the
message helper belongs beside it. Keep the write tool's own hint behavior
unchanged; it is still the right refusal for the case where the permission
layer already allowed the call.
