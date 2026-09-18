# Command Surface Reconciliation M002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/command-surface-reconciliation-corrective/002-cli-surface-cleanup.md`

Source subsystem roadmap:

- `plans/subsystems/command-surface-reconciliation-corrective-addendum.md#5-milestones`

Repository baseline reviewed: `da7fab03`

Implementation commits or pull requests:

- `da7fab03` — reconcile CLI surface truthfulness and compatibility

## 1. Executive finding

M002 is complete. The default `codegg --help`, `codegg doctor --help`,
`codegg exec --help`, and `codegg daemon --help` surfaces are now
truthful for the compiled feature set; every documented example parses
as written; duplicated execution policy resolves through one shared
Clap struct plus the single `resolve_cli_policy` validation authority;
conflicting session-launch combinations fail at parse time; internal
transports no longer clutter the user surface while staying parseable;
and the orphan plugin CLI has an explicit removal disposition with a
guard test. No provider, sandbox, daemon, scheduler, authorization, or
permission semantics were changed.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Verbosity mapping documented/tested exactly (none→warn, `-v`→info, `-vv`→debug, `-vvv`→trace) | `verbosity_log_level` in `src/main.rs`; help text `no flag: warn; -v: info; -vv: debug; -vvv and above: trace`; `verbosity_mapping_matches_help` test | pass |
| `codegg doctor [subsystem]` positional syntax works | `Doctor.subsystem` positional `value_enum`; `doctor_positional_and_compat_flag_parse` + `documented_examples_parse_as_written` tests; live `doctor search` smoke | pass |
| `--subsystem` retained as hidden/deprecated compat spelling | `subsystem_flag` (`hide = true`) + deprecation stderr warning + differing-spellings rejection in `resolve_doctor_subsystem`; live `--subsystem search` smoke | pass |
| Enum reconciled with help; only real implementations advertised | Help `Run diagnostics for search, MCP, LSP, deterministic tools, providers, and credentials (default: all)`; stale provider/storage claim removed; `doctor_advertised_subsystems_match_enum` pins the 7-value set | pass |
| `providers` diagnostic is real (read-only, no network) | `print_providers_report` reuses config-aware `register_builtin_with_config` listing only; live `doctor providers` smoke | pass |
| `credentials` diagnostic is real (metadata only, never secrets) | `print_credentials_report` reuses `CredentialStore::at_default_location().list()` printing id/kind/account/expiry/scopes; live `doctor credentials` smoke | pass |
| `installation` deliberately NOT advertised | No variant; data owned by blocked self-contained-installation M002 per its §6 (`codegg doctor --help` cannot promise what that milestone has not landed); recorded in §7/`architecture` docs instead of empty help text | pass (deferred with owner) |
| Feature-aware root examples | `root_after_help()` with `cfg(feature = "server")` variants applied at runtime in `main()` and `cmd_completions`; default help shows no `server`/`attach`; `default_help_excludes_feature_gated_commands` + server-gated `all_feature_help_includes_gated_commands` / `server_examples_parse_as_written` tests; both builds smoked | pass |
| Shared execution-policy `Args` + one validation authority | `ExecutionPolicyArgs` flattened into root `Cli` and `Exec`; `resolve()` delegates to `policy_surface::resolve_cli_policy`; `execution_policy_uses_one_shared_authority` proves identical outcomes on both spellings; `--yolo`+different-mode still parses but is rejected at resolution (alias compat preserved) | pass |
| Explicit launch-mode conflicts, no last-branch-wins | `conflicts_with_all` across `continue_session`/`session`/`fork`/`no_session`; `session_launch_conflicts_fail_at_parse_time` covers all 6 conflicting pairs + 5 valid singles; live `-c -s` exits 2 | pass |
| One-shot surface: canonical format option + compat aliases | Root `--format` canonical with hidden `output-format` alias; `exec --format` canonical with `--json-output`/`-j` retained (OR semantics via `resolve_output_format`); `output_format_compat_aliases` test | pass |
| Root one-shot honors its output/quiet flags | `run_single_shot` prints via `OutputFormat::format` and suppresses the Tokens line under `--quiet` (both flags were previously parsed-but-ignored); defaults unchanged | pass |
| Shared model parsing | `ExecMode::parse_model` made `pub` and reused by `run_single_shot`; duplicate private `parse_model` in `src/main.rs` deleted | pass |
| Transports reclassified, compat preserved | `hide = true` on `--core-transport`/`--stdio`/`--core-endpoint`/`core-stdio`/`attach-daemon`; `--standalone` stays visible; `internal_transports_stay_parseable_but_hidden` test; live hidden-flag parses | pass |
| `codegg daemon attach` canonical, alias retained | `DaemonCommand::Attach` + shared `cmd_daemon_attach` helper; `daemon_attach_canonical_and_compat_alias` test (parses + hidden); distinct from feature-gated remote HTTP `attach` in help text | pass |
| Orphan plugin CLI disposition: removed | `src/command/plugin.rs` deleted (verified never compiled: no `mod plugin` in `src/command/mod.rs`, zero references; wired to the obsolete empty marketplace tier model and duplicated install policy instead of calling `PluginManager`/`install.rs`); stale `architecture/command.md` section replaced with headless-authority note; `default_help_excludes_feature_gated_commands` asserts no `plugin` subcommand | pass |
| About/naming cleanup | `about = "CodeGG, the Rust-native AI coding agent for terminal workflows"`; stale "lightweight implementation of Codegg" gone; no mass rename (hidden `output-format` alias, hidden `attach-daemon`) | pass |
| Docs separate user vs internal surface | README (daemon attach, positional doctor, hidden-compat note, `--format` note); `architecture/core.md` (transport visibility table note); `architecture/command.md` (plugin disposition); `architecture/lsp.md` (`doctor lsp`); `.opencode/skills/core/SKILL.md` (visibility note alongside `architecture/core.md`) | pass |

