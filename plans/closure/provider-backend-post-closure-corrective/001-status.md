# Provider Backend Post-Closure Corrective C001 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/provider-backend-post-closure-corrective/001-direct-openai-endpoint-composition-correctness.md`

Source subsystem roadmap: `plans/subsystems/provider-backend-post-closure-corrective-addendum.md#c001--direct-openai-endpoint-composition-correctness`

Repository baseline reviewed: `62653a85b09ae10efe5e3c379cc68c96a62050f6`

Implementation commits:

- `7963db4461dea738eb5d72ddcb10b7490f3ebf47` — feat: provider backend post-closure correctives C001-C003 implementation (contains C001 endpoint composer, constructor audit, capture-server regressions, docs, and static guard)

## 1. Executive finding

C001 is complete. The native direct-OpenAI request URL contract is fixed:
the default/env/config path emitted `https://api.openai.com/v1/v1/chat/completions`
at the baseline and now emits `https://api.openai.com/v1/chat/completions`
exactly once. One documented API-prefix contract (`base_url` is the versioned
prefix; `chat_completions_url()` appends exactly one `/chat/completions`)
covers default, explicit prefix, trailing-slash, and custom-path cases.
Legacy vendor-specific `OpenAiConfig::{groq,xai,mistral,cerebras}` helpers are
removed; the remaining `default_with_key` + `openai` constructors share the
contract. Body, auth/org headers, stream usage, retry/error mapping, and
shared-wire decoding are unchanged. No unresolved medium-or-higher finding
remains.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Default direct OpenAI submits to `/v1/chat/completions`, never `/v1/v1/chat/completions` | `crates/codegg-providers/src/openai.rs::chat_completions_url()` + `native_openai_capture_server_emits_single_version_prefix` asserts `POST /v1/chat/completions` and absence of `/v1/v1` | Pass |
| Explicit API-prefix overrides compose exactly once | `api_prefix_composer_appends_chat_completions_exactly_once` (explicit `https://.../v1`, trailing slash, custom prefix) | Pass |
| All retained constructors share one base-prefix contract | `remaining_constructors_share_one_api_prefix_contract`; legacy `groq/xai/mistral/cerebras` removed (zero prior call sites; `additional.rs` owns those vendors via compatible transport) | Pass |
| Body/auth/stream behavior unchanged | `native_openai_organization_header_behavior_is_unchanged` (org + auth headers, non-empty body); `cargo test -p codegg-providers --lib` 196 passed including wire/transcript unit coverage | Pass |
| Config/setup compatibility | `setup_catalog_openai_constant_matches_native_default` (`OPENAI_BASE_URL == OpenAiConfig::default().base_url`); `build_durable_provider` OpenAiNative path unchanged except corrected URL | Pass |
| Invalid endpoint fails before I/O without secrets | `api_prefix_composer_rejects_invalid_endpoints_without_secrets` + `native_openai_invalid_endpoint_fails_before_network_io` (`invalid_endpoint`, no secret in debug) | Pass |
| Static guard prevents recurrence | `native_source_does_not_reintroduce_duplicated_version_prefix` (unit) + `scripts/check_openai_endpoint_composition.py` wired into `scripts/verify.sh quick` | Pass |

## 3. Production implementation evidence

- `crates/codegg-providers/src/openai.rs`: documented API-prefix contract on
  `OpenAiConfig.base_url`; new `pub fn chat_completions_url()` (trim, require
  `http(s)://`, reject empty/control, trim trailing `/`, append exactly one
  `/chat/completions`); `stream()` uses `self.cfg.chat_completions_url()?`;
  removed dead `groq/xai/mistral/cerebras` constructors (which mixed host-root
  `https://api.mistral.ai`, `https://api.cerebras.ai` with API-prefix values).
- `architecture/provider.md`: OpenAI section documents the prefix contract and
  the removal rationale.
- `scripts/check_openai_endpoint_composition.py` (new, executable, wired into
  `verify.sh quick`): forbids `{}/v1/chat/completions` reintroduction and the
  legacy helpers, requires the composer.
- Old exact URL: `https://api.openai.com/v1/v1/chat/completions` (baseline
  `base_url=https://api.openai.com/v1` + `format!("{}/v1/chat/completions")`).
  New exact URL: `https://api.openai.com/v1/chat/completions`.

