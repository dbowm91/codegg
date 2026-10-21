# ADR-0044: Contained Git Execution, Mutable Object Reads, and Trusted Metadata Promotion

Status: proposed — **not accepted; no production authority granted**

Date: 2026-10-18

Decision owners: project maintainers

Related specification sections:
- `plans/000-long-term-specification.md` §§4, 17, 27, 29
- `plans/001-terminology-and-domain-model.md` §8

Affected subsystem roadmaps:
- `plans/subsystems/linux-execution-isolation-roadmap.md`
- `plans/subsystems/linux-trusted-git-cost-corrective-roadmap.md`

Related accepted decisions: ADR-0004 (approvals/ceilings), ADR-0016 (platform-owned launch/obtained enforcement), ADR-0034 (syntax advisory, authority enforced), ADR-0046 (composable Linux sandbox). Historical evidence: Linux C010 no-admission (`plans/closure/linux-execution-isolation/018-status.md`), C012 closed-negative (`020-status.md`), M004 closed (`004-status.md`).

## Context and forces

`egggit::process::GitEnvPolicy::{apply,apply_sync}` calls `prepare_private_object_view` for object-reading trusted Git requests. This provides immutable per-child object bytes but imposes an object-store-sized cold preparation cost, a 4-entry/2-GiB process cache, watchers/fingerprints, a global preparation mutex, and per-command leases. C012's Btrfs fragmented cold p95 of 1,109.391 ms missed C010's 1,000 ms gate, and an in-process workspace mutex cannot exclude host editors or external Git writers. That **negative finding remains valid**: a read-only bind mount or in-process mutex is not an immutable snapshot.

Two requirements must not be conflated: (1) repository-controlled content cannot cause a process to acquire host/daemon capabilities; (2) a Git read must observe one immutable object generation. The existing trusted-read route requires both. This proposal investigates whether a **new, explicitly weaker consistency class** can provide (1) through obtained OS containment while reporting that (2) is not guaranteed. It does not redefine an old immutable-view receipt.

## Decision drivers

- Prevent Git config, hooks, configured-hook commands (where supported), clean/smudge/process filters, merge drivers, fsmonitor, external diff/textconv, credential/transport helpers, URL rewrite, executable attributes, and repository aliases from elevating code execution.
- Retain daemon-authorized operation/lease, parent/child sandbox ceilings, M004 process/IPC isolation, M005 *obtained* offline network protection where requested, and no privileged Git process consuming untrusted executable configuration.
- Read original objects without copying the entire history; avoid requiring Btrfs, reflinks, FUSE, subvolume snapshots, persistent mirrors, or global host mutation.
- Treat source Git object/config/index/refs as concurrently mutable by processes outside Codegg: a read-only mount does not freeze bytes.
- Keep mutating Git operations compatible with official Git behavior while preventing arbitrary agents from changing protected gitdir/common-dir and poisoning later higher-authority operations.

## Considered options

### A. Keep immutable full-object snapshots for all object reads
Preserves existing generation semantics and conservative trust model; cold fragmented costs, copying/cache invalidation, lock contention, and ext4 dependence remain. **Retain as baseline/fallback for operations explicitly requiring immutable reads** until separately superseded.

### B. Contained Git CLI with live read-only original object database (proposed candidate)
Execute a daemon-pinned Git executable only within an M004/M005-qualified isolated child, bind selected original ODB read-only, hide host home/credentials, sockets/proc, and disallow unapproved network. Restrict repository config and execution surfaces. Report `LiveReadInconsistentPossible` (name illustrative) rather than immutable generation. Cheap reads but external writers may alter content, repack, delete objects, or cause failures/TOCTOU; no generation-consistent result may be implied.

### C. Native Rust read APIs (gix) with explicit config filtering
Potentially avoids process spawn and CLI parsing; adds dependency/compile burden and moves hostile parsing into daemon if used in-process. Evaluate both worker and in-process alternatives without assuming safe-owner detection implies agent trust. Does not by itself provide mutation or network authority.

### D. Private index/new-objects overlay with transactional broker promotion
Read original objects through narrowly admitted paths; perform index/object writes in private scratch and validate + publish only authorized effects with real Git locks/compare-and-swap refs. Promising for mutations but multi-resource atomicity (worktree/index/refs) is not automatic and must be qualified per operation.

