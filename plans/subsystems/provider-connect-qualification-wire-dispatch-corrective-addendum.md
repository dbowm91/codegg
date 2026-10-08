# Provider Connections — Qualification and Multi-Surface Dispatch Corrective Addendum

Status: closed (both milestones accepted)

Closure records:

- `plans/closure/provider-connect-qualification-wire-dispatch-corrective-addendum/010-status.md`
- `plans/closure/provider-connect-qualification-wire-dispatch-corrective-addendum/011-status.md`

Repository baseline reviewed: `d85ed67bef970cfe99e320a7876e7a51373b7e37`

Parent and predecessor work:

- `plans/subsystems/provider-connections-roadmap.md` — historical M001–M005 closure.
- `plans/subsystems/provider-opencode-session-affinity-corrective-addendum.md` — M008 closed.
- `plans/subsystems/provider-direct-call-session-context-corrective-addendum.md` — M009 closed.
- `plans/subsystems/provider-wire-kernel-consolidation-roadmap.md` — M001–M003 closed; `eggpool-wire` is the standard wire owner.
- `plans/subsystems/provider-backend-post-closure-corrective-addendum.md` — endpoint/catalog discovery reconciliation closed.
- EggPool sibling roadmap `plans/subsystems/shared-provider-profile-contract-roadmap.md` on branch `codex/plan-shared-provider-profile-contract`.

Long-term references:

- `plans/000-long-term-specification.md#13-provider-architecture-and-eggpool`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md#phase-2--eggpool-and-daemon-owned-provider-connections`
- `plans/003-planning-process.md`

No ADR is required for the two registered correctives. They preserve the accepted boundary: CodeGG owns direct-provider connection state, credentials, HTTP transport, and turn/runtime selection; `eggpool-wire` owns shared wire grammar; the new EggPool profile crate supplies secret-free provider metadata only. Stop and write an ADR if implementation would require the EggPool daemon at runtime, move credential authority out of CodeGG, or introduce speculative retry across potentially accepted inference requests.

## 1. Corrective trigger

A production-path review of `/connect` and OpenCode Go found that the current provider connection capability is not semantically complete even though the historical workflow is closed.

### Finding A — model enumeration is being used as credential verification

The setup catalog configures several providers with `SetupProbeStrategy::DirectModels`. The provisioning path treats `Provider::models()` as a connection/authentication probe.

That is not a valid general contract:

- several providers return hard-coded model arrays without network I/O, so an invalid credential can be marked healthy;
- generic OpenAI-compatible model discovery is deliberately best-effort and may fall back rather than proving auth;
- OpenCode Go's `/zen/go/v1/models` endpoint is publicly readable, so a 200 response proves catalog reachability, not credential validity.

The result is both false-positive and false-negative connection state.

### Finding B — OpenCode Go is one provider with three wire surfaces

Current first-party OpenCode Go documentation reviewed 2026-10-07 assigns models across:

- `/responses` for current GPT/Grok/Muse models;
- `/chat/completions` for current GLM/Kimi/DeepSeek/MiMo/Hy/LongCat/Space Bunny models;
- `/messages` for current MiniMax/Qwen models.

The current CodeGG factory still produces one `OpenAiCompatibleProvider`, so ordinary inference is fundamentally Chat-Completions-shaped. The Messages surface also uses `x-api-key` while Chat/Responses use Bearer authentication.

### Finding C — the public model catalog does not solve wire selection

The current public OpenCode Go `/models` response is an OpenAI-style list of IDs and does not include a wire-surface field. CodeGG must not infer protocol from successful catalog discovery or default every model to Chat Completions.

### Finding D — EggPool already has the reusable metadata shape

EggPool already models provider-level and per-wire-surface endpoint/auth facts plus model-wire preferences. A new sibling extraction is being planned there so CodeGG can consume one neutral profile contract rather than add another provider-specific table.

## 2. Corrective milestones

### Milestone 010 — Provider connection qualification semantics

Status: closed. Closure record:
`plans/closure/provider-connect-qualification-wire-dispatch-corrective-addendum/010-status.md`.

Implementation plan:

- `plans/implementation/provider-connect-qualification-wire-dispatch-corrective-addendum/010-provider-connection-qualification-semantics.md`

Primary class: capability correctness / invariant.

Objective: separate catalog discovery from credential verification, make `/connect` truthful for providers whose credentials cannot be safely verified at provisioning time, and ensure inference auth outcomes update connection state without using static/public model lists as proof.

Dependencies:

- historical Provider Connections M009 closed;
- no external hard dependency.

### Milestone 011 — Shared provider-profile consumption and OpenCode Go multi-surface dispatch

