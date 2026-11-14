# CodeGG Embedding Host E001 — Inline provider descriptor: closure record

Status: **closed**.

Plan: [001-inline-provider-descriptor](../../implementation/codegg-embedding-host/001-inline-provider-descriptor.md).
Roadmap: [codegg-embedding-host-roadmap](../../subsystems/codegg-embedding-host-roadmap.md#7-milestones).
Commit: `089f9479c` on `feat/codegg-embedding-host`.

## Delivered

| Requirement | Where |
|---|---|
| Descriptor type with bounded id, display name, absolute `baseUrl`, bounded header list, credential, `protocol`, model catalog | `crates/codegg-providers/src/inline.rs` (`InlineProviderDescriptor`, `InlineModel`, `InlineCredential`, `ProviderHeader(s)`), camelCase serde |
| One stable typed error per rejection, naming the field and never the value | `InlineProviderError` (24 variants) |
| Redaction in `Debug`, in an error's `Display`, and a log-safe view | `Debug` for `InlineCredential`/`InlineProviderDescriptor`, `redacted_json()`, `REDACTED` |
| Bounded request headers on the OpenAI-compatible provider, with the old single-header builder kept as a wrapper | `OpenAiCompatibleProvider::with_provider_headers`, `declared_headers`, reserved-family and duplicate refusal |
| Descriptor registration with no config and no credential store | `register_inline_provider(&mut ProviderRegistry, &InlineProviderDescriptor)` |
| Model profile from declared metadata, through the loop's existing resolver | `crates/codegg-core/src/model_profile/inline.rs` (`InlineModelProfile` → `ModelProfileConfig` + `CompactionConfig`) |
| Provider error class | `ProviderErrorClass` in `crates/codegg-providers/src/error.rs` (`auth`, `model_not_found`, `model_not_supported`, `context_length`, `rate_limit`, `invalid_request`, `transport`, `protocol`, `upstream`) |
| A headless caller can register from a descriptor instead of config | `ExecMode::with_provider_descriptor`; a missing config file does not fail the session |
| Documentation | `architecture/provider.md` — descriptor, declared-surface rule, secret handling |

## Evidence

### Validation and redaction (`cargo test -p codegg-providers inline`)

14 passed, 0 failed:

```
inline::tests::crlf_headers_are_refused_by_validation_before_io ... ok
inline::tests::debug_and_redacted_json_never_render_secrets ... ok
inline::tests::descriptor_validation_matrix_names_each_rejected_field ... ok
inline::tests::descriptor_wire_form_is_camel_case_and_round_trips ... ok
inline::tests::env_credential_resolves_at_registration_and_stays_redacted ... ok
inline::tests::header_validation_matrix_names_fields_without_values ... ok
inline::tests::log_capture_never_renders_secrets ... ok
inline::tests::missing_env_credential_is_refused_without_registering ... ok
inline::tests::provider_id_is_absent_without_a_descriptor ... ok
inline::tests::registered_descriptor_drives_one_request_with_declared_headers ... ok
inline::tests::unknown_protocol_is_refused_without_registering ... ok
inline::tests::validation_error_display_never_renders_the_credential ... ok
```

The request test drives a real capture server: a descriptor with two headers
produces a request carrying both verbatim, and a `\r`/`\n` value is refused
before any socket is opened.

### Model profile

`declared_metadata_reaches_the_resolved_profile_and_execution_bounds` proves the
declared numbers reach `config.model_profile` and `config.compaction`, and that
`InlineModelProfile::resolve` prefers them. `absent_metadata_falls_back_to_the_existing_resolver_untouched`
proves a model without metadata leaves the config untouched and resolves exactly
as the existing resolver does.

### Error class

`ProviderErrorClass` is derived from the upstream status and transport error in
`ProviderError::class()`; the existing message and code are unchanged. The class
travels to the host wire in E002 (`turn/error.class`).

### Compatibility

Config-based registration, `TurnCredentialLocator` and `exec` are untouched: the
existing `codegg-providers` suite passes unchanged and `cargo test -p codegg
--bin codegg exec` is unchanged.

## Known limits

- A descriptor model with no `contextWindow` produces `ModelInfo.context_window
  = 0`, which is the catalog's existing "unknown" value; the loop's execution
  policy does not read it (it reads the resolved `ModelProfile`), so a descriptor
  model without metadata falls back exactly as the plan requires.
- Only `chat_completions` is accepted. `responses`/`messages` are deferred as the
  plan says.
