# Upgrade Module

Native verified replacement of CodeGG's managed runfile bundle on the
prebuilt Linux and macOS targets.

## Ownership

CodeGG owns GitHub release identity, version comparison, target mapping,
archive naming, the pinned eggsearch version, helper identity semantics, and
CLI output. Eggup owns bounded acquisition contracts and local staged,
verified, locked multi-file replacement with rollback and recovery receipts.
Eggpack remains producer authority. The fresh-install `install.sh` remains a
separate bootstrap path and is never fetched or executed by `codegg upgrade`.

## Flow

`cmd_upgrade()` (`src/main.rs:2510`) calls `upgrade::upgrade()`
(`src/upgrade/mod.rs:101`). It checks the latest release with the existing
Eggfetch client/profile, then for a newer supported release:

1. maps the running Linux/macOS OS and architecture to one of the four
   installer targets;
2. downloads `checksums.txt` and exactly `codegg-<target>.tar.gz` using the
   CodeGG-owned Eggfetch client through an `AcquisitionTransport` adapter;
3. selects one exact basename entry and verifies archive SHA-256 before
   opening the gzip/tar stream;
4. extracts privately, accepting only the three required top-level runfiles
   and optional `THIRD-PARTY-NOTICES.txt`; links, special files, duplicate or
   case-colliding names, nested paths, unexpected entries, and over-bound
   members are rejected;
5. hashes extracted regular files and supplies those SHA-256 values to
   `eggup-core`; bounded staged candidate probes verify exact CodeGG and
   eggsearch identities/versions and the helper's safe refusal behavior;
6. proves live ownership from the running CodeGG path/version and the sibling
   identity probes, then commits the complete runfile set through Eggup.

The release check is `check_for_updates()` (`mod.rs:64`): it reads
`tag_name` from the GitHub "latest release" API, strips a leading `v`, and
sets `needs_update` by **exact string comparison** against the crate version
(`mod.rs:92`) — it is not a semver ordering, so any tag that is not
character-for-character the crate version reads as an upgrade. The version
that is then downloaded is re-parsed as semver by `managed::update()`
(`managed.rs:57`), so a malformed tag fails closed there rather than earlier.
The blocking work runs on `tokio::task::spawn_blocking` (`mod.rs:110`) so the
synchronous extraction and commit never block the async runtime.

The archive checksum is integrity evidence, not publisher authenticity. The
transitive digest from archive to extracted member is not independent member
signing. The optional notice is treated as data and never executed.

## Configuration Surface

`codegg upgrade` takes no configuration. The only config-shaped symbol in the
module is inert.

The `autoupdate` configuration is inert: `Config.autoupdate`
(`crates/codegg-config/src/schema.rs:236`) is an untagged
`AutoupdateConfig` (`Bool(bool)` / `Notify(String)`, `schema.rs:205`) whose
`Default` is `Bool(true)` (`schema.rs:210`-`:213`) and whose field type is
`Option<AutoupdateConfig>`. The field is deserialized and destructured
(`crates/codegg-config/src/paths.rs:188`) but read by no production code path
— `autoupdate` has exactly two occurrences in any `.rs` file outside the
unrelated `--rerere-autoupdate` git flags. Nothing schedules or triggers an
upgrade automatically; this path runs only when the user invokes
`codegg upgrade`.

The fresh-install script has one version-pin contract, and it is CodeGG's
rather than the installer's: `INSTALLER_VERSION_ENV` is `CODEGG_VERSION`
(`mod.rs:35`), and `installer_invocation()` (`mod.rs:57`) returns the script
URL plus exactly that one environment entry. The installer normalizes
`CODEGG_VERSION=0.1.1` or `CODEGG_VERSION=v0.1.1` to the tag `v0.1.1`;
exporting any other name silently installs the latest release instead of the
checked version. `INSTALLER_SCRIPT_URL` (`mod.rs:31`) is printed only as
manual guidance — `codegg upgrade` never fetches or executes it.

## Failure and platform behavior

