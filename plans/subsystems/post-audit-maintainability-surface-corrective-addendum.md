# Post-Audit Maintainability and Surface Corrective Addendum

Status: active

Repository audit baseline: `98bc89fa613f5a1390202a90b497f59d5732d431`

Parent roadmap and closure:

- `plans/subsystems/post-audit-maintainability-surface-roadmap.md` — M001-M005 remain historically closed.
- `plans/closure/post-audit-maintainability-surface/001-status.md` through `005-status.md` — preserved as historical evidence.

Related subsystem:

- `plans/subsystems/search-eggsearch-integration-roadmap.md` — remains closed; its explicitly configured built-in search fallback is intentionally retained and is not reopened by this addendum.

Long-term references:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/003-planning-process.md#7-corrective-passes`

No new ADR is required. Both milestones converge compatibility/security implementations onto already-selected canonical owners without changing the daemon, scheduler, protocol, or product architecture. A change to those owners requires a separate ADR/plan.

## 1. Purpose and corrective trigger

The earlier maintainability work established a strong rule: compatibility surfaces may remain when needed, but they should delegate to a canonical implementation rather than accumulate independent behavior. A September 10, 2026 source audit found two residual violations/near-violations worth correcting:

1. the historical `terminal` model tool is intentionally deferred and retained for compatibility, but it still owns a separate dangerous-command regex/list/allowlist policy beside the canonical Bash policy while executing through the same managed-process substrate;
2. MCP OAuth token persistence has a separate AES-GCM/key-normalization/key-environment scheme (`CODEGG_TOKEN_KEY`, `CODEGG_ENC_v1`) even though CodeGG now has canonical secret encryption/master-key infrastructure under `codegg-providers`/`codegg-config`.

The same audit rechecked the legacy built-in search fallback. That overlap is already explicitly classified by the closed search/eggsearch roadmap: Eggsearch is the primary external-search owner, fallback is disabled by default, and `src/search/*` may remain a compatibility-only configured fallback. There is no new evidence requiring deletion, so no search milestone is registered here.

## 2. Work classification

### Invariants

- A retained compatibility tool contains no independent security authority when the canonical operation already has one.
- `bash` remains the canonical model shell and managed process/scheduler boundaries remain unchanged.
- Historical `terminal` names in stored runs/imports/permissions remain readable while the tool stays retained.
- Secret material is never logged or exposed in protocol/UI/audit output.
- New secret writes use one canonical master-key/encryption policy unless an explicitly documented compatibility reader is required.
- Legacy encrypted data remains recoverable when the required legacy key exists; migration never destroys the only readable copy.
- File persistence remains permission-safe, atomic where currently promised, restart-safe, and bounded.

### Capabilities

- Users retaining the `terminal` compatibility tool receive the same canonical command-risk decisions as Bash for equivalent shell content.
- MCP OAuth sessions survive restart and key-format migration without requiring two unrelated long-lived secret-key configurations for new data.
- Operators receive non-secret diagnostics for legacy token-store/key migration state.

### Infrastructure

- One reusable command-safety evaluation seam used by Bash and the compatibility terminal adapter, without introducing a new parser framework.
- Backward-compatible MCP token-store reader plus canonical-encryption writer/migration path.

### Polish

- Remove duplicated regexes/lists, stale encryption documentation, and misleading comments.
- Document explicit removal/deprecation conditions for retained compatibility material.

## 3. Non-goals

- Removing the `terminal` tool name solely because it overlaps Bash.
- Changing Bash command syntax, permission classes, scheduler admission, child-process supervision, or interactive PTY behavior.
- Creating a new shell parser or generalized command-policy engine.
- Forcing MCP OAuth token sets into `CredentialStore` if its single-secret record model cannot represent access+refresh token state cleanly.
- Adding OAuth providers, changing OAuth authorization flows, or broad authentication redesign.
- Replacing the CodeGG master-key scheme or introducing OS keychain integration.
- Deleting the configured legacy search fallback.
- New CI lanes, security scanners, migration frameworks, or dependency-injection systems.

## 4. Current-state evidence

### Terminal compatibility policy

