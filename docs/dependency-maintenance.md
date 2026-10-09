# Dependency maintenance

CodeGG dependency maintenance is a manual, bounded maintainer activity. It is
not a scheduled CI job and does not imply a fixed release cadence.

When maintenance is useful or a release is being prepared:

1. Inspect direct and outdated dependencies locally with `cargo outdated` (or
   an equivalent current Cargo dependency inspection command).
2. Inspect RustSec/advisory status with the repository's chosen local advisory
   command, such as `cargo audit` when it is available.
3. Review direct dependencies for deprecation, archival, MSRV, licensing, and
   feature-graph changes.
4. Update one bounded dependency group at a time.
5. Run focused consumer tests and `scripts/verify.sh quick`.
6. Run package/release checks only as part of an actual release.
7. Record material compatibility or ownership decisions in an ADR or the
   owning subsystem plan, not in generated CI artifacts.

## YAML compatibility

YAML is a read-only compatibility format for markdown frontmatter in agents,
commands, and skills. New configuration and generated assets use the
subsystem's canonical TOML or JSON/JSON5 format. YAML parsing is centralized
in `codegg-config`'s document codec (`crates/codegg-config/src/document.rs`)
and uses `serde_norway` 0.9.42, a maintained Serde-compatible fork. Existing
YAML files are not rewritten automatically.

## Feature ownership checkpoints

The accepted dependency baseline keeps feature ownership explicit:

- `eggfetch-core` consumers disable defaults and select only `http1`,
  `tls-rustls`, and (where needed) `json`; its packaged WebPKI trust set is
  preferred over native roots. The supported floor is `0.2.0`; generic
  decoded-body limiting is Eggfetch-owned via request-scoped
  `max_decoded_body_size()` and CodeGG owns only SSRF policy, static
  resolved routing, and secret-safe error projection;
- `sqlx` consumers disable defaults and select Tokio, SQLite, `derive`
  (`sqlx::FromRow` is the only macro facility used; handwritten migrations
  replace `migrate`), and only the serialization/time features their source
  uses. `sqlx-mysql`/`sqlx-postgres`/`rsa` remain in the lockfile union but
  are unreachable in every supported feature resolution (`cargo tree -i`
  empty); the `rsa` audit exception in `.cargo/audit.toml` documents that
  upstream-only path;
- `arboard` disables defaults so the default clipboard surface remains
  text-capable without enabling image clipboard support;
- `futures-util`, `futures-executor`, `grep-regex`, and `grep-searcher` are
  used directly rather than through an umbrella crate (no `grep` package
  exists in the lockfile);
- the legacy MD5 dependency remains only for compatibility reads/migration;
  new durable memory namespaces use domain-separated SHA-256.
- the optional `image` stack disables defaults: `image` is
  `default-features = false` with only `png`, `jpeg`, `gif`, `webp`
  plus `bmp` (the `bmp` decoder is built-in with no extra dependency
  family and is retained because `is_supported_image_format` already
  accepts `image/bmp`); `ratatui-image` enables only the `crossterm`
  backend and no longer enables `image-defaults` (`image/default`,
  which pulled `rayon` + all 15 `default-formats`). The TUI decoding
  contract is PNG/JPEG/GIF/WebP (+BMP retained); no broader format
  support is advertised.

These are review checkpoints for bounded maintenance, not a continuously
enforced binary-size or dependency-update gate.

## Workspace ownership (M002)

The root `Cargo.toml` is the authoritative owner of shared versions and
default-feature policy:

- `[workspace.package]` owns `version`, `edition`, `rust-version`,
  `license`, `repository`, and `homepage`. Descriptions and `authors`
  stay local because packages intentionally differ.
