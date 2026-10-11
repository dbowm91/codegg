# Human Execution Corrective C001 — Human-Shell Promotion and Redaction

Status: implemented

Repository baseline: `578b62bd9580382e00fb19e096fc1794e1f5618b`

Source roadmap: `plans/subsystems/human-execution-corrective-roadmap.md#c001--promotion-redaction-and-config-truth`.

Predecessor: `architecture/human_shell.md`, `plans/shell_output_projection_rtk_roadmap.md` (historical projection phases), and existing `/shell-*` UX. This is newly discovered post-audit work, not a revision of closed PTY M003.

Long-term requirements: `plans/000-long-term-specification.md#29-system-invariants`; `#45-locality-by-default`; `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`. Applicable ADRs: none unless source reveals an incompatible session/context ownership requirement.

Primary class: invariant/capability.

## 1. Objective

Make `!!command`, `/shell-include` and `/shell-ask` genuinely transmit user-approved, bounded, redacted shell evidence into the correct model-facing session/turn. Keep `!command` human-only. Never claim promotion because a local chat bubble was inserted. Reconcile unused or misleading human-shell configuration.

## 2. Why this milestone is ready

Existing `ShellRuntime`, `ShellOutputStore`, `CommandOutputStore`, `ProjectionSelector`, redaction rules, turn dispatch, daemon session identity and TUI command/async primitives provide the needed foundations. No external dependency or redesign of PTYs is necessary. The accepted session/context owner, not another frontend-local store, must own a promotion once acknowledged.

## 3. Current implementation evidence and verification deficiency

`src/tui/app/prompt_turn.rs::send_prompt` intercepts bangs and bypasses normal `dispatch_turn_submit_request`. `src/tui/commands/shell.rs::handle_shell_event` computes `config_command_projection(..., ProjectionTarget::ModelContext)` for shell-cell metadata only. Auto-promotion in that handler and manual `handle_shell_include` and `handle_shell_ask` instead format raw `ShellOutputStore` stdout/stderr or `ShellDigest` and invoke `messages_state.messages.add_user_message`, which only appends a local `UIMessage`. No canonical `SessionPromptSubmit`/turn-context acceptance is demonstrated. The existing tests validate TUI state rather than fake-provider model-facing messages. Direct raw-store formatting also risks bypassing projector redaction and model budget.

The config schema exposes `enabled`, `default_timeout_secs` and `auto_promote_bangbang`; production `spawn_human_shell` currently hardcodes a 300-second timeout, and the latter setting appears unconsumed. Audit all effective reads before changing behavior.

## 4. Invariants that must not regress

- `!` causes no model-facing output, even when the human later submits a normal turn; commands are not run by the agent's `bash` tool.
- Explicit consent is required for promotion; observer/unbound sessions cannot promote or invoke execution by claiming a human origin.
- Exactly one canonical, redacted, byte/token-bounded model-target projection crosses from shell output to session/model context. Never use raw head/tail or a digest independently to build model-facing content.
- The projection remains untrusted user-supplied evidence, not a system/developer instruction, trusted tool result or permission escalation.
- A toast/`promoted` flag is not success until the session owner acknowledges the action; stale tab/workspace results cannot be attached elsewhere.

## 5. Scope

In: complete `!!`/include/ask promotion path; source/projection identity, canonical context acceptance; explicit next-turn vs ask-submit semantics; config behavior; negative/privacy/recovery tests and skill/manual corrections. Out: moving subprocesses to daemon (C002), PTY changes (C003), agent Bash, generic context redesign, auto-model-run on `!!`.

## 6. Required production changes

### Core/domain
Build or reuse a typed human-output promotion with command/run ID, session/project/workspace identity, consent/provenance, source completion/truncation, projection kind, redaction state and idempotency token. Use the existing `ProjectionSelector` model-target call to generate all modes (full, summary/digest, stdout, stderr, tail, `!!`). Ensure hard output/byte bounds in addition to configured model budget, and reject unsupported binary/partial content if safe semantics cannot be guaranteed.

### Storage and migrations
Stage an approved promotion under the canonical session/context owner so it is consumed by the next accepted turn, not just displayed in the TUI. Prefer existing session/context mechanisms; an additive schema migration is permitted only if a durable pending-promotion identity/consumption state is truly required. Persist redacted/bounded bytes or governed references, not raw command output. Acknowledged staging must survive frontend redraw/reconnection, and a rejected turn cannot silently consume it.

### Protocol and DTOs
Audit `SessionPromptSubmit`, `TurnSubmit` and existing context/artifact mechanisms before adding protocol shapes. If insufficient, add a narrow session-scoped `HumanShellOutputPromote`-like request with authenticated human principal, exact workspace/session binding, content/source limits and acknowledgement. Use typed capability negotiation and backward-compatible error; no fabricated agent tool call or direct TUI-only insertion.

