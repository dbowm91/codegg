# Declarative model adapters

Model behavior that varies by model family is described by the versioned TOML
files in `crates/codegg-core/assets/model-adapters/`. `codegg-core/build.rs`
parses the files with `deny_unknown_fields`, validates bounds, regular
expressions, transforms, and reversible aliases, sorts them, and embeds the
source text into deterministic Rust in `$OUT_DIR`. Runtime operation therefore
does not depend on Python or on the source checkout.

`codegg_core::model_profile::resolve_adapter` is pure and returns an immutable
`ResolvedModelAdapter`. Matching uses exact model, provider, prefix/suffix, and
regex specificity, with explicit priority as the tie-breaker. The adapter
contains the existing compatibility `ResolvedModelProfile`, canonical-to-wire
tool aliases, argument aliases, prompt fragments, bounded request transforms,
recovery hints, serving diagnostics, provenance, version, and a SHA-256
fingerprint. Unknown models select the conservative `generic` adapter.

Adapter data is policy only: it cannot execute code, grant permissions, or
replace provider authentication/transport. The existing tool-surface resolver
continues to enforce canonical permissions and reversibility. Turns resolve
their adapter before prompt and tool-surface construction; later configuration
refreshes affect later turns only.

To add an adapter, add one TOML file, keep its `schema_version = 1`, provide a
unique `[adapter]` id/version and at least one `[[match]]`, then run:

```bash
cargo test -p codegg-core model_profile::
cargo package -p codegg-core --allow-dirty --no-verify
```

Reasoning preservation and provider-specific reasoning round trips remain the
responsibility of Milestone 008.