Any release, checksum, download, archive, candidate, ownership, or lock failure
occurs before live mutation. Commit failures use Eggup rollback. A
`RecoveryRequired` receipt surfaces its retained evidence path. A previously
installed CodeGG binary may add absent helper or eggsearch siblings; arbitrary
existing files are never treated as owned. An existing notice is preserved
because the updater cannot prove its ownership.

Only `x86_64` and `aarch64` Linux/macOS prebuilt targets are eligible for
in-place replacement (`SUPPORTED_TARGETS`, `managed.rs:25`; the
`std::env::consts::OS`/`ARCH` mapping is `supported_target()`,
`managed.rs:32`). An unsupported host — including Windows — fails inside
`managed::update()` (`managed.rs:51`) with the pinned manual fresh-install
command, so no release asset is fetched at all.

`describe_manual_fresh_install()` (`mod.rs:124`) is a **pure, network-free
helper**: it returns the already-current message, fails closed on missing or
non-semver release data, and otherwise returns the same version-pinned
bootstrap guidance. It is public API on `codegg::upgrade` but is **not**
called by `cmd_upgrade` — the CLI path reaches the guidance through
`managed::update()`. The two carry the same version-pinned command
(`CODEGG_VERSION=v<version> curl -fsSL <script> | sh`) but different lead-in
wording, because the helper also reports the current version and validates
the release tag as semver.

A foreign or ambiguous existing file does **not** get manual guidance: the
ownership verifier classifies it as `Ownership::Foreign` or `Ownership::Unknown`
(`managed.rs:404`-`:455`) and the commit fails closed. Only an *absent*
sibling may be created, under `AbsentPolicy::AllowCreate`
(`managed.rs:124`), which is what lets a historical single-`codegg`
installation gain the helper and eggsearch siblings.

## Invariants & Gotchas

- The archive allowlist is exactly three required runfiles (`codegg`,
  `codegg-sandbox-helper`, `codegg-eggsearch`) plus the optional
  `THIRD-PARTY-NOTICES.txt`. A missing required member fails the update
  (`managed.rs:305`).
- Extraction bounds are explicit: `MAX_ARCHIVE_BYTES` 256 MiB,
  `MAX_RUNFILE_BYTES` 128 MiB per runfile, `MAX_NOTICE_BYTES` 1 MiB, and
  `CHECKSUM_LIMIT` 64 KiB for the manifest (`managed.rs:20`-`:23`). Members
  are written with `create_new`, so a pre-existing path is never overwritten
  in the extraction directory.
- The checksum manifest must contain **exactly one** well-formed line for the
  expected archive basename; a duplicate entry is a hard error
  (`checksum_for_archive`, `managed.rs:187`). The manifest is rejected
  wholesale if any line is malformed, even for an unrelated basename.
- Identity is proven by running the staged candidate, not by trusting its
  name: `codegg --version` and `codegg-eggsearch --version` must match the
  expected identity and version (`CodeggValidator`, `managed.rs:358`), and
  `codegg-sandbox-helper` must exit **non-zero** with a refusal on stderr
  (`managed.rs:378`-`:393`).
- The updater never shells out to `curl`, never fetches `install.sh`, and
  never executes the notice. The only external processes it spawns are the
  three staged candidates it is verifying.
- The updater does not sign anything it downloads: the archive checksum is
  integrity evidence, not publisher authenticity.

## Source locations

- `src/upgrade/mod.rs` — release metadata, version policy, CLI-facing entry
- `src/upgrade/managed.rs` — CodeGG-specific target/archive policy, Eggfetch
  adapter, checksum parser, strict extraction, candidate checks, and ownership
- `src/install.rs` — installation-owned sibling identity and the pinned
  eggsearch version/commit (`PINNED_EGGSEARCH_VERSION`,
  `PINNED_EGGSEARCH_COMMIT`)
- `src/main.rs` — `cmd_upgrade()`
- `tests/config_provider/upgrade.inc` and `src/upgrade/managed.rs` — deterministic policy and
  archive fixtures

