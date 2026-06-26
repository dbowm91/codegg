# LSP Phase 13 Plan: Real-World Validation and `/lsp-doctor`

Status date: 2026-06-26
Phase type: validation / observability / user diagnostics
Prerequisites: Phases 6-12 closed.

## Purpose

Phase 13 should validate the mature LSP subsystem against real server profiles and expose a user-facing diagnostic path for common failure modes. The goal is not to add more semantic operations. The goal is to prove the current operations work across realistic workspaces, server binaries, root layouts, capabilities, degraded states, and platform differences.

The primary product deliverable is `/lsp-doctor [path]`: a read-only diagnostic command that explains why LSP is or is not working for a path, what server/profile/root is selected, what capabilities are available, and what remediation is recommended.

## Current baseline

The repo already has:

- lifecycle/status commands,
- root diagnosis helpers,
- capability summaries,
- health states,
- stderr tails,
- restart/stop controls,
- bounded semantic workflows,
- optional semantic cache,
- preview apply hardening.

Phase 13 should stitch these into validation and diagnosis workflows.

## Non-goals

Do not add new LSP semantic operations.

Do not add disk cache.

Do not execute LSP `workspace/applyEdit` or `workspace/executeCommand`.

Do not make `/lsp-doctor` start servers unless explicitly requested by a future extension.

Do not require every developer machine to have every LSP server installed for normal tests.

Do not make CI depend on heavy global tool installation unless the workflow pins and installs those tools deliberately.

## Workstream 1: define validation tiers

### Problem

The repo needs a clear distinction between unit/integration tests that always run and real-server smoke tests that depend on installed binaries.

### Target files

- `architecture/lsp.md`
- `.opencode/skills/lsp/SKILL.md`
- `AGENTS.md`
- `crates/egglsp/tests/`
- root `tests/`
- CI workflow files if present

### Validation tiers

Define these tiers:

- `unit`: no server process; pure DTO/render/cache/policy tests.
- `fake-server`: controlled fake LSP server with deterministic capabilities/responses.
- `real-server-smoke`: optional or CI-pinned tests against real LSP binaries.
- `manual-doctor`: user-facing diagnosis through `/lsp-doctor`.

### Implementation steps

1. Add docs explaining the tiers.
2. Ensure real-server tests are feature-gated or environment-gated.
3. Add helper names/commands for running each tier.
4. Mark which server profiles are pinned/CI-verified vs best-effort.

### Acceptance criteria

- Contributors know which tests are mandatory and which require installed servers.
- Real-server tests do not make normal local test runs brittle.

## Workstream 2: real-server smoke matrix

### Target profiles

Start with these profiles if already supported by repo server definitions:

- Rust: `rust-analyzer`.
- Python: `pyright` or `pylsp`, whichever is already easiest in repo config.
- TypeScript/JavaScript: `typescript-language-server`.
- Go: `gopls`.
- Fallback: unsupported/no-server profile.

### Test fixtures

Create small fixture workspaces:

- Rust crate with one library, one test, one trait/interface-like symbol, and one diagnostic.
- Python package with one module, one test-like file, one import, one diagnostic.
- TypeScript project with one module, one import/export boundary, one diagnostic.
- Go module with one package, one interface/function, one diagnostic.
- No-root directory and nested-root directory for root diagnosis.

Keep fixtures tiny. They should test LSP integration, not language semantics.

### Operations to smoke test

For each available real server, test the operations it supports:

- server start and initialization,
- root detection,
- capability snapshot,
- diagnostics,
- definition,
- references,
- document symbols,
- hover if available,
- rename preview where safe,
- formatting preview where safe,
- status/degraded state where mockable.

### Implementation steps

1. Add fixture directories under a dedicated test fixture path.
2. Add feature-gated real-server smoke tests.
3. Add helper functions for server binary discovery and skip reasons.
4. Ensure skipped tests say exactly which binary/config is missing.
5. Keep smoke tests bounded in runtime and avoid long indexing projects.

### Acceptance criteria

- At least `rust-analyzer` has a pinned smoke path if feasible.
- Other servers are best-effort or CI-pinned with explicit skip reasons.
- Smoke tests cover root, capability, diagnostics, and one navigation operation.

