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
in-place replacement. Windows, unsupported targets, and installations outside
the qualified sibling contract receive pinned manual fresh-install guidance.
The inert `autoupdate` configuration remains inactive; this path runs only
when the user invokes `codegg upgrade`.

## Source locations

- `src/upgrade/mod.rs` — release metadata, version policy, CLI-facing entry
- `src/upgrade/managed.rs` — CodeGG-specific target/archive policy, Eggfetch
  adapter, checksum parser, strict extraction, candidate checks, and ownership
- `src/main.rs` — `cmd_upgrade()`
- `tests/upgrade.rs` and `src/upgrade/managed.rs` — deterministic policy and
  archive fixtures

The Eggup dependency is pinned to immutable revision
`66813b3b94de3a9b2f270e0000dc339ef6f0b478`; it is not a floating branch.
CodeGG retains its current Eggfetch/Rustls/WebPKI trust and redirect policy.
