# Execution Reliability, Approval, and Autonomy M006 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/execution-reliability-approval-autonomy/006-automatic-approval-reviewer.md`

Source subsystem roadmap:

- `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md#7-milestones`

Repository baseline reviewed: `dd2ddd1a`

Implementation commits or pull requests:

- `dd2ddd1a` — execution-reliability M006: automatic approval reviewer

## 1. Executive finding

M006 is complete. `Automatic` escalations now resolve through a
dedicated, fast, bounded, read-only reviewer (`src/permission/reviewer.rs`)
when a reviewer model is configured and registry-validated; otherwise
Automatic keeps the M003 safe-defer behavior. The reviewer sees only
`Escalate` (never deterministic `Allow`/`Deny`), investigates only via
`read`/`glob`/`grep`/`list`/`diff`/`git_read`, returns strict
Allow/Deny/DeferUser JSON, and fails closed (DeferUser interactive,
explicit deny in configured headless mode) on malformed, timeout,
unavailable, over-budget, cancelled, stale-policy, or forbidden-tool
behavior. Valid Allow applies only to the unchanged original
request/policy revision; Deny returns bounded feedback to the primary
model; repeated equivalent denials backstop to defer/deny. The reviewer
cannot mutate state, shell, network, spawn children, recurse into the
router, or widen the sandbox/authority ceiling. No storage migration;
Interactive/Yolo behavior is unchanged.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Structured reviewer contract WP-A | `src/permission/reviewer.rs`: `ReviewerRequest::from_approval_request` (bounded request/policy/objective/tool/scope/effects/reasons/capability-delta/justification/sandbox/session/turn), `ReviewerVerdict::{Allow,Deny,DeferUser}`, `parse_reviewer_output` strict schema (unknown/empty/missing verdict, risk, reason rejected; code-fence tolerant within budget), `REVIEWER_SYSTEM_PROMPT` (short/stable, untrusted-evidence wording), `ReviewerConfig` clamped bounds + `resolve_model` (unknown `provider/` prefix → Unavailable, never silent switch) | pass | Invalid response → Defer/deny, never Allow |
| Isolated read-only runtime WP-B | `REVIEWER_ALLOWED_TOOLS == ["read","glob","grep","list","diff","git_read"]`; `RegistryReviewerInvestigator` whitelist gate + read-only reviewer `ToolExecutionContext` (cwd = workspace root, caller `approval-reviewer`, `max_effect_class=read_only`, `native_only`) + arg/output budgets; `ProviderReviewerBackend` non-tool bounded call (temp 0, small output budget, JSON object); default max 2 investigations, hard cap 3, default 30s deadline; `scripts/check_approval_reviewer.py` isolation guard | pass | No ApprovalRouter recursion, no subagent/task/shell/network construction (guard-pinned) |
| Router integration/feedback WP-C | `src/agent/tool_batch.rs`: both Automatic branches (sensitive/security escalation + general `Ask`) resolve via `resolve_automatic_escalation` with human fallback preserving original dialog payload; stale-policy revalidation at review start, pre-Allow, and post-review fresh-snapshot compare (mode/sandbox/revision drift discards Allow); Deny feedback returned as compact tool outcome; `RepeatedDenialTracker` + `AgentLoop::reviewer_denial_counts` backstop (default 3); headless (`headless_deny`) deny vs interactive Defer | pass | Sync `route_escalation` still never auto-allows (M003 matrix unregressed) |
| Security/fault qualification WP-D | Injection (`ALLOW`-file has no authority; prose never parses), malformed matrix, provider outage/unavailable, forbidden tools (`bash`/`terminal`/`edit`/`write`/`task` denied + budget-counted), sandbox/profile widening (mismatched profile invalidates; snapshot immutable), recursive-approval absence (guard + no router import), cancellation, parallel independence, restart drop/re-evaluate | pass | See §4, §6, §8 |
| Receipts/audit | `ReviewerReceipt` (request/decision/model IDs, verdict, bounded risk/reason, policy revision, investigation count, elapsed_ms); secret-free; `tracing::{info,warn}` per resolution; no hidden reasoning persisted | pass | Bounded 64/256/64/512-char fields |
| Docs/static guards | `architecture/approval_reviewer.md` (new), `architecture/permission.md` M006 item, `architecture/security.md` M006 threat-model note, `architecture/config.md` `approval_reviewer` entry; `scripts/check_approval_reviewer.py` | pass | No new CI lane (focused ownership lint, plan §6 allowance) |
| Compatibility/migration | Additive `Config.approval_reviewer: Option<ApprovalReviewerConfig>` (no DB migration); unconfigured Automatic defers (M003 preserved); Interactive/Yolo unchanged (matrix + full harness) | pass | Out-of-box Automatic still defers until a reviewer model is configured (intended) |

