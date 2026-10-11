# Cross-Agent Skill Compatibility M005 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/cross-agent-skills-compatibility/005-maintenance-skills-and-validation.md`

Source roadmap: `plans/subsystems/cross-agent-skills-compatibility-roadmap.md`

Repository baseline reviewed: `11e18b0797b2290f7e1d7c0f12c301466e98c03f`

Implementation commits: `fb63817439f9bd470c01e61f84cd49193f9bca86`,
`bfa5bcd001f297500e6cf6c36bcba984c0212f04`, and
`9f55a80c5c15208a25e0705f67b2c4ceaaa331d2`.

## 1. Executive finding

M005 is closed. Four source-checked maintenance guides cover testing/CI,
tool-execution ownership, security hardening, and LSP/IDE maintenance. Existing
cross-links now use canonical `.opencode/skills` paths, and human-shell guidance
matches the implemented one-shot command contract. The first-party guard checks
frontmatter and folder names, indexed guides, relative links, source
registration, and the two skill-directory aliases; its negative fixture is
checked as part of the guard.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Four targeted maintenance guides | `.opencode/skills/{testing-ci,tool-execution,security-hardening,lsp-ide}/SKILL.md` | pass |
| Accurate execution and human-shell statements | `.opencode/skills/human-shell/SKILL.md`; execution-owner references | pass |
| Canonical first-party links and index | `AGENTS.md`; `.opencode/skills/*/SKILL.md` | pass |
| Bounded source-aware guard and negative fixture | `scripts/check_first_party_skills.py` | pass |
| Appropriate verification integration without a new lane | `scripts/verify.sh quick` | pass |

## 3. Verification executed

- `rtk python3 scripts/check_first_party_skills.py` — passed.
- `rtk cargo test -p codegg --lib skills:: --locked` — 48 passed.
- `rtk cargo test --test skills_registry --locked` — 29 passed.
- Snapshot and TUI focused regressions — 3 passed total.
- `rtk cargo fmt --all -- --check` — passed.
- `rtk cargo clippy --workspace --all-targets --locked -- -D warnings` — passed.
- `rtk bash scripts/verify.sh quick` — passed.

## 4. Security and findings

The guides grant no authority, and the guard makes no network requests or broad
repository scan. Vendor execution metadata remains inert. No blocking findings
remain. Native Windows/macOS runs and a hosted CI run were not available for
this branch; local Linux quick verification, Clippy, and focused tests passed.

## 5. Roadmap disposition

M001–M005 are closed. No successor milestone is unblocked or required for this
line; all closure records are under
`plans/closure/cross-agent-skills-compatibility/`.
