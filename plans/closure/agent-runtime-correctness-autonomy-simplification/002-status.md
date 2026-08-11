# Agent Runtime Correctness, Autonomy, and Simplification M002 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/agent-runtime-correctness-autonomy-simplification/002-textual-tool-call-repair-safety.md`
Source subsystem roadmap: `plans/subsystems/agent-runtime-correctness-autonomy-simplification-roadmap.md#7-ordered-milestones`
Repository baseline reviewed: `e88d6f4f67ff729c894b228ddb8b5324582f3fbc`
Implementation commits: `86f8f43 — safely gate textual tool-call repair`; closure commit recorded by Git history

## 1. Executive finding

M002 is strictly closed. Structured provider tool calls remain canonical. The
generic agent loop no longer scans assistant text unless the resolved model
adapter explicitly enables a named repair grammar. Repair is bounded,
tool-surface validated, shape/schema checked, and passed through the existing
permission and broker path.

No built-in adapter currently opts into textual repair. An exact model profile
override may opt into a documented grammar when compatibility evidence exists;
unknown models remain structured-tool-only.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Explicit adapter/profile capability | `AdapterTools.text_tool_repair`, `ResolvedModelAdapter.text_tool_repair`, and `ModelProfileConfig.text_tool_repair`; resolver test for `fixture/hermes` | pass |
| Structured calls take precedence | Loop invokes repair only after `processor.tool_calls()` is empty | pass |
| No arbitrary prose execution | Profile-specific parser; no generic raw-JSON substring search; prose/fence adversarial tests | pass |
| Retained grammar is bounded | 64 KiB input, 8 calls, balanced JSON, typed errors | pass |
| Current tool surface and argument shape are enforced | Repair receives `request.tools`; unknown tools and missing required fields are rejected | pass |
| Normal authorization remains authoritative | Repaired calls are assigned only to the existing `tool_calls` execution path | pass |
| Provider outcome is simple for M005 | Repair returns `Ok(Some(calls))`, `Ok(None)`, or typed `TextRepairError`; no continuation policy is embedded | pass |

## 3. Production implementation evidence

- `crates/codegg-providers/src/text_tool_parser.rs` now exposes three explicit
  grammars: `hermes_xml`, `invoke_json`, and `raw_json_envelope`.
- Hermes XML retains bounded balanced JSON parsing and permits surrounding
  explanatory text without parsing that text. `invoke_json` requires the
  complete response to be one or more balanced `invoke(...)` envelopes.
  `raw_json_envelope` requires the complete response to be exactly a
  `{name,arguments}` object. Fenced blocks and embedded JSON are not accepted.
- `src/agent/loop.rs` resolves the adapter through `ModelProfileResolver` at
  both provider-response sites and calls repair only when its capability is
  present. There are no model-name branches in the loop.
- `request.tools` is the validation surface, so hidden/unavailable tools cannot
  be repaired into executable calls. Required object fields are checked before
  execution.

### Grammar inventory

| Previous form | Disposition | Evidence |
|---|---|---|
| Hermes `<tool_call>{...}</tool_call>` | retain, explicit `hermes_xml` profile | exact provider parser fixture |
| `invoke("tool", {...})` | retain, exact `invoke_json` profile | exact parser fixture/API contract |
| Fenced ` ```tool {...}` | delete | ordinary documentation syntax is ambiguous; adversarial fence remains text |
| Raw JSON embedded in prose | delete from repair | prose example is rejected; only exact envelope profile remains |
| Multiple calls | retain only for Hermes/invoke profiles, bounded at 8 | call-limit implementation and parser tests |

## 4. Verification executed

Local verification:

- `rtk cargo fmt --all` — passed.
- `rtk cargo test -p codegg-providers text_tool_parser` — 4 passed.
- `rtk cargo test -p codegg-core model_profile::adapter` — 6 passed.
- `rtk scripts/verify.sh quick` — passed, including formatting, generated
  agent checks, core boundary, sandbox/execution guards, and capped workspace
  all-target checking.

## 5. Invariant review

Structured calls are still consumed first. Structured-only adapters have no
repair profile. Repaired IDs use a bounded per-response namespace and cannot
override existing structured calls because repair is attempted only when that
list is empty. Unknown, hidden, malformed, or schema-incomplete calls do not
reach authorization or execution.

## 6. Failure and recovery review

Malformed profiles, oversized responses, malformed envelopes, unknown tools,
argument-shape failures, and call-limit failures return typed repair errors and
are logged by the loop. No retry or autonomous continuation is introduced;
that remains M005 ownership. A failed repair therefore cannot create a parse /
retry cascade.

## 7. Migration and compatibility review

No storage or public wire-protocol migration is required. Existing structured
providers are unchanged. The compatibility contract is additive: a model can
opt into a named grammar through its exact model profile. The current built-in
adapters intentionally opt out because the repository contains no verified
built-in model mapping requiring one.

## 8. Security review

This change removes execution authority from arbitrary model text. The tests
cover final prose containing a destructive-looking bash JSON example, fenced
tool examples, unknown tools, and missing required arguments. No destructive
command is executed by the tests. Repaired mutating tools remain subject to
the normal permission checker and broker authority path.

## 9. Documentation and operations

`architecture/provider.md` and `architecture/agent.md` now document structured
calls as canonical and textual repair as explicit adapter-owned compatibility.
The config field is documented inline in `ModelProfileConfig`.

## 10. Unresolved findings

None at critical, high, or medium severity. Low-severity operational note:
future support for a specific fragile model should add an exact adapter
mapping and fixture before enabling a grammar; no built-in mapping is enabled
by this milestone.

## 11. Roadmap disposition

M002 is closed. M005 is not unblocked yet: its other hard dependency M004
remains open. M003, M004, M007, and M008 remain independently ready. No
corrective pass is required.

## 12. Registry updates

- Move M002 from dependency-ready work to recently closed work.
- Mark the implementation plan `Status: implemented` and the roadmap M002
  status `closed`.
- Keep M005 blocked, updating its blocker to record that M001 and M002 are
  closed and M004 remains outstanding.
- M006 and M009 remain blocked by their existing predecessor sets.
- No future registered plan became dependency-ready from M002 alone.
