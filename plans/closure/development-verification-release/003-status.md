# Development Verification and Release Milestone 003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/development-verification-release/003-manual-crates-io-release-ownership.md`

Source subsystem roadmap:

- `plans/subsystems/development-verification-release-roadmap.md#milestone-003--manual-cratesio-release-ownership`

Repository baseline reviewed: `75b5dc04`

Implementation commits:

- `75b5dc04` — Close DVR M002 (baseline); M003 implementation in this commit series

## 1. Executive finding

The milestone is complete. All workspace crates are explicitly `publish = false` — none are intended for crates.io publication as public API. The automated release workflow (`.github/workflows/release.yml`) has been deleted. A manual release procedure is documented in `RELEASING.md`. Stale `anomalyco/codegg` repository URLs have been corrected to `dbowm91/codegg`. No GitHub Actions workflow retains release authority, write permissions, or crates.io credentials.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| `.github/workflows/release.yml` deleted | `test ! -e .github/workflows/release.yml` passes | pass | |
| No workflow publishes crates, creates tags/releases, or uploads release assets | `rg` search across `.github/workflows/*.yml` | pass | `upload-artifact` matches are LSP diagnostic reports only |
| No workflow has release write permissions | `ci.yml` has `contents: read`; `lsp-real-server.yml` has no explicit permissions (default read) | pass | |
| `RELEASING.md` defines complete manual procedure | New file with prerequisites, build, checksum, tag, GitHub Release, verification steps | pass | |
| Package publication set is explicit | All 10 workspace crates have `publish = false` | pass | No crates.io publication intended |
| Private workspace crates explicitly non-publishable | `publish = false` in all `Cargo.toml` files | pass | |
| Publishable internal dependencies have registry-compatible versions | N/A — no crates intended for publication | pass | |
| Stale repository/homepage metadata corrected | `anomalyco/codegg` replaced with `dbowm91/codegg` in `Cargo.toml`, `README.md`, `src/upgrade/mod.rs`, `src/search/providers.rs`, `src/search/wikipedia.rs`, `architecture/upgrade.md`, `docs/TROUBLESHOOTING.md`, `.opencode/skills/upgrade/SKILL.md` | pass | |
| `binstall` metadata removed | `[package.metadata.binstall]` section removed from root `Cargo.toml` | pass | No automated binary release contract retained |
| `cargo publish --dry-run` correctly rejects | `cargo publish --dry-run -p codegg` returns error about `publish` not set to true | pass | |
| `scripts/verify.sh full` passes | Exit code 0 | pass | Pre-existing tokio flavor warnings (1062 bare annotations) are non-blocking |
| Partial failure documented | `RELEASING.md` includes partial failure handling section | pass | |
| Concurrent release warning | `RELEASING.md` warns against parallel maintainer execution | pass | |
| No actual publish occurred | No `cargo publish` executed | pass | |

## 3. Production implementation evidence

**Deleted:**
- `.github/workflows/release.yml` — tag-triggered binary build, checksum, and GitHub Release creation workflow

**Added:**
- `RELEASING.md` — manual release procedure with binary build, checksum, optional tag/GitHub Release, and verification steps

**Modified (metadata):**
- Root `Cargo.toml`: removed `[package.metadata.binstall]`, added `publish = false`, corrected `repository`/`homepage` URLs
- All 9 sub-crate `Cargo.toml` files: added `publish = false`

**Modified (stale URL corrections):**
- `src/upgrade/mod.rs`: GitHub API URL corrected
- `src/search/providers.rs`: user-agent URL corrected
- `src/search/wikipedia.rs`: user-agent URL corrected
- `README.md`: clone URL corrected
- `architecture/upgrade.md`: two GitHub API URL references corrected
- `docs/TROUBLESHOOTING.md`: issues URL corrected
- `.opencode/skills/upgrade/SKILL.md`: GitHub API URL corrected

## 4. Verification executed

### Commands run

