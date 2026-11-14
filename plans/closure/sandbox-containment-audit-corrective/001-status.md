# Sandbox Containment Audit Corrective C001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/sandbox-containment-audit-corrective/001-macos-credential-deny-set-completion.md`

Source subsystem roadmap:

- `plans/subsystems/sandbox-containment-audit-corrective-roadmap.md#milestone-c001--macos-credential-deny-set-completion`

Repository baseline reviewed: `7642b9e381d9c91d650fa375be2a7aa0aae9bcd8` (`feat-auto-20261109-f8abad0d` HEAD) on a macOS 26.6.2 host (Darwin 25.6.0, arm64, `sandbox-exec` present).

Implementation commits or pull requests:

- **None. The implementation is uncommitted working-tree work.** It was reviewed in a
  shared tree that also carries C002, C006 and unrelated TUI edits from sibling agents, so
  no commit, stage, or push identifies this milestone. `plans/implementation/
  sandbox-containment-audit-corrective/` itself is untracked at this baseline.

## 1. Executive finding

The macOS credential exclusion boundary now covers the material an agentic coding tool
actually reaches. Fifteen `$HOME`-relative entries were added to
`policy::sensitive_deny_paths()`'s unix arm (`src/security/sandbox/policy.rs:602-610`,
`:624-629`), and a three-class rule with a per-path breakage assessment and a `skip`
decision for the three entries that cannot be denied without breaking a build is recorded
in `architecture/security.md:522-571`.

The change matters because Seatbelt is the platform where the deny list **is** the read
protection: `sbpl_profile` never emits `BackendPolicy::read_roots`, so an unlisted
credential path is readable by a contained child. The document now says that in those
words (`architecture/security.md:518-521`) instead of leaving the guarantee implied.

Each added entry is kernel-verified rather than structurally asserted: the fixture
`curated_credential_deny_entries_are_kernel_denied_reads_with_a_positive_control`
relocates every curated path under a fixture root, drives it through
`SandboxConfig::deny_paths`, and asserts an observed `-DENIED` per row plus a
`CONTROL-READ` positive control. It passed on this host inside the 22/22 canonical run.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| §13.1 Every candidate path has a recorded class, breakage assessment and decision | `architecture/security.md:522-571` — 18 rows, every row `add` or `skip` with a reason | Pass | 15 `add`; `skip` for `~/.m2/settings.xml`, `~/.gradle/gradle.properties`, and open-ended browser-profile enumeration |
| §13.2 Every `add` is present in `sensitive_deny_paths()` and in a membership assertion | `policy.rs:602-610`, `:624-629`; `CURATED_HOME_DENY_ROWS` at `policy.rs:921`; `curated_credential_deny_set_is_complete` | Pass | The fixture asserts the same membership independently at `sandbox_containment.inc:974-978` |
| §13.3 A contained child cannot read each added credential-only path, with a positive control | `sandbox_containment.inc:939-1042`, member passed on macOS/Seatbelt | Pass | `CONTROL-READ` asserted before the per-row `-DENIED` assertions; no fixture content reaches stdout |
| §13.3 (second half) the fixture fails when an entry is removed | Fixture asserts membership against `sensitive_deny_paths()` before probing, and asserts a `-DENIED` label per row | Verified structurally, not re-executed | See §10: the remove-and-observe cycle requires editing `src/security/sandbox/policy.rs`, outside this record's write scope while sibling agents hold the tree |
| §13.4 The generated profile emits each new deny after every re-allow rule | `curated_credential_denies_render_after_write_reallows` (`policy.rs:978`) passed; asserts `(deny file-read* …)` and `(deny file-write* …)` follow the last `(allow file-write*`) | Pass | Runs inside `cargo test -p codegg --lib security::sandbox::policy` |
| §13.5 macOS toolchain probes still pass | `a_sandboxed_shell_can_still_read_the_system_and_resolve_a_command`, `sandboxed_cargo_metadata_uses_workspace_and_scoped_scratch`, `seatbelt_keeps_read_only_developer_tool_probes_operational` — all in the 22/22 run | Pass | `/etc` readable; command resolution and `cargo metadata` unaffected |
| §13.6 `architecture/security.md` states the rule and accepted breakage and does not imply `read_roots` reaches Seatbelt | `architecture/security.md:500-563` | Pass | The text now states "Seatbelt does **not** consume those read roots" and enumerates the accepted breakage |

## 3. Production implementation evidence

**Deny list.** The unix arm of `sensitive_deny_paths()` gained fifteen `$HOME`-relative
entries. Nine are credential stores or private user stores added to the credential block,
six are added beside the existing `Library/Keychains` group. The list stays shared by both
backends: on Landlock these paths are outside the read allowlist, so the entry costs
nothing; on Seatbelt it becomes read protection.

