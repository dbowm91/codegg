# Team Collaboration Corrective M002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/team-collaboration-corrective/002-project-channel-chat-access-policy.md`

Source subsystem roadmap:

- `plans/subsystems/team-collaboration-corrective-addendum.md#M002`

Repository baseline reviewed: `758f0573`

Implementation commits or pull requests:

- `2f4bec32` — feat(chat): project/channel chat access policy (team-collaboration M002)

## 1. Executive finding

M002 is complete. The accepted ADR-0006 chat access overlay is
durable, revisioned, and enforced at every chat entry point: a Viewer
can be explicitly granted project or single-channel chat without
receiving write authority, a Contributor can be denied project or
channel chat without losing unrelated capabilities, restricted
channels are neither enumerable nor distinguishable to denied
members, and structured actions still require their semantic
capabilities. No denied request produces a channel/message/policy
mutation or event, and no frontend state grants authority.

## 2. Requirement-to-evidence matrix

| Plan requirement (§6) | Evidence |
|---|---|
| Revisioned durable chat policy: project principal override allow/deny, channel mode `inherit_project\|restricted`, channel principal override allow/deny; foreign/stale channel IDs fail closed | `crates/codegg-core/src/collaboration/policy.rs` (`CHAT_POLICY_SCHEMA_STATEMENTS`, `get/set_project_override`, `set_channel_policy`); `migrate_v64` (storage layout 64); `set_channel_policy` rejects unknown/foreign channels with `UnknownChannel` → `chat_channel_not_found` |
| Chat authorization establishes active membership/read scope before policy; `project.chat` stays the role baseline, not the only gate; centralized `effective_chat_access(project, channel?, principal)` on every entry point | `policy::effective_chat_access` (pure) + `effective_chat_access_for` (membership-first async); daemon `authorize_chat_request` gate (membership → overlay → allow/deny); handler `chat_access_allows` rechecks on ensure/list/history/send/edit/redact/read/composing/sync/action paths; `ChatChannelList` filters through the same resolver |
| Core policy inspection/mutation under `member.manage` with optimistic revisions and privacy-safe errors; policy events carry structural IDs/revisions only | `ChatPolicyGet/List`, `ChatProjectPolicySet`, `ChatChannelPolicySet` (`CoreRequest`), `ChatPolicy/ChatPolicyList` (`CoreResponse`), `ChatPolicyChanged{project_id, channel_id?, revision}` (`CoreEvent`); `operation_descriptor` maps all four to `member.manage`; stale writes → `chat_policy_conflict` with zero overwrite; events classified `Safe` in `safe_publication.rs` |
| Structured actions pass both chat access and action-specific capability checks | `handle_chat_action_submit` rechecks `chat_access_allows` before the per-kind `action_capability_allows` (`agent.delegate`/`job.submit`/`session.read`); test `structured_action_denied_despite_chat_allow_without_semantic_cap` proves no action row is created |
| Channel enumeration filtering with no restricted-channel leak | `ChatChannelList` handler filters via `chat_access_allows`; tests `viewer_one_channel_allow_*`, `channel_deny_*_restricted_allowlists`, `denied_channel_lookup_*` assert exact visibility sets and indistinguishable denial shapes |
| Audit metadata for policy changes, TUI-compatible DTOs | `audit_metadata_for_policy_change` (ids/revision/actor only); `ChatProjectPolicyDto`/`ChatChannelPolicyDto`/`ChatPolicyOverrideDto` serde shapes with no content or secrets |

## 3. Production implementation evidence

- New: `crates/codegg-core/src/collaboration/policy.rs`
  (precedence resolver, revisioned store with CAS + idempotent
  converge, `effective_chat_access_for` with membership-first
  fail-closed semantics, structural audit metadata, 2 unit tests
  pinning the ADR-0006 truth table).
- Store: `collaboration/store.rs` (`ensure_collaboration_tables`
  chains policy statements); `session/schema.rs` `migrate_v64`;
  `storage/mod.rs` layout `63 → 64`.
- Protocol: `crates/codegg-protocol/src/core.rs`
  (`ChatPolicyDecisionDto`, `ChatChannelModeDto`,
  `ChatPolicyOverrideDto`, `ChatProjectPolicyDto`,
  `ChatChannelPolicyDto`; 4 requests, 2 responses, 1 event).
- Authorization: `authorization/policy.rs` (4 `member.manage`
  descriptors + 4 representative requests; matrix guard still 5/5).
- Daemon: `src/core/daemon.rs` (`authorize_chat_request` gate with
  project-level ensure, any-visible-channel list, and
  channel-specific data paths; `chat_access_allows` + `chat_policy_denied`
  rechecks; 4 policy handlers with server-side project resolution;
  action dispatcher chat recheck); `daemon_family.rs` (4 new Chat
  routes); `authorization_denial` covers all 4 policy ops as
  `chat_private`.