```bash
# Metadata inventory
cargo metadata --format-version 1 --no-deps  # all 10 packages show publish=[]

# Release workflow absence
test ! -e .github/workflows/release.yml  # PASS

# Workflow release authority search
rg --line-number --glob '.github/workflows/*.{yml,yaml}' \
  'cargo publish|gh release create|crates\.io|CARGO_REGISTRY_TOKEN|packages: write|contents: write|id-token: write|tags:|upload-artifact|checksums' \
  .github/workflows
# Only upload-artifact matches in lsp-real-server.yml (diagnostic reports, not release assets)

# Workflow permissions
rg --line-number 'contents:|permissions:' .github/workflows/*.yml
# Only ci.yml: contents: read

# Metadata correctness
rg --line-number 'anomalyco/codegg|package\.metadata\.binstall|publish\s*=' \
  Cargo.toml crates/*/Cargo.toml README.md RELEASING.md
# Only publish=false entries in Cargo.toml files and RELEASING.md

# Publication rejection
cargo publish --dry-run -p codegg  # error: `codegg` cannot be published (publish=false)

# Full verification
scripts/verify.sh full  # exit 0 (pre-existing tokio flavor warnings are non-blocking)

# Compilation check
cargo check --workspace --all-targets  # 0 errors
```

### Results

All commands pass. No regressions introduced.

## 5. Invariant review

| Invariant | Status | Evidence |
|---|---|---|
| No GitHub Actions trigger publishes crates, creates tags/releases, or uploads release assets | upheld | Workflow deleted; search confirms no release authority in remaining workflows |
| No crates.io token or release credential in workflows or config | upheld | No credentials found in workflow files |
| Release cadence remains maintainer decision | upheld | `RELEASING.md` documents manual procedure only |
| Every intended publishable package has complete metadata | upheld | All packages marked `publish = false`; no crates.io publication intended |
| Every internal-only package is explicitly non-publishable | upheld | All 10 workspace crates have `publish = false` |
| Published workspace dependencies use compatible versions | N/A | No crates intended for publication |
| Publication order follows dependency graph | N/A | No crates intended for publication |
| Already-published crate versions are immutable | N/A | No crates published |
| Partial release requires new version | documented | `RELEASING.md` documents partial failure handling |
| Optional tags/releases remain manual | upheld | `RELEASING.md` documents optional manual tag/release |
| Removing automated binary releases doesn't leave misleading binstall metadata | upheld | `[package.metadata.binstall]` removed |
| Release procedure consumes Milestone 002 verification | upheld | `RELEASING.md` references `scripts/verify.sh full` |

## 6. Failure and recovery review

This milestone involves repository metadata and workflow changes, not runtime behavior. Failure modes are:

- **Dirty working tree during release**: `RELEASING.md` requires clean-tree preflight
- **Parallel maintainer releases**: `RELEASING.md` warns against concurrent execution
- **Partial binary build failure**: `RELEASING.md` documents rebuilding only the failed target

No runtime failure, cancellation, or recovery semantics apply.

## 7. Migration and compatibility review

- Deleting `release.yml` means pushing a `v*` tag no longer creates release assets automatically. This is the intended behavior.
- `cargo-binstall` users lose the automated asset contract. The `[package.metadata.binstall]` section has been removed, so binstall will not attempt to find binary assets.
- `cargo install --path .` from source remains the documented installation method.
- No database migrations, protocol changes, or configuration schema changes involved.

## 8. Security review

- No crates.io tokens or release credentials exist in repository workflows or configuration.
- No GitHub OIDC trusted publishing configured.
- Workflow permissions reduced: only `ci.yml` has explicit `contents: read`.
- No new secret-handling code introduced.

## 9. Documentation and operations

**Updated:**
- `RELEASING.md` — new comprehensive manual release procedure
- `README.md` — corrected clone URL
- `architecture/upgrade.md` — corrected GitHub API URL references
- `docs/TROUBLESHOOTING.md` — corrected issues URL
- `.opencode/skills/upgrade/SKILL.md` — corrected GitHub API URL

**Not requiring update:**
- `CONTRIBUTING.md` — references `scripts/verify.sh full` which is correct
- `AGENTS.md` — no release-specific content requiring update

## 10. Unresolved findings

None.

## 11. Roadmap disposition

Milestone closed and next dependency may proceed.

M004 (optional integration evidence cleanup and closure) was blocked on M003. With M003 closed, M004 can proceed.

## 12. Registry updates

- Move M003 from `ready` to `closed` in `plans/registry.md`
- Move `Development verification and release` subsystem from current milestone to "Milestone 003 closed"
- Unblock M004: move from `blocked` to `ready` in `plans/registry.md`
- Add M003 to recently closed work table
