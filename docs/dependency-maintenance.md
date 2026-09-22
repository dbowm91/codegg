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
in `codegg-config`'s document codec and uses `serde_norway` 0.9.42, a
maintained Serde-compatible fork with the repository's Rust 1.89-compatible
MSRV. Existing YAML files are not rewritten automatically.

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
  used directly instead of the removed umbrella dependencies;
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
  only `runtime-tokio,sqlite`; `uuid` baseline is `v4`, serde stays
  local; `url` baseline has no `serde`, egglsp adds it).
- Single-consumer dependencies (clap, ratatui, crossterm, comrak,
  syntect, image stack, server/plugin optionals, lsp-types/zip/xz2,
  rustpython-parser, notify, and similar) stay local to their owning
  crate and must not be hoisted for uniformity.
- `[workspace.lints.rust] unsafe_code = "deny"` is inherited only by
  `codegg-core`, the one library crate that already enforced it with no
  deliberate unsafe. The root package stays outside package-wide
  inheritance because `src/bin/codegg-sandbox-helper.rs` contains
  deliberate, reviewed `unsafe` (fd ownership + fcntl); its library
  keeps `#![deny(unsafe_code)]` in `src/lib.rs`. Other crates keep
  explicit local `#[allow(unsafe_code)]` on reviewed test helpers.
- Remaining duplicate majors in `cargo tree -d` are third-party owned
  (for example `base64` 0.22/0.23 via `eggsact`, `md5` 0.7/0.8,
  `strum` 0.26/0.28 via Ratatui) and are retained with evidence, not
  patched.
