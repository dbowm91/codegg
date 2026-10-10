# Cross-Agent Skill Compatibility M005 — Closure Status

Status: closed

Source plan: `plans/implementation/cross-agent-skills-compatibility/005-maintenance-skills-and-validation.md`

Source roadmap: `plans/subsystems/cross-agent-skills-compatibility-roadmap.md`

Repository baseline reviewed: M004 closure at `plans/closure/cross-agent-skills-compatibility/004-status.md`.

Implementation commit: `d0164e95`.

## 1. Finding

M005 is closed. The repository now has focused `testing-ci`, `tool-execution`, and `security-hardening` skill packages; the human-shell guide describes the current local one-shot behavior accurately. `scripts/check_skill_guides.py` validates first-party guide metadata, portable names, local links, and registry symlink aliases, with a negative fixture.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Add targeted maintenance guidance | Three new packages under `.opencode/skills/` | pass |
| Remove aspirational human-shell claims | Updated `.opencode/skills/human-shell/SKILL.md` | pass |
| Validate guide identity and links | `scripts/check_skill_guides.py` | pass |
| Keep repository aliases aligned | Guard checks `.agents/skills` and `.skills` targets | pass |
| Run lightweight check in canonical quick | `scripts/verify.sh quick` output includes guard and passes | pass |

## 3. Implementation evidence

The guide packages have portable lowercase directory-matched names and real repository links. Testing guidance reflects `scripts/verify.sh`; tool-execution guidance points to `ToolBroker`/scheduler ownership; security guidance describes current deterministic sandbox and authority boundaries. No general scanner or installer was added.

## 4. Verification executed

- `rtk python3 scripts/check_skill_guides.py` — passed.
- `rtk scripts/verify.sh quick` — passed after the final source and TUI changes.
- `rtk cargo clippy --workspace --all-targets --locked -- -D warnings` — passed after the final TUI test.
- `rtk cargo fmt --all -- --check` — passed through quick verification.
- `rtk git diff --check` — passed before closure-document edits.

## 5. Guard negative fixture

The checker exercises a deliberately broken package/link fixture so a broken local link or mismatched package identity produces a finding. It also validates repository alias symlinks without requiring external network access.

## 6. Security and authority

Guides describe existing capabilities and boundaries. They add no runtime capability, permission, scheduler bypass, command execution, skill installation, or write surface. Quick verification's execution ownership, sandbox, and TUI-authority guards passed.

## 7. Maintenance scope

The guide set remains on-demand and repository-owned. Human-shell functional plans retain ownership of future locality/PTY behavior; this milestone changed guidance only.

## 8. Platform findings

The guide guard passed on Linux. No Windows/macOS or hosted CI run is claimed.

## 9. Documentation and indexing

Updated the canonical `AGENTS.md` skill index, `architecture/skills.md`, `docs/agents-skills.md`, and the on-demand skills module guide. The new guides link to source and project documentation that exists in this checkout.

## 10. Deviations and remaining findings

`lsp-ide` was not added: existing LSP guidance and ownership already cover the relevant maintenance surface, while the three requested gaps were directly addressed. The plan-template all-features Clippy command was replaced by repository-approved workspace/all-targets Clippy because all-features activates real-server tests. An additional run of `scripts/check_work_plan_repository_binding.py` failed on its pre-existing hard-coded expectation that `STORAGE_LAYOUT_VERSION` is 68; source is at 69 and this skill-only work made no storage changes. This unrelated guard drift is left for its owner. No unresolved M005 finding remains.

## 11. Dependency and registry disposition

M005 closed after M004. No downstream plan was unblocked. The cross-agent skills compatibility roadmap is terminal and all five closure records are linked from the registry.

## 12. Final disposition

M005 acceptance criteria are met. Closure accepted.
