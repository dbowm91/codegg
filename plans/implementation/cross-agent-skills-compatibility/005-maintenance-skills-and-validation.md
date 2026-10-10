# Cross-Agent Skill Compatibility Milestone 005 — Codegg maintenance skills and conformance validation

Status: blocked (hard prerequisite: prior milestone closure)
Repository baseline: `578b62bd9580382e00fb19e096fc1794e1f5618b` (2026-10-09; re-audit before execution)
Source roadmap: `plans/subsystems/cross-agent-skills-compatibility-roadmap.md`, M005
Long-term references: `plans/000-long-term-specification.md#12-repository-asset-and-harness-interoperability`, `plans/001-terminology-and-domain-model.md`, `plans/003-planning-process.md`
Applicable ADRs: existing Runtime Assets contracts; ADR required only if public authority boundary changes.
Primary class: polish

## 1. Objective

Codegg maintenance skills and conformance validation, with complete verification and documentation for the named contract.

## 2. Why this milestone is not yet ready

Hard dependency: M004 accepted closure; the inspector must expose honest package diagnostics before packaging further advice. Draft skill prose may be reviewed earlier but do not mark ready without M004.

## 3. Current implementation evidence

Repository already owns 23 specialized maintenance skills in `.opencode/skills/*/SKILL.md`, aliased through `.agents/skills`, and indexes them in `AGENTS.md`. Important underrepresented disciplines include `scripts/verify.sh`/CI and test qualification, `ToolBroker`/scheduler tool execution ownership, and deterministic sandbox/security-hardening contracts. Existing `human-shell` describes several aspirational fields that should be distinguished from the currently implemented TUI shell; the separate human-shell implementation plans retain ownership of functional changes.

## 4. Invariants that must not regress

- Asset discovery/inspection is bound to the explicitly selected project/workspace and stable source provenance.
- Existing project/global precedence, passive discovery, foreign read-only policy and plugin namespacing remain intact.
- Permission grants, tool scripts and vendor execution metadata are not authorized by skills.
- Refresh remains transactional and pins in-flight turns to their captured snapshot/digests; no reads from ambient CWD.

## 5. Scope

**In scope:** only the production interfaces and fixtures below.

**Out of scope:** writing into foreign harness dirs, installing remote skills, general-purpose code search, a second plugin/tool runtime, live-provider CI and new CI lanes.

## 6. Required production changes

**Skill content:** add narrowly targeted `testing-ci`, `tool-execution` and `security-hardening` skill packages with factual source-verification sections and no implied invocation functionality. If present and justified by real source, add a foundational `lsp-ide` skill; otherwise record its deferral, not a stub. Reconcile existing high-impact skills' cross-links to canonical `.opencode/skills` paths, `human-shell` current-vs-future capability statements (without implementing the shell line), model invocation directions and ignored `version/tags/process` metadata. **Validation:** add a small source-aware fixture/guard (Rust test preferred if parser already has a harness, otherwise targeted Python script in `scripts/`) checking own skill package names/metadata, nonbroken first-party relative links and asset registration. No network, full-repo crawler, broken fixture overreach, or separate CI lane. **Docs:** update `AGENTS.md` skills index and architecture guidance, optionally provide external-portable skill examples without shipping a large mandatory distribution.

**Storage/migrations:** avoid persistent schema changes; any new serialized diagnostic/source fields must preserve existing consumers. **Protocol:** reuse existing project-asset DTO, refresh and query services. Add no unaudited file access via the TUI. **Runtime/concurrency:** no scan at render/keystroke time; maintain coalesced refresh and generation pinning. **Security:** bound all path, content, metadata and diagnostic output, redact potentially sensitive physical paths in public logs.

## 7. Ordered work packages

**A — Review source:** inspect canonical modules and tests for owned execution/test/security facts, avoid hard-coded volatile counts or docs line numbers. **B — Add packages:** two to four highly targeted on-demand skills, portable metadata, realistic commands and explicit safety boundaries; do not add demo-only generic skills. **C — Reconcile old packages:** canonical links, misleading activation descriptions, `human-shell` realized/TUI-only contract; stay out of external human-shell implementation branch changes. **D — Qualification:** parser discovery, AGENTS registration, no alias duplicates, relative-link/metadata guard self-test with deliberately broken fixture, focused CI.

## 8. Failure, cancellation, restart, and contention semantics

Missing optional skill dirs are not errors. A malformed candidate cannot eliminate a valid lower-precedence candidate. Races, cancellation and restart must preserve the last valid generation or return an actionable stale-generation status, never partially publish. Concurrent projects must not share active scoped assets or report another project's paths.

## 9. Compatibility and migration

Support existing native and portable skills, `skills.paths`, renamed/aliased global roots and plugin-contributed skills; document all new source ordering and any intentionally excluded vendor-specific behavior. No auto-migration or copying foreign files.

## 10. Required tests

**Validation:** every added package discovered exactly once, name/dir/frontmatter conformance, all primary first-party links resolve, corrupted test fixture fails deterministically, no global user dirs needed. **Semantic:** tests/guards do not advertise unimplemented shell, browser, IDE or vendor execution; model invocation is the actual `skill` tool. **Regressions:** existing 23 skills load, `AGENTS.md` index and `scripts/verify.sh quick` remain correct.

## 11. Required verification commands

~~~bash
cargo test --lib skills::
cargo test --lib tui::
cargo fmt --all -- --check
scripts/verify.sh quick
cargo clippy --workspace --all-targets --all-features -- -D warnings
~~~

Select exact focused integration test binaries after inspecting current Cargo targets; record actual exit statuses and broader qualification if protocol/frontend changes.

## 12. Documentation updates

- `AGENTS.md` index, `architecture/skills.md`, `docs/agents-skills.md`, and each added `.opencode/skills/<name>/SKILL.md`.

## 13. Acceptance criteria

- At least three source-verified maintenance guides close the highest-priority gaps without duplicating architecture docs.
- Canonical docs/links are consistent; one bounded verification path catches high-impact drift.
- No mandatory global skills installation or new runtime behavior is introduced.

## 14. Stop conditions

Stop if providing a guide requires pretending its capability exists, needs a functional implementation owned by another plan, or expands to a general skill marketplace/installer.

## 15. Closure evidence required

Create `plans/closure/cross-agent-skills-compatibility/005-status.md` with exact commits, effective-vs-shadow matrix, fixture/platform/security results, concurrency and pinned-generation checks, documents changed, commands actually run, unresolved findings and updated registry/roadmap gate. Do not close on compile-only evidence.

## 16. Handoff notes

Rebase/check current branch and preserve unrelated work; never modify the historical Runtime Assets M001–M004 closure records. This is a deliberately additive maintenance workstream.
