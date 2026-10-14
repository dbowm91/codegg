# Windows Native Sandbox C042 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/windows-native-sandbox/042-typed-windows-guarantee-and-negotiation-contract.md`
Source subsystem roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0027-capability-stratified-successor-handoff-post-pr-111-merge-2026-10-13`
Repository baseline reviewed: `edde2bebe`
Implementation commits: `edde2bebe` — feat(windows-sandbox): C041 mainline CI/build guard, C042 guarantee contract, C044 two-hop harness, C047 coverage audit

## 1. Executive finding

C042 is closed as internal invariant infrastructure. The new pure
contract in `src/security/sandbox/guarantee.rs` types requested
requirements vs observed capabilities across read/write/network/Job/
host-mutation dimensions, with exact `Unknown`/`NotRun`/`Unavailable`/
`Failed`/`ObservedDenied`/`ObservedEnforced` semantics, and refuses
before any untrusted target starts. Strict profiles are never satisfied
by broad reads, `Unrestricted` network never satisfies offline,
unknown/downgraded tiers never satisfy the no-host-mutation
prohibition, test-only candidates never report production `Enforced`,
and parent ceilings hold. No public profile was added (ADR-0028 stays
proposed), no backend registered, no Unix behavior changed.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Inventory + typed requirement/observed-capability model | `RequestedGuarantees`, `AttestedCapabilities`, `CapabilityFact`, `SatisfiedContract`, `Refusal` in `guarantee.rs` | pass | Secrets/SIDs/paths not serialized beyond bounded provenance |
| Pure negotiation, immutable request, proof-not-grant | `negotiate()` + `verify_helper_backend()`; 10 unit tests | pass | All fail-closed; never `FullHost` fallback, never child broadening |
| No partial broad-read satisfaction of strict profiles | `broad_read_allowed` hard refusal test | pass | Both `ReadOnly` and `WorkspaceWrite` covered |
| Network separation | Offline requires proven WFP fact; `Unrestricted` refuses | pass | Matches `SandboxEnforcement` truthful reporting |
| Tier/fallback prohibition | Unknown/downgraded/unproven-mutation refusals | pass | No vendor-label inference |
| Unix status/config preserved | No `SandboxConfig`/status-frame changes; existing sandbox tests green | pass | Internal-only addition |
| No public broad-read profile without ADR-0028 | No `SandboxProfile` variant, no serde/config change; `child_sandbox_allowed` reused | pass | Guard + code review |
| Docs updated after code | `architecture/security.md` + `architecture/process-tool-execution-ownership.md` paragraphs | pass | Internal contract only |

## 3. Production implementation evidence

- `src/security/sandbox/guarantee.rs` (new, ~300 lines + 10 tests):
  `CapabilityFact` (7 variants with `is_proven`), `RequestedGuarantees`
  (+ `for_profile`, `with_parent_ceiling`, `with_network_offline`),
  `AttestedCapabilities` (+ `tier_is_known`), `SatisfiedContract`,
  `Refusal`, `negotiate()`, `verify_helper_backend()`.
- `src/security/sandbox.rs`: `pub mod guarantee;` + re-exports; no
  other changes.
- Docs: one paragraph each in `architecture/security.md` (negotiation
  rules, ADR-0028 not authorized) and
  `architecture/process-tool-execution-ownership.md` (future Windows
  gate, no fallback/ceiling broadening).
- No storage migration, no protocol/DTO, no runtime dispatch, no
  frontend, no public schema change. `codegg-core` boundary untouched
  (only consumes existing `SandboxProfile`/`child_sandbox_allowed`).

## 4. Verification executed

### Commands run

```bash
cargo fmt --all
git diff --check
python3 scripts/check_sandbox_contract.py
python3 scripts/check_adr_integrity.py
python3 scripts/check_execution_ownership.py
./scripts/check-core-boundary.sh
cargo check -p codegg --lib --locked
cargo test -p codegg --lib security::sandbox::guarantee -- --nocapture
cargo test -p codegg --lib security::sandbox::backend -- --nocapture
```

### Results

Local (darwin, `edde2bebe`): fmt applied clean; all listed guards
passed including core-boundary; `cargo check` exit 0 (2m35s);
guarantee 10/10 passed; backend 4/4 passed. Focused approval/
permission/tool-execution suites beyond sandbox were not re-run
separately — the change adds a new module with no callers, so Unix
regression risk is confined to compilation + existing sandbox suites,
both green. Windows native capability-probe run is not applicable
(no backend to probe) and recorded as not-run.

## 5. Invariant review

- Profiles/ceilings: `child_sandbox_allowed` enforced inside
  `negotiate`; strict→broad denied by test; FullHost never inferred.
- Fail-closed: every unproven fact refuses with a named dimension;
  `verify_helper_backend` refuses empty/mismatched/test-only claims.
- Unix containment, helper status machine, approval/audit semantics
  unchanged (no call-site edits).
- No host mutation authorized: `forbid_host_mutation` defaults true
  and requires known tier + proven no-DACL-mutation path.

## 6. Failure and recovery review

Pure function: no I/O, no state, no cancellation/restart/contention
semantics. Malformed input (empty backend, FullHost-with-requirements,
unknown tier) refuses deterministically. No persistence, leases,
events, or artifacts.

## 7. Migration and compatibility review

No migration. Old serialized configs parse as before (no serde
changes). Internal Windows helper status versioning untouched. No
removal or fallback path.

## 8. Security review

Authorization unchanged (no new authority). No secrets handled
(provenance is host/SDK metadata only). Path validation unchanged.
Privilege boundary preserved (negotiation grants nothing; launcher
must still verify private status). DoS bounds unaffected (bounded
strings via existing core truncation). Audit benefits: future
backends must produce an exact contract or a named refusal.

## 9. Documentation and operations

Updated `architecture/security.md` and
`architecture/process-tool-execution-ownership.md` as above. No
operator diagnostics, installer, or static-guard changes required
(new module needs no guard; existing sandbox guard still passes).
Handoff: C043 may now consume the stable contract; C047 audit uses it
as its gate.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | No live-backend integration exercise | Contract has no production caller yet (by design) | C043/C044 qualify against it on a Windows host; C048 admits |
| low | Approval/permission/tool-execution suites not re-run beyond sandbox | Negligible (no callers changed) but unrun | Next broad sweep covers them |

No medium/high/critical findings.

## 11. Roadmap disposition

Milestone closed. C042 was the hard dependency for C043 (contract)
and C047 (audit): C043's contract gate is now satisfied (host gate
still blocks — see C043 closure); the C047 audit slice is closed
herewith. No public broad-read work is unblocked (ADR-0028 still
proposed). C045/C046/C048 remain blocked on their separate ADRs and
positive-backend gates.

## 12. Registry updates

Same registry/roadmap updates as C041 §12 (single closure commit for
the line). Implementation plan `042-*.md` status: `ready` →
`implemented; closed`. Unblock audit: C042 satisfies C043's hard
contract dependency and C047's audit hard dependency; C043 stays
blocked (operational host), C047 audit slice closes, nothing else
newly ready.