- `[workspace.dependencies]` owns versions/default policy for
  dependencies repeated across multiple members (tokio, serde,
  serde_json, thiserror, anyhow, tracing, chrono, sqlx, dashmap, dirs,
  regex, url, uuid, async-trait, futures-util/executor, tokio-util,
  tempfile, toml, sha2, base64, rand, aes-gcm, argon2, hmac, hex,
  similar, libc, once_cell, subtle, flate2, tar, walkdir, eggfetch-core,
  http, tokio-stream, parking_lot, proptest, and the internal
  `codegg-*`/`egg*` path+version pairs). The baseline keeps minimal
  features; member manifests add only the features they use
  (for example `sqlx` baseline is version/default policy only, root/core
  add `runtime-tokio,sqlite,derive,chrono,json` while providers adds
  only `runtime-tokio,sqlite`; `uuid` baseline is `v4` with no `serde`,
  root and core add `serde`; `url` baseline has no `serde`, egglsp adds
  it).
- Single-consumer dependencies (clap, ratatui, crossterm, comrak,
  syntect, image stack, server/plugin optionals, lsp-types/zip/xz2,
  rustpython-parser, notify, and similar) stay local to their owning
  crate and must not be hoisted for uniformity.
- `[workspace.lints.rust] unsafe_code = "deny"` is inherited by three
  crates — `codegg-core`, `codegg-client`, and `codegg-document` — via
  `[lints] workspace = true`. Where deliberate, reviewed `unsafe` is
  required, it is carved out with narrow local `#[allow(unsafe_code)]`:
  `codegg-core` on its `libc::flock` memory-lock helpers
  (`crates/codegg-core/src/memory/mod.rs`,
  `crates/codegg-core/src/memory/habit.rs`) and `codegg-client` on its
  Windows pipe/process modules; `codegg-document` has no `unsafe` at all.
  The root package stays outside package-wide
  inheritance because `src/bin/codegg-sandbox-helper.rs` contains
  deliberate, reviewed `unsafe` (fd ownership + fcntl); its library
  keeps `#![deny(unsafe_code)]` in `src/lib.rs`.
- Remaining duplicate majors in `cargo tree -d` are not all third-party
  owned. `base64` 0.22 (the workspace pin, used by root/axum/sqlx and
  friends) coexists with 0.23 pulled by `eggfetch-core` and `eggsact`;
  `md5` 0.8 is `codegg-core`'s own legacy compatibility dependency while
  0.7 comes from `eggsact`; and `strum` 0.26 is the root crate's direct
  pin while 0.28 arrives through Ratatui (`ratatui-core`,
  `ratatui-widgets`). Each is retained with evidence, not patched.

## External Egg-stack dependency status

Checked against crates.io / the upstream repositories on 2026-10-09.

| Dependency | Pinned | Latest | Status |
|---|---|---|---|
| `eggfetch-core` | `0.2.2` | `0.2.2` | current |
| `eggserve-core` | `0.2.0` | `0.4.0` | **cannot adopt** (see below) |
| `egggress` | not used | — | no dependency anywhere in the workspace |
| `eggup-core` / `eggup-acquisition` | git `66813b3b` | git `70ec4e63` | behind; immutable-pin policy owned by `codegg upgrade` |
| `eggpool-*` (wire + provider-profile) | git `9ac6a131` | git `eed0d58f` | behind; M001 closure revision, pinned deliberately |
| `eggplan-*` | git `0dd33b76` | git `37ad290c` | behind |
| `eggwork-client` / `eggwork-core` | git `e6a5d82e` | git `17ff53fc` | behind |

**Why `eggserve-core` stays at 0.2.0.** `eggserve-core` 0.4.0 pulls in
`landlock`, which is Linux-only: it references `libc::SYS_landlock_create_ruleset`,
`libc::O_PATH`, and `libc::prctl`, none of which exist on `darwin`, so the
dependency cannot compile on macOS. This affects only
`crates/eggwork-test-node`, which is **excluded** from the workspace
(`exclude = [...]`, own lockfile) and is therefore already unbuildable on macOS
at its pinned version. Adopting 0.4.0 needs upstream feature gating of the
sandbox dependency, not a CodeGG-side change.

The remaining git pins are behind their upstreams. They are reviewed closure
revisions rather than floating branches, and moving them is a deliberate
dependency-review step, not a maintenance patch.
