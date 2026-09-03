# Memory-to-Skill Promotion M001 — Closure Record

Status: closed

Implementation revision: `2f029d8dd7de49876cf6527c835e586bd3d46e3c`

Source plan:

- `plans/implementation/memory-skill-promotion/001-habit-observation-and-candidate-store.md`

Source roadmap:

- `plans/subsystems/memory-skill-promotion-roadmap.md`

## 1. Outcome

M001 is implemented as a project-scoped, host-owned workflow observation and
candidate store. The implementation stops at `HabitCandidate::Ready`; it does
not draft, publish, or refresh skills and it does not alter text-memory
consolidation or prompt injection.

## 2. Safe observation inventory

The single production observation adapter is `AgentLoop::record_habit_tool_results`
in `src/agent/loop.rs`. It runs after completed tool batches and before the
logical turn reaches the existing `AgentFinished` publication boundary. The
collector accepts only exact, statically allowlisted canonical tool names:

| Tool family | Persisted action metadata |
|---|---|
| `read` | `FileRead` |
| `glob`, `grep`, `list`, `diff`, search/fetch/map tools | `Search` |
| `edit`, `write`, `replace`, `multiedit` | `Edit` |
| `apply_patch` | `Patch` |
| `test` | `Test` |
| `git` | `GitRead`/`GitWrite` plus an exact bounded subcommand when recognized |
| `lsp` | `LspRead` |
| `skill` | `SkillActivate` |
| `task` | `Delegate` |
| deterministic validation tools | `DeterministicValidate` plus exact tool name |
| `bash`, `terminal` | `ShellExec` only |

Each persisted action also contains a bounded `WorkflowEffectClass` derived
from the existing tool contract. Unknown tool names are ignored. Git
subcommands are read from the call only to select a fixed allowlisted enum
label; the original arguments are never copied into the action.

Explicitly excluded by construction: raw shell text, executable/argv,
filesystem paths, arbitrary JSON arguments, tool output/error bodies, URLs,
environment variables, prompts, model text, and hidden reasoning. The
collector receives the raw call/result only transiently at the execution
boundary, and the store serializes only the constructed action, bounded
session/turn/run provenance, counters, timestamps, and fingerprint.

## 3. Normalization, thresholds, and lifecycle evidence

`crates/codegg-core/src/memory/habit.rs` provides:

- versioned `codegg-habit-v1` domain-separated SHA-256 fingerprints scoped by
  the existing `memory::project_namespace` helper;
- immediate duplicate-action collapse, a 32-action sequence cap, bounded
  variants, and a two-distinct-action minimum;
- explicit successful provider terminal requirement (`stop`/`end_turn`), with
  failed tool outcomes, cancellation, errors, stalls, incomplete streams, and
  missing results excluded from successful observation finalization;
- occurrence idempotency keyed by bounded opaque session+turn(+run) identity;
- default readiness at three successful occurrences and a hard floor of two
  distinct sessions; repeated turns in one session cannot become ready;
- `Observing`, `Ready`, `Dismissed`, `Promoted`, and `Superseded` state with
  host-only transition methods; dismissed fingerprints do not reopen;
- neutral host summaries such as `read -> edit -> test`, never model names or
  generated descriptions.

The focused `codegg-core` habit tests cover deterministic/project-scoped
fingerprints, duplicate turns, one-session readiness prevention, three
successes across two sessions, failed and dismissed observations, malformed
and oversized files, and concurrent writers.

## 4. Persistence and bounds

`HabitStore` uses the existing config/memory ownership tree:

```text
~/.config/codegg/memory/habits/project/{sha256_namespace}.json
```

The generated namespace is validated as `project/<64 hex characters>` before
path construction, preventing traversal or caller-selected paths. Reads reject
files above 256 KiB before JSON decoding and reject unsupported versions,
more than 128 candidates, oversized action/variant/session/provenance fields,
or malformed JSON. Candidate records retain no more than 64 session IDs and
128 occurrence digests. Candidate admission prunes only oldest observing
records; if all records are ready/promoted/finalized, admission fails rather
than discarding durable confidence.

Each mutating store operation takes a per-project advisory lock, reads the
current complete file, writes a complete bounded JSON document to a temporary
file, calls `sync_all`, and atomically renames it. The concurrency test runs
eight independent writers and verifies both the merged count and decodable
final JSON.

## 5. User surface and compatibility

The command registry and TUI dispatch expose:

- `/habits` for bounded candidate summaries;
- `/habits ready` for ready candidates only;
- `/habit-dismiss <id>` for explicit dismissal.

Long output uses the existing scrollable memory info dialog. The surface says
ready candidates are eligible for a later proposal and makes no promise of
skill creation. No protocol, SQLite migration, config schema, skill registry,
asset-refresh, or memory prompt path was changed.

Existing `MemoryStore` and `PatternDetector` remain untouched apart from the
memory module’s additive habit submodule declaration. Existing text-memory
tests pass, and no habit candidate is included in `get_memory_summary` or
automatic model context.

## 6. Verification

Passed:

```text
cargo fmt --all
cargo test -p codegg-core habit --locked       # 5 passed
cargo test -p codegg-core memory --locked      # 23 passed
cargo check -p codegg --locked                 # passed
git diff --check                               # passed
```

`cargo test --test tui --locked` reached the linker but could not produce the
test binary on this host: the repository is being linked for `x86_64-apple-darwin`
while `/opt/local/lib/liblzma.dylib` and `libiconv.dylib` are arm64, yielding
undefined x86_64 lzma symbols. There were no Rust compilation or test
assertion failures in that command. Root compilation and the command dispatch
code type-check successfully.

The canonical `scripts/verify.sh quick` command passed for this exact worktree
after the focused checks. It completed the repository’s quick formatting,
generated-agent, boundary, sandbox, execution-ownership, and capped workspace
all-targets checks.

## 7. Downstream readiness audit

The registered dependency graph was audited after implementation. M001 is the
hard dependency named by M002, so M002 was moved from `blocked` to `ready for
handoff` in its plan, and the registry dependency-ready table now lists M002.
M003 remains `blocked` because it depends on M002’s explicit proposal and
validation closure. No other registered future plan names M001 as a hard,
interface, or operational dependency, so no additional plan became ready.

## 8. Findings and recommendation

No in-scope correctness, privacy, lifecycle, persistence, or compatibility
finding remains. The TUI integration binary could not link solely because of
the pre-existing host architecture/library mismatch; this does not weaken the
implementation contract and is explicitly retained as environment evidence.
M001 is recommended and recorded as strictly closed. M002 may proceed under
its own explicit-initiation and proposal-validation plan.
