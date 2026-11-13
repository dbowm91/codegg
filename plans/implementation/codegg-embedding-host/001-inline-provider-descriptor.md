# CodeGG Embedding Host Milestone 001 — Inline provider descriptor

Status: proposed

Repository baseline: `1d2d9dce2`

Source roadmap:

- `plans/subsystems/codegg-embedding-host-roadmap.md#7-milestones` (E001)

Long-term requirements:

- `plans/000-long-term-specification.md` — provider ownership and secret handling
- `plans/001-terminology-and-domain-model.md` — provider connection, model, session

Applicable ADRs:

- None yet. This milestone does not need one; the embedding wire does (E002).

Primary class: capability

## 1. Objective

Let one session use a provider connection supplied inline — endpoint,
credential or environment reference, request headers, model catalog and a
declared wire surface — so a host can point CodeGG at the connection it already
uses and the model it already uses, with no configuration file, no credential
store entry and no reviewed-model restriction.

## 2. Why this milestone is ready

No hard dependency. The interfaces it needs already exist:

- Per-turn provider registration with an authority locator
  (`register_builtin_with_config_connections`, `TurnCredentialLocator` in
  `crates/codegg-providers/src/provider_core.rs`).
- A provider that already carries one request header by construction
  (`with_session_affinity_header` in `crates/codegg-providers/src/openai_compatible.rs`).
- A crate-level model-profile concept
  (`crates/codegg-core/src/model_profile/{types,adapter,resolve}.rs`).
- A headless loop driver that builds its registry from config
  (`src/exec.rs`), which is the call site to generalise.

## 3. Current implementation evidence

- `ProviderConfig` (`crates/codegg-config/src/schema.rs`) has `api_key`,
  `base_url`, `models`, `options`, `auth`, `account_id` — **no headers field**.
- `ModelConfig` is `{name, variants}` — no context window, output budget or tool
  support, so metadata cannot be expressed today.
- `CreateProviderConnectionRequest` (`crates/codegg-protocol/src/provider.rs`)
  carries `provider_id`, `endpoint`, `credential`, `credential_kind`, `scope` —
  no headers, and `provider_id` must exist in the setup catalog.
- The model's provider segment is resolved in the built-in registry
  (`src/core/daemon_turns.rs`); an unknown id is `provider_not_found`.
- `opencode_go` resolves its base URL from the shared profile
  (`crates/codegg-providers/src/opencode_go.rs`) and refuses a model without a
  reviewed wire mapping (`model "…" on provider "opencode_go" has no reviewed
  wire mapping`).
- `ExecMode::run` (`src/exec.rs`) loads `Config`, registers providers from it,
  resolves the provider by id and resolves agents from config. It has no
  descriptor path.

## 4. Invariants that must not regress

- The existing config-based and credential-store registration paths behave
  exactly as they do today.
- A credential is never accepted on argv, never written to disk, never logged,
  never echoed in an error, and never serialized in `Debug`.
- CodeGG never guesses a wire surface. The reviewed-wire-mapping rule stays for
  every built-in provider; only a descriptor's explicit declaration is honoured.
- Header values are bounded and validated; a header name that is not a valid
  HTTP token is refused before network I/O.
- Descriptor registration is per session and touches no global state.

## 5. Scope

### In scope

- `InlineProviderDescriptor` with: bounded id and display name, absolute
  `base_url`, bounded header list, credential (inline secret or `env` name),
  model catalog entries (`id`, `context_window`, `max_output_tokens`,
  `supports_tools`), and a declared `protocol` (v1: `chat_completions` only).
- Validation and a stable typed error for every rejection.
- A registration function that builds a ready provider from the descriptor.
- Generalised request headers on the OpenAI-compatible provider.
- Model profile construction from descriptor metadata.
- A provider error class on failures, mapped from the upstream status and
  transport error.

### Explicitly out of scope

- The host wire protocol (E002).
- Non-`chat_completions` surfaces (`responses`, `messages`).
- Provider discovery through a descriptor.
- Persisting a descriptor or a connection.
- A proxy, retries, or fallback between providers.
- Changing the setup catalog or the TUI connection manager.

## 6. Required production changes

### Core/domain

- `codegg-providers`: a new `InlineProviderDescriptor` (and `InlineModel`), a
  validated `ProviderHeaders` newtype, and `register_inline_provider`.
- `codegg-providers`: `ProviderError` gains a `class` (`auth`, `model_not_found`,
  `model_not_supported`, `context_length`, `rate_limit`, `invalid_request`,
  `transport`, `protocol`, `upstream`), populated where the status is known. The
  existing message and code remain.

### Storage and migrations

None. Descriptors are never persisted.

### Protocol and DTOs

- None in `codegg-protocol`. The descriptor type lives in `codegg-providers` so
  the frontend protocol does not acquire an embedding-shaped type.

### Runtime and concurrency

- `src/exec.rs` and the future host mode share one helper that either registers
  from config or registers a descriptor. Config loading becomes optional on that
  path; a missing config must not fail a descriptor session.
- Agent resolution: built-in agents remain available without a project config.

### Frontend or operator surface

None. No TUI change; no CLI flag that carries a secret. A debug-only
`--describe-provider` that prints the descriptor with the credential redacted may
be added for tests.

### Security and authorization

