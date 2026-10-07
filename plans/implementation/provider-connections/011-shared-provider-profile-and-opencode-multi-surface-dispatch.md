# Provider Connections Milestone 011 — Shared Provider Profile and OpenCode Go Multi-Surface Dispatch

Status: blocked

Repository baseline: `d85ed67bef970cfe99e320a7876e7a51373b7e37`

Source roadmap:

- `plans/subsystems/provider-connect-qualification-wire-dispatch-corrective-addendum.md#milestone-011--shared-provider-profile-consumption-and-opencode-go-multi-surface-dispatch`

Long-term requirements:

- `plans/000-long-term-specification.md#13-provider-architecture-and-eggpool`
- `plans/001-terminology-and-domain-model.md`
- `plans/003-planning-process.md`

Applicable ADRs:

- None currently. Stop and write an ADR if direct-provider operation would require an EggPool process, if credential authority moves out of CodeGG, or if speculative cross-surface inference retry becomes necessary.

Primary class: capability

## 1. Objective

Consume the closed EggPool `eggpool-provider-profile` contract by immutable revision, use it as the shared secret-free metadata source for overlapping providers, and replace CodeGG's one-surface OpenCode Go wrapper with model-aware Chat Completions / Responses / Anthropic Messages dispatch using `eggpool-wire` and the profile's per-surface auth/path rules.

## 2. Why this milestone is blocked

Hard dependency 1: M010 must close so connection/catalog/auth state has truthful semantics before multi-surface execution is qualified.

Hard dependency 2: EggPool Shared Provider Profile Contract M001 must close and publish an immutable Git revision containing:

- neutral provider profile types/registry;
- OpenCode Go per-surface paths/auth;
- conservative credential-verification metadata;
- reviewed non-fixed exact model-wire hints.

Do not copy the planning branch implementation into CodeGG. Consume only the accepted closure revision.

## 3. Current implementation evidence

- `codegg-providers` already pins `eggpool-wire` and delegates standard OpenAI Chat, Anthropic Messages, and Gemini wire grammar to the shared kernel.
- `create_opencode_go()` currently constructs the generic `OpenAiCompatibleProvider`.
- The generic compatible stream path therefore targets `/chat/completions` and global compatible auth semantics.
- OpenCode Go requires stable `x-opencode-session`, already carried through `ProviderRequestContext` by M008/M009.
- Current first-party OpenCode Go documentation uses three endpoint families. Representative current assignments:
  - `gpt-6-luna`, `gpt-5.6-luna`, Grok 4.6/4.7, Muse contributor models -> `/responses`;
  - GLM/Kimi/DeepSeek/MiMo/Hy/LongCat/Space Bunny -> `/chat/completions`;
  - MiniMax/Qwen -> `/messages`.
- The public `/models` response exposes IDs but not a wire-surface field, so CodeGG cannot safely derive the surface from discovery alone.
- OpenCode Go's Messages surface uses `x-api-key`; Chat and Responses use Bearer auth.
- The historical wire consolidation intentionally left provider endpoint/auth/model-catalog metadata CodeGG-owned and explicitly deferred later profile metadata consolidation.

## 4. Invariants that must not regress

- CodeGG continues to run direct providers with no EggPool daemon.
- CodeGG retains credential values, secret references, connection lifecycle, HTTP transport, cancellation, and retry/error ownership.
- Shared profile data contains no secrets.
- `eggpool-wire` remains the standard protocol grammar/stream authority.
- M008/M009 stable OpenCode `x-opencode-session` semantics apply to every OpenCode Go wire surface.
- A model is sent only to an explicitly resolved surface.
- Unknown/unmapped OpenCode Go models are unresolved; do not default to Chat Completions and do not guess by model family/prefix.
- No speculative retry from one inference surface to another after an upstream request could have been accepted.
- Per-surface auth owns Authorization/`x-api-key`; static extras cannot collide with credential/session/content headers.
- Model selection/storage IDs remain stable.

## 5. Scope

### In scope

- Pin `eggpool-provider-profile` at EggPool M001 closure revision.
- CodeGG adapter from shared profile IDs to setup/catalog presentation.
- Consolidate overlapping endpoint/auth/models-profile facts where safe; preserve CodeGG-only providers/extensions locally.
- Dedicated OpenCode Go provider/factory capable of selecting three shared wire surfaces.
- Stateless direct OpenAI Responses request/stream bridge through `eggpool-wire`.
- Anthropic Messages use for Go models without reusing MiniMax-specific provider code.
- Model catalog filtering/qualification for known wire-resolved models.
- Focused capture-server tests for endpoint, auth header, session header, request grammar, stream decode, and unknown-model failure.

### Explicitly out of scope

- EggPool runtime/account/router import.
- General reactive wire negotiation in CodeGG.
- Wildcard/family model-wire rules.
- Runtime provider-doc scraping.
- Stateful/hosted Responses program subsystem redesign.
- Provider retry/fallback redesign.
- New provider additions or pricing work.
- Changing M010 credential qualification semantics.