## 3. Production implementation evidence

Landed ownership and behavior:

```text
Reviewer contract (src/permission/reviewer.rs)
  REVIEWER_SYSTEM_PROMPT (short/stable, untrusted-evidence wording)
  REVIEWER_ALLOWED_TOOLS = [read, glob, grep, list, diff, git_read]
  REVIEWER_HARD_MAX_INVESTIGATION_CALLS = 3 (default 2)
  ReviewerConfig{preferred_model, max_investigation_calls, deadline_ms,
    max_output_chars, headless_deny, max_equivalent_denials} (clamped)
    from_approval_reviewer_config / from_config / resolve_model
  ReviewerRequest (bounded; from_approval_request wraps ApprovalRequest
    + snapshot profile; justification/objective untrusted+bounded)
  ReviewerVerdict Allow{risk,reason} / Deny{risk,reason,feedback?} /
    DeferUser{reason} + strict parse_reviewer_output (malformed never Allow)
  ReviewerModelBackend / ReviewerInvestigator traits
    ScriptedReviewerBackend/ScriptedInvestigator (tests/qualification)
    ProviderReviewerBackend (bounded non-tool provider call, no tools)
    RegistryReviewerInvestigator (whitelist + read-only ctx + budgets)
  resolve_automatic_escalation (stale check → bounded loop → verdict/
    fail-closed mapping; Allow re-validated; cancellation fail-closed)
  RepeatedDenialTracker + denial_key_for (tool|path|summary-hash)
  ReviewerReceipt (secret-free, bounded) + ApprovalReviewer handle
  source::REVIEWER_ALLOW / REVIEWER_DENY / REVIEWER_DEFER

Router integration (src/agent/tool_batch.rs + loop.rs)
  Both Automatic arms (Escalate + general Ask) attempt reviewer first:
    unconfigured/unknown model → human fallback (interactive) or deny
    backstop reached → human fallback (interactive) or deny (headless)
    Allow → Allowed(reviewer_allow) after fresh-snapshot drift check
    Deny → Denied with bounded primary-agent feedback + backstop record
    Defer/failure → human fallback (interactive) or deny (headless)
    cancelled turn → Denied, never a human wait
  AgentLoop.reviewer_denial_counts: HashMap<String, usize> (per-loop)

Config (crates/codegg-config/src/schema.rs)
  ApprovalReviewerConfig{model, max_investigation_calls, deadline_ms,
    max_output_chars, headless_deny, max_equivalent_denials} (all Option,
    clamped resolvers); Config.approval_reviewer additive Option
```

Automatic resolution flow:

```text
Escalate(ApprovalRequest) x Automatic snapshot
  ├─ backstop reached? ──► human fallback / headless deny
  ├─ no configured/valid model? ──► human fallback / headless deny
  └─ reviewer available:
       ReviewerRequest (bounded, profile pinned)
         → ProviderReviewerBackend steps (verdict | investigate×≤max)
         → RegistryReviewerInvestigator (allowlist only; forbidden
            denied + budget-counted; output untrusted data)
         → strict verdict parse (malformed → fail-closed)
       stale (revision/profile/tool drift, incl. mid-review change)?
         ──► discard Allow → defer/deny
       Allow ──► Allowed(reviewer_allow) [fresh-snapshot drift re-check]
       Deny ──► Denied + bounded feedback + backstop record
       Defer/failure ──► human fallback (interactive) / deny (headless)
       cancelled ──► Denied (no human wait)
```

Decision matrix (reviewer unit + integration):

```text
Allow x {Interactive,Automatic,Yolo} → Allow (mode-independent, no reviewer)
Deny x {Interactive,Automatic,Yolo} → Deny (never Yolo/reviewer)
Escalate x Interactive → human wait (unchanged)
Escalate x Yolo → Allow(yolo) within ceiling (unchanged)
Escalate x Automatic + reviewer Allow → Allow(reviewer_allow)
Escalate x Automatic + reviewer Deny → Deny(reviewer_deny) + feedback
Escalate x Automatic + reviewer Defer/failure → human (interactive) / deny (headless)
Escalate x Automatic, no reviewer → human fallback (M003 preserved)
```

## 4. Verification executed

