# Agent Runtime, Model Adaptation, and ACP Milestone 007 — Closure Status

Status: closed

Source plan: `plans/implementation/agent-runtime-model-adaptation-acp/007-declarative-model-adapter-registry.md`

Roadmap: `plans/subsystems/agent-runtime-model-adaptation-acp-roadmap.md#milestone-007--declarative-model-adapter-registry-and-build-generation`

## Outcome

Milestone 007 is implemented and strictly closed. The former plan header said
blocked, but M001 and M002 were already accepted in the registry; that stale
handoff state was corrected as part of this closure.

## Evidence

- Built-in adapter TOMLs live under `crates/codegg-core/assets/model-adapters/`.
- `crates/codegg-core/build.rs` enumerates assets in sorted order, emits
  `cargo:rerun-if-changed`, rejects unknown keys, validates regexes/bounds,
  rejects duplicate match keys, validates typed transform names, and rejects
  non-reversible tool/argument aliases.
- Generated Rust embeds the TOML definitions in `$OUT_DIR`; runtime parsing
  uses those embedded strings and needs no Python or source-tree access.
- `ResolvedModelAdapter` exposes adapter id/version, source provenance,
  fingerprint, profile, tool/argument aliases, prompt fragments, recovery and
  serving requirements, and typed transforms.
- Profile resolution no longer contains model-name branch selection; the
  compatibility resolver delegates to the declarative registry. Prompt
  selection and the canonical/wire tool surface consume the resolved adapter.
- Unknown models select `generic`; MiniMax, OpenAI, Anthropic, Google, and local
  families have built-in conservative profiles. Legacy `model_profile` config
  remains a bounded profile overlay.
- Focused result: `cargo test -p codegg-core model_profile::` — 20 passed.
- Compile result: `cargo check -p codegg-core` and `cargo check -p codegg` — 0
  errors.
- Formatting result: `cargo fmt --all` completed successfully.
- Package inventory evidence: `cargo package -p codegg-core --allow-dirty
  --no-verify --list` includes all six adapter TOMLs, `build.rs`, and the
  adapter module. Full package preparation is blocked by the pre-existing
  workspace dependency `codegg-config` not being published on crates.io.

## Known boundaries

Provider transports remain authoritative for authentication and wire protocol.
The adapter transform enum is validated and carried as typed data; provider
implementations decide which approved transforms they support. Reasoning
preservation is intentionally deferred to M008. Existing compatibility helper
constructors remain private and are no longer used for model selection.

## Dependency audit

M008 is now dependency-ready and was promoted from `blocked` to `ready`.
M009 remains blocked because its final context/reasoning convergence requires
M008. M010 remains blocked on M009, and M011 remains blocked on M004–M010.
No other registered plan became ready from this closure.

## Closure recommendation

Accept strict closure for M007 and hand off M008 as the next agent-runtime
milestone.
