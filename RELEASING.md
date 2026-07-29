# Releasing CodeGG

This document describes the manual release procedure. There is no automated
release pipeline. All steps are maintainer-operated and sequential.

## Scope and ownership

- crates.io publication is manual; version and cadence are maintainer decisions.
- GitHub Actions does not publish, create tags, create releases, or hold
  crates.io credentials.
- GitHub tags and binary releases are optional separate actions performed
  manually after crates.io publication.

## Package graph

The workspace publishes 10 crates to crates.io in topological order:

| Order | Package | Description | Internal dependencies |
|-------|---------|-------------|----------------------|
| 1 | `codegg-config` | Configuration schema and loading | — |
| 2 | `codegg-protocol` | Core protocol types | — |
| 3 | `codegg-git` | Typed Git operation model | — |
| 4 | `eggsentry` | Security scanning primitives | — |
| 5 | `eggcontext` | Token counting utilities | — |
| 6 | `egggit` | Read-only git facts | — |
| 7 | `egglsp` | Language Server Protocol client | — |
| 8 | `codegg-providers` | LLM provider implementations | `codegg-config` |
| 9 | `codegg-core` | Core runtime and domain types | `codegg-config`, `codegg-git`, `codegg-protocol`, `codegg-providers`, `egggit`, `egglsp`, `eggsentry` |
| 10 | `codegg` | Binary application | all of the above |

All packages are tightly coupled at version `0.1.0` with exact version
requirements (`=0.1.0`). A version bump in any package requires a
coordinated bump across all packages that depend on it.

## Prerequisites

- Rust 1.81+ stable toolchain
- Clean git working tree on `main`
- crates.io account with owner access to all 10 package names
- `cargo login` completed (token stored in `~/.cargo/credentials.toml`)

## Release procedure

### 1. Clean-tree and account preflight

```bash
git switch main
git pull --ff-only
git status --short
cargo login --help   # documentation only; do not expose token
cargo owner --list codegg
cargo owner --list codegg-core
cargo owner --list codegg-config
cargo owner --list codegg-protocol
cargo owner --list codegg-providers
cargo owner --list codegg-git
cargo owner --list eggsentry
cargo owner --list eggcontext
cargo owner --list egggit
cargo owner --list egglsp
```

Confirm the working tree is clean and you have owner access to all packages.

### 2. Version and dependency preparation

If this is not the initial release, update versions in all workspace crates
and update the path-plus-version dependency requirements. All tightly
coupled packages must change versions together.

Ensure the working tree remains clean after edits:

```bash
git status --short
```

### 3. Verify

```bash
scripts/verify.sh full
```

This must pass before proceeding. Do not duplicate a divergent command list.

### 4. Package inspection

Inspect each package in topological order:

```bash
cargo package -p codegg-config --list
cargo package -p codegg-protocol --list
cargo package -p codegg-git --list
cargo package -p eggsentry --list
cargo package -p eggcontext --list
cargo package -p egggit --list
cargo package -p egglsp --list
cargo package -p codegg-providers --list
cargo package -p codegg-core --list
cargo package -p codegg --list
```

Inspect for:
- missing source or migration files
- missing README/license
- generated files required for compilation
- accidental large fixtures, target artifacts, local databases, logs, secrets,
  or planning evidence
- path dependencies that normalize without a registry version

### 5. Dry-run

Run dry-runs in topological order. Leaf crates must pass before dependents:

```bash
# Leaf crates (no internal deps) — these must pass
cargo publish --dry-run -p codegg-config
cargo publish --dry-run -p codegg-protocol
cargo publish --dry-run -p codegg-git
cargo publish --dry-run -p eggsentry
cargo publish --dry-run -p eggcontext
cargo publish --dry-run -p egggit
cargo publish --dry-run -p egglsp

# Dependent crates — blocked until leaf crates are published
cargo publish --dry-run -p codegg-providers
cargo publish --dry-run -p codegg-core
cargo publish --dry-run -p codegg
```

Interpretation:
- **Leaf crate dry-run passes**: publication is ready.
- **Dependent crate dry-run fails** with "no matching package found" for an
  internal dependency: expected — the dependency has not been published yet.
  Publish the dependency first, then re-run.

### 6. Irreversible publication

**Do not execute these commands until you are ready to publish.**

Publication must follow the exact topological order. After each leaf
publication, verify registry availability before publishing dependents:

```bash
# Step 1: Publish leaf crates
cargo publish -p codegg-config
cargo publish -p codegg-protocol
cargo publish -p codegg-git
cargo publish -p eggsentry
cargo publish -p eggcontext
cargo publish -p egggit
cargo publish -p egglsp

# Step 2: Verify leaf crates are available (check index propagation)
cargo search codegg-config --limit 1
cargo search codegg-providers --limit 1

# Step 3: Publish mid-level crates
cargo publish -p codegg-providers

# Step 4: Verify mid-level crates are available
cargo search codegg-core --limit 1

# Step 5: Publish core
cargo publish -p codegg-core

# Step 6: Verify core is available
cargo search codegg --limit 1

# Step 7: Publish root
cargo publish -p codegg
```

Index propagation may take a few seconds. Verify availability with
`cargo search` before publishing dependents.

### 7. Partial failure and immutability

- **Successful versions cannot be overwritten.** An immutable published
  version is never replaced or retried as mutable state.
- **Fix and bump**: if a published version is defective, prepare a new
  version (patch bump) and publish that.
- **Yanking is not deletion**: `cargo yank --version` removes the version
  from the default install resolution but the tarball remains.
- **Do not blindly rerun the same version**: it will fail with "version
  already exists".
- **Record which packages were successfully published** before continuing.
  If the process is interrupted, resume from the next unpublished package.

### 8. Tags and optional GitHub binary release

After crates.io publication, optionally create a Git tag and binary release.
These are manual and separate — they do not trigger automation:

```bash
git tag -a v<VERSION> -m "Release v<VERSION>"
git push origin v<VERSION>
```

To create a GitHub Release with pre-built binaries:

```bash
# Build release binaries (see build targets below)
# ...

gh release create v<VERSION> \
  --title "Release v<VERSION>" \
  --generate-notes \
  release/codegg-* \
  release/checksums.txt
```

Build targets:

```bash
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin
cargo build --release --target x86_64-unknown-linux-gnu
cargo build --release --target aarch64-unknown-linux-gnu
cargo build --release --target x86_64-pc-windows-msvc
```

### 9. Installation verification

After an actual crates.io release, verify end-user installation:

```bash
cargo install codegg --version <VERSION>
```

Source/development installation remains:

```bash
cargo install --path .
```

## Concurrent releases

Only one maintainer should execute the release sequence at a time. Confirm
no parallel release is underway before starting step 6.