- Events: `ChatPolicyChanged` published on every project/channel
  mutation; `safe_publication.rs` classifies it `Safe` (ids/revision
  only; receivers re-fetch through authorized get/list).
- Docs: `architecture/collaboration.md` (M002 overlay section +
  test table), `architecture/authorization.md` (overlay paragraph),
  `architecture/protocol.md` (policy variants),
  `architecture/storage.md` (v64 entry).
- Tests: `tests/collaboration_m002_chat_policy.rs` (13 boundary
  tests, see §4).

Resolver truth table (pure `effective_chat_access`, pinned by unit
test `resolver_truth_table_matches_adr0006`): Viewer deny /
Contributor+ allow with no rows; inactive membership denies before
lookup; project allow replaces Viewer baseline; project deny blocks
Contributor; channel deny wins over project allow; Viewer
one-channel allow; restricted resets default to deny with explicit
allowlist restoring access; inherit preserves the project decision;
channel allow over project deny.

Authorization matrix changes: 4 new rows (`chat_policy_get`,
`chat_policy_list` → `member.manage`; `chat_project_policy_set`,
`chat_channel_policy_set` → `member.manage`), all `DirectProject`.
No existing row changed capability or scope.

## 4. Verification executed (commands + results; local vs CI truthfully)

Local (this machine, `CARGO_BUILD_JOBS=1`):

- `cargo test -p codegg-core --lib collaboration --locked -- --test-threads=1` — 19 passed (17 pre-existing domain + 2 new resolver/parse tests).
- `cargo test --test collaboration_m001_chat --locked -- --test-threads=1` — 12 passed (role-default compatibility unchanged).
- `cargo test --test collaboration_m003_chat_actions --locked -- --test-threads=1` — 10 passed (non-escalation intact).
- `cargo test --test collaboration_m002_chat_policy --locked -- --test-threads=1` — 13 passed:
  `viewer_default_deny_contributor_default_allow`,
  `viewer_project_allow_grants_chat_without_write_authority`,
  `viewer_one_channel_allow_scopes_to_single_channel`,
  `contributor_project_deny_blocks_chat_without_losing_other_caps`,
  `channel_deny_overrides_project_allow_and_restricted_allowlists`,
  `revoked_member_with_stale_allow_is_denied_immediately`,
  `denied_channel_lookup_and_cross_project_probe_are_indistinguishable`,
  `policy_admin_requires_member_manage`,
  `concurrent_override_cas_conflicts_and_duplicates_converge`,
  `restart_preserves_policy_and_reconnect_reevaluates`,
  `structured_action_denied_despite_chat_allow_without_semantic_cap`,
  `read_markers_and_composing_follow_effective_policy`,
  `foreign_channel_policy_probe_fails_closed`.
- `python3 scripts/check_authorization_matrix.py` — 5/5 pass.
- `bash scripts/check-core-boundary.sh` — pass.
- `python3 scripts/check_project_catalog_invariants.py` — 7/7 pass (layout 64 tracks `migrate_v64`).
- `cargo fmt --all -- --check` — pass; `git diff --check` — pass.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — pass.
- `cargo clippy -p codegg --all-targets --locked --features server,plugins,lsp-test-support -- -D warnings` — pass.
- `scripts/verify.sh quick` — pass.

Substituted verification (documented, not weakened): the plan lists
`cargo clippy --workspace --all-targets --all-features`. Per
`AGENTS.md`, workspace sweeps never use `--all-features` (it drags
real-server tests); the two clippy invocations above cover the same
code under the supported feature sets (`verify.sh full` uses
`--features server,plugins,lsp-test-support`). No finding suppressed.

CI truthfully: only local evidence is claimed here; CI (`verify`
job) will re-run the canonical guards.

## 5. Invariant review

- Active membership is mandatory: `effective_chat_access_for`
  returns `false` for missing/non-active memberships and disabled
  principals before any policy row is read; override rows cannot
  resurrect revoked/suspended membership (pinned by revocation test).
- Chat free text remains inert: no handler parses bodies; the only
  execution seam is the explicit typed action operation behind two
  capability checks.
- Chat grant never satisfies structured-action capabilities: the
  dispatcher requires chat access AND the per-kind semantic
  capability; viewer-with-chat agent-task denies with zero rows.
- Role defaults unchanged with no policy rows: Viewer denied,
  Contributor+ allowed, channels inherit (pinned by default test +
  untouched m001 suite).
- Enumeration privacy: list filters through the same resolver;
  direct denied/unknown/foreign lookups return indistinguishable
  not-found shapes (gate `project_not_found`, handler
  `chat_channel_not_found`; cross-project probe test asserts
  equality).
