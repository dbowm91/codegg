# C038 — MXC evidence review and disposition

Date: 2026-10-13

Decision: **B — no candidate for a future production review from this evidence set.** This is a stop on the current MXC candidate, not a backend-admission decision. The native host and pinned API were available; the decisive issue is missing mandatory boundary evidence, not operational inability to run the probes.

## Evidence reviewed

- C033 baseline: `plans/closure/windows-native-sandbox/033-status.md` and its hosted evidence. Its tool and process observations remain bounded to the synthetic probes recorded there.
- C035 native helper and handle controls: `plans/closure/windows-native-sandbox/035-status.md`; hosted regression run `38427163860` passed the same-file ordinary inherited-handle positive and preserved the C035 negative. This does not qualify MXC transfer or containment.
- C036: `plans/closure/windows-native-sandbox/036-status.md`; exact implementation SHA `1803105fbf2434eaaf9ba673f77e2b124c4eb151`; [Windows run 38429625037](https://github.com/dbowm91/codegg/actions/runs/38429625037).
- C037: `plans/closure/windows-native-sandbox/037-status.md`; exact implementation SHA `1803105fbf2434eaaf9ba673f77e2b124c4eb151`; [Windows run 38429625036](https://github.com/dbowm91/codegg/actions/runs/38429625036).
- SDK source: pinned `mxc-sdk` 1.0.0, checksum `e4866ca589a68edc8e931eba58eed9253ad1ce670e974aedc4830a1d94bca6cd`, VCS commit `a846837d827b496fc4a08dfe60554515758c6647`. `ContainerRequest` exposes no arbitrary host-handle field; its native launcher explicitly passes standard I/O endpoints.

Both C036 and C037 ran on Windows 11 Enterprise 25H2, build 26200, UBR 9457, ARM64. Their workflow steps passed as test/evidence collection; the machine reports set `qualified_test_only=false`. A green workflow does not override the report qualification result.

## Boundary matrix

| Dimension | Status | Evidence and limit |
|---|---|---|
| Effective backend/tier | pass for observed run | C036/C037 selected ProcessContainer / BaseContainer on the listed host. No broader host or edition matrix. |
| Ordinary direct inherited handle | pass | C035 and C037 inherit-on controls proved same-file identity and read of the known synthetic bytes. |
| Ordinary inheritance disabled | pass | C037 negative control had no same-file handle. The broadly readable path remained readable, as expected for this ordinary control. |
| MXC actual child entry/token/stage | pass for observed child | C036/C037 reported nonce-bound child entry; C037 observed token/integrity and stage proof. |
| MXC direct synthetic host-path read | pass for tested operation | C036/C037 actual child received `ERROR_ACCESS_DENIED` (5); fixture content remained unchanged. This is not general filesystem enforcement qualification. |
| C036 capture pointer/provenance | pass | Terminal structured output metadata pointed to the stamped, run-owned capture file; bounded schema and summary validated. |
| Child-correlated block denial event | fail | C036 capture document had `totalDenials=0`, no event correlated to the denied child. No per-child telemetry qualification. |
| C036 timeout and cleanup | fail/incomplete | Child entered and timed out, but final report says `timeout_process_gone=false`; bounded cleanup was not confirmed. |
| C037 ambient numeric handle observation | bounded pass only | The numeric value was not a present same-file handle in the MXC child, and no secret was read. This does not prove an explicitly transferred valid handle would be denied. |
| Explicit arbitrary handle transfer | unavailable | Pinned public request API has no arbitrary handle field or supported safe test route. It is unsupported, not denied. |
| Descendant handle/secret boundary | fail/incomplete | Descendant process was created and reaped, but exited with `0xC0000008` before proof; `descendant_probe_result=null`. No descendant confidentiality claim. |
| SDK stdin/stdout/stderr | pass for those streams | C037 pipe write and stdout/stderr checks passed. This does not cover other IPC. |
| DuplicateHandle/cross-process authority | not run | No API/authority matrix. |
| Named pipes beyond SDK stdio | not run | No synthetic non-stdio named-pipe transfer matrix. |
| Inherited process/token handles | not run | Not exercised by these reports. |
| Cancellation, crash, restart, concurrent worktrees | not run | No lifecycle, recovery, or concurrency qualification. |
| Windows edition/build matrix | not run | Only the specific Windows 11 ARM64 host above. |
| Offline network/WFP egress | not run | No network enforcement evidence; no `network=Enforced` claim. |
| Stage-to-workspace promotion | not run | Not covered by these diagnostic harnesses. |
| Original host ACL/content integrity | pass for synthetic scope | Only disposable synthetic fixtures were granted; reports say fixture hashes/ACL setup remained stable and original host ACLs were not mutated. |

## Decision and stopping point

**No candidate.** C036 fixed the output-path provenance ambiguity but did not observe a child-correlated block event and did not confirm timeout-child cleanup. C037 demonstrates ordinary handle controls and a narrow child observation; its descendant probe is incomplete, its API does not support arbitrary explicit transfer, and several relevant IPC/authority paths were not tested. Together these results do not meet the evidence floor for recommending a separate production review. No positive independent security review is claimed or required for this negative decision.

The prior C033/C035 conclusions are preserved as written; this review does not promote their bounded observations into general claims. It does not assert a synthetic secret leak, nor does it convert absent evidence into proof of denial. It stops this candidate rather than scheduling another replay of the same probes.

Any future Windows production candidate requires a new explicit accepted decision and fresh, independently reviewed evidence that first addresses: correlated denial telemetry and bounded cancellation cleanup; an actual supported transfer or precise narrowed threat claim; verified descendant and alternate IPC behavior; lifecycle/restart/concurrency; supported host coverage; offline egress; and stage promotion. Until then, Windows `SandboxRequest::Required` stays unavailable/fail-closed, filesystem and network `Enforced` remain unclaimed, C011/M004–M006/C015–C018 stay blocked, and C030/C031 remain terminally blocked.
