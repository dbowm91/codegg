# Releasing CodeGG

This document describes the manual release procedure. There is no automated
release pipeline. All steps are maintainer-operated and sequential.

## Package scope

All workspace crates are internal (`publish = false`). CodeGG is distributed
as a compiled binary, not as a crates.io library package. Installation from
source uses `cargo install --path .`.

| Package | Publish | Purpose |
|---------|---------|---------|
| `codegg` (root) | false | Binary application |
| `codegg-core` | false | Internal domain types |
| `codegg-config` | false | Internal configuration |
| `codegg-protocol` | false | Internal protocol types |
| `codegg-providers` | false | Internal LLM providers |
| `codegg-git` | false | Internal git model |
| `codegg-protocol` | false | Internal protocol types |
| `eggsentry` | false | Internal security scanning |
| `eggcontext` | false | Internal token counting |
| `egggit` | false | Internal git facts |
| `egglsp` | false | Internal LSP client |

No crates.io publication is required or supported for this repository.

## Prerequisites

- Rust 1.81+ stable toolchain
- Clean git working tree on `main`
- Full local verification passes

## Release procedure

### 1. Prepare

```bash
git switch main
git pull --ff-only
git status --short
```

Confirm the working tree is clean. If not, commit or stash changes first.

### 2. Verify

```bash
scripts/verify.sh full
```

This must pass before proceeding. The full verification runs formatting,
static checks, compilation, clippy, and workspace tests.

### 3. Build release binaries

Build for each target platform. The release profile enables LTO, stripping,
and single codegen unit for optimal binary size:

```bash
# macOS Apple Silicon (primary)
cargo build --release --target aarch64-apple-darwin

# macOS Intel
cargo build --release --target x86_64-apple-darwin

# Linux x86-64
cargo build --release --target x86_64-unknown-linux-gnu

# Linux ARM64 (requires cross)
cargo install cross
cross build --release --target aarch64-unknown-linux-gnu

# Windows x86-64
cargo build --release --target x86_64-pc-windows-msvc
```

### 4. Create checksums

```bash
mkdir -p release
cp target/aarch64-apple-darwin/release/codegg release/codegg-macos-aarch64
cp target/x86_64-apple-darwin/release/codegg release/codegg-macos-x86_64
cp target/x86_64-unknown-linux-gnu/release/codegg release/codegg-linux-x86_64
cp target/aarch64-unknown-linux-gnu/release/codegg release/codegg-linux-aarch64
cp target/x86_64-pc-windows-msvc/release/codegg.exe release/codegg-windows-x86_64.exe

cd release
sha256sum codegg-* > checksums.txt
cat checksums.txt
cd ..
```

### 5. Tag (optional)

Tags are manual and not prerequisites for anything automated:

```bash
git tag -a v<VERSION> -m "Release v<VERSION>"
git push origin v<VERSION>
```

### 6. Create GitHub Release (optional)

Create the release manually using the GitHub CLI or web UI:

```bash
gh release create v<VERSION> \
  --title "Release v<VERSION>" \
  --generate-notes \
  release/codegg-* \
  release/checksums.txt
```

Or create it via the GitHub web UI at
`https://github.com/dbowm91/codegg/releases/new`.

### 7. Verify the release

After publishing, confirm:

- The GitHub Release page shows the correct assets and checksums
- `cargo install --path .` works from a clean checkout
- The binary runs correctly on at least one target platform

## Partial failure handling

- **Nothing published**: fix locally and rerun from step 2.
- **Some binaries built, one failed**: fix the failing target, rebuild only
  that target, regenerate checksums.
- **Published version is defective**: do not replace. Optionally yank the
  GitHub Release. Prepare and publish a new version.

## Binary installation (end users)

```bash
cargo install --path .
```

Or download pre-built binaries from the GitHub Releases page.

## crates.io

CodeGG does not publish to crates.io. All workspace crates are internal
implementation details with `publish = false`.

## Concurrent releases

Only one maintainer should execute the release sequence at a time. Confirm
no parallel release is underway before starting step 3.