### Commands run (local, this environment)

```bash
cargo test --test approval_reviewer
cargo test -p codegg --lib permission::reviewer
cargo test --test permission
cargo test --test approval_router
cargo test --test agent_loop_harness
cargo test --test agent_loop_harness -- approval
cargo test --test agent_loop_harness -- reviewer
cargo test --test sandbox_policy_wiring
python3 scripts/check_approval_router.py
python3 scripts/check_approval_reviewer.py
python3 scripts/check_sandbox_contract.py
bash scripts/check-core-boundary.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
./scripts/verify.sh quick
```

Plan §11 names `python3 scripts/check_core_boundary.py`; the canonical
guard in this repo is `bash scripts/check-core-boundary.sh` (same
invariant, executed). Plan §11 clippy omits `--locked`; the stricter
locked invocation was executed and `verify.sh quick` re-ran the locked
workspace check. No live external-model test was used; scripted provider
responses cover schema/failure semantics per the plan.

### Results

- `cargo test --test approval_reviewer`: 22/22 pass — palette exactness; prompt stability; verdict parser Allow/Deny/Defer/malformed matrix; request bounds; model-resolution unavailable paths; denial backstop; scripted Allow (with 1 investigation)/Deny+feedback/Defer; Allow/Deny never invoke reviewer; budget bind (4 requests at max 2 → fail-closed, count 2); stale policy/sandbox invalidation; cancellation; parallel independence (distinct request IDs); restart drop with no latent approval + fresh re-evaluation; injection file grants nothing; forbidden tools denied end-to-end; malformed/outage never Allow (interactive defer/headless deny); unconfigured-Automatic defer + Interactive/Yolo unchanged; receipt bounds/budget.
- `cargo test -p codegg --lib permission::reviewer`: 12/12 pass — palette, config clamps, parser matrix, bounds, model resolution, backstop, scripted Allow/Deny/Defer, malformed/unavailable fail-closed both modes, forbidden tools, stale policy, prompt stability, injection hierarchy.
- `cargo test --test permission`: 44/44 pass (deterministic ruleset/store intact).
- `cargo test --test approval_router`: 16/16 pass (M003 contract unregressed; sync Automatic still defers even with rollout enabled).
- `cargo test --test agent_loop_harness`: 50/50 pass — includes 2 new M006 loop tests proving reviewer Allow executes the escalated tool with no human `PermissionPending`, and reviewer Deny surfaces bounded feedback to the primary model with no human prompt; all pre-existing Interactive/human flows preserved.
- `cargo test --test agent_loop_harness -- approval`: 2/2 pass (the new loop tests).
- `cargo test --test agent_loop_harness -- reviewer`: 2/2 pass (same loop tests).
- `cargo test --test sandbox_policy_wiring`: 20/20 pass (M005 truthfulness/orthogonality intact; reviewer changes no sandbox behavior).
- `check_approval_router.py`: pass (single human-wait owner intact; reviewer adds no second registrar).
- `check_approval_reviewer.py` (new): pass (exact 6-tool palette; no router/human/subagent/shell/mutation/network construction; no mode/profile mutation; tool_batch reviewer wiring present).
- `check_sandbox_contract.py`: pass; `check-core-boundary.sh`: pass; `cargo fmt --all -- --check`: pass; `cargo clippy --workspace --all-targets --locked -- -D warnings`: pass; `scripts/verify.sh quick`: pass (fmt, builtin-agents, core-boundary, sandbox, execution-ownership, tui-authority, workspace check).

Reviewer capability proof: `REVIEWER_ALLOWED_TOOLS` asserted exactly
`["read","glob","grep","list","diff","git_read"]` in both unit
(`reviewer_tool_palette_is_exactly_read_only`) and integration
(`reviewer_tool_palette_is_exactly_read_only`) tests plus the guard;
forbidden names (`bash`, `terminal`, `git`, `edit`, `write`,
`apply_patch`, `replace`, `task`, `question`, `webfetch`, `websearch`,
`skill`, `mcp__*`) asserted denied, with live `ForbiddenTool` denials
for `bash`/`terminal`/`edit`/`write`/`task`.

Malformed-output matrix (all → Defer/deny, never Allow): non-JSON,
empty, bare prose, unknown verdict, missing verdict/risk/reason, empty
risk/reason, over-budget fence-stripped output, investigate-shaped
non-verdict, exhausted/unavailable backend, provider outage shape —
each exercised in both interactive (Defer + human fallback) and
headless (explicit Deny) modes.

