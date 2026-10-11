---
name: authorization
description: Per-operation authority descriptors, capability narrowing, and the durable audit trail in codegg
version: 1.0.0
tags:
  - authorization
  - audit
  - security
  - provenance
---

# Authorization & Audit Guide

Operational guide for changing `crates/codegg-core/src/authorization/` and
`crates/codegg-core/src/audit.rs`. The full contracts live in
`architecture/authorization.md` and `architecture/audit.md`; this skill covers
the invariants that keep the matrix and the trail honest.

## Layout

| Path | Purpose |
|------|---------|
| `crates/codegg-core/src/authorization/policy.rs` | `operation_descriptor` — the per-request classification that everything else keys off |
| `crates/codegg-core/src/authorization/authority.rs` | `CapabilitySet` narrowing/delegation, projection mapping, audit provenance |
| `crates/codegg-core/src/authorization/attribution.rs` | Actor/project attribution for a decision |
| `crates/codegg-core/src/audit.rs` | `AuditAction`, `AuditStore`, `AuditEventBuilder`, query/export, writer config |
| `crates/codegg-core/src/audit_instrumentation.rs` | Instrumented call-site wrappers |
| `src/executor_audit_hooks.rs`, `src/live_execution_audit.rs`, `src/scheduler/job_complete_audit.rs` | Runtime audit emission points |

## Key Invariants

- **`operation_descriptor` is the single classifier.** Every `CoreRequest`
  maps to exactly one `OperationDescriptor`
  (`crates/codegg-core/src/authorization/policy.rs:101`). Authority checks,
  audit records, and the projection all read that descriptor rather than
  re-deriving policy from the request variant.
- **The descriptor decides project visibility.** Operations carrying
  project-keyed data are project-scoped. Permission/question responses, tasks,
  notifications, and daemon-global snapshots are *global* and carry no
  project key. Filesystem-locator, credential-adjacent, and cross-project
  operations without a project locator are **opaque and fail closed** for team
  principals (`policy.rs:96-100`).
- **Capabilities only narrow.** `narrow_authority` and `child_escalates`
  (`authority.rs:21,29`) enforce that a delegated child can never exceed its
  parent. `authorize_child_delegation` (`:35`) is the enforcement point — do not
  add a path that assigns capabilities directly.
- **Denials can be masked deliberately.** `denial_as_not_found` (`:260`)
  exists so an opaque/cross-project denial does not leak existence. Use it
  rather than inventing a new error shape for that case.
- **The audit trail never fabricates success.** `AuditFailurePolicy`
  (`crates/codegg-core/src/audit.rs:1226`) is `FailClosed` (surface the
  storage error) or `FailVisible` (record and report failed, never as
  appended) — and the `Default` impl selects `FailClosed` (`:1249-1250`).
  There is no third "drop it" option; do not add one.
- **The writer is bounded.** `AuditWriterConfig` (`audit.rs:1239`) caps
  concurrent appends (`max_inflight`, default 32) and bounds each by
  `write_timeout_ms` (default 2000) — `:1248-1249`. `AuditWriter` turns
  `max_inflight` into a `Semaphore` permit (`:1290`); when saturated,
  `try_append` fails fast with `AuditError::Backpressure` and a `dropped`
  counter rather than buffering (`:1321-1327`). Keep it that way — an
  unbounded queue would make a stalled store invisible.
- **Identity digests are canonical.** `sha256_hex` and `metadata_digest`
  (`audit.rs:448,465`) hash a `BTreeMap`, so ordering is stable; use them
  rather than hashing a `HashMap` directly.

## Guards

```bash
python3 scripts/check_authorization_matrix.py   # matrix vs operation_descriptor
python3 scripts/check_audit_coverage.py         # every audit-sensitive site emits
python3 scripts/check_audit_invariants.py       # fail-closed / bounded-writer rules
python3 scripts/audit_tokio_tests.py
```

Only `check_audit_coverage.py` is wired into `scripts/verify.sh quick` and CI
(`scripts/verify.sh:82-83`, `.github/workflows/ci.yml:117`). The matrix,
invariants, and tokio-test guards are **manual** — run them by hand before
opening a change that adds an operation or touches audit semantics, or a
matrix gap will reach CI unnoticed.

`architecture/authorization.md` carries a per-operation matrix. When you add an
operation, update that table in the same change or `check_authorization_matrix.py`
will fail.

Repository initialization uses `project_init_draft` (`direct_project`,
`file.read`) and `project_init_publish` (`direct_project`, `file.modify`). The
draft token is single-use and bound to the authenticated client and canonical
project/workspace scope; the publish request contains no path or file content.

## Testing

The `authorization/` modules carry no inline `#[test]`s — authority behavior is
covered by integration tests at the repo root:

```bash
cargo test --test identity_m003_daemon_authorization
cargo test --test scheduler_authority_matrix
cargo test --test identity_m004_audit_foundation
cargo test --test identity_m005_audit_instrumentation
cargo test -p codegg-core --test identity        # CapabilitySet / narrowing
cargo test -p codegg-core audit::                # inline audit unit tests
```

Do not write `cargo test -p codegg-core authorization::` expecting coverage — it
matches nothing today.

## See Also

- `architecture/authorization.md` — authoritative per-operation matrix
- `architecture/audit.md` — audit trail contract
- `architecture/identity.md` — identity types behind attribution
- `architecture/projection.md` — how the matrix reaches frontends
- `.opencode/skills/permission/SKILL.md` — approval modes and sandbox
- `.opencode/skills/session-storage/SKILL.md` — `AuditStore` persistence

## Source verification

Re-verified 2026-10-06 against
`crates/codegg-core/src/authorization/{policy,authority,attribution}.rs`,
`crates/codegg-core/src/audit.rs`,
`crates/codegg-core/src/audit_instrumentation.rs`, `scripts/verify.sh:67-104`,
and `.github/workflows/ci.yml:117`. All descriptor/authority line numbers
re-confirmed. Corrected the guard coverage claim — only
`check_audit_coverage.py` runs in `verify.sh quick`/CI; the matrix, invariants,
and tokio-test guards are manual. Corrected `AuditWriterConfig` to `:1239`
and pinned the `FailClosed` default plus the `Backpressure` saturation path.
Claims without a traceable source were removed rather than guessed.
