# Dependency Security and Workspace Consolidation M003 — Optional Image Feature-Graph Slimming

Status: blocked

Repository baseline reviewed: `b3459640765261057e902a9df05bc71faa6cecc4`

Source subsystem roadmap:

- `plans/subsystems/dependency-security-workspace-consolidation-roadmap.md`

Hard predecessor:

- M002 accepted closure.

Primary class: polish + infrastructure.

## 1. Objective

Contract CodeGG's optional `image` dependency graph to the image formats the application intentionally supports, preserving behavior while removing default-format and parallel-decoder closure that is currently enabled implicitly.

This is an optional-feature optimization. It is not a justification to remove image support or change the default binary topology.

## 2. Current evidence

The root manifest currently declares:

- `ratatui-image` with `default-features = false` but the `image-defaults` feature enabled;
- `image = "0.25"` with explicit `png`, `jpeg`, `gif`, and `webp` features but without `default-features = false`.

As a result, the explicit format list does not bound the actual `image` feature closure. The optional graph can include formats and support crates CodeGG does not advertise.

## 3. Invariants

- `--features image` retains PNG, JPEG, GIF, and WebP rendering/decoding behavior used by CodeGG.
- TUI rendering/layout behavior remains unchanged.
- Default builds remain unaffected because image support is optional.
- No custom image decoder/encoder is introduced.
- Security limits and input validation around image handling remain unchanged.

## 4. Scope and non-goals

In scope:

- root `ratatui-image` and `image` feature declarations;
- image-format tests/fixtures needed to prove retained behavior;
- optional-image dependency-tree and artifact measurements;
- active docs describing supported formats/features.

Out of scope:

- adding/removing user-visible supported formats beyond PNG/JPEG/GIF/WebP;
- image UI redesign;
- changing image generation/provider APIs;
- replacing the `image` crate;
- SIMD/codec micro-optimization;
- default-binary size claims based on an optional feature;
- permanent size gates.

## 5. Ordered work packages

### WP1 — Baseline the optional graph

Record:

```bash
cargo tree --features image --locked
cargo tree -e features --features image --locked
cargo build --release --features image --locked
cargo bloat --release --features image --bin codegg --crates --locked -n 40
```

Identify which image formats/support crates are present solely through defaults.

### WP2 — Make format ownership explicit

Change the root dependencies so:

- `image` has `default-features = false` and explicitly enables only `png`, `jpeg`, `gif`, `webp` plus any proven runtime feature CodeGG requires;
- `ratatui-image` does not reactivate `image` defaults through `image-defaults`; retain only the terminal backend/features actually required after M001's Ratatui migration.

Do not guess feature names if upstream changed; inspect the published versions resolved after M001/M002.

### WP3 — Format behavior tests

Ensure focused tests exercise representative decoding/render preparation for PNG/JPEG/GIF/WebP. Existing tests may be reused; add only the smallest fixtures needed to prove a format was not accidentally removed.

A malformed/unsupported-format test should continue to fail safely through the existing error path.

### WP4 — Measure and document

Re-run the feature tree and optional release artifact/bloat observation on the same host/toolchain. Record removed dependency families and byte deltas as descriptive evidence.

Update active docs if they imply broader format support than the tested set.

## 6. Verification

Required:

```bash
cargo check --workspace --all-targets --features image --locked
cargo test --features image --lib -- --test-threads=1
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
scripts/verify.sh quick
```

Also run focused image/TUI tests discovered by source census.

## 7. Acceptance criteria

- `image` defaults are disabled in CodeGG-owned declarations.
- `ratatui-image` no longer re-enables broad image defaults unless a retained feature is demonstrated as necessary.
- PNG/JPEG/GIF/WebP behavior is covered and green.
- The optional dependency graph is equivalent or smaller and all removed families are attributable to unused default formats/features.
- Default feature behavior and default binary remain unchanged.
- No new medium-or-higher finding is introduced.

## 8. Stop conditions

Stop if narrowing defaults removes a CodeGG-supported format or terminal path not represented by current docs/tests; first classify that real contract and update this plan rather than silently dropping it.

## 9. Required closure evidence

Record before/after image feature trees, focused format tests, optional artifact/bloat observations, broad verification, documentation updates, and any retained default feature with its exact justification.