Performance/budget observation: scripted local reviews resolve
sub-millisecond wall time with `investigation_count ≤ 2` and bounded
receipt fields (`reviewer_receipt_is_bounded_and_within_budget`);
provider latency is bounded by `deadline_ms` (default 30s) and
`max_output_chars` (default 4000) with timeout → fail-closed. No
live-model timing is claimed for CI, per the plan.

## 5. Invariant review

| Plan invariant (§4) | Evidence |
|---|---|
| Reviewer sees only Escalate, never re-decides hard Deny | `resolve_automatic_escalation` takes `&ApprovalRequest` (escalation-only type); Allow/Deny arms return before any reviewer construction; `deterministic_allow/hard_deny_never_invokes_reviewer` tests + counting-backend assertions |
| Reviewer cannot change mode/sandbox/ceiling/credentials/parent-child authority | No setters exist on the reviewer path; snapshot is `Clone`d read-only input; mismatch invalidates (`sandbox_change_invalidates_review`; snapshot asserted unchanged); child ceilings untouched (`narrow_for_child`/`resolve_child_sandbox` unregressed, sandbox suite 20/20) |
| No mutating/process/shell/network/subagent tools by default | Exact palette test (unit + integration) + guard; `RegistryReviewerInvestigator` denies `bash`/`terminal`/edits/`task`/network without execution; end-to-end forbidden-tool test |
| No recursive ApprovalRouter/reviewer invocation | Guard bans `ApprovalRouter`/`request_human_approval`/`PermissionRegistry::register`/`PermissionPending` in reviewer.rs; router guard still passes single-owner |
| Repo/tool/primary-justification untrusted, not instructions | Fixed system prompt; evidence labeled untrusted in prompt construction; `ALLOW`-file test proves prose grants nothing; strict JSON required |
| Malformed/timeout/unavailable never Allow | Parser + fail-closed mapping tests in both modes; loop tests never observe Allow from failure paths |
| Bounded model calls, investigations, tokens, deadline | Default 2/cap 3 investigation counter test; output-char budgets; deadline timeout test path (`Timeout` → Defer/deny); receipt carries counts |
| Primary model gets bounded feedback only, not hidden reasoning | `primary_feedback` ≤512 chars in Deny outcome only; receipts carry risk/reason, never reasoning; loop deny test asserts feedback content |
| Selection failure has explicit fallback (Defer interactive / deny headless) | `resolve_model` errors → human fallback or headless deny; `automatic_without_configured_reviewer_defers` migration test |

## 6. Failure and recovery review

- Reviewer provider timeout/error/malformed → DeferUser (interactive) or explicit deny (`headless_deny`), never Allow (matrix-tested).
- Cancellation of the primary turn cancels the reviewer: pre-set cancel → `Cancelled` → fail-closed; loop helper additionally refuses human fallback after cancel (Denied `turn cancelled during review`).
- Policy/sandbox/workspace revision change during review invalidates: stale check at start, pre-Allow revalidation, and post-review fresh-snapshot drift compare (mode/sandbox/revision) discarding Allow to defer/deny.
- Daemon restart does not persist a pending reviewer request: reviewer holds no store; hanging-review drop test proves no latent approval and that retry re-evaluates current policy with an independent receipt.
- Repeated same denied action cannot create unbounded reviewer calls: pre-invocation backstop check + post-deny recording (default bound 3; interactive defers, headless denies).
- Concurrent approval reviews have independent request IDs and respect provider/resource bounds (parallel join test with distinct IDs and per-review verdicts).

## 7. Migration and compatibility review

- No storage migration: additive `Config.approval_reviewer: Option<…>` only; `runtime_preferences` v58 untouched; `STORAGE_LAYOUT_VERSION` unchanged.
- Automatic with no configured reviewer follows the documented M003 defer behavior (migration test); Interactive/Yolo unchanged (router matrix + full 50-test harness green).
- Sync `route_escalation` semantics unchanged (never auto-allows for Automatic even with rollout enabled); the async reviewer is the only Allow-producing Automatic path.
- `ApprovalMode`/`SandboxProfile` orthogonality preserved (sandbox suite 20/20; mode changes still never touch sandbox).
- Static-guard surface is additive (`check_approval_reviewer.py`); `verify.sh quick` lanes unchanged. Plan-command naming deviations recorded honestly in §4 (core-boundary script name, locked clippy).

## 8. Security review

