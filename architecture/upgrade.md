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

`cmd_upgrade()` calls `upgrade::upgrade()`. It checks the latest release with
the existing Eggfetch client/profile, then for a newer supported release:

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

The archive checksum is integrity evidence, not publisher authenticity. The
transitive digest from archive to extracted member is not independent member
signing. The optional notice is treated as data and never executed.

## Failure and platform behavior

Any release, checksum, download, archive, candidate, ownership, or lock failure
occurs before live mutation. Commit failures use Eggup rollback. A
`RecoveryRequired` receipt surfaces its retained evidence path. A previously
installed CodeGG binary may add absent helper or eggsearch siblings; arbitrary
existing files are never treated as owned. An existing notice is preserved
because the updater cannot prove its ownership.

Only `x86_64` and `aarch64` Linux/macOS prebuilt targets are eligible for
in-place replacement (`SUPPORTED_TARGETS`, `managed.rs:25`). Windows,
unsupported targets, and installations outside the qualified sibling
contract receive pinned manual fresh-install guidance
(`describe_manual_fresh_install`, `mod.rs:124`).

The `autoupdate` configuration is inert: `Config.autoupdate`
(`crates/codegg-config/src/schema.rs:236`) is an untagged
`AutoupdateConfig` (`Bool(bool)` / `Notify(String)`, defaulting to
`Bool(true)`, `schema.rs:205`) that is deserialized and destructured but
never read by any production code path. Nothing schedules or triggers an
upgrade automatically; this path runs only when the user invokes
`codegg upgrade`.

## Source locations

- `src/upgrade/mod.rs` — release metadata, version policy, CLI-facing entry
- `src/upgrade/managed.rs` — CodeGG-specific target/archive policy, Eggfetch
  adapter, checksum parser, strict extraction, candidate checks, and ownership
- `src/main.rs` — `cmd_upgrade()`
- `tests/upgrade.rs` and `src/upgrade/managed.rs` — deterministic policy and
  archive fixtures

The Eggup dependencies — `eggup-core` and `eggup-acquisition` — are both
pinned to the same immutable revision `66813b3b94de3a9b2f270e0000dc339ef6f0b478`
(`Cargo.toml:116-117`); neither is a floating branch.
CodeGG retains its current Eggfetch/Rustls/WebPKI trust and redirect policy.

## Source verification

Verified 2026-10-06 against `src/upgrade/{mod,managed}.rs`, `src/main.rs`,
`tests/upgrade.rs`, `Cargo.toml`, and
`crates/codegg-config/src/schema.rs`. No stale refs remained from the prior
pass; the corrections here are additive precision. Confirmed correct as
written: the Eggup pin `66813b3b94de3a9b2f270e0000dc339ef6f0b478` for both
`eggup-core` and `eggup-acquisition` (`Cargo.toml:116-117`), the four
`SUPPORTED_TARGETS` entries (`managed.rs:25`) and the exact `supported_target()`
Linux/macOS x86_64/aarch64 mapping (`managed.rs:31`), the pinned eggsearch
version (`PINNED_EGGSEARCH_VERSION`, `managed.rs:24`), `cmd_upgrade()` in
`src/main.rs`, `upgrade()` (`mod.rs:101`), the manual fresh-install guidance
path pointing at `install.sh` on raw.githubusercontent.com (`mod.rs:32,124`),
and the presence of `tests/upgrade.rs` (10 tests).
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
`tests/upgrade.rs`, and `Cargo.toml`.
- Corrected "The Eggup dependency" to name both `eggup-core` and
  `eggup-acquisition`, which share the single pinned revision
  (`Cargo.toml:116-117`).
- Re-confirmed, no change needed: the release API URL
  (`mod.rs:68`), `check_for_updates()` (`mod.rs:64`), `upgrade()`
  (`mod.rs:101`), `VersionInfo` (`mod.rs:44`), `describe_manual_fresh_install()`
  (`mod.rs:124`), the four-entry `SUPPORTED_TARGETS` (`managed.rs:25`), the
  pinned eggsearch `0.3.9` (`managed.rs:24`), the 10-second Eggfetch profile
  (`managed.rs:156`, `:176`), exact-404-only asset absence
  (`managed.rs:552`, `:621`), the three required runfiles plus optional
  `THIRD-PARTY-NOTICES.txt` (`managed.rs:263`-`:264`, `:305`), and the
  10 tests in `tests/upgrade.rs`.
- No stale claims remain; the CodeGG/Eggup/Eggpack ownership split and the
  "keep Eggfetch, do not adopt `eggup-eggfetch`" constraint are unchanged
  and still match source.