## 6. Required production changes

### Shared dependency and adapter

Add `eggpool-provider-profile` as a direct dependency at the immutable M001 closure revision, following the existing `eggpool-wire`/`eggpool-model-routing` pinning precedent.

Create a narrow adapter that projects shared profile facts into CodeGG setup/runtime needs. Do not expose EggPool root config/runtime types.

For provider IDs represented in both repos, remove or guard duplicated base URL/per-surface auth/path facts where the shared profile now supplies them. CodeGG-only UI labels/config extension data may remain local.

Provider IDs differ in spelling where existing public/durable compatibility requires it (for example local underscore vs hyphen conventions). Use an explicit adapter map; do not rename durable CodeGG IDs merely to match EggPool.

### OpenCode Go provider

Replace the generic one-surface factory with a provider that:

1. resolves the requested model against the shared OpenCode Go profile;
2. obtains the model's preferred `WireSurface`;
3. obtains the corresponding path/auth profile;
4. converts the CodeGG semantic `ChatRequest` into the shared canonical request;
5. encodes using the correct `eggpool-wire` surface;
6. builds one HTTP request with CodeGG-owned transport plus profile-selected auth and the required stable `x-opencode-session`;
7. decodes the selected surface stream/events back into CodeGG `ChatEvent`.

The provider should share as much transport/header/error/cancellation machinery as practical with existing compatible providers without forcing all providers into multi-surface behavior.

### Direct stateless Responses support

Extend `crates/codegg-providers/src/wire.rs` or the existing shared-wire bridge so a semantic CodeGG request can be encoded/stream-decoded through `WireSurface::OpenaiResponses` for a direct stateless provider request.

Do not route this through CodeGG's higher-level hosted/stateful Responses program subsystem. This milestone only needs the ordinary stateless provider surface supplied by `eggpool-wire`.

Preserve tools, usage, finish/error events, reasoning/private-history policy, cancellation, and completed tool-call accumulation according to existing shared-kernel semantics. Any unsupported CodeGG semantic field must fail/adapt according to the shared fidelity policy, not be silently discarded.

### Anthropic Messages support under OpenCode Go

Use the shared Anthropic Messages codec/stream path for models whose profile resolves to Messages.

Authentication for this surface must follow the profile (`x-api-key`) rather than the generic OpenAI-compatible Bearer path.

Do not instantiate the dedicated direct-Anthropic provider if that would bring Anthropic-specific base/version assumptions not present in OpenCode Go. Share wire codec helpers, not provider identity.

### Model catalog qualification

The upstream `/models` list can remain the availability source, but only models with a resolved shared wire hint are selectable/executable in this milestone.

Required behavior for a newly discovered model absent from the shared profile:

- keep the raw discovery result available to diagnostics if useful;
- mark it wire-unresolved and omit it from selectable model choices, or surface it disabled with an actionable reason if the existing UI supports disabled catalog entries;
- never advertise it as a normal selectable model and then default to Chat.

A later shared-profile update can make the model selectable without a CodeGG source-code provider branch.

### Authentication/header ownership

Per-surface credential application must be deterministic:

- Chat/Responses: Bearer Authorization;
- Messages: `x-api-key`;
- `x-opencode-session`: all OpenCode Go inference surfaces that require the existing affinity contract;
- content type and any static headers: one owner each.

Reserved-header collision checks from M008 remain in force.

### Error classification

Keep CodeGG's typed status/error taxonomy. M010 owns connection verification state; M011 must feed actual 401/403 and success through that existing path.

A wrong/unresolved surface is a local configuration/metadata error before network I/O, not an authentication error.

## 7. Ordered work packages

### Work package A — Pin and adapt the shared profile crate

Add the dependency, map provider IDs, project setup metadata, and add parity tests showing shared facts match intended CodeGG presentation.

### Work package B — Generalize the shared wire bridge for direct Responses

Add stateless Responses encode/stream projection and focused semantic parity tests.

### Work package C — Implement OpenCode Go multi-surface request construction

Resolve model -> surface -> path/auth; preserve session affinity and CodeGG transport/error ownership.

### Work package D — Qualify discovery/selectability

Filter/mark unresolved model IDs so a catalog row is never mistaken for executable wire support.

### Work package E — End-to-end capture fixtures and cleanup

Capture representative Chat/Responses/Messages requests, remove obsolete OpenCode-Go-specific duplicated metadata/one-surface assumptions, update docs/guards, and close.

## 8. Failure, cancellation, restart, and contention semantics

One logical provider request selects exactly one surface before network send.

Do not attempt another surface after any response state that could indicate the request reached model execution. This milestone has no speculative wire negotiation.

Cancellation/deadline behavior remains CodeGG-owned and must be identical across surfaces.

Restart requires no new state: shared profile metadata is embedded; durable connection/model selection remains existing CodeGG data.

