# Execution Reliability, Approval, and Autonomy M007 — Yolo, Automatic, and Full Host User Surfaces

Status: blocked

Repository baseline: `18365458f881f6ac4524c9ea05224b69923faa4f`

Source roadmap:

- `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md#7-milestones`

Long-term requirements:

- `plans/000-long-term-specification.md#1-product-definition`
- `plans/000-long-term-specification.md#4.4-frontends-render-projections`
- `plans/000-long-term-specification.md#8-deployment-profiles-and-authentication`
- `plans/000-long-term-specification.md#29-system-invariants`

Applicable ADRs:

- `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`

Primary class: capability / polish

Hard dependencies: M003-M006 plus M004 preference convergence.

## 1. Objective

Expose the completed approval/sandbox architecture as a low-friction, frontend-neutral user feature: Interactive, Automatic, and Yolo approval modes; ReadOnly, WorkspaceWrite, and explicit FullHost sandbox profiles; durable last-used preference; accurate effective-state rendering; and risk-proportionate warnings/confirmation. Refine remembered approvals toward capability scope where practical without redesigning the underlying rule system.

## 2. Why this milestone is blocked

The UI must not invent semantics ahead of the daemon. ApprovalRouter/persistence, durable model preferences, actual sandbox enforcement, and Automatic reviewer must close first.

## 3. Current implementation evidence

- TUI has permission prompt UX and session/model selection state, but no canonical approval/sandbox selector.
- config supports PermissionConfig and built-in review/debug/docs modes, which are not equivalent to approval modes.
- `with_exec_mode()` is a special headless permissive path, not an interactive Yolo product mode.
- M003-M006 will provide frontend-neutral effective policy state and safe reviewer/sandbox semantics.
- current PermissionChoice supports AllowOnce/AlwaysAllow/DenyOnce/AlwaysDeny and persistent decisions are keyed broadly by tool/path; some approvals can be made more precise using command/effect/capability metadata without replacing the rule language.

## 4. Invariants that must not regress

- UI displays daemon-resolved effective state, not merely the user's requested preference;
- Automatic/Yolo never imply FullHost;
- FullHost never activates by silent fallback;
- explicit deny/project/admin/parent ceilings remain enforceable and visible as constraints;
- dangerous warnings are clear but do not force confirmation on every command after mode selection;
- preference persistence is daemon-owned and frontend-neutral;
- mode changes do not retroactively alter pending/in-flight authorization;
- no frontend stores credentials or becomes the source of security truth.

## 5. Scope

### In scope

- protocol/CLI/TUI selection and status display for approval and sandbox profiles;
- persisted last-used mode/profile application and override precedence;
- warning/confirmation UX for Yolo and especially Yolo+FullHost;
- status line/panel/help explaining filesystem versus network containment;
- headless flags/config mapping to the same daemon policy contract;
- conservative migration/refinement of broad remembered approvals toward command/effect/capability scope when exact metadata exists;
- test/docs/help text.

### Explicitly out of scope

- new sandbox backend;
- reviewer logic changes except UX/fallback configuration;
- team policy administration UI;
- generic policy-rule editor;
- mandatory repeated confirmations after a mode is already selected;
- silent conversion of legacy AlwaysAllow into broader capability grants.

## 6. Required production changes

### Core/protocol

Finalize frontend-neutral operations such as:

```text
RuntimePolicyGet
RuntimePolicySet { approval_mode?, sandbox_profile? }
RuntimePolicySnapshot { requested preference, effective mode/profile, enforcement, ceilings, reviewer availability }
```

Names may follow current protocol conventions. Settings update returns the new effective snapshot and a revision.

### TUI

Add a discoverable selector/command/key path showing both dimensions, e.g.:

```text
Approval: Interactive | Automatic | Yolo
Sandbox:  ReadOnly | WorkspaceWrite | FullHost
```

The visible effective state should distinguish requested preference from a stricter project/parent/host constraint.

Recommended warnings:

- Yolo + WorkspaceWrite: explain that approval prompts are skipped for escalations but explicit denies and configured sandbox remain enforced; agent may modify/delete workspace files and run commands without asking.
- FullHost (any permissive approval mode): explain that CodeGG filesystem containment is disabled and the process has the OS user's host authority; network may also be unrestricted.
- Yolo + FullHost: strongest warning plus explicit second confirmation/typed confirmation according to TUI conventions. Once selected, do not re-prompt for each action.

Do not use patronizing copy or pretend FullHost is safe.

### CLI/headless

Map CLI flags/options to the same `ApprovalMode`/`SandboxProfile` contract; avoid parallel booleans with different semantics. Existing exec permissive behavior becomes a compatibility alias/deprecation path where appropriate.

### Automatic status

If reviewer is unavailable, show Automatic as unavailable/degraded and apply M006 fallback; do not silently display Automatic while routing as Yolo.

### Capability-scoped remembered approvals

