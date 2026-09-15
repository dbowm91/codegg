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
  preferred over native roots;
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

These are review checkpoints for bounded maintenance, not a continuously
enforced binary-size or dependency-update gate.