Status: closed. Closure record:
`plans/closure/provider-connect-qualification-wire-dispatch-corrective-addendum/011-status.md`.

Implementation plan:

- `plans/implementation/provider-connect-qualification-wire-dispatch-corrective-addendum/011-shared-provider-profile-and-opencode-multi-surface-dispatch.md`

Primary class: capability correctness / infrastructure.

Objective: pin the closed EggPool shared provider-profile contract, replace duplicated overlapping setup metadata with a CodeGG adapter, and make direct OpenCode Go model invocation choose the correct Chat/Responses/Messages surface and per-surface auth through `eggpool-wire`.

Dependencies:

- hard: M010 closed;
- hard: EggPool shared-provider-profile-contract M001 closed with immutable revision;
- interface: existing `eggpool-wire` canonical direct-provider codecs/stream decoders.

## 3. Invariants

- Direct providers work without an EggPool process.
- Credentials remain daemon-owned CodeGG secrets and never enter shared provider-profile data.
- `/connect` must not claim a credential is verified unless an authenticated operation actually proved it.
- A provider may be configured with a usable catalog while credential state remains unverified.
- No automatic billable dummy completion is issued merely to validate a key.
- First genuine inference may transition an unverified credential to verified or authentication-failed.
- 401/403 authentication failures are not reported as generic network/catalog failures.
- OpenCode Go session affinity from M008/M009 remains stable across all three wire surfaces.
- Standard payload grammar and stream decoding come from `eggpool-wire`; provider code owns endpoint/auth/header/transport.
- Unknown OpenCode Go model-wire mapping fails closed/unresolved rather than silently using Chat Completions.
- No speculative cross-surface retry after a request might have been accepted or produced side effects.

## 4. Non-goals

- EggPool account/routing/quota/health import.
- A generalized multi-provider wire-negotiation engine in CodeGG.
- Runtime scraping of provider docs.
- Arbitrary frontend/header passthrough.
- New provider additions.
- Model pricing/routing work.
- Live credentials in routine CI.
- Rewriting historical M001–M009 closure evidence.

## 5. Dependency graph

```text
CodeGG M010 — truthful connection qualification   [closed]
        |
        +-----------------------------+
                                      |  [satisfied]
EggPool shared profile M001 ----------+
                                      v
CodeGG M011 — profile consumption + OpenCode Go multi-surface execution   [closed]
```

M010 is intentionally independently ready so the misleading `/connect` semantics can be corrected without waiting for cross-repository extraction.

All three prerequisites are now **historical and satisfied**: M010 closed, the EggPool shared-provider-profile M001 contract closed at immutable revision `9ac6a1318e8db3c034b5ab54987317752d5ffea6`, and the `eggpool-wire` interface was already consumed by the existing direct-provider codecs and stream decoders. Nothing in this roadmap is blocked.

## 6. Completion definition

This corrective closes after M010 and M011 have accepted closure records: `/connect` no longer equates model enumeration with key validity, and every selectable OpenCode Go model has an explicit supported wire mapping that uses the correct endpoint/auth/codec path or is explicitly unresolved rather than misrouted.

**Satisfied** by the two accepted closure records above. M010 delivered separate typed catalog/credential axes with durable revision-scoped inference feedback; M011 delivered single-surface resolution through the pinned shared profile with per-surface auth and a stable session header, failing closed on unknown models.

## 7. Corrective status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M010 provider connection qualification semantics | closed | `plans/implementation/provider-connect-qualification-wire-dispatch-corrective-addendum/010-provider-connection-qualification-semantics.md` | `plans/closure/provider-connect-qualification-wire-dispatch-corrective-addendum/010-status.md` | none |
| M011 shared provider profile + OpenCode Go multi-surface dispatch | closed | `plans/implementation/provider-connect-qualification-wire-dispatch-corrective-addendum/011-shared-provider-profile-and-opencode-multi-surface-dispatch.md` | `plans/closure/provider-connect-qualification-wire-dispatch-corrective-addendum/011-status.md` | none — resolved 2026-10-07 (EggPool M001 closed at `9ac6a131`); WP-A additionally realigned CodeGG's `eggpool-wire` pin to the same rev, since the profile's path dependency would otherwise link two copies. Unresolved models fail closed locally rather than defaulting to Chat. |

This roadmap is terminal. Post-closure reconciliation, the durable
connection → selection → OpenCode inference trajectory, low-finding disposition,
and hosted CI evidence are owned by the follow-on corrective
`plans/subsystems/provider-connect-qualification-wire-dispatch-post-closure-corrective-addendum.md`
(C001), which does not reopen M010 or M011 behavior.