`src/tool/terminal.rs` documents itself as a one-shot compatibility tool: the historical name is retained for stored runs/session imports/permission modes/agent deny-lists; `bash` is canonical for model shell execution; human interactive terminals use the daemon interactive-process service. It is deferred from ordinary model disclosure.

Despite that compatibility status, `TerminalTool` maintains its own `BLOCKED_PATTERN` regex, `blocked_commands` set, optional allowlist interpretation, environment-name filtering, and associated decision logic. `src/tool/bash/policy.rs` separately owns canonical blocked-pattern classification and surrounding Bash execution policy. Both eventually rely on the managed-process service. This creates policy-drift risk without establishing a distinct capability.

### MCP OAuth secret persistence

`src/mcp/auth.rs` stores token sets in `~/.config/codegg/mcp_tokens.json` and implements local AES-256-GCM encryption. Its key comes from `CODEGG_TOKEN_KEY`; short keys are SHA-256 normalized and long values are truncated to 32 bytes. The encrypted file is identified by `CODEGG_ENC_v1`. OAuth replay-used-code state is stored separately.

Canonical provider/config secret handling now uses `codegg_config::encryption::get_master_key()` and `codegg_providers::crypto`, whose current format uses AES-256-GCM with Argon2id-derived keys and a versioned `v2:` representation. `CredentialStore` writes `credentials.json` through that canonical key path and supports bearer-token records, but its data model represents one encrypted secret per provider/account and therefore is not automatically a correct home for a complete OAuth `TokenSet` containing both access and refresh tokens.

## 5. Target architecture

### Terminal

```text
TerminalTool (compat input/name adapter)
          |
          +--> canonical shared shell safety/classification seam
          |
          `--> canonical managed execution path

BashTool
          |
          +--> same canonical shell safety/classification owner
          `--> scheduler/managed execution
```

The compatibility adapter may retain differences intrinsic to its historical parameter shape (separate command/args/env/timeout) but must not own a second list of dangerous shell semantics. Any terminal-only constraint must be explicitly justified as an input-contract restriction rather than a competing risk classifier.

### MCP OAuth token persistence

```text
OAuthManager / TokenSet semantics (MCP-owned)
          |
          +--> versioned token-store serialization
          +--> canonical CodeGG master-key + crypto for new writes
          `--> legacy v1 compatibility reader/migrator

CredentialStore remains provider-auth storage
unless a future shared token-set abstraction is justified.
```

The migration design must preserve readable legacy stores. New writes should converge on the canonical CodeGG master key. The exact v2 outer envelope may remain MCP-specific if needed for whole-file token-set serialization, but its cryptographic primitive/KDF/key source must delegate to canonical infrastructure rather than copy it.

## 6. Dependency graph

```text
M006 Terminal compatibility policy convergence