## Workstream 3: `/lsp-doctor [path]` command

### Purpose

Give users one command to diagnose common LSP failure modes without reading logs or knowing the internals.

### Target files

- `src/tui/command.rs`
- `src/tui/app/mod.rs`
- `src/tool/lsp.rs`
- `crates/egglsp/src/root.rs`
- `crates/egglsp/src/tui_summary.rs`
- `architecture/lsp.md`
- command dispatch tests

### Command behavior

`/lsp-doctor [path]` should be read-only and should not start a server by default.

Output should include:

- input path,
- normalized/canonical path if available,
- allowed-root status,
- selected workspace root,
- root markers found,
- inferred language/profile,
- server executable/config discovery result,
- active server key if already running,
- operational state if running,
- generation if running,
- capability summary if initialized,
- recent error/stderr tail if available,
- cache status if relevant,
- preview count/stale preview count if relevant,
- remediation suggestions.

### Failure modes to diagnose

- file outside allowed root,
- missing file,
- unsupported extension/language,
- no root marker,
- nested root ambiguity,
- server binary missing,
- server running but degraded/failed,
- capabilities unavailable before initialization,
- stale evidence after restart,
- cache disabled/enabled status,
- no active clients.

### Implementation steps

1. Add a pure `LspDoctorReport` DTO if needed.
2. Compose existing root diagnosis, server summary, capability summary, and error renderers.
3. Add remediation hints as deterministic strings.
4. Wire command parser and TUI handler.
5. Add dispatch tests for missing path/default path, explicit path, no tool, and outside-root path.
6. Add renderer tests for common failure modes.

### Acceptance criteria

- `/lsp-doctor` is read-only.
- It does not start servers unexpectedly.
- It explains common no-LSP cases with actionable remediation.
- It has handler and renderer tests.

## Workstream 4: observability metrics

### Purpose

Expose enough metrics to debug real-world LSP behavior without logging every protocol event.

### Metrics to add or expose

- server startup duration,
- initialization duration,
- request latency by operation,
- restart count,
- degraded/failed transitions,
- open document count,
- pending request count,
- stderr tail count,
- preview registry count/stale count,
- semantic cache entries/hits/misses/stale misses/evictions.

### Target files

- `crates/egglsp/src/health.rs`
- `crates/egglsp/src/service.rs`
- `crates/egglsp/src/tui_summary.rs`
- `src/tool/lsp.rs`
- metrics/tracing utilities if present

### Implementation steps

1. Inventory existing metrics and traces.
2. Add a bounded `LspObservabilitySnapshot` DTO if useful.
3. Render a concise summary in `/lsp-doctor` or `/lsp-status --detail`.
4. Keep detailed logs under debug/trace only.
5. Add tests for snapshot rendering.

### Acceptance criteria

- Users can see high-level health and latency state.
- Logs do not become noisy by default.

## Workstream 5: docs and support matrix

### Target files

- `architecture/lsp.md`
- `.opencode/skills/lsp/SKILL.md`
- `README.md` if command list is public
- `AGENTS.md`

### Documentation requirements

Document:

- validation tiers,
- real-server smoke-test commands,
- server support matrix,
- `/lsp-doctor` usage,
- profile support status: pinned/CI-verified, best-effort, unsupported,
- skip behavior for missing binaries,
- known platform caveats.

## Test matrix

Focused:

```bash
cargo fmt --check
cargo test -p egglsp tui_summary
cargo test -p egglsp root
cargo test -p egglsp health
cargo test --test phase5_context_integration lsp
```

Optional real-server smoke:

```bash
cargo test -p egglsp --features lsp-real-server-tests --test real_server_smoke
```

Broader if feasible:

```bash
cargo test -p egglsp
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## Final acceptance criteria

Phase 13 is complete when:

- validation tiers are documented,
- fixture workspaces exist for at least Rust and one other language or are explicitly staged,
- real-server tests are feature/env gated and skip cleanly when binaries are missing,
- `/lsp-doctor [path]` exists, is read-only, and diagnoses common failure modes,
- status/capability/root/error/cache information is visible in one diagnostic surface,
- docs explain support tiers and test commands.
