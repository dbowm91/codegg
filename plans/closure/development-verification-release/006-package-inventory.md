# M006 Package Inventory — crates.io Publication Evidence

Generated: 2026-07-30 (M006 implementation revision)
Source of truth: `cargo metadata --format-version 1 --frozen` at the
accepted M006 implementation head plus targeted manifest inspection.
Every row is verified against either `cargo metadata` or the manifest at
`crates/<pkg>/Cargo.toml` (root: `Cargo.toml`).

This inventory replaces the M005 inventory, which has been marked
historical / stale at
`plans/closure/development-verification-release/005-package-inventory.md`.

## Package table

`Publish` reflects `cargo publish` behavior. None of the workspace
crates set `publish = false`, so all ten are publishable by Cargo's
default. `Topological Layer` is derived from the direct normal/build
internal dependencies below it using the standard `1 + max(dep_layer)`
recursion over the resolved workspace graph. `Direct Internal Deps`
lists workspace members used as normal or build dependencies; dev-only
dependencies are listed separately.

| Package | Manifest | Version | Publish | Layer | Direct Internal Deps | Dev-only Internal Deps | Metadata |
|---|---|---|---|---|---|---|---|
| `codegg-config` | `crates/codegg-config/Cargo.toml` | 0.1.0 | yes (default) | 0 | — | — | description, license=MIT, repository, homepage, rust-version=1.81 |
| `codegg-protocol` | `crates/codegg-protocol/Cargo.toml` | 0.1.0 | yes (default) | 0 | — | — | description, license=MIT, repository, homepage, rust-version=1.81 |
| `codegg-git` | `crates/codegg-git/Cargo.toml` | 0.1.0 | yes (default) | 0 | — | — | description, license=MIT, repository, homepage, rust-version=1.81 |
| `eggcontext` | `crates/eggcontext/Cargo.toml` | 0.1.0 | yes (default) | 0 | — | — | description, license=MIT, repository, homepage, rust-version=1.81 |
| `egggit` | `crates/egggit/Cargo.toml` | 0.1.0 | yes (default) | 0 | — | — | description, license=MIT, repository, homepage, rust-version=1.81 |
| `eggsentry` | `crates/eggsentry/Cargo.toml` | 0.1.0 | yes (default) | 0 | — | — | description, license=MIT, repository, homepage, rust-version=1.81 |
| `egglsp` | `crates/egglsp/Cargo.toml` | 0.1.0 | yes (default) | 0 | — | — | description, license=MIT, repository, homepage, rust-version=1.81 |
| `codegg-providers` | `crates/codegg-providers/Cargo.toml` | 0.1.0 | yes (default) | 1 | `codegg-config` | — | description, license=MIT, repository, homepage, rust-version=1.81 |
| `codegg-core` | `crates/codegg-core/Cargo.toml` | 0.1.0 | yes (default) | 2 | `codegg-config`, `codegg-git`, `codegg-protocol`, `codegg-providers`, `egggit`, `egglsp`, `eggsentry` | — | description, license=MIT, repository, homepage, rust-version=1.81 |
| `codegg` | `Cargo.toml` | 0.1.0 | yes (default) | 3 | `codegg-config`, `codegg-core`, `codegg-git`, `codegg-protocol`, `codegg-providers`, `eggcontext`, `egggit`, `egglsp`, `eggsentry` | — | description, license=MIT, authors, repository, homepage, readme, keywords, categories, rust-version=1.81 |

## Publication order

Topological layers from the table above. Layers publish bottom-up; each
layer can be published in any order within itself.

1. **Layer 0** (leaves): `codegg-config`, `codegg-protocol`, `codegg-git`, `eggcontext`, `egggit`, `eggsentry`, `egglsp`
2. **Layer 1**: `codegg-providers` (depends on `codegg-config`)
3. **Layer 2**: `codegg-core` (depends on seven layer-0/1 packages)
4. **Layer 3**: `codegg` (depends on nine layer-0/1/2 packages)

## crates.io name availability

All ten crate names return `404 crate '<name>' does not exist` from the
crates.io HTTP API as of 2026-07-30. None are owned by any account. No
name conflicts were detected. The first successful publisher of each
name establishes ownership; the maintainer therefore retains full
release authority on initial publication.

The names that are individually reserved or commonly squatted on
crates.io were not detected for any of the ten. `cargo search <name>`
returns "no results" for every name in this set.

## Leaf package verification