- Daemon truth: gate + handler rechecks evaluate current durable
  state per request; reconnect re-evaluates without cached
  authority; frontend hints never grant access.

## 6. Failure and recovery review

- Stale administrative revisions return `chat_policy_conflict`
  (`RevisionConflict`) with zero overwrite (pinned).
- Duplicate identical set/clear requests converge without a
  revision bump (pinned for both project and channel paths).
- Membership revocation wins immediately at request time, no
  reconnect needed (pinned).
- Reconnect re-evaluates policy against current rows (pinned via
  restart + grant-revoke-list sequence on the same pool).
- Policy storage unavailable for a team principal fails closed
  (gate and handler map errors to denial, never to role-default
  allow).
- Channel deletion has no delete path in this milestone; foreign or
  stale channel IDs fail closed at both resolution layers and never
  address a different channel identity.
- Concurrent sends/retention/composing semantics from M001 are
  untouched (m001 suite green).

## 7. Migration and compatibility review

Additive `migrate_v64` (`IF NOT EXISTS` tables + indexes, no
backfill, no role changes). Pre-policy databases open cleanly with
empty policy state and behave exactly as before. Rollback drops the
binary: v64 tables sit unused. Older clients keep ordinary chat
when authorized (role defaults preserved) and simply cannot
administer policy (new variants are additive; unknown variants fail
closed at the serde layer). `STORAGE_LAYOUT_VERSION` 63 → 64 tracks
the highest migration (catalog guard 7/7).

## 8. Security review

Cross-boundary matrix proven: viewer/project/channel grants and
denies, restricted allowlist, revocation race, list filtering,
direct-lookup indistinguishability, cross-project probe equality,
foreign-channel admin probe, `member.manage` admin matrix
(maintainer/contributor/viewer denied, owner allowed), CAS
conflict, restart durability, structured-action non-escalation,
read-marker/composing parity. Overrides carry principal IDs and
decisions only — never message content or secrets. Denials carry no
project/existence signal beyond the operation/capability names the
caller supplied. `eggsentry`/adversarial suites were not re-run
beyond the focused suites; no new network, crypto, or
secret-handling surface was added.

## 9. Documentation and operations

- `architecture/collaboration.md`: M002 overlay section (schema,
  precedence, administration, events, DTO reuse) + updated test
  table.
- `architecture/authorization.md`: overlay paragraph replacing the
  static-only chat description (baseline vs gate, filtering,
  `member.manage` administration, dual denial shapes).
- `architecture/protocol.md`: policy request/response/event
  paragraph.
- `architecture/storage.md`: v64 migration entry.
- `architecture/identity.md`: unchanged (policy targets canonical
  `ProjectId`/`ChannelId`/`PrincipalId`; no new identity).
- No TUI admin commands land here by design: M003 owns the `/team`
  administration surface and consumes these DTOs; M005 consumes the
  same DTOs for selected-project rendering.

## 10. Unresolved findings (severity: critical/high/medium/low)

None. No critical/high/medium findings remain. Note: TUI team
administration (`/team` policy editing) and Workspace
selected-project chat rendering are explicitly out of scope and
owned by M003/M005 respectively; the protocol DTOs are shaped for
both.

## 11. Roadmap disposition

M002 closes the chat-policy capability. Per the addendum dependency
graph, M003 (team administration) and M005 (Workspace selected-project
chat) may now proceed — both list M001+M002 as hard dependencies
with M001 already closed. M004 remains independently ready. M006
still requires M001–M005. The subsystem roadmap stays `active`
with M002 marked closed.

## 12. Registry updates

- `plans/implementation/team-collaboration-corrective/002-project-channel-chat-access-policy.md`:
  `ready` → `implemented` (closure at this record;
  implementation `2f4bec32`).
- `plans/registry.md`: M002 `ready` → `closed` (closure at this
  record); M003 and M005 audited for unblock (see below) and moved
  to `ready`; M004 stays `ready`; M006 remains `blocked` on
  M001–M005.
- Subsystem roadmap
  `plans/subsystems/team-collaboration-corrective-addendum.md`:
  M002 `ready` → closed in the milestone table via this closure.

Dependency audit: M003
(`003-team-membership-and-token-administration.md`) requires
M001+M002 (M001 closed, M002 closed here; consumes accepted
ADR-0006 overlay) — all hard deps now closed, so it becomes
`ready`. M005 (`005-workspace-selected-project-chat-view.md`)
requires M001+M002 (same) — becomes `ready`. M004
(`004-shared-session-controller-lease.md`) requires only M001 —
already `ready`, unchanged. M006 requires M001–M005 (M003/M005
still open → stays `blocked`). No corrective follow-up is
registered: no new defect was found that M002 itself must fix.
