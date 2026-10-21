# Developer Harness Lab M002 — ACP Unattended Execution and Truthful Terminal Contract

Status: implemented

Repository baseline: `f37840ef5fca36d44b35daec5da6b26cd049a3ee` (`main`, 2026-10-18). Refresh source before implementing.

Source roadmap: `plans/subsystems/harness-lab-roadmap.md#7-milestones`

Long-term requirements: `plans/000-long-term-specification.md#23-acp-boundary`, `#24-protocol-and-storage-requirements`, `#27-security-requirements`, `#28-observability`, `#29-system-invariants`; `plans/001-terminology-and-domain-model.md`.

Applicable existing authorities: `src/policy_surface.rs`, `crates/codegg-core/src/approval.rs`, native `RuntimePolicySet`/`ExecutionPolicyGet` (`crates/codegg-protocol/src/core.rs` and `src/core/daemon_ops.rs`); original ACP v1 semantics https://agentclientprotocol.com/protocol/v1/prompt-turn .

Primary class: invariant/capability

## 1. Objective

Make `codegg acp` capable of a reproducible, **noninteractive, explicitly requested** native-daemon agent turn with `yolo` approval plus `full-host` OR `workspace-write` sandbox, while preserving daemon-native permission precedence and emitting truthful, correlated terminal outcomes. Establish one real process/tool integration proof; do not add another execution runtime.

## 2. Why ready

CodeGG already resolves `--approval-mode`, `--yolo`, `--sandbox` in root/exec via `src/policy_surface.rs`, and the daemon already owns `ApprovalPreferenceGet`, `RuntimePolicySet`, `ExecutionPolicyGet` and persisted per-principal policy snapshots. ACP currently calls `connect_or_start_daemon` and native `TurnSubmit`; no new transport or auth bypass is required.

## 3. Current evidence/gaps

- `src/main.rs` declares `Commands::Acp` without a policy-bearing subcommand; root `ExecutionPolicyArgs` are **not applied to ACP**. Merely running `codegg --yolo acp` today does not prove an unattended daemon policy.
- `src/acp.rs::session/prompt` selects `agents[0]`, `model` from the request or `EMERGENCY_DEFAULT_MODEL`, `reasoning: None` and uses native `TurnSubmit`; it must not silently claim to test a different agent/model/profile than it selected. Session load/resume may not restore a known project root to `SessionBinding`.
- `session/prompt` supports one in-flight request/connection; that is fine. Concurrency belongs to multiple ACP subprocesses bound to separately scoped daemons (M001).
- `terminal_reason()` maps **all** `TurnFailed` native outcomes to ACP v1 `cancelled`, conflating failed and deliberately cancelled. ACP v1 defines `end_turn`, `max_tokens`, `max_turn_requests`, `refusal`, `cancelled` stop reasons; a failed operation should not be mislabeled cancelled or a novel standard stop reason invented. An ACP JSON-RPC error with typed, bounded reason/correlation is a viable alternative.
- `tests/acp_stdio.rs` asserts initialize/shutdown only. `src/acp.rs` has lifecycle/correlation unit tests but no real policy/permission behavior test through full daemon+tool.

## 4. Invariants

1. ACP remains a **thin frontend adapter**. Native `CoreRequest::TurnSubmit` and the existing daemon policy evaluation own execution, not a direct `AgentLoop` instance, a parallel approval registry, or Python approval spoofing.
2. `yolo` suppresses Ask prompts only insofar as existing canonical rules permit; explicit Deny, hard-deny, principal rights, broker and sandbox ceilings still win. Full-host is an explicit opt-in for the trusted development workstation, not a global default.
3. A requested policy must match the **effective daemon snapshot** before the controller admits a trial. Fail closed on no preference store, policy update rejection, headless Ask, or narrowed/unsupported sandbox. Report requested and effective modes separately.
4. `TurnFailed` must never become a successful `end_turn` or a misleading `cancelled`; actual cancellation still yields `cancelled`. No duplicate, stale or cross-session terminal response; stdout remains JSON-RPC only.
5. Existing ACP v1 clients remain interoperable. Do not silently implement ACP v2 semantics or unrecognized standard enum values. Preserve existing cancellation/event-floor races and event visibility filtering.
6. No plan fixture may depend on making destructive Git operations or network changes that production policy explicitly prohibits.

## 5. Scope

In: explicit ACP launch policy selection, daemon-native application/verification (prefer per-lane isolated principal/DB), stable effective-policy evidence, typed failure vs cancellation, truthfully reported agent/model selection, bounded real-process tests, CLI documentation.

Out: product-wide unrestricted defaults, VM/container setup, a new ACP runtime, wholesale protocol upgrade to v2, large telemetry rewrite (M003), automatic candidate editing, Linux sandbox backend changes, bypassing sensitive tool hard-denies.

## 6. Required changes

### CLI / ACP adapter

