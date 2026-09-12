# Tool-Surface Upstream Compatibility M009 — Eggsact 1.2.5 MSRV Adoption

Status: implemented

Repository baseline reviewed: `b9a020cf`

Source corrective addendum:

- `plans/subsystems/tool-surface-upstream-compatibility-corrective-addendum.md`

Predecessor implementation and closure:

- `plans/implementation/tool-surface-upstream-compatibility/008-eggsact-1.2.5-inprocess-compatibility.md`
- `plans/closure/tool-surface-upstream-compatibility/008-status.md`

## 1. Objective

Complete the one deferred M008 requirement now that the project has explicitly
approved Rust `1.89` as its MSRV: adopt eggsact `1.2.5`, align the workspace
manifest declarations and compatibility documentation, and produce strict
closure evidence without reopening completed profile or MCP scope.

## 2. Unclosed predecessor finding

M008 landed the in-process profile parser, fail-closed invalid-profile behavior,
audience tests, and curated-tool-surface disposition. Its only medium finding
was that eggsact `1.2.5` declares Rust `1.89.0`, above CodeGG's then-approved
Rust `1.81` MSRV. The user has now explicitly resolved that decision. The
predecessor verification could not catch the dependency upgrade because
changing it would have violated the accepted MSRV at that time.

## 3. Scope and non-goals

In scope:

- raise the root and workspace package `rust-version` declarations to `1.89`;
- make eggsact `1.2.5` the manifest and lockfile baseline;
- update active user, contributor, release, dependency, architecture, and
  planning documentation;
- rerun M008's focused tests and repository verification;
- assess whether any downstream plan becomes dependency-ready.

Out of scope:

- importing eggsact's MCP discovery facade;
- expanding CodeGG's model-visible deterministic palette;
- adopting typed `DependencyPreflight` without an existing consumer;
- broad dependency modernization unrelated to the approved MSRV change;
- rewriting the historical M008 closure record.

## 4. Invariants

- Eggsact remains an in-process dependency through `ToolRegistry`.
- CodeGG owns model-facing palette and progressive disclosure policy.
- Invalid profiles fail visibly and never fall back to `Profile::Default`.
- Model and harness audience boundaries remain unchanged.
- No new CI lane, network-dependent test, or dependency-update automation is added.
- Historical closure records remain intact; this pass owns new evidence.

## 5. Ordered work packages

### WP1 — Adopt the approved dependency/toolchain baseline

Update all workspace package MSRV declarations and the direct eggsact
requirement, then resolve the lockfile to `1.2.5`. Confirm the resulting tree
has no stale eggsact `1.1.4` resolution.

### WP2 — Refresh active documentation and planning control surfaces

Replace stale Rust `1.81`/eggsact `1.1.4` claims in active product,
contributor, release, dependency, architecture, roadmap, and registry docs.
Keep historical M008 evidence unchanged and link the new corrective plan.

### WP3 — Re-verify the existing compatibility boundary

Run profile, audience, deterministic-tool, and preflight tests against the
resolved eggsact `1.2.5`. Run formatting, strict Clippy, and the canonical
quick verification. Record host-specific limitations truthfully if any remain.

### WP4 — Close and audit dependencies

Create the M009 closure record, move the plan through implemented/closed state,
and audit the registry's blocked section and affected dependency graphs. Only
reclassify a future plan if this change satisfies all of its blockers.

## 6. Required verification

```bash
cargo fmt --all -- --check
cargo test --lib eggsact -- --test-threads=1
cargo test --test eggsact_adapter -- --test-threads=1
cargo test --test eggsact_deterministic_tools -- --test-threads=1
cargo test --test preflight_integration -- --test-threads=1
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

## 7. Acceptance criteria

- Root and workspace package manifests declare Rust `1.89`.
- `Cargo.toml` and `Cargo.lock` resolve eggsact `1.2.5`.
- No active documentation claims the old MSRV or deferred eggsact baseline.
- Existing M008 profile, exposure, and preflight behavior passes against 1.2.5.
- No downstream plan is marked ready unless its complete dependency graph is now satisfied.
- M009 has a complete closure record with verification evidence and no
  unresolved medium-or-higher finding.

## 8. Stop conditions

Stop and record a blocker if eggsact `1.2.5` does not compile against the
approved toolchain, changes the established exposure contract, or requires a
broader architectural migration.