The Eggup dependencies — `eggup-core` and `eggup-acquisition` — are both
pinned to the same immutable revision `66813b3b94de3a9b2f270e0000dc339ef6f0b478`
(`Cargo.toml:117-118`); neither is a floating branch.
CodeGG retains its current Eggfetch/Rustls/WebPKI trust and redirect policy.

## Source verification

Verified 2026-10-06 against `src/upgrade/{mod,managed}.rs`, `src/main.rs`,
`tests/config_provider/upgrade.inc`, `Cargo.toml`, and
`crates/codegg-config/src/schema.rs`. No stale refs remained from the prior
pass; the corrections here are additive precision. Confirmed correct as
written: the Eggup pin `66813b3b94de3a9b2f270e0000dc339ef6f0b478` for both
`eggup-core` and `eggup-acquisition` (`Cargo.toml:116-117`), the four
`SUPPORTED_TARGETS` entries (`managed.rs:25`) and the exact `supported_target()`
Linux/macOS x86_64/aarch64 mapping (`managed.rs:31`), the pinned eggsearch
version (`PINNED_EGGSEARCH_VERSION`, `managed.rs:24`), `cmd_upgrade()` in
`src/main.rs`, `upgrade()` (`mod.rs:101`), the manual fresh-install guidance
path pointing at `install.sh` on raw.githubusercontent.com (`mod.rs:32,124`),
and the presence of `tests/config_provider/upgrade.inc` (10 tests).
Re-verified the dead-config claim: it is still true and is now stated
precisely — `Config.autoupdate` (`schema.rs:231`) and its `AutoupdateConfig`
enum (`schema.rs:205`) are the only occurrences of the symbol in any `.rs`
file outside the unrelated `--rerere-autoupdate` git flags, so the field is
parsed and destructured (`codegg-config/src/paths.rs:180`) but never read.