- Descriptor validation is the gate: scheme, host, header bounds, id bounds,
  model-count bound, declared-protocol allow-list.
- Redaction tests assert the secret never appears in a `Debug`, a log capture or
  an error.

### Documentation and static guards

- `architecture/provider.md`: the descriptor and the declared-surface rule.
- A guard (or an existing provider guard) that fails when a new provider surface
  is added without a descriptor entry.

## 7. Ordered work packages

### Work package A — Descriptor type and validation

Intent: make the descriptor expressible and impossible to misuse.

Required changes: the type, its validation, a stable error per rejection, and a
`Debug` that redacts the credential.

Acceptance evidence: a table-driven validation suite (scheme, host, header name,
header length, duplicate header, empty model list, model id bounds, unknown
protocol, oversized descriptor). Credential redaction asserted.

### Work package B — Header injection

Intent: any descriptor-declared header reaches the request.

Required changes: replace the single session-affinity header with a bounded list
on the OpenAI-compatible provider, keeping the existing builder as a thin
wrapper so its callers and tests do not change.

Acceptance evidence: the existing capture-server test proves the header arrives
verbatim; a second test proves two headers arrive and that a `\r`/`\n` value is
refused before I/O.

### Work package C — Descriptor registration

Intent: a descriptor produces a working provider with no config.

Required changes: `register_inline_provider(registry, &descriptor)` building the
OpenAI-compatible provider from the endpoint, credential and headers; the model
catalog attached for resolution; config and credential-store lookups bypassed
for this provider id.

Acceptance evidence: a unit test builds a registry from a descriptor with a fake
endpoint and resolves the provider; a second test proves the same id is absent
when no descriptor is supplied.

### Work package D — Model profile from metadata

Intent: the loop uses the host's known facts instead of guessing.

Required changes: construct a `ModelProfile` from an `InlineModel`
(context window, reserved output, tool support), and prefer it over resolution
for a descriptor-declared model. Bounds are validated.

Acceptance evidence: context limit and reserved output reflected in the
execution policy; a missing entry falls back to the existing resolver, and that
fallback is explicit in a test.

### Work package E — Error class and documentation

Intent: a host can act on a failure without parsing prose.

Required changes: map upstream status and transport errors to the class, keep
the message, add the class to the error's public surface; document the
descriptor and the declared-surface rule.

Acceptance evidence: a matrix test (401 → `auth`, 404 model → `model_not_found`,
400 context → `context_length`, 429 → `rate_limit`, connect failure →
`transport`), and the docs section.

## 8. Failure, cancellation, restart, and contention semantics

- An invalid descriptor fails before any network I/O and names the field, never
  the value.
- Two sessions may use two descriptors with the same id; registration is
  per registry, so they do not collide.
- A descriptor session holds no state across process restart; the host re-opens.
- A canceled turn releases the provider; nothing is cached.
- Concurrent turns within one process keep their own registries; the descriptor
  is never installed globally.

## 9. Compatibility and migration

The descriptor path is additive. No config schema change, no migration, no
removal. `ProviderConfig` gains no field; the reviewed-wire-mapping rule and the
setup catalog are untouched. A host that wants the old behaviour simply does not
supply a descriptor.

## 10. Required tests

### Focused unit tests

- Descriptor validation matrix and credential redaction.
- Header injection with one and two headers; CR/LF refusal.
- Model-profile construction and bounds.
- Error class matrix.

### Integration tests

- A descriptor-built registry resolves the provider and drives one request
  through the capture server.
- `exec`'s config path is unchanged (existing suite).

### Restart and recovery tests

- A second process with the same descriptor starts cleanly; no file or global
  state is involved.

### Contention and cancellation tests

- Two descriptors with the same id in two registries do not interfere.

### Security and negative tests

- Secret absent from `Debug`, logs and error text.
- Invalid header name, oversized header value, unknown protocol refused.

### Migration and compatibility tests

- Existing config registration and `TurnCredentialLocator` paths unchanged.

## 11. Required verification commands

```bash
cargo test -p codegg-providers inline_provider
cargo test -p codegg-providers header
cargo test -p codegg-core model_profile
cargo test -p codegg --bin codegg exec
scripts/verify.sh scoped
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

## 12. Acceptance criteria

1. A descriptor with endpoint, credential, two headers and one model id produces
   a working provider without a config file.
2. Both headers arrive verbatim at the upstream; a CR/LF value is refused before
   I/O.
3. A model outside any built-in list works on a descriptor-declared
   `chat_completions` connection.
4. Descriptor metadata sets the context limit and reserved output used by the
   loop.
5. A 401/404/400-context/429/transport failure carries the documented class.
6. The credential appears in no log, `Debug`, error or serialized frame.
7. Existing config and credential-store registration are unchanged.

## 13. Stop conditions

Stop and re-scope if:

- Header injection cannot be bounded without weakening the provider's existing
  security model.
- The model profile cannot be overridden per session without a global.
- A credential must cross argv or a file to make the path work.
- The descriptor would need to bypass a validation the built-in path relies on.

## 14. Required closure evidence

Closure record under `plans/closure/codegg-embedding-host/001-status.md`:

- the descriptor type and where it is validated;
- the header-injection test with a real capture server;
- the model-profile test proving the policy uses the supplied numbers;
- the error-class matrix;
- the redaction proof;
- the exact commands and results;
- a statement that config registration is unchanged.