- Yolo/Automatic cannot disable containment: reviewer input pins the snapshot profile; enforcement wiring untouched (sandbox suite green).
- Injection resistance: strict schema + untrusted labeling + fixed prompt; negative tests for `ALLOW`-file, prose verdicts, and oversized/fenced payloads.
- Forbidden-tool denial is structural (allowlist gate before dispatch), not prompt-dependent: even a model requesting `bash`/`task` gets a denial note, never execution.
- No recursive approval or second human-wait path (both guards pass).
- No secrets in new types: requests carry bounded summaries/IDs; receipts carry verdict metadata; hidden reasoning never persisted; preference/model IDs only.
- Authorization unchanged: reviewer resolves within the already-captured ceiling; deterministic Deny precedes it; frontends gain no authority (config is daemon-read; protocol surface untouched).

## 9. Documentation and operations

Updated:

- `architecture/approval_reviewer.md` — new reviewer ownership doc (flow, config/model preference, threat model, invariants, tests, related docs).
- `architecture/permission.md` — reviewer row in the artifact table + M006 invariant item.
- `architecture/security.md` — approval-vs-security boundary updated for M006 + reviewer threat-model note.
- `architecture/config.md` — `approval_reviewer` config section entry.
- Guard: `scripts/check_approval_reviewer.py` (focused ownership lint; not a new CI lane).

Operator notes: configure `[approval_reviewer] model` to activate
Automatic review (bare id for the primary provider, or
`provider/model` with a registered provider); leave it absent to keep
safe human deferral. Watch `automatic approval reviewer failed closed
to defer/deny` (warn with bounded reason) vs `reviewer allow/deny/defer`
(info with model/risk/investigation count) vs `reviewer allow
discarded: policy changed during review` (warn, stale Allow dropped) vs
`Tool … denied: repeated reviewer denials (N)` (backstop engaged).
`headless_deny = true` is for genuinely noninteractive operation only.

No new CI lane: the guard is a focused ownership lint run locally (plan
§6 allowance); `verify.sh quick` lanes unchanged.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| Low | Out-of-box Automatic (no `approval_reviewer.model`) still defers to the human | Safe but not autonomous until configured; intended migration behavior, documented | M007 surfaces should present reviewer configuration/explicit containment alongside mode selection; no M006 code change |
| Low | Investigation/output budgets are char-based, not token-based | Bounds are conservative and test-pinned but not model-token-exact | M008 qualification may calibrate char↔token ratios against observed reviewer calls; no correctness impact for M006 |
| Low | Bare reviewer model ids rely on the primary provider to fail closed (no catalog pre-check beyond the `provider/` prefix gate) | Unknown bare ids surface as provider errors → Defer/deny, never Allow; acceptable fail-closed posture | M007/M008 may add catalog pre-validation with diagnostics; no M006 scope change |
| — | No other open items | — | — |

No stop condition triggered (no mutation/process tools, broad network
access, recursive agents, hidden-reasoning persistence, or deterministic-deny
override was required).

## 11. Roadmap disposition

Milestone closed with one downstream unblock:

- M006 (automatic approval reviewer): hard dependencies were M003+M005. Both closed and consumed as designed. **Close.**
- M007 (Yolo/Automatic/FullHost user surfaces): hard dependency was M006 (M003+M004+M005 closed). **Unblock to `ready`.**
- M008 (fault-injection and reliability qualification): remains **blocked** on M007 (M001+M002+M003+M004+M005+M006 closed).
- No corrective pass required; no deferred product work registered.

## 12. Registry updates

- `plans/registry.md`: subsystem row `M001+M002+M003+M004+M005 closed; M006 ready` → `M001+M002+M003+M004+M005+M006 closed; M007 ready`; dependency-ready table M006 row `ready` → `closed` with closure link and implementation `dd2ddd1a`, M007 row added as `ready` with M003+M004+M005+M006-contract note; execution-order item 2 rewritten (M006 closed, M007 ready, M008 still blocked); M006 appended to recently-closed work; blocked-work M007 row removed, M008 row narrowed to the remaining gate.
- `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md`: M006 section `ready` → `closed` with closure link; M007 `blocked` → `ready`; M008 blocker `M006-M007` → `M007`; status table updated.
- `plans/implementation/execution-reliability-approval-autonomy/006-automatic-approval-reviewer.md`: `Status: ready for handoff` → `Status: implemented`.
- `plans/implementation/execution-reliability-approval-autonomy/007-yolo-auto-and-full-host-user-surfaces.md`: `Status: blocked` → `Status: ready for handoff`.