Generic `OpenAiCompatibleProvider` endpoint composition (`{base}/chat/completions`)
is untouched per plan non-goals.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo test -p codegg-providers --lib
cargo test -p codegg-providers --lib openai::tests
cargo clippy -p codegg-providers --all-targets -- -D warnings
cargo check --workspace --all-targets --locked
./scripts/check-core-boundary.sh
python3 scripts/check_provider_wire_cutover.py
python3 scripts/check_openai_endpoint_composition.py
cargo test --test provider_transcripts
```

### Results

- `cargo fmt --all -- --check`: pass.
- `cargo test -p codegg-providers --lib`: 196 passed (includes 11 new
  `openai::tests` + existing wire/circuit/fallback suites).
- `cargo clippy -p codegg-providers --all-targets -- -D warnings`: no issues.
- `cargo check --workspace --all-targets --locked`: pass (396 crates).
- `./scripts/check-core-boundary.sh`: pass.
- `check_provider_wire_cutover.py`, `check_openai_endpoint_composition.py`: pass.
- `cargo test --test provider_transcripts`: environmental link failure
  (root-crate link pulls `libgit2-sys`/`libz` against `/opt/local/lib`
  arm64-only dylibs on an x86_64 link; `ld: symbol(s) not found for
  architecture x86_64`). Unrelated to this change: `codegg-providers` lib
  tests (which own the changed code) are green, and the failure is in
  workspace-link infrastructure, not provider URL/body logic. Recorded as an
  environmental block, not a C001 regression.

## 5. Invariant review

- `OpenAiConfig.base_url` has one documented meaning (API prefix): evidenced by
  doc comment + composer + consistency test.
- Default targets the standard prefix exactly once: capture-server + unit tests.
- Explicit configured base URLs supported per prefix contract (custom path
  preserved): `native_openai_custom_prefix_is_preserved`.
- Body, org headers, stream usage, retry/error mapping, shared-wire decoding
  unchanged: org-header test + full lib suite green.
- Generic compatible composition untouched: no diff in
  `openai_compatible.rs` stream URL logic for C001 (C002 later reuses its parser
  with bounds, preserving fallback policy).
- No live credential in tests: synthetic `test-key`, no secrets in errors.
- Provider IDs/storage/descriptors unchanged: no ID/schema diff.

## 6. Failure and recovery review

URL composition occurs before submission; invalid construction returns typed
`invalid_endpoint` before network I/O (`native_openai_invalid_endpoint_fails_before_network_io`).
Stream cancellation/retry behavior untouched. No new background task, retry
loop, or cache. Control-character and non-HTTP(S) inputs rejected locally.

## 7. Migration and compatibility review

No durable migration, no provider-ID/config-schema change. Configured values
already in API-prefix form become correct. Repository search found zero call
sites for the removed `groq/xai/mistral/cerebras` helpers; those vendors are
served by `additional.rs` compatible factories, so removal cannot repoint a
stored connection. `openai()` vs `default_with_key` differ only in
`requires_org_header`, both on the same prefix.

## 8. Security review

Never logs credentials/query secrets: composer errors carry only stable
`invalid_endpoint` code; tests assert no `sk-secret` in debug. Auth header
remains `Bearer <key>`; org header gated by `requires_org_header`. No URL
secrets in diagnostics.

## 9. Documentation and operations

- `architecture/provider.md` OpenAI section updated (contract + removal).
- `scripts/check_openai_endpoint_composition.py` + `verify.sh quick` wiring.
- Closure record (this file).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | `provider_transcripts` integration binary cannot link in this worktree (arm64 `/opt/local` dylibs vs x86_64 link) | No C001 signal from that binary here | None for C001; run in CI/hosted Linux or fixed toolchain before release |

No medium-or-higher findings.

## 11. Roadmap disposition

C001 closed. C002 and C003 proceeded independently on the same branch
(`7963db44`); no downstream plan was gated on C001 alone. The corrective
addendum remains active until C002/C003 close.

## 12. Registry updates

- `plans/registry.md`: move C001 row from ready to closed with this closure link
  (done in the closure commit).
- `plans/implementation/.../001-...md`: Status `ready for handoff` → `closed`
  with closure link (done in the closure commit).
- Unblock audit: no registered blocked plan lists C001 as a hard/interface
  dependency; nothing unblocked by C001 alone.
