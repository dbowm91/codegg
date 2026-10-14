# Windows Native Sandbox C053 — Closure Status

Status: closed (negative source/ABI gate; native re-probe not authorized)
Source implementation plan: `plans/implementation/windows-native-sandbox/053-mxc-api-delta-and-strict-reentry-gate.md`
Source subsystem roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md`
Repository baseline reviewed: `39f149a15ef529d66787d1d11551872ea22ec384`
Implementation commits: `162c0c53` — close C053 MXC source gate

## 1. Executive finding

The current published Rust SDK release remains `mxc-sdk 1.0.0` (tag
`v1.0.0`, release commit `7bf2102`). The typed public Rust request API does
not expose a caller-controlled `fallback.allow_dacl_mutation` field, and the
typed `ProcessContainerConfig` has no such member. The raw versioned JSON
contract can represent `fallback.allowDaclMutation=false` and the internal
dispatcher honors it, but CodeGG's C043 adapter uses the typed API. No newer
published SDK/ABI delta was found that also addresses C043/C044 child entry,
handle transfer, descendants, or teardown. The strict re-entry gate fails;
no Windows rerun is authorized by C053.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Pin current versioned Rust SDK and release | `cargo info mxc-sdk@1.0.0`; Cargo registry source at `mxc-sdk-1.0.0`; Microsoft releases page lists v1.0.0 as latest on review date | Pass; no later published SDK release found |
| Inspect typed ProcessContainer API | `src/configs/process_container.rs`: `ProcessContainerConfig` contains learning, capabilities, capture, UI, filesystem and network only | No public typed fallback control |
| Inspect versioned raw fallback contract | v1.0.0 exact contract, parser and dispatcher contain `allow_dacl_mutation`; dispatcher refuses DACL fallback when false | Raw JSON has the field; not reachable through the typed CodeGG adapter without changing integration surface |
| Require both no-DACL and launch/handle/lifecycle deltas | No later release/ABI delta found; historical C043/C044 gaps remain | Gate fails; native re-probe not run |
| Preserve historical negative closure records | No edits to C038 or C043/C044 closure files | Pass |

## 3. Production implementation evidence

No CodeGG production or test-support code changed. No MXC backend was selected
or registered. The C043/C044 negative results remain authoritative for their
tested revisions.

## 4. Verification executed

- `cargo info mxc-sdk@1.0.0` — resolved published crate metadata and source.
- Local source inspection confirmed the typed API has no fallback field and
  the raw v1.0.0 parser/dispatcher handles the JSON property.
- Microsoft release listing reviewed 2026-10-14; v1.0.0 is shown as latest.
- Windows native tests — not run, because the source delta gate failed.
- `python3 scripts/check_adr_integrity.py` — passed; active ADR IDs are unique and local paths resolve.
- `python3 scripts/check_sandbox_contract.py` — passed.
- `git diff --check` — passed.

## 5. Invariant review

No production path, profile, schema, installer, network claim, or fallback
behavior changed. Absence of a typed control is not treated as permission to
rely on the raw JSON contract or to assume fallback is disabled.

## 6. Failure and recovery review

The source gate stopped before native setup. No machine state, DACL, token,
process, or fixture was changed by this milestone.

## 7. Migration and compatibility review

No CodeGG or MXC dependency migration was made. Reconsider only when a
published SDK exposes the fallback control through the intended integration
surface and a concrete launch/handle/lifecycle change addresses the independent
C043/C044 stop conditions.

## 8. Security review

The raw schema's opt-out is not sufficient evidence for CodeGG's typed adapter.
The pinned typed request defaults must not be assumed safe. No host-object
mutation or child containment claim is made.

## 9. Documentation and operations

The source result is recorded here and in the roadmap/registry. Source links:
<https://github.com/microsoft/mxc/releases> and
<https://github.com/microsoft/mxc/blob/main/docs/schema.md>. The exact SDK
source reviewed is the crates.io `mxc-sdk 1.0.0` package, matching the
Microsoft `v1.0.0` release (`7bf2102`).

## 10. Unresolved findings

- Medium: a typed Rust SDK control for refusing host-DACL fallback is still
  absent from the integration surface used by C043.
- Medium: C043/C044 child startup, inherited/explicit handle boundaries,
  descendant proof and teardown remain unqualified. C053 did not authorize a
  repeat run.

## 11. Roadmap disposition

C053 closes negative on source/ABI evidence. No native retry is opened. C045,
C046, C047 production integration, and C048 remain separately gated.

## 12. Registry updates

Mark C053 closed negative with this record. The dependency audit unblocks
nothing: C054 still requires C049, C050, C052 and this source disposition; C051
still requires a positive C050 write-confinement result.