- Choose an explicit, discoverable ACP command syntax, preferably `codegg acp --approval-mode yolo --sandbox full-host` with optional `--yolo` alias, using the **same** canonical `resolve_cli_policy` as root and `exec`. Decide how top-level flags interact; reject contradictory flags and document exactly one truth.
- Extend ACP startup/first daemon attachment to apply the requested mode+profile using existing authorized `CoreRequest::RuntimePolicySet` (atomic CAS if relevant). Query `ExecutionPolicyGet` or equivalent daemon-native effective-policy contract before first `TurnSubmit` and record a diagnostic receipt. **Do not** set policy on the user's normal daemon as an unnoticed side effect; the dev controller uses a private lane instance. When ACP is used interactively without explicit overrides, existing behavior remains unchanged.
- Ensure the policy is stable for the turn, not changed underneath another shared session. If the current per-principal preference can be changed concurrently in the same daemon and no turn-scoped policy pin exists, identify the race and either add a canonical typed per-turn snapshot under daemon authority or **fail closed / serialize trial admission per daemon**. Do not invent an ACP-only authority override.
- Verify agent/model selection through the actual session/turn setup; require an explicit requested agent/model for benchmark mode if needed, fail on unknown/unavailable rather than silently choosing a fallback. Do not leak provider secrets in diagnostics.
- Make any nonstandard benchmark metadata additive and namespaced (e.g. JSON-RPC error `data` with safe native error class, session/turn correlation, or a separate native sidecar query). Do not forward private reasoning.

### Terminal reporting

- Route native complete, failed, cancelled, refusal/limit (where independently known), protocol error, daemon disconnect and controller timeout into distinct **internal** terminal categories.
- For ACP v1, send `stopReason: "cancelled"` only on confirmed cancellation; send `end_turn` only on actual successful native completion; use a standard JSON-RPC error for genuine failure rather than a fabricated ACP v1 stop reason. The Python driver must classify a JSON-RPC error as failed and inspect bounded `data` if available.
- Preserve first-terminal-wins, pending pre-turn cancel semantics, idempotency, close/cancel races, frame-size cap, session scoping and stale event rejection.
- Explicitly fail on an unanswered permission request in noninteractive mode: do not hang until an external watchdog fires and then report success.

### Storage/protocol compatibility

Reuse existing native policy DTOs and versioning; no new storage schema merely for benchmarking. If extending ACP initialize capabilities or result metadata, keep optional additions backward compatible and add unknown-field tests. Record version of ACP v1 client SDK used by tests.

## 7. Work packages

**A — Native policy contract audit.** Trace `ApprovalPreferenceGet`/`RuntimePolicySet`/`ExecutionPolicyGet` and effective turn construction. Write focused tests showing only the requested private lane changes and existing denies remain effective. If daemon-owned policy selection cannot be safely made turn-stable, stop for an explicit design correction.

**B — ACP CLI and effective-policy handshake.** Add shared flag parsing and ACP attachment verification, with a test for each of yolo/full-host, yolo/workspace-write, contradictory flags, default unchanged and effective-mode mismatch.

**C — Terminal correctness and correlation.** Fix `TurnFailed` false cancellation, ensure typed error path with no false success, and test accepted/cancelled/failed/pre-ID-cancel/stale cross-session events through ACP.

**D — Full native smoke.** Drive an actual daemon and non-paid stub/fixture provider (or deterministic test double at provider boundary, but **real** native tool broker/permission path) through ACP to a read/search/safe write/shell task and assert tool receipts, status, policy, output and no `session/request_permission` prompt. Any feature requiring paid live model is a separate **optional** qualification.

## 8. Failure, cancellation, restart, contention

Connection loss mid-turn does not imply `end_turn`; record ambiguous/interrupted and consult native correlated receipt if possible. Native `TurnFailed` returns exactly one failed outcome. ACP cancel before turn binding remains deferred and first-terminal-wins. `session/close` removes projection subscription without destroying native session. Two ACP clients per one daemon may contend for per-principal policy; tests must either prove a stable turn snapshot or show explicit admission refusal/serialization. Never assume two different processes sharing one daemon can simultaneously use different privilege envelopes without independent native authority.

## 9. Compatibility and migration

Keep ACP v1 method names and text-only prompts. No migration of saved sessions, provider config or SQLite. Existing interactive clients default to their prior behavior and receive no imposed full-host/yolo. A genuine failure previously misreported cancelled is a **correctness fix** that clients may observe as a JSON-RPC error; document and test this deliberate change.

## 10. Tests

- CLI policy mode/profile parsing and rejected aliases, `RuntimePolicySet` accepted/rejected/effective-narrowed cases, explicit deny staying denied.
- In-process ACP state tests plus `tests/acp_stdio.rs` expanded process fixture with daemon and deterministic provider; no fake tool completion.
- Fault injections: TurnFailed before/after tool, cancel-before-ID, delayed stale TurnCompleted, duplicate TurnFailed, wrong session, abrupt daemon exit, blocked headless Ask, unsupported policy profile.
- Verify output channel purity and bounded, secret-safe error data.

## 11. Verification commands

```bash
cargo test -p codegg --lib acp
cargo test -p codegg --test acp_stdio -- --nocapture
cargo test -p codegg --lib policy_surface
cargo test -p codegg --lib approval
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Adjust test targets to actual crate names at implementation time; document if a specified filter selects zero tests, which is **not** evidence.

## 12. Acceptance and stop conditions

Close only if an ACP invocation with explicit `yolo + full-host` and another with `yolo + workspace-write` are demonstrably no-Ask in correctly authorized circumstances; requested/effective policy and actual tool calls are correlated; real denial stays denied; a native failed turn is reported **failed**, not cancelled or succeeded; default interactive ACP clients remain compatible. Block if the existing daemon policy seam cannot safely select a per-trial effective policy, or if a test uses a mocked bypass rather than the real broker.

## 13. Closure artifact

`plans/closure/harness-lab/002-status.md`: implementation SHA, exact ACP frames/policy snapshots (secret-safe), source of policy authority, pass/fail/cancel evidence, test/CI commands with counts, compatibility impact, unresolved races and documented changes to `architecture/acp.md`, `docs/cli.md`, `docs/daemon.md` as applicable.
