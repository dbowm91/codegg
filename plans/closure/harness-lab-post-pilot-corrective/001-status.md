# Harness Lab Post-Pilot Corrective 001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/harness-lab-post-pilot-corrective/001-failed-turn-evidence-and-budget-authority.md`

Source subsystem roadmap:

- `plans/subsystems/harness-lab-post-pilot-corrective-addendum.md`
- `plans/subsystems/harness-lab-roadmap.md`

Repository baseline reviewed: `2a9fc376707ef14dab4a2f9a27f6aba8ac00dd59`

## 1. Executive finding

A failed native turn now carries a secret-safe failure classification and an
explicit usage provenance, and a live trial now has to prove an enforceable
spend cap before it is allowed to start.

The pilot's central evidence gap was that a failed turn reported no usage at
all, so a run that had spent money looked identical to a run that had never
reached the provider. The native adapter now projects an allowlisted
`TurnFailureCategory` and a `TurnUsage` with an explicit `UsageAvailability`
on every failure terminal, and the ACP `-32010` payload carries both. The
three provenance states — `reported`, `not_observed`, `not_provided` — are
distinct facts and stay distinct; `codeggUsage` is `null` whenever no count
was available, so a missing measurement is never read as zero usage.

The spend gap was that nothing stopped a trial from starting without a cap.
`dev/harness_lab/budget.py` now refuses a trial that cannot prove an
enforceable hard cap, naming the missing source, and the CLI runs that gate
before any provider request, daemon spawn, or subprocess launch.

The failure payload deliberately does **not** carry
`CoreEvent::TurnFailed.message`. That message embeds upstream provider text
and can carry credentials; it stays in the local transcript. The category is
derived from the typed native error, never from message text.

## 2. Requirement-to-evidence matrix

| Plan requirement | Evidence | Status |
|---|---|---|
| §4.4-A: classify every native `TurnFailed` | `TurnFailureCategory` in `crates/codegg-protocol/src/core.rs`; classifier in `src/agent/turn_runtime.rs` | Met |
| §4.4-B: usage provenance on every terminal event | `TurnUsage`/`UsageAvailability`; per-run aggregate in `src/agent/loop.rs` and `src/agent/provider_turn.rs` | Met |
| §4.4-C: budget admission preflight | `dev/harness_lab/budget.py`; gate in `dev/harness_lab/__main__.py` | Met |
| §3: no ACP v1 shape change | Success-path `codeggUsage` keys unchanged and asserted; `-32010` code unchanged | Met |
| §3: no `usage` DB schema change | No migration added; only in-memory protocol types | Met |
| §3: secret redaction | `CoreEvent::TurnFailed.message` never enters the payload; asserted by `acp_turn_failed_error_carries_the_redacted_category_and_usage` | Met |
| §4.4-D: provider-free end-to-end regression | `dev/harness_lab/provider_fixture.py` fault injection; `dev/harness_lab/tests/test_provider_fixture_faults.py` | Met |
| §5: no live provider calls | Every test in this closure uses the local fixture or a local fake provider | Met |
| §6: versioned ledger with old data readable | `SCHEMA_VERSION = 2`, `SUPPORTED_SCHEMA_VERSIONS = (1, 2)` in `dev/harness_lab/ledger.py` | Met |
| §7: never treat missing usage as zero | `report()`/`compare()` keep unmeasured aggregates `None`; `usage_unavailable` carries a named reason | Met |
| §8: five terminal distinctions tested | See §4 below | Met |

## 3. Implementation and safety evidence

### 3.1 Native failure classification

`TurnFailureCategory` (`crates/codegg-protocol/src/core.rs`) is an allowlist
of nine values: `provider_transport`, `provider_auth`, `provider_rate_limit`,
`provider_timeout`, `provider_api`, `cancelled`, `daemon_restarted`,
`agent_internal`, `native_failure_unspecified`.

Every variant has exactly one real producer. `permission_denied`,
`budget_exceeded`, and `tool_error` were **removed during implementation**
because no native path produces a turn-level failure for them: a permission
denial and a tool failure are surfaced to the model and the turn continues,
and a goal budget limit ends the turn through a completion terminal rather
than a failure terminal. Shipping them would have been a claim without a
hook.

`src/agent/turn_runtime.rs` classifies from the typed `AppError`, never from
message text:

- `AppError::Provider(p)` maps through `ProviderError::error_class()`, whose
  documented contract is that it never contains credentials, URLs, or response
  bodies.
- `AppError::Cancelled` is a new typed variant on `codegg-core`'s `AppError`,
  carrying the previous `provider turn cancelled` display string so existing
  log greps and the retry-disposition behavior are unchanged.
- `AppError::Permission` and `AppError::Tool` fall to
  `native_failure_unspecified`, because neither reaches the turn-level error
  path in practice.

`AppError::Cancelled` is the typed replacement for the previous free-text
`AppError::Other(anyhow!("provider turn cancelled"))`. The cancellation is
now detected from a typed signal rather than from a string comparison.

### 3.2 Usage provenance

The agent loop accumulates usage per **run**, independently of goal
accounting. The previous `unaccounted_input_tokens` / `unaccounted_output_tokens`
fields could not be used: they are goal-accounting deltas reset by
`account_goal_for_turn`, which is only reached on the success path, so on a
failed turn they do not describe the turn.