Audit current broad `AlwaysAllow(tool,path)` decisions. Where deterministic parsing already yields structured command/effect scope, add an additive decision key/fingerprint such as tool + canonical path + normalized capability/command family. Goals:

- allowing `cargo test` need not imply all Bash;
- allowing a specific remote mutation should not imply arbitrary shell;
- filesystem expansion can be scoped to exact canonical root.

Do not attempt perfect shell semantics. Legacy broad decisions remain readable and may be displayed/managed conservatively.

### Model display

Show restored last model/connection from M004 as daemon state, not TUI manifest preference, alongside policy state where helpful.

### Documentation

Update user-facing README/config/help examples and security docs.

## 7. Ordered work packages

### Work package A — Protocol/CLI surface

Land canonical get/set/effective policy and map headless flags to it.

### Work package B — TUI selector/status/warnings

Implement mode/profile selector, effective-state rendering, reviewer availability and risk-confirmation flow.

### Work package C — Preference restore UX

On new session/startup, show restored model/mode/profile and any fallback because preference became invalid/unavailable.

### Work package D — Remembered-approval precision

Add capability-scoped keying only where existing structured command/effect data makes it deterministic; retain compatibility.

## 8. Failure, cancellation, restart, and contention semantics

- failed preference update leaves previous effective policy and reports error;
- two frontends update via revision/CAS and stale update is rejected/reloaded;
- restart applies stored preference but recomputes host sandbox/reviewer availability and project ceilings;
- FullHost confirmation is tied to the requested revision/change, not a generic permanent bypass token;
- cancelling a mode-change dialog changes nothing;
- pending permission uses its captured old snapshot.

## 9. Compatibility and migration

- existing config/default remains Interactive unless project explicitly chooses another documented default;
- `with_exec_mode()`/legacy CLI flags map to the new semantics with deprecation/help text as appropriate;
- TUI manifest remains display-only;
- legacy permission decisions load conservatively;
- existing sessions keep explicit model selection.

## 10. Required tests

### Focused unit tests

- warning/confirmation mapping for each approval×sandbox combination;
- effective-vs-requested rendering;
- CLI flag mapping;
- capability decision key normalization/bounds.

### Integration tests

- Interactive + WorkspaceWrite prompt path;
- Automatic + WorkspaceWrite reviewer path;
- Yolo + WorkspaceWrite skips escalation prompt but remains sandboxed;
- Interactive/Automatic/Yolo + explicit Deny remains denied;
- Yolo + FullHost requires strong confirmation then operates without per-action prompts;
- Automatic reviewer unavailable is visibly degraded/deferred, never Yolo.

### Restart and recovery tests

- last mode/profile/model restored after daemon/TUI restart;
- unavailable remembered model/profile enforcement yields clear fallback diagnostic;
- FullHost requested preference is restored only according to documented confirmation/persistence policy.

### Contention and cancellation tests

- simultaneous frontend changes;
- pending permission unaffected by concurrent mode toggle.

### Security and negative tests

- TUI cannot forge effective mode in local state;
- Yolo+WorkspaceWrite filesystem escape remains denied on supported host;
- capability-scoped cargo/test allow does not authorize unrelated destructive shell;
- child effective state shows stricter inherited ceiling.

### Migration and compatibility tests

- legacy exec/config flags;
- legacy permission decisions;
- old TUI manifest.

## 11. Required verification commands

```bash
cargo test --test permission
cargo test --test agent_loop_harness -- approval
cargo test --test session_selection
cargo test --test tui_project_sessions
python3 scripts/check_sandbox_contract.py
python3 scripts/check_core_boundary.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
./scripts/verify.sh quick
```

## 12. Documentation updates

- README user workflow section;
- `architecture/permission.md` and `architecture/security.md`;
- CLI help/config schema examples;
- TUI command/key documentation;
- provider/model preference docs.

## 13. Acceptance criteria

- user can deliberately select Interactive, Automatic or Yolo and ReadOnly, WorkspaceWrite or FullHost through frontend-neutral state;
- Yolo+WorkspaceWrite is practical autonomous mode without host-wide containment loss;
- FullHost is explicit and strongly warned;
- effective state is visible and survives restart;
- Automatic degradation is transparent;
- remembered approvals are no broader than legacy behavior and become more precise where structured data permits;
- no per-command warning fatigue after mode selection.

## 14. Stop conditions

Stop if UI implementation requires bypassing daemon policy, silently mapping Automatic to Yolo, weakening FullHost confirmation, or attempting a brittle full shell-language capability parser.

## 15. Closure evidence required

- protocol/CLI/TUI screenshots or deterministic render fixtures;
- approval×sandbox behavior matrix;
- warnings/confirmation text evidence;
- restart/preference evidence;
- capability-scope regression tests;
- legacy compatibility results;
- exact verification commands and residual UX limitations.

## 16. Handoff notes

Keep two dimensions visible. A single “permissions: high/low” slider would recreate the ambiguity this architecture is designed to remove.