If a previously selected model is no longer wire-resolved after an application update, fail locally with an actionable model/profile diagnostic rather than switching surfaces silently.

## 9. Compatibility and migration

No database migration should be required for model IDs or provider connection IDs.

Existing OpenCode Go connections continue using the same durable provider ID/secret reference. Their runtime implementation changes underneath the same connection identity.

If CodeGG provider IDs cannot directly match shared profile IDs, keep a stable explicit mapping. Do not rewrite stored connection records solely for naming consistency.

Existing non-OpenCode compatible providers must remain on their current single-surface paths unless independently migrated.

## 10. Required tests

Shared profile integration:

- pinned profile registry loads;
- CodeGG OpenCode Go adapter resolves correct base/surfaces/auth;
- shared overlapping provider metadata has no contradictory second owner.

Representative network capture tests:

- `gpt-6-luna` -> `/responses`, Bearer, stable `x-opencode-session`, Responses grammar;
- `glm-5.3-flash` or another current Chat model -> `/chat/completions`, Bearer, stable session, Chat grammar;
- `minimax-m3` -> `/messages`, `x-api-key`, stable session, Anthropic Messages grammar;
- one current Qwen Messages model;
- one current Grok/Muse Responses model if fixtures remain small.

Unknown model test:

- model appears in discovery but has no profile wire hint;
- no inference request is sent;
- model is not exposed as ordinarily selectable or returns an explicit wire-unresolved selection diagnostic.

Stream/tool tests:

- text stream;
- tool call accumulation;
- usage/final event;
- malformed/incomplete stream behavior for each newly exercised surface;
- cancellation/timeout parity.

Auth/error tests:

- 401/403 reaches M010 auth-failure classification;
- wrong/missing session context remains local zero-network failure;
- Messages does not send Bearer instead of `x-api-key`;
- Chat/Responses do not send `x-api-key` as credential auth;
- secrets/header values absent from logs/errors.

Regression:

- existing OpenAI-compatible, OpenAI, Anthropic, provider-connections M008/M009, and shared-wire tests remain green.

## 11. Required verification commands

After replacing the placeholder dependency revision with the accepted EggPool M001 closure revision:

```bash
cargo fmt --all -- --check
cargo test -p codegg-providers -- --test-threads=1
cargo clippy -p codegg-providers --all-targets -- -D warnings
python3 scripts/check_provider_wire_boundary.py
git diff --check
scripts/verify.sh quick
```

Run the focused provider-connection M010/M008/M009 regression targets and any shared-wire transcript tests named by the current repository. Closure must record exact commands/results.

## 12. Documentation updates

- `architecture/provider.md`
- provider connection/`/connect` documentation
- shared wire/provider-profile ownership documentation
- `plans/closure/provider-connections/011-status.md`
- `plans/registry.md`

## 13. Acceptance criteria

M011 may close only when:

1. CodeGG pins the accepted EggPool shared provider-profile revision.
2. OpenCode Go base/path/auth facts come from the shared profile adapter rather than a contradictory local copy.
3. Representative Responses models use `/responses`.
4. Representative compatible models use `/chat/completions`.
5. Representative MiniMax/Qwen models use `/messages`.
6. Per-surface auth is correct.
7. Stable `x-opencode-session` survives all three paths.
8. Direct stateless Responses uses `eggpool-wire`, not the hosted/stateful program subsystem.
9. Unknown/unmapped discovered models are unresolved rather than defaulted to Chat.
10. No speculative cross-surface retry is introduced.
11. M010 verification-state semantics consume real inference outcomes correctly.
12. Existing non-OpenCode providers do not regress.
13. Focused provider/shared-wire tests and quick verification pass.
14. No medium-or-higher unresolved defect remains.

## 14. Stop conditions

Stop and report if:

- EggPool M001 closure does not expose a usable immutable neutral profile contract;
- the shared profile contains no trustworthy OpenCode model-wire mapping and current first-party evidence cannot establish one;
- `eggpool-wire` cannot represent a required direct stateless Responses semantic without broad hosted-program redesign;
- correct OpenCode execution would require speculative retry across surfaces;
- credential/session ownership would need to move into the shared profile crate;
- durable CodeGG provider IDs would need a breaking rename.

## 15. Closure evidence required

Create `plans/closure/provider-connections/011-status.md` containing:

- exact EggPool profile revision;
- shared-vs-local metadata ownership matrix;
- current OpenCode model-wire source/review date;
- representative Chat/Responses/Messages capture evidence;
- per-surface auth/session-header evidence;
- unknown-model zero-network failure evidence;
- M010 auth-state integration evidence;
- regression/verification commands;
- compatibility/security review;
- unresolved findings and recommendation;
- final corrective roadmap/registry disposition.

## 16. Handoff notes

Do not implement M011 from the EggPool planning branch. Wait for M001 closure and pin that immutable revision. If OpenCode adds a new model before implementation, update the EggPool shared profile first rather than adding a CodeGG-only model table.
