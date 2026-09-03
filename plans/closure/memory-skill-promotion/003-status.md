# Memory-to-Skill Promotion M003 — Closure Status

Status: closing

Source implementation plan:

- `plans/implementation/memory-skill-promotion/003-approved-skill-publication-and-refresh.md`

Source subsystem roadmap:

- `plans/subsystems/memory-skill-promotion-roadmap.md#13-milestones`

Implementation revision:

- `081ae51` — `feat: publish approved skill proposals`

M002 dependency revision:

- `583c2702ad45fa28401f5c10bc0a0b6e19fa73fb` — strictly closed in
  `plans/closure/memory-skill-promotion/002-status.md`

## 1. Executive finding

M003 production work is complete and satisfies the approved publication
boundary. A user-only TUI command publishes one exact validated proposal
revision and digest into a host-derived CodeGG-owned project or global skill
root. The model-facing proposal tool has no approval or publication action.
Publication is atomic, durable, locked, fail-closed on path and collision
hazards, records proposal/habit provenance, and invokes the existing daemon
asset refresh path. Refresh retains immutable active-turn snapshots and
previous generations on failure.

The closure review has no unresolved critical, high, or medium finding in
M003 scope. The final close transition is pending this record's acceptance.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Explicit user approval, exact ID/revision/digest | `/skill-proposal publish <id> project|global` parses a closed scope enum; `start_skill_publish` captures current proposal revision/digest; publisher performs CAS checks | pass |
| Model cannot self-issue approval | `skill_proposal` remains submit-only and DirectOnly; publication service is called only from the TUI command path | pass |
| CodeGG-owned roots only | project resolves to `<project>/.codegg/skills`; global resolves to `<config>/codegg/skills`; no foreign-root write API exists | pass |
| Containment and symlink safety | canonical explicit bases, non-symlink owned roots/packages/destination, safe single-component normalized names, root lock, final package/destination inspection | pass |
| Revalidation and generated restrictions | current proposal is reloaded and parsed under the shared portable parser and publication restriction layer before write and again immediately before rename | pass |
| Collision and no-force policy | existing destination must be a regular file with the exact published identity/digest for an idempotent retry; different content returns `SkillAlreadyExists` without overwrite | pass |
| Atomic/durable/concurrent write | per-root `flock`; same-directory `create_new` temporary; mode 0600 on Unix; `write_all`, `sync_all`, atomic rename, containing-directory sync | pass |
| Proposal/habit provenance and reconciliation | `PublishedSkillRef` stores proposal, scope, normalized name, relative path, digest, timestamp; `HabitStore::mark_promoted`; `reconcile` verifies digest and never rewrites | pass |
| Asset refresh and pinning | successful TUI publication completion calls existing daemon-owned `start_refresh_assets`; existing coordinator tests prove new generations and old active pins | pass |
| Refresh failure retention | existing coordinator invalid/cancelled refresh tests prove prior generation retention; publication leaves the file in place and the TUI surfaces refresh diagnostics | pass |
| Effective/precedence truth | registry remains discovery/precedence authority; foreign effective skills require same-revision preview and are reported as `shadowed_by` | pass |
| Existing compatibility | existing registry, promotion, and refresh paths remain intact; no foreign source or protocol schema was changed | pass |

## 3. Production implementation evidence

- `src/skills/publish.rs` owns the host/TUI-only publication service. It
  accepts `SkillPublicationRequest` with proposal ID, expected revision,
  expected digest, and `SkillTargetScope`; it never accepts an arbitrary
  filesystem path.
- `src/skills/promotion.rs` adds additive serde-default publication and
  preview provenance, locked CAS publication metadata, and the habit-root
  seam. Existing proposal validation and bounded privacy behavior remain
  authoritative.
- `src/tui/app/mod.rs`, `src/tui/commands/memory.rs`, and
  `src/tui/runtime/command_dispatch.rs` add the explicit publish command,
  bounded asynchronous completion, and the existing daemon refresh request.
- `architecture/skills.md` and `architecture/memory.md` document target
  roots, authority, atomicity, collision behavior, reconciliation, refresh,
  and active-turn pinning.
- `tests/skill_publication.rs` covers project publication, global publication
  with foreign precedence, stale approval, collision non-overwrite, symlink
  rejection, idempotent retry, and reconciliation.

## 4. Verification executed

Passing focused verification:

- `cargo fmt --all -- --check`
- `git diff --check`
- `cargo test -p codegg-core habit --locked` — 5 passed
- `cargo test --test skills_registry --test habit_skill_promotion --locked` —
  24 registry tests and 16 promotion tests passed
- `cargo test --lib agent::asset_refresh --locked` — 6 passed
- `cargo test --test skill_publication --locked` — 6 passed
- `scripts/verify.sh quick` — passed, including generated-agent checks, core
  boundary, sandbox, execution ownership, and capped workspace all-target
  checking
- `check_daemon_cwd_usage.py`, `check_project_agent_pwd_inference.py`,
  `check_discovery_invariants.py`, `check_scheduler_bypass.py`,
  `check_identity_path_usage.py`, `check_tui_project_authority.py`,
  projection guards, `check_websocket_bounds.py`, and
  `check_git_forbidden_patterns.py` — passed

The focused root tests and quick gate used the installed arm64 Rust toolchain
with an isolated target directory because the default Homebrew x86_64 linker
cannot link against the host's arm64 MacPorts `liblzma`/`libiconv`. The
matching arm64 run completed successfully; it emitted only the existing large
`__eh_frame` linker warning.

## 5. Unresolved findings and scope disposition

The following are unrelated pre-existing repository findings and were not
modified by M003:

- `scripts/check_project_catalog_invariants.py` reports that the code's
  `STORAGE_LAYOUT_VERSION` is 49 while that guard still expects 48.
- `scripts/check_tool_broker_boundary.py` reports the existing direct
  structured call at `src/tool/review.rs:216`.
- A standalone `cargo clippy --lib --locked -- -D warnings` invocation
  reports existing `suspicious_open_options` findings in
  `crates/codegg-core/src/memory/habit.rs:408` and `:552`; M003 does not
  touch those files. The required quick gate's workspace check passed.

The full workspace test sweep and opt-in LSP-real-server/all-features paths
were not run, per the repository's test-resource budget and because M003 does
not change those surfaces. No new ADR, CI lane, provider call, foreign-root
writer, migration, or force-overwrite behavior was introduced.

## 6. Registry updates and downstream unblock audit

The registry's `Blocked work` section and all affected dependency graphs were
searched for plans naming memory-to-skill M003 as a hard or interface
dependency. No registered future plan depends on M003. M001 and M002 are
already strictly closed; the memory-to-skill roadmap has no later registered
milestone. Therefore no future plan was unblocked and no other plan status
was changed.

M003 creates no corrective follow-up: the explicit approval, CodeGG-owned
writer, provenance, reconciliation, refresh, precedence, and pinning
boundaries are all implemented and covered by the evidence above.

## 7. Final disposition

Pending final registry acceptance, this is a strict closure: M003 has no
unresolved in-scope critical/high/medium defect, its hard M002 dependency is
strictly closed, and the subsystem can move to `closed` with this record.
