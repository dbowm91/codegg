# Provider Reasoning Admission Corrective Addendum

Status: closed

Audit date: 2026-11-22
Repository baseline: `b554ed7bc` on `feat/auto-20261109-f8abad0d-artifact-safe`.
Provider, model-profile, and daemon turn sources were read in this worktree at
that baseline. The audit itself did not modify them.

Long-term references:
- `plans/000-long-term-specification.md#28-observability`
- `plans/000-long-term-specification.md#29-system-invariants`

Related ADRs:
- `plans/adrs/ADR-0050-per-model-reasoning-capability-authority.md` — **amended**
  by this line. The authority order, the narrowing-only rule, the `Unknown`
  semantics and the always-reasons treatment stand. The local admission
  disposition set changes; the amendment note in the ADR says so in place.
- `plans/adrs/ADR-0049-open-provider-scoped-reasoning-control-vocabulary.md` stays
  accepted. The open label vocabulary is untouched.

Predecessor, not reopened:
- `plans/subsystems/provider-reasoning-control-roadmap.md` (closed, M001–M005).
  Its M003 closure already recorded the consequence that this corrective makes
  untenable: "ADR-0050's fail-closed rule refuses an explicit reasoning
  selection on any model whose provider family publishes no reasoning contract
  (`google`, `local`, `laguna`, `generic`), where today such a selection is
  forwarded." That was recorded as a deliberate rollout consequence. It is a
  defect: the refusal happens before any provider request, so the turn never
  reaches a model.
- `plans/closure/provider-reasoning-control/003-status.md` and `005-status.md`.
  Neither is rewritten. Both remain the historical record of what was built.
- `plans/closure/agent-loop-completion-reliability-corrective/`. That line made
  a spent budget visible. It did not touch reasoning admission.

## 1. Purpose and ownership boundary

A reasoning selection must never decide whether a model call happens. It may
decide what is sent, and it may be dropped with a notice, but a user preference
about depth of thought cannot make a model unreachable.

Owns: the daemon's local admission policy for a resolved reasoning selection,
and the per-dimension application of a declared `Unsupported` fact.

Does not own: the wire encoders, the open label vocabulary, the capability
resolution order, the adapter baselines' contents, the discovery overlay's
live source, or the frontend's option surface. Those stay with their owners.

## 2. Work classification

### Invariants

- A resolved reasoning selection never refuses a turn.
- An `Unknown` dimension is not an `Unsupported` one. An absent contract
  forwards the selection unchanged and the provider is the authority.
- A declared `Unsupported` dimension drops the selection with a notice that
  names what was dropped. The turn still runs.
- The kernel supplies the decision function; CodeGG supplies the facts and the
  disposition policy. No second policy table is introduced.
- A declared `Unsupported` fact is applied per dimension, ahead of the
  kernel's overall-status read, so a partial record cannot lose the one fact it
  declares.

### Capabilities

- A person with a persisted selection — including `off` — can make a model
  call on every provider family.
- A person who selected a level a model cannot express is told the level was
  dropped, rather than being told the turn failed.
- An operator override that declares a dimension `unsupported` is honoured.

## 3. Findings

**F1 — Four of seven adapter families publish no capability record.**
`crates/codegg-core/assets/model-adapters/` contains `anthropic.toml`,
`openai.toml`, and `minimax.toml` with a `[profile.reasoning_capability]`
block; `generic.toml`, `google.toml`, `laguna.toml`, and `local.toml`
declare none. `resolve_reasoning_capability(None, None, None)` therefore
returns an all-`Unknown` record for every model in those families. The
discovery overlay is inert machinery with no live source, so it narrows
nothing.

**F2 — The daemon applied the kernel's fail-closed default.**
`src/core/daemon_turns.rs` `admit_reasoning_capability` passed
`ReasoningCapabilityPolicy::default()`: `unknown → Reject`,
`unsupported → Reject`, `mixed → WarnDrop`. Combined with F1, any resolved
selection on four families was a turn refusal with code
`reasoning_capability_unsupported`, returned before provider resolution and
before any request was dispatched.

**F3 — The refusal was live on this machine.** `runtime_preferences` held one
row with the serialized selection `{"mode":"off"}` — `ReasoningOverride::Off`.
A persisted selection applies to every subsequent turn, so every turn was
refused. Selecting `off` again could not clear it: a disable is itself judged,
and `Unknown` toggle refused it.

**F4 — A covered family refused too.** `minimax.toml` declares
`disable_status = "unsupported"` with `always_reasons = true`, so the same
persisted `Off` refused every MiniMax turn under `unsupported → Reject`. The
baseline was correct; the disposition was not.

**F5 — A partial record lost its one declared fact.** The kernel's
`reasoning_capability_notices_neutral` reads the model's **overall** status
first and falls through to it unless that status is `Supported`. A record that
declares only `effort_status = "unsupported"` has overall status `Unknown`,
so the effort dimension — the one fact declared — was judged as unknown. That
is the exact shape an operator override creates, so an override narrowing
effort to unsupported was silently ineffective.

## 4. Scope

In: the shipped admission policy, its daemon call site, the per-dimension
`Unsupported` short-circuit, the doc and ADR amendment, and the tests that
encoded the refusal.

Out: no new adapter baselines (the families without a published contract still
carry none — inventing one is the fabrication ADR-0050 refuses), no discovery
overlay wiring, no config knob for the policy, no protocol variant, no storage
change, no change to the encoders or to the open label vocabulary, and no
frontend change. The frontend's option surface already derives from the
capability record; it is not the reason a call was impossible.

## 5. Milestone

| Milestone | Scope | Status |
|---|---|---|
| C001 | A reasoning preference never blocks a call | closed; `plans/closure/provider-reasoning-admission-corrective/001-status.md` |