M007 MCP OAuth crypto/key lifecycle convergence
```

The milestones are independent and both are dependency-ready. They may be implemented separately; serial execution is preferred only to keep handoff/closure simple. Neither is blocked by the unrelated operational evidence items elsewhere in the registry.

## 7. Milestones

### M006 — Terminal compatibility policy convergence

Class: invariant / polish.

Implementation: `plans/implementation/post-audit-maintainability-surface/006-terminal-compatibility-policy-convergence.md`

Objective: make the retained `terminal` tool a thin compatibility adapter over canonical command-safety and managed-execution owners, removing independent dangerous-command policy.

Exit conditions:

- one repository-wide owner classifies equivalent Bash/terminal shell content for destructive/blocked semantics;
- terminal-specific argument/env validation is limited to documented input-contract requirements;
- historical tool name, parameters, deferred disclosure, permission/risk compatibility, timeout/output bounds, and stored-run readers remain compatible;
- parity/negative tests prove no safety widening and no duplicate blocked-pattern authority remains.

### M007 — MCP OAuth crypto and key-lifecycle convergence

Class: invariant / security / maintainability.

Implementation: `plans/implementation/post-audit-maintainability-surface/007-mcp-oauth-crypto-key-lifecycle-convergence.md`

Objective: move new MCP OAuth token persistence onto the canonical CodeGG master-key/crypto path while preserving restart-safe legacy-store recovery and explicit migration semantics.

Exit conditions:

- new MCP token-store writes use canonical CodeGG encryption/key retrieval rather than local AES/KDF/key-normalization code;
- legacy `CODEGG_ENC_v1` data can be read with `CODEGG_TOKEN_KEY` when available and is migrated atomically only when the canonical replacement is known readable;
- migration failure leaves the original readable store intact;
- restart/refresh/revocation/replay behavior is unchanged except for explicitly documented at-rest migration;
- secret/key/token values never appear in diagnostics;
- `architecture/mcp.md`, `architecture/crypto.md`, and auth documentation accurately describe surviving key formats and compatibility rules.

## 8. Cross-cutting requirements

### Storage and migration

M006 has no storage migration. M007 is explicitly a format/key migration and must version it. The reader must distinguish legacy/new formats deterministically. Never overwrite an unreadable or only-legacy-readable store. Atomic replacement should use the repository's existing secure file-write conventions where practical; permissions remain owner-only.

### Protocol and compatibility

No native wire-protocol change is expected. Historical tool names and stored run/transcript readers remain supported. MCP token-store format is an on-disk compatibility contract and therefore receives explicit migration tests rather than silent replacement.

### Security

Canonicalization must not weaken policy. For M006, differences between Bash and terminal decisions are findings to classify before changing behavior; widening requires stop/report. For M007, authentication codes/tokens/master keys are secrets; test fixtures must use synthetic values and diagnostics must be structural only.

### Concurrency, cancellation, restart

M006 leaves managed-process cancellation/timeouts unchanged. M007 must preserve OAuthManager synchronization semantics and verify restart during/after migration. A failed write cannot corrupt both old and new copies.

### Performance/resource use

No new background migration service. Migration happens at the bounded token-store load/write boundary. Shared policy evaluation should not add subprocesses or network work.

## 9. Verification strategy

M006 should use table-driven differential tests over equivalent dangerous, benign, environment, workspace-boundary, timeout, and output cases. The desired evidence is one classification owner and compatibility parity, not a larger security scanner.

M007 should use temporary-store fixtures covering current-format round trip, legacy-v1 load, successful rewrite, absent legacy/canonical keys, wrong key, corrupted/truncated ciphertext, interrupted/failed replacement, token refresh/revocation after restart, and file-permission behavior where portable.

Broad closure posture remains:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

No new workflow lane is authorized.

## 10. Risks and decision points

- Bash and terminal currently accept different input shapes. Converge safety authority, not necessarily schemas.
- A terminal-only block may protect a compatibility contract absent from Bash. Such a case must be documented and represented as an adapter constraint, not silently deleted.
- Whole-token-set OAuth persistence does not map cleanly to a single `CredentialStore` record. Do not force an abstraction mismatch for cosmetic unification.
- Legacy key migration can lock users out if rewrite occurs before successful canonical decryption/readback. Migration must be transactional and test read-after-write before deleting/replacing the source where feasible.
- If raw replay authorization codes are persisted in the used-code store, M007 should perform a focused exposure assessment. Changing replay-key representation (for example to a digest) is allowed only if equality/expiry semantics and backward migration can be proven; otherwise record a separate finding rather than broadening the crypto migration.

No current decision requires an ADR.

## 11. Completion definition

This corrective addendum closes when M006 and M007 have accepted closure records proving that retained compatibility surfaces no longer maintain avoidable duplicate security logic and that MCP OAuth secret persistence uses the canonical CodeGG key/crypto lifecycle for new data without losing legacy recoverability.

The closed search/eggsearch roadmap remains closed unless new evidence shows its configured compatibility fallback has become an active duplicate owner.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M006 | closed | `plans/implementation/post-audit-maintainability-surface/006-terminal-compatibility-policy-convergence.md` | `plans/closure/post-audit-maintainability-surface/006-status.md` | — |
| M007 | active | `plans/implementation/post-audit-maintainability-surface/007-mcp-oauth-crypto-key-lifecycle-convergence.md` | `plans/closure/post-audit-maintainability-surface/007-status.md` | implementation landed; closure evidence in progress |
