# Windows Native Sandbox C043 — Closure Status

Status: blocked
Source implementation plan: `plans/implementation/windows-native-sandbox/043-mxc-basecontainer-managed-process-adapter-feasibility.md`
Source subsystem roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0027-capability-stratified-successor-handoff-post-pr-111-merge-2026-10-13`
Repository baseline reviewed: `edde2bebe`
Implementation commits: none (no adapter code landed; C042 contract `edde2bebe` is the enabling predecessor)

## 1. Executive finding

C043 is blocked without implementation. Its hard C042 contract
dependency is now satisfied (C042 closed at `edde2bebe` with a stable
pure negotiation gate), but its operational gate — a pinned eligible
serviced Windows 11 host with measurable BaseContainer deny-path
fidelity — is unavailable in this environment (darwin, no Windows
host, no MXC SDK run). Per the plan's stop conditions, no adapter
skeleton was fabricated without a host to measure it against, and no
production registration occurred. Windows `SandboxRequest::Required`
stays unavailable/fail-closed.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| C042 stable contract | `042-status.md`, `src/security/sandbox/guarantee.rs` at `edde2bebe` | pass (predecessor) | Hard dependency now satisfied |
| Pinned SDK/runtime/tier + MSRV-neutral isolation | not run | blocked | No eligible Windows 11 x64/ARM64 host in this environment |
| `fallback.allowDaclMutation=false` schema validation | not run | blocked | Requires actual pinned SDK schema on host |
| ManagedProcessService-compatible adapter | not implemented | blocked | Deliberately not stubbed without measurement |
| Native child/matrix/handle/lifecycle evidence | not run | blocked | C033/C035–C038 historical evidence preserved, not reused as proof |
| No host DACL mutation fallback | unproven | blocked | Would fail the C042 host-mutation prohibition until proven |

## 3. Production implementation evidence

None. No source, config, storage, protocol, runtime, frontend, or docs
changes for C043. The C042 contract it must consume exists and is
closed; the adapter itself does not.

## 4. Verification executed

### Commands run

```bash
git diff --check
python3 scripts/check_adr_integrity.py
```

### Results

Local: tree clean for C043 scope (no C043 files to verify); guards
pass. No Windows execution attempted — honest `not-run`, never `pass`.
No exact-SHA native run exists for C043 and none is claimed.

## 5. Invariant review

All invariants preserved by inaction: no silent BaseContainer
selection because another harness uses MXC; no DACL fallback assumed;
no MSRV uplift introduced; no production `Required` path; C011/M004–
M006, C015–C018, C030/C031, C029/C034/C038 dispositions unchanged.

## 6. Failure and recovery review

Not applicable (no adapter, no runs, no fixtures created or leaked).

## 7. Migration and compatibility review

None (no changes).

## 8. Security review

No new authority, surface, or secret handling. Blocking rather than
stubbing is the secure disposition: an unmeasured adapter cannot
become a confused-deputy launch path.

## 9. Documentation and operations

No operator surface. This closure record is the documentation: future
implementers start from C042 `negotiate()` + the C043 plan's work
packages A–E on a pinned eligible host.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | No eligible Windows 11 host/SDK run | Adapter cannot be built or measured here | Provision pinned serviced Windows 11 tier + MXC v1.0.0 and execute C043 A–E |
| low | MXC fallback-knob schema unverified | Unknown whether `allowDaclMutation=false` exists in the pinned API | Inspect actual pinned schema on host; stop if absent/ambiguous |

Blocking is the correct status, not a defect in prior work.

## 11. Roadmap disposition

Milestone blocked. C043 remains registered as blocked, now with its
blocker narrowed: hard C042 satisfied, operational eligible-host gate
outstanding. It does not block C044 (parallel), C047 audit (closed
via C042), or C041/C042 (closed). C045/C046/C048 are unaffected.

## 12. Registry updates

Covered by the single line-closure commit (see C041 §12): C043 row
stays `blocked` with updated note (C042 satisfied at `edde2bebe`;
awaiting eligible Windows 11 host + pinned SDK measurement). No
unblocking results from this closure. Implementation plan `043-*.md`
status stays `blocked`.