`AgentLoopState` now carries `run_provider_responses` plus the accumulated
input, output, cached, and reasoning counts. The producer derives the
availability:

| Condition | `UsageAvailability` |
|---|---|
| No provider response was observed | `not_observed` |
| A provider response arrived but reported no counts | `not_provided` |
| Counts were reported | `reported` |

The failure aggregate covers **every** provider response in the failed run, so
a turn that made two metered calls and then failed reports both. The success
path is unchanged and still reports the last `Finish`'s usage.

The ACP adapter adds a fourth producer, `lost_at_projection`, for a terminal
that arrived through the projection stream without usage: at that observation
point the usage may have been lost, which is a fact distinct from "never
observed".

### 3.3 Budget admission

`dev/harness_lab/budget.py` is provider-free: it imports no network,
subprocess, or supervisor module and performs no I/O. `admit_budget` returns
`admitted` or `budget_unverifiable` with a `missing_source` from the
allowlist `token_limit`, `cost_limit_usd`, `max_provider_requests`,
`max_wallclock_seconds`, `provider_quota`, `provider_quota_retry_safety`.

Two rules are strict on purpose, and both are recorded decisions rather than
defaults:

- A `cost_limit_usd` with no stated `cost_enforcement` is refused. A guessed
  or locally estimated price is never treated as an enforceable spend limit,
  and a requested-but-unenforceable cap refuses the trial rather than being
  silently ignored.
- A provider quota must be retry-safe. A ceiling that resets on a provider
  retry does not bound the trial.

Invalid limits (`0`, negative, `NaN`, `inf`, booleans, strings) are never
coerced to zero; they are recorded in `detail["invalid_caps"]` and treated as
not present.

The CLI gate runs once, at the top of `main()`, before `_binary()` resolves a
path and before any trial function is reached. `budget-check` is a
standalone, provider-free subcommand that prints the decision and exits `0`
when admitted and `2` when refused.

### 3.4 Provider-free fault injection

`dev/harness_lab/provider_fixture.py` can now fail after a metered
tool-calling attempt. In that mode the tool-calling response also carries a
usage chunk, so the injected fault lands *after* a metered attempt and the
failure path can distinguish "spent something" from "never reached the
provider". Two fault kinds are supported: `transport` (a truncated response
body) and `http_error` (a 503).

## 4. Verification executed

See `plans/closure/harness-lab-post-pilot-corrective/001-evidence.md` for the
verbatim command output.

The five terminal distinctions required by §8 are covered by:

| Distinction | Test |
|---|---|
| Successful terminal with usage | `acp_harness_extensions_expose_turn_usage_and_argument_fingerprint` (success shape asserted unchanged) |
| Provider failure after at least one metered attempt | `failed_turn_after_a_metered_attempt_reports_the_aggregate_usage` |
| Failure before any provider response | the `M001MidStreamToolFailProvider` harness test, extended to assert `not_observed` |
| Failure with no provider usage support | `failed_turn_without_provider_usage_support_reports_not_provided` |
| Projection disconnection | `acp_projection_turn_failure_without_usage_reports_lost_at_projection` |

The redaction and category assertions are covered by
`acp_turn_failed_error_carries_the_redacted_category_and_usage`,
`acp_native_turn_failure_without_usage_reports_unspecified`, and
`acp_projection_turn_failure_keeps_a_reported_category_and_usage`.

## 5. Remaining conditions

None that block this plan. Two findings are recorded for the successor plans
rather than fixed here:

1. **Goal accounting still skips failed turns.** `account_goal_for_turn` is
   reached only on the success path, so a failed turn's metered attempts are
   not counted against the goal budget. This plan delivers the *evidence*; the
   budget *accounting* semantics are a separate change and were deliberately
   not made here.
2. **The OpenCode CLI version pin is stale.** `improve.py` pins
   `SUPPORTED_OPENCODE_VERSION = "1.19.0"` and compares it for exact string
   equality, but the installed CLI reports `1.19.3`. The pinned preflight
   therefore refuses the installed binary. Re-pinning is C003 work, because
   the plan requires the *actual installed* version and its documented
   `run --auto --format json` semantics.

## 6. Planning audit

- No plan file was edited to make this implementation look complete.
- The `usage` SQLite table schema was not changed.
- The ACP protocol version, method names, error codes, `stopReason` values,
  and the successful result shape are unchanged.
- The Python harness suite was wired into `scripts/verify.sh quick` and the
  CI workflow, so the evidence above is a standing guard rather than a
  one-off claim.
- The ledger's `SCHEMA_VERSION` moved from 1 to 2 with an accepting range, so
  the stored pilot ledgers remain readable.

## 7. Registry updates

- `plans/registry.md`: the C001 milestone row moves from `in_progress` to
  `closed` and points at this record.
- `plans/subsystems/harness-lab-post-pilot-corrective-addendum.md`: C001
  moves from `ready` to `closed`; C003's dependency on C001 is discharged.
- `plans/subsystems/harness-lab-roadmap.md`: the post-pilot corrective
  milestone row records the closure.
