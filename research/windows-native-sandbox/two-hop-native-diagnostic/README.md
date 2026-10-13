# C044 — Compiled native two-hop launch diagnostic (test only)

Status: research harness landed; Windows execution not run on this host.
Authority: ADR-0027 bounded test-only (one bounded helper-startup
correction permitted, then honest positive/negative/blocked closure).
C032 is the historical negative predecessor: PowerShell as the inner
restricted child was created/suspended/Job-assigned/resumed but never
reached user entry within 30 seconds. This harness replaces
PowerShell/.NET/loader inference with a tiny compiled native
first-instruction probe so token/identity, loader startup, and the
read-boundary result are independent test outcomes.

No production backend, no persistent account, no original-host ACL
mutation, no `SandboxRequest::Required` activation. ADR-0028 remains
proposed; a write-only outcome must never be promoted to strict
`ReadOnly`/`WorkspaceWrite`.

## Layout

| Path | Role |
|---|---|
| `Cargo.toml` | Isolated harness workspace (`[workspace]`, `publish = false`); Rust 1.89-compatible, no MSRV uplift. |
| `src/main.rs` | Disposable-host runner: first hop `CreateProcessWithLogonW` (dedicated standard user), second hop restricted own-primary-token via `CreateRestrictedToken` + `CreateProcessAsUserW`, Job/suspend/resume verification, nonce-bound entry proof, same-file broad-read baseline vs restricted read, stage/host write probes, cleanup. Windows-only launch code is `cfg(windows)`-gated; other hosts print an unqualified report and exit 0 without claiming anything. Portable manifest/nonce logic has unit tests that run anywhere. |
| `native_probe.c` | Minimal C first-instruction probe: emits the nonce-bound entry marker before any interpreter loads, reports token SID/restricting groups, then performs the ordered read/write probes. Pinned build: `cl.exe /O1 /GS /sdl native_probe.c` on the disposable host; hash pinned per run. No installer; the binary lives in the runner-owned stage directory only. |
| `RUN.md` | Exact disposable-host operator steps (consented VM only): create/remove the temporary local user, stage layout, run, collect JSON evidence, tear down. |

## Evidence protocol (JSON, one line per event)

`C044_REPORT={"schema":"codegg.two_hop_c044.v1","nonce":"…","phase":"entry|token|broad-read|restricted-read|stage-write|host-write|cleanup","pid":…,"result":"allowed|denied|error","detail":"…"}`

The runner verifies: actual child SID/token differs from the first hop,
same-file broad read allowed under the first-hop account, restricted
read denied under the restricted child, stage writes allowed, host
writes denied, no host descriptor drift (hash compare), deterministic
Job cleanup (descendant gone via OS process query after timeout/close).

If the child fails before the entry marker, the run stops and reports
loader/privilege/environment diagnostics — that is NOT a read-denial
result. One bounded helper-startup correction is permitted; afterwards
the milestone closes positive, negative, or blocked without another
verification loop.

## Stop conditions

- Child never reaches entry → diagnose bounded loader/privilege/env
  errors, close negative/blocked; do not count as read-denied.
- Limiting SID requires wide original-user ACEs or special host setup →
  close negative or request a separate ADR; no opportunistic invariant
  relaxation.
- Any persistent account, original-tree ACL grant, or production
  registration → out of scope, refuse.