Verified 2026-10-06 against source after upstream `2573f9c0` ("Decision runtime
ownership migration and M006 closure") and against in-flight config work.
Corrected: the `Config.autoupdate` ref `crates/codegg-config/src/schema.rs:231`
->`:236`, re-checked to land on the field; it moved because of a 5-line
doc-comment insertion immediately above it in the working tree, not because of
that commit. The inert-autoupdate claim is unchanged: the field is still
deserialized and destructured but read by no production path, and
`codegg upgrade` still runs only on explicit invocation.

Verified 2026-10-06 (third pass) against `src/upgrade/{mod,managed}.rs`,
`tests/config_provider/upgrade.inc`, and `Cargo.toml`.
- Corrected "The Eggup dependency" to name both `eggup-core` and
  `eggup-acquisition`, which share the single pinned revision
  (`Cargo.toml:116-117`).
- Re-confirmed, no change needed: the release API URL
  (`mod.rs:68`), `check_for_updates()` (`mod.rs:64`), `upgrade()`
  (`mod.rs:101`), `VersionInfo` (`mod.rs:44`), `describe_manual_fresh_install()`
  (`mod.rs:124`), the four-entry `SUPPORTED_TARGETS` (`managed.rs:25`), the
  pinned eggsearch `0.4.4` (`managed.rs:24`), the 10-second Eggfetch profile
  (`managed.rs:156`, `:176`), exact-404-only asset absence
  (`managed.rs:552`, `:621`), the three required runfiles plus optional
  `THIRD-PARTY-NOTICES.txt` (`managed.rs:263`-`:264`, `:305`), and the
  10 tests in `tests/config_provider/upgrade.inc`.
- No stale claims remain; the CodeGG/Eggup/Eggpack ownership split and the
  "keep Eggfetch, do not adopt `eggup-eggfetch`" constraint are unchanged
  and still match source.

Verified 2026-10-23 (deep-dive pass) against `src/upgrade/{mod,managed}.rs`,
`src/main.rs`, `src/install.rs`, `src/http_client.rs`,
`tests/config_provider/upgrade.inc`, `Cargo.toml`, and
`crates/codegg-config/src/{schema,paths}.rs`. Line numbers in the earlier
2026-10-06 entries above are point-in-time; the body above carries the
current references.

Corrected:
- `Cargo.toml:116-117` -> `:117-118` for the two Eggup pins (both still
  `66813b3b94de3a9b2f270e0000dc339ef6f0b478`).
- The unsupported-target paragraph no longer attributes the CLI's manual
  guidance to `describe_manual_fresh_install()`. `cmd_upgrade` reaches that
  guidance through `managed::update()` (`managed.rs:51`); `describe_manual_fresh_install()`
  (`mod.rs:124`) is a pure, network-free helper that `cmd_upgrade` never
  calls (verified: its only non-definition reference in `src/` and `tests/` is
  the test file).
- The same paragraph no longer claims that "installations outside the
  qualified sibling contract receive pinned manual fresh-install guidance".
  A foreign or ambiguous existing file fails closed through
  `Ownership::Foreign`/`Ownership::Unknown`
  (`CodeggOwnership::verify`, `managed.rs:404`-`:455`); only an *absent*
  sibling may be created, under `AbsentPolicy::AllowCreate`
  (`managed.rs:124`).
- `crates/codegg-config/src/paths.rs:180` -> `:188` for the `autoupdate`
  destructuring site, and the `AutoupdateConfig` default is now pinned to
  `schema.rs:210`-`:213` (`impl Default` at `:210`, `Bool(true)` at `:212`)
  rather than the enum's own line.

Added (depth): a `## Configuration Surface` section covering the inert
`[autoupdate]` field and the `CODEGG_VERSION` installer version-pin
contract (`INSTALLER_VERSION_ENV`, `installer_invocation()`); an
`## Invariants & Gotchas` section covering the archive allowlist, the size
bounds, the single-checksum-entry rule, the run-the-candidate identity
probes, and the no-`curl`/no-signature boundaries; and the
`src/install.rs` entry in `## Source locations` for the installation-owned
sibling identity and the pinned eggsearch version/commit.

Confirmed correct, no change needed: `cmd_upgrade` at `src/main.rs:2510`;
`upgrade::upgrade()` at `src/upgrade/mod.rs:101`; `check_for_updates()` at
`mod.rs:64` with the release API URL at `mod.rs:68`; `VersionInfo` at
`mod.rs:44`; `INSTALLER_SCRIPT_URL` at `mod.rs:31`; the four installer
targets `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`,
`x86_64-apple-darwin`, `aarch64-apple-darwin` (`SUPPORTED_TARGETS`,
`managed.rs:25-30`, mapped by `supported_target()` at `managed.rs:32`); the
three required runfiles `codegg`, `codegg-sandbox-helper`, `codegg-eggsearch`
with optional `THIRD-PARTY-NOTICES.txt` (`managed.rs:263`-`:264`, `:305`,
`:328`-`:334`); the archive and member bounds (`MAX_ARCHIVE_BYTES` 256 MiB,
`MAX_RUNFILE_BYTES` 128 MiB, `MAX_NOTICE_BYTES` 1 MiB, `CHECKSUM_LIMIT`
64 KiB, `managed.rs:20`-`:23`); exact-404-only asset absence
(`managed.rs:552`, `:621`); the 10-second connect timeout on both the
metadata and artifact fetch limits (`managed.rs:156`, `:176`); the pinned
upstream eggsearch version `0.4.4` at commit
`32f48bb57a43274d763c7f9ba919eab84d125edc` (`src/install.rs:22`, `:24`, and
`PINNED_EGGSEARCH_VERSION` at `managed.rs:24`); and the 10 tests in
`tests/config_provider/upgrade.inc`.

Also recorded (not corrected here): the release check compares the normalized
tag against the crate version by **exact string equality**
(`mod.rs:92`), not by semver ordering, so a tag that differs from the crate
version in any character reads as an upgrade; the semver parse that follows
(`managed.rs:57`) is what fails closed on a malformed tag.