### Deliberate non-merge recorded (§5 one-shot execution surface)

Root `--run` and `codegg exec` were NOT routed through one execution
implementation. They have different public contracts: `--run` takes a
prompt string with CLI model/agent flags and prints a text/JSON
response envelope, while `exec` takes structured JSON input and emits
`ExecOutput` with CI exit codes. Merging them in this corrective pass
would have changed observable automation behavior, which the roadmap
invariant forbids without an explicit deprecation path ("Existing user
scripts do not break"). What was unified without semantic change: one
policy `Args` struct, one validation authority, one `OutputFormat`
type with canonical `--format`, and one model parser. A future
deprecation-path proposal may revisit the execution-path merge; it is
not a defect of this closure.

## 3. Production implementation evidence

- `src/main.rs` — `ExecutionPolicyArgs` (+`resolve`), `verbosity_log_level`,
  `resolve_output_format`, `resolve_doctor_subsystem`,
  `root_after_help()` (two `cfg(feature = "server")` variants),
  `cmd_daemon_attach`; `Cli` gains launch `conflicts_with_all`, hidden
  transports, positional doctor + hidden `--subsystem` flag,
  `--format`/`output-format` alias; `Exec` gains flattened policy +
  `--format` alongside `--json-output`; `DoctorSubsystem` gains
  `Providers`/`Credentials`; `cmd_doctor` dispatches all six sections
  under `All`; `DaemonCommand::Attach`; runtime `after_help`
  application in `main()` and `cmd_completions`; `run_single_shot`
  honors `output_format`/`quiet`; local `parse_model` deleted;
  12-test `cli_surface_tests` module (10 default + 2 server-gated).
- `src/exec.rs` — `ExecMode::parse_model` made `pub` (shared, no
  behavior change). `ExecMode`'s JSON/quiet/session/policy contract
  untouched.
- `src/command/plugin.rs` — deleted (orphan, never compiled).
- Docs — README, `architecture/command.md`, `architecture/core.md`,
  `architecture/lsp.md`, `.opencode/skills/core/SKILL.md` (see matrix).

No storage, protocol, daemon, scheduler, provider, sandbox,
authorization, or permission semantics were added or changed. No
public command or flag was removed; removals are visibility-only
(`hide = true`) with parsing preserved.

## 4. Verification executed

### Commands run

```bash
cargo test --bin codegg cli_surface_tests
cargo test --bin codegg --features server cli_surface_tests
cargo test --bin codegg
cargo test -p codegg --lib -- policy_surface exec:: auth::cli
cargo test -p codegg --lib -- core::instance
cargo run --bin codegg -- --help
cargo run --bin codegg -- doctor --help
cargo run --bin codegg -- exec --help
cargo run --bin codegg -- daemon attach --help
cargo run --bin codegg --features server -- --help
cargo run --bin codegg -- doctor search
cargo run --bin codegg -- doctor --subsystem search
cargo run --bin codegg -- doctor providers
cargo run --bin codegg -- doctor credentials
cargo run --bin codegg -- -c -s foo
cargo run --bin codegg -- completions bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
git diff --check
```

### Results

- `cli_surface_tests` default: 10 passed, 0 failed.
- `cli_surface_tests` with `--features server`: 12 passed, 0 failed
  (includes gated help/attach/server example coverage).
- Full `--bin codegg` suite: 12 passed, 0 failed.
- Adjacent lib suites: `policy_surface`/`exec`/`auth::cli` 15 passed;
  `core::instance` 10 passed.
- Help smokes: default help shows the corrected about text, no
  `server`/`attach`/`attach-daemon`/`--core-transport`/`--stdio`/`--core-endpoint`
  entries; `doctor --help` shows `[SUBSYSTEM]` with the 7 real values;
  `exec --help` shows `--format` + retained `--json-output`;
  `daemon --help` lists `attach`; server build shows gated
  `server`/`attach` plus their examples.
- Live smokes: `doctor search` runs; `--subsystem search` runs with a
  deprecation warning; `doctor providers` lists configured providers;
  `doctor credentials` reports metadata-only status; `-c -s` fails at
  parse time with exit code 2; completions still generate.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- `scripts/verify.sh quick`: passed (fmt, builtin-agents check,
  core-boundary, sandbox, execution-ownership, tui-authority,
  workspace check).
- `git diff --check`: passed.
- Full workspace `cargo test` and `verify.sh full` were not run; the
  plan's verification scope is the focused suites above plus quick.

## 5. Invariant review

- User scripts do not break without a path: every removed spelling
  persists as a parseable alias (`--output-format`, `--json-output`,
  `--subsystem`, `attach-daemon`, all hidden transports); `--run`
  defaults (text output, Tokens line) are unchanged; `--yolo` +
  `--approval-mode yolo` redundancy still resolves.
- Authorization/observer/permission checks untouched: doctor additions
  are read-only listings through existing authorities; no secret
  material enters doctor output (credential metadata only).
- Dynamic project/plugin command precedence unchanged: no registry or
  dispatch code touched.
- Discovery performs no new I/O on any hot path: doctor providers read
  config/registry on explicit invocation only.
- CLI cleanup exposes no internal transports or secret operations:
  hidden flags stay parseable but unadvertised; credentials output is
  metadata-only.
- Default-build help lists only compiled commands; examples parse as
  written in both feature sets (tested).

## 6. Failure and recovery review

No asynchronous or durable operation was added. Parse-time conflicts
exit via Clap (code 2) before runtime state opens. Differing doctor
spellings return a concise `AppError` before config load. Policy
resolution failures keep their existing runtime failure modes
(`AppError` in one-shot/exec paths, stderr + exit 2 in TUI launch).
Doctor sections fail independently per pre-existing behavior; the new
credentials section degrades to a one-line unavailable notice instead
of erroring.

## 7. Migration and compatibility review

No schema, protocol, or config migration. Compatibility aliases
retained and tested: `--output-format`, `exec --json-output`/`-j`,
`doctor --subsystem`, `attach-daemon`, `--core-transport`,
`--stdio`, `--core-endpoint`, `core-stdio`. The only non-alias
disposition is the deleted orphan `src/command/plugin.rs`, which was
never compiled or reachable, so there is nothing to migrate. Help
visibility changes (`hide = true`, feature-gated examples) do not
alter parsing.

## 8. Security review

Doctor `providers` performs no network probes (model inventory stays
behind `codegg models`). Doctor `credentials` prints store metadata
only — provider id, kind, account label, expiry, scopes; no
plaintext, ciphertext, or fingerprint (same fields as the reviewed
`auth status` path). Hidden transport flags remain fully functional
for harnesses; hiding changes discoverability, not authority.

## 9. Documentation and operations

- Implementation plan status `ready` → `implemented` (this closure).
- Subsystem roadmap M002 `ready` → closed; with M001 already closed,
  the corrective roadmap is now closed.
- `plans/registry.md` updated (subsystem row, M002 plan row,
  recently-closed entry).
- No new CI lane, scanner, gate, bot, or release automation added.

## 10. Unresolved findings

No critical, high, medium, or low findings remain. Two explicitly
deferred items (not defects of this milestone):

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| info | `installation` doctor subsystem not added; data owned by blocked self-contained-installation M002 | `doctor --help` stays honest; installation diagnostics land with their owning milestone | Self-contained-installation M002 adds the variant + runfile report when its data lands; the positional syntax makes that additive |
| info | Root `--run` and `exec` keep separate execution paths behind the unified policy/format surface | No behavior change for automation; documented in §2 | A future proposal with an explicit deprecation path may unify them; not corrective work |

## 11. Roadmap disposition

Milestone closed. With M001 closed (`001-status.md`) and M002 closed
here, the command-surface-reconciliation corrective roadmap is fully
closed.

## 12. Registry updates

- Move M002 from ready to recently closed with implementation
  `da7fab03` and this closure record.
- Mark the corrective roadmap closed (M001+M002 both closed).
- Blocked-work audit: no registered plan lists M002 as a hard or
  interface dependency (registry handoff note: "Independent of M001;
  parser/help/ownership cleanup"). Self-contained-installation M002
  (blocked on installation M001 + provider-connect M003) and the
  provider-connect chain (M001 ready; M002/M003 internally blocked)
  reference CLI doctor syntax as a consumer, not a gate: this closure
  lands the syntax side they will extend but unblocks no milestone, so
  their statuses are unchanged. No future plan is newly blocked.