Each leaf crate was exercised with the production publish dry-run path
at the accepted M006 implementation head. `cargo publish --dry-run`
exits 0 for all seven leaves. Exit codes are recorded verbatim.

| Package | `cargo package --list` files | `cargo publish --dry-run` exit | Packaged size |
|---|---|---|---|
| `codegg-config` | 10 | 0 | 178.6 KiB (37.1 KiB compressed) |
| `codegg-protocol` | 24 | 0 | 443.5 KiB (80.6 KiB compressed) |
| `codegg-git` | 15 | 0 | 255.1 KiB (40.1 KiB compressed) |
| `eggsentry` | 10 | 0 | 107.0 KiB (19.0 KiB compressed) |
| `eggcontext` | 5 | 0 | 14.5 KiB (4.4 KiB compressed) |
| `egggit` | 14 | 0 | 140.5 KiB (28.0 KiB compressed) |
| `egglsp` | 65 | 0 | 2.7 MiB (484.0 KiB compressed) |

`cargo package --list` for each leaf was inspected and contains only
`.cargo_vcs_info.json`, `Cargo.lock`, `Cargo.toml`, `Cargo.toml.orig`,
and the crate's own `src/` source files plus, where applicable, the
crate's test fixtures under `src/projection/fixtures.rs` (codegg-protocol).
No secrets, databases, log files, target output, planning archives,
oversized fixtures, or repository-only paths were found in the leaf
package contents.

## Dependent package verification

The dependent crates publish-dry-run with exit 101 (failure) because
their internal path-plus-version dependencies are not yet published.
The failure is the expected registry-sequencing error, not a local
packaging defect:

```text
error: failed to prepare local package for uploading

Caused by:
  no matching package named `codegg-config` found
  location searched: crates.io index
  required by package `codegg-providers v0.1.0`
```

`cargo package -p <pkg> --list` for each dependent crate succeeds and
returns a sane file list; the registry-resolution error is raised before
the normalized manifest is written, but the source manifest at
`crates/<pkg>/Cargo.toml` declares the dependency as
`{ version = "=0.1.0", path = "../<dep>" }`, which is the intended
publishable form (the `path` segment is stripped when publishing).

| Package | `cargo package --list` exit | Dry-run exit | Cause | Honest classification |
|---|---|---|---|---|
| `codegg-providers` | 0 | 101 | registry sequencing: `codegg-config` not yet on crates.io | blocked until dependency publication |
| `codegg-core` | 0 | 101 | registry sequencing: `codegg-config` not yet on crates.io | blocked until dependency publication |
| `codegg` | 0 | 101 | registry sequencing: `codegg-config` not yet on crates.io | blocked until dependency publication |

## Root crate packaging hygiene observation

`cargo package -p codegg --list` returns 1100 files. The root `Cargo.toml`
does not declare an `[package] exclude` field, so cargo defaults to
including every tracked path under the workspace root. The list mixes:

- 472 source files under `src/`
- 215 integration test files under `tests/` (test fixtures included)
- 263 development / planning files (`.opencode/skills/`, `.agents/skills/`, `scripts/`, `architecture/`, `plans/`, `examples/` SDK trees, etc.)
- The root `README.md`, `LICENSE`, `.cargo/config.toml`, and `.github/workflows/ci.yml`
- The `crates/egglsp-test-server/src/main.rs` binary referenced from `[[bin]]`

No `target/`, `.git/`, or `node_modules/` paths are present (cargo
excludes these by default). No secrets, databases, or log files were
detected.

This is a packaging-hygiene defect for the root crate only. M006 does
not own fixing it: a future corrective plan must add an explicit
`[package] exclude` (or equivalent workspace-level inclusion list) to
`Cargo.toml` before `codegg` is actually published. The leaf crates and
the dependent crates in layers 1–2 are unaffected.

## Verification evidence

Final M006 implementation revision: `<recorded by the closure record>`

Canonical local verification commands, exit codes, and per-package
observations are recorded by the closure record at
`plans/closure/development-verification-release/006-status.md`. The
inventory above is a static manifest-and-metadata view and is not a
substitute for those command transcripts.

The previous M005 inventory's contradictory claim that "full verification
passed while one included test failed" is explicitly **not** retained.
Either every included workspace test passed or the run is not claimed as
a pass.

## No actual publication performed

This milestone regenerates the package inventory from current manifests
and exercises the dry-run path. It does not publish any package to
crates.io. `cargo publish --dry-run` aborts at the network upload step;
no registry state was modified.