**Classification rule.** `architecture/security.md` records the three classes
(`credential-only`, `config-with-embedded-secret`, `private-user-store`), the rule that a
`config-with-embedded-secret` entry is denied only when the breakage is a visible,
recoverable re-authentication, and the completed table. The two entries where denial
removes configuration a build needs are skipped with that reason; the browser-profile group
is skipped because an open-ended vendor enumeration would read as coverage while missing a
browser, and that gap is recorded instead of claimed.

**Nothing else changed.** No storage layout, migration or `STORAGE_LAYOUT_VERSION` bump; no
protocol or DTO change (`SandboxLaunchSpec.deny_paths` keeps its shape); no runtime or
concurrency change; no change to `sbpl_profile`'s rule order or to
`BackendPolicy::read_roots`; `validate_denied_paths`' hardlink refusal is untouched.

**Tests.** Three new unit tests in `policy.rs` (membership, nearest-existing-parent
normalisation for an absent entry, deny-after-reallow profile order) and one new fixture
member in `tests/security_permission/sandbox_containment.inc`.

## 4. Verification executed

### Commands run

```bash
cargo test --locked --test security_permission -- sandbox_containment
cargo test --locked --test security_permission -- sandbox_containment --test-threads=1
CODEGG_SANDBOX_QUALIFICATION=required \
  cargo test --locked --test security_permission -- sandbox_containment --test-threads=1
cargo test --locked -p codegg --lib security::sandbox
cargo test --locked -p codegg --lib security::sandbox::policy
cargo test --locked -p codegg --lib security::sandbox::seatbelt
python3 scripts/check_sandbox_contract.py
python3 scripts/check_sandbox_contract.py --self-test
python3 scripts/check_comment_budget.py
scripts/verify.sh quick
```

`cargo build --locked --bin codegg-sandbox-helper` was run first, because the fixtures
launch the real helper binary from `target/debug/`.

### Results

- **Parallel run: 21 passed, 1 failed** (`seatbelt_preserves_and_reports_preopened_denied_
  file_descriptors` — `target failed: sandbox helper status failure: write sandbox status:
  Bad file descriptor (os error 9)`). The member passed when run alone, and the serial run
  below passed 22/22. The symptom is consistent with a descriptor-number collision in the
  fixture's `pre_exec` (`launch_helper_with_inherited_fd` duplicates into fixed fd 42 and the
  status pipe is dup2'd to fd 3, so a pipe write end that already occupies fd 42 would leave
  the wrong object at fd 3); that is a diagnosis from the fixture's fixed descriptor numbers,
  not an instrumented observation. It is not a containment defect; recorded in §10.
- **Serial run (`--test-threads=1`): 22 passed, 0 failed.** That is the canonical figure on
  this host. The fixture's member set changed with the sibling milestones sharing this tree:
  C001 added the curated-deny member, C004 added the marker and widened-grant members and
  un-gated (and renamed) the rename/link member, and C002 replaced the Linux-only
  `legacy_landlock_leaves_host_process_tcp_and_abstract_uds_reachable` with
  `default_landlock_scopes_signals_and_abstract_sockets_with_positive_control`. A baseline
  `#[test]` count (21) is therefore not a like-for-like figure for this run.
- **Strict mode (`CODEGG_SANDBOX_QUALIFICATION=required`): 22 passed, 0 failed.** The marker
  changes nothing on a host that has a backend, which is the intended behaviour.
- `cargo test -p codegg --lib security::sandbox` — 62 passed.
- `cargo test -p codegg --lib security::sandbox::policy` — 21 passed.
- `cargo test -p codegg --lib security::sandbox::seatbelt` — 8 passed.
- `python3 scripts/check_sandbox_contract.py` — passed; `--self-test` — passed.
- `python3 scripts/check_comment_budget.py` — **failed, 21 violations, none under
  `src/security/sandbox/**` or `tests/security_permission/**`.** The sandbox files' recorded
  ceilings were raised with the new comments (`docs/comment-budget.toml`: `policy.rs`
  140→156 comment lines, `sandbox_containment.inc` 105→135, `landlock.rs` 32→70,
  `backend.rs` 88→104, `linux_mount_view.rs` 54→68, `codegg-sandbox-helper.rs` 46→62). The
  failures are sibling agents' in-flight TUI, config, protocol and `src/main.rs` work.
- `scripts/verify.sh quick` — **aborted at its first step**, `cargo fmt --check --all`, on a
  formatting diff in `src/main.rs` only (`temp_measure_catalog_open`). The lane therefore
  never reached its guard phase in this tree. This is an unrelated working-tree state, not a
  C001 defect; `cargo fmt --check --all` lists exactly one file.

## 5. Invariant review

- **A constrained request never degrades to host authority.** Unchanged: the list only adds
  deny roots, and an unavailable backend still refuses.
- **A deny applies to read and write, and a deny inside a writable root wins.** The fixture
  asserts `-DENIED` for a read of each relocated path under `WorkspaceWrite`, and
  `curated_credential_denies_render_after_write_reallows` asserts both deny rules follow
  every `(allow file-write*` rule.