### Runtime and concurrency
`!!` and `/shell-include` stage output for the next user-initiated turn without starting inference. `/shell-ask` must intentionally submit exactly one question turn with its selected evidence (or stage then submit atomically); if busy/unbound, report refusal and retain appropriate pending state. Exactly-once logical consumption on successful turn admission; duplicates/retries/reconnect must not double-promote. Handle late shell completion, task cancellation, history eviction and tab switches.

### Frontend/operator
Differentiate shell completed, projection prepared, staged, consumed/submitted and failed. Only update `promoted` after acknowledgement, with an actionable error on unsupported/denied. Preserve syntax and manual include choices. Disable all execution paths when the effective human-shell feature is disabled.

### Security/authorization
Every model-facing mode must use one canonical projection/redaction hook, including failure digests and fallback outputs. Test secrets crossing chunk boundaries, ANSI, PEM/URL credentials and truncation. Resolve effective `redact_model_visible_output=off` semantics explicitly: either enforce an approved policy with clear warning/authorization and tests, or fail closed for human promotion and document that limit. Pattern matching `evaluate_command` is UX screening, not a sandbox or permission system.

### Documentation/static guards
Update `architecture/human_shell.md`, `.opencode/skills/human-shell/SKILL.md`, `docs/tui.md`, `AGENTS.md` and config reference/example as appropriate. Correct claims that shell-cell UI equals model context. Prefer focused tests/guard over new CI lane.

## 7. Ordered work packages

A. Inventory all effective config reads and all projection/promotion modes. Freeze a behavior matrix for `!`, `!!`, include, ask; bound/unbound sessions, failure, retry and observer.
B. Introduce shared redacted model-target projection and typed canonical staging/turn-use contract. Remove direct raw-store-to-model formatting and ensure legitimate, authenticated session ownership.
C. Implement a guarded `/shell-ask` turn path and next-turn `!!`/include path. Make UI flags truthful and wire `enabled`, effective timeout and `auto_promote_bangbang` or explicitly deprecate unsupported config with compatible error.
D. End-to-end test using deterministic fake provider, docs/skill reconciliation and closure evidence.

## 8. Failure, cancellation, restart and contention

A cancelled/timed-out command is distinguishable from successful output; no fabricated complete transcript. Late exit cannot promote into a new selected session. Duplicate promotion keys are idempotent; a deliberate separate promotion uses a new key. The owner must preserve an acknowledged pending promotion across UI restart, or expose a typed gone/expired state under an explicitly documented finite retention contract. Rejected turn admission must not drop staged evidence.

## 9. Compatibility and migration

Keep bang and `/shell-*` grammar and shell history. New capability-gated request must fail explicitly against older daemon, not silently show success. No old session/tool history rewrite. An additive data migration, if necessary, has restart and old-session compatibility evidence.

## 10. Required tests

Focused unit: all promotion modes, redact classes, ANSI/malformed UTF-8/secret chunk boundaries, input/output budget and omission. Integration: real human `!` shell cell but no fake-provider context; `!!` redacted marker exactly once in the next accepted fake-provider turn; include likewise; ask dispatches one actual turn with the question; no unsolicited turn. Negative: observer, no session, workspace switch, evicted ID, disabled config, unavailable daemon, forged source, cancelled/failed process. Concurrency: duplicate acknowledgements, reconnect, restarted store, turn busy/refused/cancelled; no wrong-session or duplicate consumption. Check config defaults/precedence and user-visible status.

## 11. Verification commands

```bash
cargo fmt --all -- --check
cargo test -p codegg --lib shell
cargo test --test shell_projection_harness
cargo test --test shell_projection_phase10
python3 scripts/check_execution_ownership.py
scripts/verify.sh quick
```

Add a focused test target covering daemon/model input; report the exact commands actually run. Use existing broad verification policy; no live provider/network CI or new permanent lane.

## 12. Documentation updates

Human shell architecture and skill, TUI user guide, AGENTS invariant, config schema/docs/examples as needed. Explain that `!!` stages for next turn rather than invoking an agent immediately.

## 13. Acceptance criteria

A real provider-facing turn includes exactly one bounded/redacted, intentionally promoted output in the intended session; `!` never does. Every promotion command proves this end to end, not merely via `MessagesWidget`. Effective config and docs agree; agent `bash` remains unchanged.

## 14. Stop conditions

Stop if no authoritative context owner can accept the proposed staging without an architectural decision; if secret redaction can be bypassed; if new persistent storage cannot be safely migrated; or if a test proves only TUI text. Do not fold C002's process locality work into this milestone.

## 15. Closure evidence

Production source-to-model trace with fake-provider captured messages and source IDs; authority/config/projection matrices; duplicate/reconnect/restart evidence; test and guard outputs; docs/skill diffs; known residual risk.

## 16. Handoff notes

`ShellOrigin::HumanPromoted` exists but is not constructed; changing that enum alone is not a context bridge. Preserve unrelated work and historical closure artifacts. Close C001 before C002.
