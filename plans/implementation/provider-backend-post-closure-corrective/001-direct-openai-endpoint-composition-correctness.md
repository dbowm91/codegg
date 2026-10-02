# Provider Backend Post-Closure Corrective C001 — Direct OpenAI Endpoint Composition Correctness

Status: ready for handoff

Repository baseline: `62653a85b09ae10efe5e3c379cc68c96a62050f6`

Source corrective addendum:

- `plans/subsystems/provider-backend-post-closure-corrective-addendum.md#c001--direct-openai-endpoint-composition-correctness`

Long-term requirements:

- `plans/000-long-term-specification.md#13-provider-architecture-and-eggpool`
- `plans/001-terminology-and-domain-model.md`

Applicable ADRs:

- None required.

Primary class: invariant

## 1. Objective

Fix the native direct-OpenAI request URL contract so the default/env/config path cannot construct `/v1/v1/chat/completions`, and add endpoint-composition tests that cover default, configured API-prefix, and trailing-slash inputs without changing request-body/wire behavior.

## 2. Why this milestone is ready

The defect is local and directly evidenced at the baseline:

- `OpenAiConfig::default().base_url == "https://api.openai.com/v1"`;
- `OpenAiProvider::stream()` appends `"/v1/chat/completions"`;
- `register_builtin` and `register_builtin_with_config` construct `OpenAiConfig::default_with_key`.

No external dependency or protocol redesign is required.

## 3. Current implementation evidence

- `crates/codegg-providers/src/openai.rs` owns the native OpenAI transport wrapper.
- `setup_catalog.rs::OPENAI_BASE_URL` also uses the versioned API prefix `https://api.openai.com/v1`.
- The shared `eggpool-wire` encoder only creates the body and is not responsible for endpoint composition.
- Current provider transcript tests qualify body semantics but do not capture the final native OpenAI URL.
- Legacy `OpenAiConfig::{groq,xai,mistral,cerebras}` helpers use inconsistent base-prefix conventions and appear to have no repository call sites; they must be audited rather than silently preserved as examples of the broken contract.

## 4. Invariants that must not regress

- `OpenAiConfig.base_url` has one documented meaning.
- Default direct OpenAI must target the standard API prefix exactly once.
- Explicit configured base URLs remain supported according to that documented prefix contract.
- Request body, auth/org headers, stream usage options, retry/error mapping, and shared-wire decoding remain unchanged.
- Generic `OpenAiCompatibleProvider` endpoint composition is not changed by this milestone.
- No live OpenAI credential is required for tests.
- Provider IDs/storage/connection descriptors remain unchanged.

## 5. Scope

### In scope

- Choose and document one base URL contract. The preferred evidence-backed shape is **API prefix**, e.g. `https://api.openai.com/v1`, with `chat/completions` appended exactly once.
- Implement a small URL/path composer or equivalent normalized join used by native OpenAI.
- Correct the default/env/config path.
- Audit/remove/narrow/update unused convenience constructors so every remaining constructor obeys the same contract.
- Add local capture-server tests asserting the exact request path.
- Add setup-catalog/native-provider consistency tests for OpenAI.
- Update provider docs.

### Explicitly out of scope

- Provider wire/body changes.
- Responses API migration.
- OpenAI model catalog refresh.
- Other provider endpoint audits (owned by C002).
- Retry/fallback changes.
- Public provider ID/config schema changes.

## 6. Required production changes

### Endpoint contract

Make the code distinguish "API prefix" from "host root" by construction/documentation rather than relying on each constructor to remember whether `/v1` is already present.

The implementation may use a URL-aware helper or a tightly bounded string/path join, but must:

- reject/propagate invalid configured URLs through existing typed provider errors where relevant;
- handle one trailing slash;
- avoid `/v1/v1`;
- avoid stripping arbitrary configured path prefixes;
- never log credentials/query secrets.

Do not special-case the OpenAI hostname; the contract should work with an explicitly configured OpenAI-compatible API prefix passed to the native OpenAI wrapper.

### Legacy convenience constructors

Search all call sites for `OpenAiConfig::{groq,xai,mistral,cerebras,openai}`.

If unused and not part of an intentional public compatibility promise, narrow/remove them or mark them consistently. If retained, update their values to obey the same API-prefix contract and add tests. Do not leave mixed host-root/API-prefix semantics.

## 7. Ordered work packages

### Work package A — Freeze failing endpoint regression

Add a capture-server test that proves the baseline default attempts `/v1/v1/chat/completions` and the corrected implementation emits `/v1/chat/completions`.

### Work package B — Normalize native endpoint composition

Implement the prefix contract and update constructors/callers.

### Work package C — Config/setup compatibility

Test default env registration plus explicit base URL override/trailing slash and setup-catalog construction.

### Work package D — Documentation/guard closure

Update `architecture/provider.md` and add the narrowest static/unit guard that prevents reintroducing hard-coded duplicated `/v1` composition in the native provider.

## 8. Failure, cancellation, restart, and contention semantics

Unchanged. URL composition occurs before submission; invalid endpoint construction fails before network I/O. Stream cancellation/retry behavior is untouched.

## 9. Compatibility and migration

No durable migration.

Configured values that already follow the documented API-prefix form become correct. If repository evidence shows supported users historically supplied a host root instead, implementation must either preserve both unambiguously with typed normalization or stop and document the compatibility decision rather than guessing.

## 10. Required tests

- exact default native OpenAI request path;
- explicit `https://.../v1` prefix;
- same prefix with trailing slash;
- configured non-default path prefix;
- invalid URL/control-character negative case if validation is introduced;
- organization header behavior unchanged;
- stream-usage/body transcript unchanged;
- provider setup construction uses the corrected endpoint contract.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo test -p codegg-providers
cargo test --test provider_transcripts
cargo clippy -p codegg-providers --all-targets -- -D warnings
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Run current provider-wire/static guards and `scripts/verify.sh quick` if required by the repository testing contract. If broad workspace failures reproduce on untouched baseline, record that evidence rather than weakening tests.

## 12. Documentation updates

- `architecture/provider.md`
- corrective addendum/registry
- closure record.

## 13. Acceptance criteria

- default direct OpenAI submits to `/v1/chat/completions`, never `/v1/v1/chat/completions`;
- explicit API-prefix overrides compose exactly once;
- all retained `OpenAiConfig` constructors share one base-prefix contract;
- body/auth/stream behavior is unchanged;
- provider tests and required broader verification pass or unrelated baseline failures are explicitly reproduced/classified.

## 14. Stop conditions

Stop if fixing the endpoint requires changing the public config schema, provider ID/storage, or wire grammar; or if historical supported base-url semantics are contradictory and cannot be preserved without an explicit migration decision.

## 15. Closure evidence required

Record implementation commit, old/new exact URLs, constructor/call-site audit, capture-server tests, config/setup compatibility evidence, provider/full verification, and residual findings.

## 16. Handoff notes

This is a correctness fix, not a provider refactor. Keep it small enough that C002 can independently own the broader endpoint/catalog audit.
