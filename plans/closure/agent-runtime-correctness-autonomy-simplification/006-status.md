# Agent Runtime Correctness, Autonomy, and Simplification M006 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/agent-runtime-correctness-autonomy-simplification/006-prompt-compilation-and-control-policy-consolidation.md`
Source subsystem roadmap: `plans/subsystems/agent-runtime-correctness-autonomy-simplification-roadmap.md#7-ordered-milestones`
Repository baseline reviewed: `4cd004dba86230555728c0c6e884b802852addcb`
Implementation commits: `4cd004dba86230555728c0c6e884b802852addcb` — consolidate prompt compilation and control policy

## 1. Executive finding

M006 is closed. `PromptCompiler` is the sole production startup behavior-contract
composition path. Model-profile startup policy is represented as typed stable
compiler blocks and no longer mutates provider messages after compilation.
Dynamic controls retain their provider-compatible late-placement behavior.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| One startup compiler path | `DefaultTurnRuntime`, subagent worker, standalone CLI, and legacy context/snapshot helpers delegate to `PromptCompiler`; the old full assemblers were removed | Pass |
| Startup profile policy is consolidated | `ControlPolicy` blocks cover explicit tool calls, small patches, and capability-gated todo discipline; `apply_startup_profile_policy` and its tests are deleted | Pass |
| Resolved surface is authoritative | plan-mode contract is generated from the supplied resolved capability names; textual `Available tools` and `Using model` blocks are removed | Pass |
| Capability guidance is gated | planning surfaces require todo/goal tools; research requires a spawnable research agent and task tool; web guidance requires websearch | Pass |
| Fingerprint covers startup behavior | compiler hashes block kind, source, content hash, required flag, adapter, snapshot, and execution identity; profile test proves policy changes alter the fingerprint | Pass |
| Dynamic controls remain volatile | `push_control_instruction` remains in loop steering, recovery, compaction, notification, permission, and todo-reminder paths; no dynamic message is compiled into the stable startup prompt | Pass |
| Legacy loaders do not diverge | deprecated cwd/context/snapshot loaders now resolve legacy content and delegate to one compiler helper; standalone CLI uses compiler directly | Pass |

## 3. Production implementation evidence

Before M006, startup behavior came from `PromptCompiler` plus a separate
`apply_startup_profile_policy()` message mutation, a hard-coded plan-mode tool
inventory, textual tool/model labels, verbose backend enumeration, and parallel
legacy prompt assembly.

After M006:

- `src/agent/prompt.rs` owns profile policy, capability gating, plan-surface
  wording, deterministic ordering, and fingerprint inputs.
- `crates/codegg-core/src/model_profile/policy.rs` owns only dynamic message
  placement and deduplication.
- `src/agent/loop.rs` no longer applies startup policy after compilation.
- `src/main.rs` and subagent/runtime paths use the compiler.
- Obsolete built-in prompt text and the old full prompt assembly path were
  removed; compatibility loaders are thin delegation boundaries.

Deleted or merged prompt content:

| Old content | Disposition | Rationale |
|---|---|---|
| Tool-use startup mutation | merged into `profile:explicit-tool-contract` | stable behavioral delta and fingerprinted |
| Small-patch startup mutation | merged into `profile:small-patches` | stable behavioral delta and fingerprinted |
| Todo startup mutation | merged into capability-gated `profile:todo-discipline` | retained only when todo surface exists |
| Hard-coded plan tool list | replaced by resolved-surface list | prevents advertising unavailable tools |
| `Available tools` text | deleted | provider schemas are authoritative |
| `Using model` text | deleted | descriptive label has no behavior contract |
| Web backend/key enumeration | deleted | behavior rule is sufficient and safer |
| Duplicate identity line | merged into role contract | identity and role have one owner |
| Parallel built-in/legacy prompt assembly | deleted/delegated | prevents prompt drift |

Retained guidance is intentional: harness safety, role/output contracts,
profile behavioral deltas, web-use behavior, research spawning when actually
available, skills, project instructions, runtime assets, and dynamic controls.

## 4. Verification executed

All results are local verification:

- `rtk cargo test -p codegg --lib agent::prompt --no-default-features` — 23 passed.
- `rtk cargo test -p codegg-core --lib model_profile::policy` — 4 passed.
- `rtk cargo check -p codegg --no-default-features` — passed.
- `rtk scripts/verify.sh quick` — passed, including formatting, generated-agent
  checks, core boundary, sandbox/execution guards, and workspace all-targets
  check.
- `rtk git diff --check` — passed before the implementation commit.

The focused semantic tests cover deterministic ordering, profile-specific
guidance, plan-mode capability derivation, research/web gating, duplicate
block diagnostics, and fingerprint changes when startup policy changes.

## 5. Invariant review

Prompt compilation remains pure: no network or process-global CWD is used by
the compiler. Runtime-asset snapshots and execution identity remain explicit
inputs. Stable, slow-changing, and volatile classes remain separated; profile
policy is stable, while runtime controls remain late/volatile. Provider models
that avoid late system messages continue to use the existing user/system
placement rules for dynamic controls.

## 6. Failure and recovery review

No scheduler, execution, cancellation, or restart authority changed. Failure
to resolve a capability still follows the existing resolved-tool-surface path;
the compiler receives the resulting surface and does not guess a static list.
Recovery, steering, compaction, notifications, permissions, and todo reminders
remain turn-local controls.

## 7. Migration and compatibility review

No storage or protocol migration is required. Config instructions, agent
prompts, skills, project instructions, and model-profile semantics remain
supported. Prompt fingerprints change as expected because startup content is
now consolidated; no persistent state depends on the previous fingerprint.
Deprecated prompt-loader APIs remain callable, but their assembly is now a
thin compiler delegation and their cwd discovery remains isolated to the
deprecated boundary.

## 8. Security review

No permissions or tool execution authority was weakened. Removing textual tool
inventory reduces the chance of advertising a capability absent from provider
schemas. Plan-mode wording uses the resolved surface, and retrieved web
content is explicitly treated as untrusted evidence. Existing provider,
permission, and broker authorities remain unchanged.

## 9. Documentation and operations

Updated `architecture/agent.md`, `architecture/cache-aware-context.md`, and
`architecture/provider.md` to document compiler ownership, cache identity, and
dynamic message placement. No new static string-scanning guard was added.

## 10. Unresolved findings

- Critical: none.
- High: none.
- Medium: none.
- Low: none.

## 11. Roadmap disposition

M006 is closed. M007 and M008 remain independently ready. M009 is not
unblocked because it still requires M007 and M008 in addition to the already
closed M001–M006 set.

## 12. Registry updates

- Removed M006 from blocked/active closure work and recorded it under recently
  closed implementation plans.
- Updated M006's roadmap status to `closed`.
- Audited all registered downstream dependencies: no future plan became ready
  from M006 alone; M009 remains blocked on M007 and M008.