### E. More snapshot optimization (reflink, Btrfs subvolume, incremental mirror, lazy FUSE)
May improve certain hosts/operations but does not solve general ext4 cold costs or reduce overall mechanism complexity. Keep as optional, explicitly qualified optimization rather than a required dependency.

## Proposed decision — subject to maintainer acceptance

1. Introduce an explicit internal Git **consistency/authority requirement**, distinguishing an immutable-object read from a sandbox-contained *live-object* read. Only the latter may read the original ODB without copying; its receipt must say mutable/non-snapshot and must not claim reproducible generation identity.
2. An untrusted repository-dependent Git CLI may use the live read only when the actual child obtains the full required filesystem/protected-metadata, process/IPC/FD/seccomp and offline-network boundary **before exec**. A probe, unverified helper, legacy Landlock-only route, or hoped-for M005 network claim is insufficient. No fallback to host Git or FullHost on denial.
3. Preserve `GitExecutionService` / `GitMutationExecutor` / `ManagedProcessService` and typed approval/lease authority. The daemon may use Git data from an untrusted repository but must not spawn uncontained Git with executable repo-controlled configuration or treat writable metadata as a trusted authority input.
4. Trusted reads must bind selected worktree, gitdir, common-dir and authorized ODB identity. Explicitly reject or mediate alternates, replace refs, shallow/grafts/commit graphs, include paths, dynamic aliases, symlink/mount changes and cross-worktree references when they cannot be safely resolved or contained. Do not rely on command-spelling allowlists for the security property.
5. For mutations, use operation-scoped private index/new-object storage and a narrow **daemon-owned** publisher. Validate changed objects/index/refs and allowed paths, reject stale observed state, coordinate Git-compatible locks/CAS, and prove recovery after a partial failure. The publisher must never execute hooks, filters, helpers or worktree-controlled code with its own host authority. A Codegg lease does **not** exclude external writers.
6. Network transport/credentials remain separately mediated under Linux C003/M006; no `RemoteHelper`/Credential permission follows from ordinary contained local Git. Destructive reset/clean retains `DestructiveRepositoryChange` authorization.
7. No new persistent setting, public DTO, production-default switch, M003 backend admission, M005 `NetworkEnforcement::Enforced` claim, or historical closure reversal is authorized by this **proposed** ADR. Acceptance and separate exact-revision qualification/rollout decisions are required.

## Consequences

Positive: may eliminate ODB-sized setup cost, reduce shared cache mutex contention and per-session storage, restore common Git workflows under the real execution boundary. Negative: live reads sacrifice immutable-generation consistency, require adversarial parser/TOCTOU tests, and create per-operation promotion complexity. Deferred: cross-platform parity, native gix adoption, full offline-network rollout, remote transport and generalized transactional workspace snapshots.

## Compatibility, authority, and migration

Legacy Landlock-only Git and existing `GitEnvPolicy` snapshot path remain unchanged until new route qualifies. C010 no-admission and C012 negative closure remain authoritative for **immutable-view-based M003 admission**; only a new named capability with revised guarantees may be considered after acceptance and evidence. C002/C003 and M008 are unchanged until independently closed. New outcome/receipt fields must be additive/versioned where they reach persisted/public surfaces, never reinterpret an existing `Enforced` flag. CLI semantics and typed Git results must remain test-compatible; no migration is implied by drafting this ADR.

## Security, lifecycle, and reliability

Tests must cover malicious local and configured hooks, Git filters/merge drivers, config includes/aliases, credential and remote helpers, host proc/abstract/path sockets, environment/inherited FD, untrusted gitdir/commondir and nested worktrees, alternate ODB/replace refs, source mutation during object reads, object corruption/repack/gc, and worker cancellation. No secrets in ordinary logs; cap resource/time/output; fail closed on unsupported userns/kernel and forged/late enforcement receipts. An externally concurrent ref/index edit must cause retry/conflict or safe failure, not silent overwrite.

## Verification and acceptance gate

Test-only proof may proceed under ADR-0046 while this ADR is proposed. Before enabling any live-ODB trusted read or private promotion route: independent security review accepts/revises this ADR; native supported Linux and restricted-userns negative proofs; real-tool equivalence and performance evidence; exact head CI; rollback; explicit operator release decision. Any missing requirement is a hard blocker, not a waived test.

## Supersession

None at proposal time. This ADR would introduce a **separately named** authority/consistency contract; it does not rewrite ADR-0046 or the historical C010/C012 decisions.
