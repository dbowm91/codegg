# Agent Runtime, Model Adaptation, and ACP Milestone 015 — Closure

Status: closed

Implementation plan: `plans/implementation/agent-runtime-model-adaptation-acp/015-adapter-driven-reasoning-safety.md`

## Outcome

M015 is strictly closed. Provider-private reasoning is byte bounded without
invalid UTF-8 slicing, and the OpenAI-compatible wire path now uses an explicit
adapter request policy with exclusion-aware matching. Generic and excluded
models do not receive private reasoning or thinking parameters. The canonical
tool/wire alias boundary is preserved for request history, schemas, and
inbound tool calls.

## Evidence

- UTF-8 boundary coverage is in `tests/event_processor.rs`: exact byte budget,
  multibyte boundary, fragmented Unicode, valid UTF-8, and bounded output.
- `RequestTransform` is a closed serde enum in
  `crates/codegg-core/src/model_profile/adapter.rs`. Build-time generation in
  `crates/codegg-core/build.rs` rejects unknown operations, duplicate operation
  targets, unsafe nested/authority fields, and unsupported reasoning/thinking
  fields. The Laguna adapter resolves to typed reasoning and thinking
  transforms; generic resolution has no transforms.
- OpenAI-compatible transcripts cover two Laguna reasoning rounds, aliases,
  generic privacy omission, and non-Laguna behavior in
  `tests/provider_transcripts.rs`. Inbound wire aliases are normalized before
  the event reaches canonical tool permission/execution handling.
- Serving requirement diagnostics remain adapter-scoped, bounded metadata
  only, and do not include credentials or reasoning bodies.
- `ContentPart::Reasoning` continues to skip serde text and redact debug
  content; no public ACP/projection serialization path was changed to expose
  it.

## Verification

Passed:

```text
cargo fmt --all
cargo check -p codegg-providers --all-targets
cargo check -p codegg-core --all-targets
cargo check -p codegg --all-targets
cargo test -p codegg-core model_profile::adapter
cargo test -p codegg-providers openai_compatible
cargo test --test event_processor -- --test-threads=4
cargo test --test provider_transcripts -- --test-threads=4
```

The checks completed with warnings only; no M015 test failure occurred.

## Limitations

The provider crate cannot depend on `codegg-core` because the existing
workspace dependency graph has the reverse edge. Its OpenAI-compatible wire
projection therefore keeps a small, static projection of the built-in Laguna
adapter contract at the provider boundary. It is non-executable, explicit,
exclusion-aware, and does not use substring activation. A future shared
adapter-policy crate would remove this duplication, but it is not required for
M015 correctness or closure.

## Downstream status

M016 is unblocked and registered as `ready` because its only named hard
dependency is M015 strict closure. M017 remains `blocked` because it requires
strict closure records for M012 through M016. No unrelated registered plan was
unblocked by this milestone.

Recommendation: promote M016; do not promote M017 until M016 has its own strict
closure record.