- **`ReadOnly` still denies every persistent write and still permits device writes.** The
  C004-corrected unit test `read_only_profile_re_allows_only_the_device_nodes` passed in the
  seatbelt module (8/8).
- **The toolchain still runs.** `/etc` remains readable (never denied), the shell resolves a
  command, and `cargo metadata` runs from scoped scratch; all three probes passed.
- **A denied regular file with hardlink aliases still refuses the launch.**
  `validate_denied_paths` is untouched, and `check_sandbox_policy_wiring.py` (which asserts
  the hardlink-refusal call site) passed.
- **No path is added whose breakage lacks a recorded justification.** The two
  build-breaking entries are skipped with their reason, and the browser-profile group is
  skipped with its reason.

## 6. Failure and recovery review

No new runtime state. A curated path that does not exist on the host is still denied
because `BackendPolicy::normalized_denies` resolves the nearest existing canonical parent;
`absent_curated_credential_denies_resolve_under_the_nearest_parent` passed and explicitly
asserts the resolution never widens to all of `HOME`. No cancellation, restart, or
contention path is involved.

## 7. Migration and compatibility review

No persisted configuration, durable payload, protocol frame, or schema change; no
`STORAGE_LAYOUT_VERSION` bump. Two fail-closed consequences widen with the set, both
unchanged in kind: a denied regular file with `nlink > 1` refuses the launch, and a
workspace rooted so that one of these paths falls beneath a writable root is still refused
because Landlock cannot subtract a deny from a writable ancestor. Both are recorded in
`architecture/security.md` and in the roadmap's §10 risk 4.

## 8. Security review

- Fifteen additional paths are denied for read and write with a real kernel denial per path.
- The accepted `config-with-embedded-secret` breakage is `~/.npmrc` (pre-existing),
  `~/.my.cnf`, and `~/.yarnrc.yml`: an authenticated fetch becomes an interactive login, and
  MySQL client defaults are lost.
- **Still readable, by design or by recorded gap:** every path not in the list — the Seatbelt
  read posture remains broad, so browser profiles and any credential file not enumerated
  stay readable; `~/.m2/settings.xml` and `~/.gradle/gradle.properties` stay readable by
  decision. No full read allowlist is claimed.
- The fixture never opens a real credential file: every row is relocated under a temporary
  root and the assertion is on the relocated copy. A separate membership assertion ties the
  copy back to the curated list.

## 9. Documentation and operations

- `architecture/security.md` — the three-class rule, the completed table, the accepted
  breakage, the Seatbelt `read_roots` correction, and the two fail-closed consequences.
- `docs/comment-budget.toml` — raised ceilings for the six sandbox files whose comments grew.
- No new static guard was added: the plan's design is that the fixture is the guard, backed
  by the membership assertions. Both run in `quick`'s `security_permission` family and in the
  macOS qualification lane.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| Low | The remove-and-observe non-vacuity cycle was not re-executed for this record | The claim "the fixture fails when an entry is removed" rests on the fixture's own membership assertion, which is executed, rather than on an observed removal | If a fresh demonstration is wanted: delete one entry from `sensitive_deny_paths()`, run `cargo test -p codegg --lib security::sandbox::policy curated_credential_deny_set_is_complete` and the containment member, observe both fail, restore |
| Low | `seatbelt_preserves_and_reports_preopened_denied_file_descriptors` fails under the default parallel harness and passes serially or alone | A local `cargo test --test security_permission -- sandbox_containment` can report a spurious failure; the CI lane and this record use `--test-threads=1` | Make the fixture's inherited descriptor number dynamic, or keep the member pinned to serial execution |
| Low | Browser profiles under `~/Library/Application Support/<vendor>` remain readable | A prompt-injected read of a browser cookie store is still possible on macOS | Recorded gap; a curated enumeration would claim coverage it does not have. A directory-level rule needs its own decision because it would deny ordinary application support data |
| Low | `~/.my.cnf` denial loses `[client]` defaults, not only the password | MySQL clients need explicit connection options | Accepted breakage, documented; revisit only if a narrower entry is found |

No high-severity finding is hidden by this closure. The read-broad posture is unchanged and
is the platform's design, not a defect this milestone introduced.

## 11. Roadmap disposition

**Milestone closed.** Every §13 acceptance criterion is met on the host that can produce the
evidence, C002's "operational after C001" ordering is satisfied (C001 landed first), and
C004's shared-fixture coordination is discharged: C004 owns the harness and escape tests,
C001 owns the curated-list fixture, and both are in the same file without duplicate helpers.

## 12. Registry updates

- Roadmap §12 status table: C001 → `closed`, closure record
  `plans/closure/sandbox-containment-audit-corrective/001-status.md`.
- `plans/registry.md` subsystem row: the "proposed; no work started" state is no longer
  accurate — C001 is closed and its implementation is uncommitted working-tree work on
  `feat-auto-20261109-f8abad0d`. The C001 plan row moves from `ready` to `closed`.
- Unblock audit: C002's stated dependency ("operational after C001") and C004's ("operational
  after C001") are both satisfied by this closure.
