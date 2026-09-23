---
name: upgrade
description: Verified managed-runfile self-upgrade via GitHub releases
version: 1.3.0
tags: [upgrade, releases, versioning, eggup]
---

Use the `/skill:upgrade` command to load context about the CodeGG managed
runfile upgrade system.

## Overview

`codegg upgrade` checks CodeGG's GitHub latest release and, on supported
prebuilt Linux/macOS targets, uses Eggup to replace the managed bundle
(`codegg`, `codegg-sandbox-helper`, `codegg-eggsearch`) as one verified local
transaction. The bootstrap `install.sh` remains a user-invoked fresh-install
path. The updater never downloads or executes it and never shells out to
external `curl`.

## Ownership

- CodeGG owns release repository, tag/version selection, supported target
  mapping, archive/checksum asset names, eggsearch pin, helper probe, CLI
  presentation, and unsupported-platform behavior.
- Eggup owns bounded acquisition contracts and local integrity, candidate
  validation seam, locked ownership revalidation, multi-member replacement,
  rollback, and recovery receipts.
- The consumer adapter uses CodeGG's existing Eggfetch client/profile. Do not
  switch to `eggup-eggfetch`: its native-root and proxy feature set widens
  CodeGG's current transport/trust policy.
- Eggpack owns producer-side release construction. Do not move its authority
  or introduce generic archive extraction into Eggup.

The Eggup dependencies use immutable git revision
`66813b3b94de3a9b2f270e0000dc339ef6f0b478` for both `eggup-core` and
`eggup-acquisition`.

## Update sequence

1. Query `https://api.github.com/repos/dbowm91/codegg/releases/latest` with
   CodeGG's current 10-second Eggfetch profile.
2. Map the running Linux/macOS OS and architecture to the checked-in four
   installer target triples. Unsupported targets keep pinned manual install
   guidance; Windows remains check-only.
3. Fetch the bounded `checksums.txt` and exact `codegg-<target>.tar.gz` release
   assets. Only exact HTTP 404 is treated as an absent asset; all other HTTP,
   TLS, redirect, timeout, I/O, and cancellation failures stop the update.
4. Require exactly one well-formed checksum line for the expected archive
   basename. Verify the archive SHA-256 before parsing or extracting it.
5. Extract privately. Accept only top-level `codegg`,
   `codegg-sandbox-helper`, `codegg-eggsearch`, and optional
   `THIRD-PARTY-NOTICES.txt`. Reject links, special entries, nested/traversal or
   absolute paths, backslashes, duplicates/case collisions, unexpected names,
   and entries above their bounds.
6. Hash each extracted file, stage it with Eggup, and validate CodeGG's exact
   selected version, eggsearch's pinned `0.3.9`, and the helper's safe refusal
   probe. The notice is never executed.
7. Classify current managed siblings by CodeGG-owned identity probes and commit
   the whole runfile generation with Eggup. Missing managed siblings may be
   created for the historical single-CodeGG layout. Foreign or ambiguous
   existing files fail closed.
8. Interpret `Committed`, `RolledBack`, and `RecoveryRequired` distinctly.
   Always include the retained evidence path for `RecoveryRequired`.

Archive checksum is integrity evidence, not publisher authenticity. Per-file
digests preserve continuity from the verified archive to Eggup's staged-file
checks; they are not independent signatures.

## Key APIs

`src/upgrade/mod.rs` provides `VersionInfo`, `check_for_updates()`, and
`upgrade()`. `src/upgrade/managed.rs` contains release target mapping, bounded
CodeGG Eggfetch adapter, checksum and archive policy, candidate validation,
and live ownership verification. `describe_manual_fresh_install()` remains a pure helper
for already-current and manual-install dispositions; it does not perform the
supported native update.

The `[autoupdate]` config remains inert. The verified update runs only after
explicit `codegg upgrade` invocation.

## Testing and verification

Focused tests cover checksum selection, archive allowlist and negative cases,
and target drift. `tests/upgrade.rs` covers version disposition and pins the
fresh-installer variable to `CODEGG_VERSION`. Run the normal project checks
from `AGENTS.md`; release/install scripts must remain green.
