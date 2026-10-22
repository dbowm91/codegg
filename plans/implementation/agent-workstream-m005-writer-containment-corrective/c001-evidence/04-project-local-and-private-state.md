# C001 Evidence — Project-Local and Managed Private State

Baseline SHA: `38f74b4632ea9a780df73ad8a96e0943fd492db2` (branch `work/agent-workstream-m005-writer-audit`)

Scope: project-LOCAL CodeGG state that lives **physically inside** a checkout, plus the managed
private/scratch state that does **not**. All citations are `file:line` on the baseline SHA above.
Every sink below carries all ten C001 fields. Where a fact could not be established from source it
is marked `UNPROVEN:` with the reason.

Terminology note: the plan text names `src/worktree/`. That directory does not exist in this tree.
`crate::worktree` resolves to `codegg_core::worktree` (re-exported at `src/lib.rs:16`), and the
managed-worktree *service* is `crates/codegg-core/src/worktree_service.rs`. This file cites the
real locations.

---

## State sink: Workspace run storage — `<workspace>/.codegg/runs`

- **Final effect owner:** `codegg_core::run_store::FsRunStore`
  (`crates/codegg-core/src/run_store.rs:1145-1149` struct; `:1151-1158` constructor;
  `:1430-1436` `impl RunStore for FsRunStore`). The `RunStore` trait contract is at
  `crates/codegg-core/src/run_store.rs:1054-1060`.
- **Physical location (inside/outside checkout) + justification:** **INSIDE the checkout tree.**
  The root is computed as `workspace.canonical_root.join(".codegg").join("runs")` at
  `crates/codegg-core/src/workspace_services.rs:406`, where `canonical_root` is the durable
  workspace directory the user opened as a project
  (`crates/codegg-core/src/workspace.rs:106-113`, field `canonical_root` on `WorkspaceRecord`).
  The same root is reconstructed independently at
  `src/tool/factory.rs:154` (`execution.workspace_root.join(".codegg").join("runs")`) and at
  `src/tui/app/mod.rs:894-896` (`PathBuf::from(&project_dir).join(".codegg").join("runs")`).
  The independent, canonical spelling of the same location is
  `src/work_plan_repository_binding.rs:181-187` (`run_store_root`, documented as "the workspace's
  durable CodeGG run-record root", matching `ProductionWorkspaceServicesFactory`).
  There is **no** re-homing, no cache relocation and no "outside the checkout" branch: the path is
  always `<workspace_root>/.codegg/runs`. If the workspace root *is* a Git checkout (the
  shared-checkout case), the run store is physically inside that checkout. Worst case: a user opens
  a shared checkout as the project directory, and the run store lands inside the shared tree.
- **Authority path:** per-run identity is `RunId::new()` minted at
  `crates/codegg-core/src/run_store.rs:1434`; the on-disk layout is
  `<root>/<date>/<run_id>/` (`run_store.rs:1171-1174`, with `validate_id_str` at `:1172`),
  plus a flat `index.jsonl` (`run_store.rs:1167-1169`, constant `INDEX_FILENAME` at
  `run_store.rs:14`) and `manifest.json` (`MANIFEST_FILENAME`, `run_store.rs:15`).
  Workspace authority is the `WorkspaceId` → `WorkspaceRecord.canonical_root` binding
  (`crates/codegg-core/src/workspace.rs:106-113`); per-workspace activation is serialized by a
  per-workspace `AsyncMutex` (`crates/codegg-core/src/workspace_services.rs:520-525`) and the
  bundle is cached in `self.active` (`:553`).
  Atomic write rules: manifest and artifact payloads are written to a temp path and renamed
  (`run_store.rs:1235-1239`, `:1254-1259`, `:1554-1560`); the JSONL index is appended in place
  (`run_store.rs:1197-1205`) or fully rewritten via temp-file + rename
  (`run_store.rs:1224-1239`).
- **Lifetime / startup ordering:** **Before any model turn, and without a run id.**
  `ProductionWorkspaceServicesFactory::build` eagerly creates the parent directory
  `<workspace>/.codegg` at `crates/codegg-core/src/workspace_services.rs:407-417` — i.e. the
  checkout is mutated as a *side effect of activating the workspace*, not as a consequence of a
  run. `build` is invoked from `WorkspaceServiceRegistry::activate`
  (`crates/codegg-core/src/workspace_services.rs:540-543`), which is reached from project
  activation (`src/core/project_activation.rs:297` `self.workspace_services.acquire(&typed_workspace_id)`)
  and from every daemon job/goal/document path that acquires workspace services
  (`src/core/daemon_jobs.rs:591`, `:630`, `:669`, `:734`; `src/core/daemon_goals.rs:811`, `:861`,
  `:912`, `:962`, `:1043`; `src/core/daemon_documents.rs:304`, `:450`, `:547`, `:726`, `:835`;
  `src/core/daemon_project_init.rs:205`). Run directories are created later, lazily, by
  `begin_run` → `ensure_root` (`run_store.rs:1432`, `:1160-1165`).
- **Reachable callers:** daemon (workspace activation and job/goal/document acquisition, as listed
  above), TUI (its own independent store instance at `src/tui/app/mod.rs:893-899`, and the
  per-session store at `src/tool/factory.rs:153-157`), HTTP server / standalone core
  (`src/main.rs:4224` builds the legacy store, and the daemon's own run store is built in
  `src/core/runtime_deps.rs` via the workspace-services factory), and the git audit path
  (`src/git_run_store.rs` writes into the same `RunStore` instance it is handed).
- **Existing coordination (and its limits):**
  - Per-instance serialization only. `FsRunStore.lock` is a `tokio::sync::Mutex<()>` **owned by the
    instance** (`run_store.rs:1147`, `:1155`), and `write_index_append` takes only that lock
    (`run_store.rs:1193-1194`). `rewrite_index_locked` is likewise guarded only by that same
    per-instance lock, and the doc comment says so explicitly
    (`run_store.rs:1212-1223`).
  - Because **three independent `FsRunStore` instances are constructed over the same root**
    (`src/tool/factory.rs:155`, `crates/codegg-core/src/workspace_services.rs:418`,
    `src/tui/app/mod.rs:897`), that mutex does **not** exclude a second instance. Each instance
    also keeps its own in-memory `index_cache` (`run_store.rs:1148`, `:1207-1208`).
  - Atomic rename protects the *file* against torn writes; it does **not** protect against a
    concurrent append being discarded by a concurrent rewrite (see below).
  - Nothing coordinates this sink with a checkout-level reservation: no `SharedCheckout`,
    `checkout_reservation` or `SharedCheckout` concept exists in `src/` at this SHA
    (searched `src/` for `shared checkout|shared_checkout|checkout_reservation|SharedCheckout` —
    no matches).
  - The per-workspace activation lock (`workspace_services.rs:520-525`) prevents *double
    construction* of one bundle inside one process; it does not fence the sink against a second
    process, a second session, or a shared-checkout holder.
- **Adversarial bypass example:** the rename-vs-append lost update. Instance **A**
  (workspace services) runs `rewrite_index_locked`: it calls `load_index`
  (`run_store.rs:1176-1191`), builds the full replacement, writes `index.jsonl.tmp`
  (`run_store.rs:1226-1235`) and renames it over `index.jsonl` (`run_store.rs:1236-1238`).
  Instance **B** (a session's tool registry) concurrently runs `write_index_append`
  (`run_store.rs:1193-1205`), appending its line to the *old* `index.jsonl`. If B's append lands
  between A's `load_index` and A's `rename`, A's rename **silently discards B's index entry** —
  a lost durable record, with no error, no lock, and no notification, inside the shared checkout.
  A second variant: a TUI-launched session (`src/tui/app/mod.rs:897`) and a daemon session
  (`src/tool/factory.rs:155`) point at the same `<project_dir>/.codegg/runs` and neither holds the
  other's lock.
- **Preliminary disposition:** **fenced-or-denied.** This is a CodeGG non-broker writer into a
  project-local sink physically inside the checkout. Per C004 work package D, it must either be
  rehomed/excluded by a documented stable store policy, or mediated/denied while a shared
  reservation exists. It is explicitly **not** `private-scratch`: the location is inside the
  checkout tree, and the content is durable user-visible run history (manifests, artifacts,
  index), not disposable scratch. Re-homing is a C004 decision, not one this audit may take.
- **Evidence:** `crates/codegg-core/src/run_store.rs:14`, `:1145-1149`, `:1151-1158`,
  `:1160-1165`, `:1167-1169`, `:1171-1174`, `:1176-1191`, `:1193-1205`, `:1212-1223`,
  `:1224-1239`, `:1254-1259`, `:1430-1436`, `:1554-1560`;
  `crates/codegg-core/src/workspace_services.rs:406`, `:407-417`, `:418`, `:520-525`, `:540-543`,
  `:553`; `crates/codegg-core/src/workspace.rs:106-113`;
  `src/tool/factory.rs:149-157`; `src/tui/app/mod.rs:893-899`;
  `src/work_plan_repository_binding.rs:76-81`, `:181-187`;
  `src/core/project_activation.rs:297`; `src/core/daemon_jobs.rs:591`;
  `src/core/daemon_goals.rs:811`; `src/core/daemon_documents.rs:304`;
  `src/core/daemon_project_init.rs:205`.

---

## State sink: Legacy project session database — `<project>/.codegg/sessions.db`

- **Final effect owner:** `codegg_core::storage::init_migrated_legacy_project_store`
  (`crates/codegg-core/src/storage/mod.rs:163-171`), which composes
  `init_pool_at_for_migration` (`:226-238`), `crate::session::schema::migrate`
  (`storage/mod.rs:168`) and `init_pool_at` (`:170`, defined `:175-218`). The unmigrated probe
  `init_legacy_project_store` is at `storage/mod.rs:144-147`.
  The legacy *import* entry point is `codegg_core::migration::migrate_legacy_project_database`
  (`crates/codegg-core/src/migration.rs:128-132`), with discovery at
  `crates/codegg-core/src/migration.rs:48-55`.
- **Physical location (inside/outside checkout) + justification:** **INSIDE the checkout tree.**
  `storage/mod.rs:145` and `:166` both build
  `project_root.join(".codegg").join("sessions.db")`; `migration.rs:49` builds the same path for
  discovery. `project_root` is the **process current working directory** in every production
  caller: `src/main.rs:1771-1775` (`cmd_sessions`), `:1812-1816` (`cmd_session_view`),
  `:1874-1878` (`cmd_export`), `:1916-1920` (`cmd_import`), `:3066-3069` (`launch_tui`),
  `:4219-4224` (`cmd_server`). There is no re-homing and no "outside the checkout" branch: if the
  user runs `codegg sessions` inside a checkout, the database is created inside that checkout.
  The source doc comment itself marks this as legacy and production-forbidden:
  "New code MUST NOT use this for production storage; production daemons use
  [`init_daemon_catalog`] instead" (`storage/mod.rs:142-143`).
  Worst case: the user launches any of the six commands with cwd = the shared checkout root, and
  `.codegg/sessions.db` (+ its `-wal`/`-shm` siblings) is created in the shared tree.
- **Authority path:** the file path is derived from cwd, **not** from a canonical workspace
  identity — this is the key authority defect. Contrast the legacy *import* path, which does
  establish canonical authority before importing: `project_storage.reconcile_workspace_path(&workspace,
  "legacy_project_import")` (`crates/codegg-core/src/migration.rs:159-163`), with
  `workspace_registry.get_or_register(project_root)` (`migration.rs:150-153`).
  Migration locking: the schema migrator runs inside an explicit transaction, and the migration
  uses a **single-connection pool** so every statement stays on that transaction's connection
  (`storage/mod.rs:159-162` doc, `:167-169` implementation, `:220-238`
  `init_pool_at_for_migration`); the migrated pool is closed before the runtime pool is opened
  (`storage/mod.rs:169-170`). Idempotence of the *import* is keyed by a `migration_marker` row on
  the source path (`migration.rs:70-84`, `:137-141`). The source database is never modified by the
  import (`migration.rs:126`).
- **Lifetime / startup ordering:** **Before any model turn, and without a run id.** All six
  callers are CLI/TUI/server **startup** paths. `cmd_sessions` (`:1770`), `cmd_session_view`
  (`:1811`), `cmd_export` (`:1873`) and `cmd_import` (`:1912`) are one-shot CLI commands;
  `launch_tui` (`:3061`, store built at `:3159` in the non-socket branch) and `cmd_server`
  (`:4196`, store built at `:4224`) build the pool before the frontend/agent exists. This is
  exactly the class C004 D calls out: "Protect startup/recovery paths that run before a model
  turn and aren't aware of run IDs."
  By contrast the **production daemon does not use this sink**: `run_daemon` resolves
  `storage::DaemonPaths::default()` and calls `storage::init_migrated_daemon_catalog(&catalog_paths)`
  (`src/main.rs:3947-3948`), and the stdio core does the same
  (`src/main.rs:4116-4120`). This confirms the "production daemon no longer calls
  `init_legacy_project_store`" claim at the baseline SHA.
- **Reachable callers:** `codegg sessions` (`src/main.rs:1775`), `codegg session view` (`:1816`),
  `codegg export` (`:1878`), `codegg import` (`:1920`), `codegg` TUI in non-socket modes
  (`:3159`), `codegg server --standalone-core` (`:4224`). Not reachable from the production
  daemon (`src/main.rs:3947-3948`).
- **Existing coordination (and its limits):**
  - SQLite's own file locking plus `busy_timeout` on the connect options
    (`storage/mod.rs:210`, `:237`, and the pragma-on-connect requirement documented at
    `storage/mod.rs:244-249`) serialize concurrent migrators on the *same file*.
  - The migrator's transaction (`storage/mod.rs:159-162`) makes the schema change atomic.
  - What it does **not** coordinate: the *physical* creation of the `.codegg/` directory and the
  database file (`storage/mod.rs:182-191` `create_dir_all`, `:230-236`
  `init_pool_at_for_migration`) is an unconditional filesystem effect with no checkout-level
  fence, no permit, and no reservation check. It happens even for a **read-only** command
  (`cmd_sessions` merely lists sessions). The import path's idempotence marker protects the
  *catalog* row, not the checkout-side file creation.
  - The import path does not create the file: `find_legacy_project_db` returns `None` when the
    file is absent (`migration.rs:48-55`, `:133-135`), so `init_legacy_project_store` at
    `migration.rs:144` is only reached for an existing file.
- **Adversarial bypass example:** two `codegg sessions` invocations started concurrently in the
  same checkout directory, while another group holds the shared checkout. Both reach
  `init_pool_at_for_migration` (`storage/mod.rs:167`) → `create_dir_all("<cwd>/.codegg")`
  (`storage/mod.rs:230`) → `connect_and_configure` (`:237`). The **first** one to reach
  `create_dir_all`/`create` materializes the directory and the `sessions.db`/`-wal`/`-shm` files
  inside the shared checkout, before any permit check exists. The second one's migrator then
  contends on the same file. Neither invocation knows a run id, and neither can be refused by a
  pre-effect shared-reservation check, because no such check exists on this path. A related
  bypass: `codegg import` (`:1912`) reaches `migrate_legacy_project_database` whose
  `reconcile_workspace_path` (`migration.rs:159-163`) establishes canonical authority for the
  *catalog write*, but the source-side open at `migration.rs:144` still physically opens (and
  therefore can create WAL/SHM siblings of) a file inside the checkout.
- **Preliminary disposition:** **fenced-or-denied.** It is a CodeGG non-broker writer into a
  project-local sink physically inside the checkout, on a startup path that never learns a run
  id. It is explicitly **not** `private-scratch` (it is durable session data inside the checkout,
  not disposable scratch) and **not** `read-proven` (the file is created, not merely read).
  C004 D's "rehome/exclude by a documented stable store policy" option is the natural remedy
  here — the store is already marked production-forbidden at `storage/mod.rs:142-143`, so
  retiring the six callers is a smaller change than fencing them. Choosing between the two is
  C004's call.
- **Evidence:** `crates/codegg-core/src/storage/mod.rs:142-147`, `:149-171`, `:175-218`,
  `:220-238`, `:244-249`; `crates/codegg-core/src/migration.rs:48-55`, `:70-84`, `:122-132`,
  `:133-141`, `:143-147`, `:149-163`;
  `src/main.rs:1770-1775`, `:1811-1816`, `:1873-1878`, `:1912-1920`, `:3061-3069`, `:3155-3160`,
  `:4196`, `:4219-4224`, `:3836-3854`, `:3947-3948`, `:4116-4120`.

---

## State sink: Project-local `.codegg/` state family (remaining writers)

One sink family, because all members share the same location rule (join onto a
project/workspace root) and the same disposition. Each member's final effect owner and citation
is listed. All are **INSIDE the checkout tree** whenever the project/workspace root is a
checkout.

- **Final effect owner / location table:**

  | Path under project/workspace root | Final effect owner | Evidence |
  |---|---|---|
  | `.codegg/context_artifacts/` | `context::artifact::FileArtifactStore` | `src/context/artifact.rs:129-141` (constructor), `:151-183` (`put`) |
  | `.codegg/research/` | `research::service::ResearchService` | `src/research/service.rs:53-61`, `:74-87` |
  | `.codegg/tool_program_calls/` (+ `locks/`) | `tool::tool_program_ledger::ToolProgramLedger` | `src/tool/tool_program_ledger.rs:92-97`, `:99-127` |
  | `.codegg/tool_program_results/` | `tool::tool_program_result::ToolProgramResultStore` | `src/tool/tool_program_result.rs:167-177` |
  | `.codegg/tool_program_sources/` | `tool::tool_program_source::ToolProgramSourceStore` | `src/tool/tool_program_source.rs:38-47` |
  | `.codegg/python_sources/` | `python_script::source_store::PythonSourceStore` | `src/python_script/source_store.rs:55-66` |
  | `.codegg/test-runs/` (index + per-run log dirs) | `test_runner::index` / `test_runner::runner` | `src/test_runner/index.rs:89-91`; `src/test_runner/runner.rs:169-178` |
  | `.codegg/goals/` | `codegg_core::goal::checkpoint` | `crates/codegg-core/src/goal/checkpoint.rs:5-7`, `:9-16` |
  | `.codegg/skills/` (project-scope publication) | `skills::publish` | `src/skills/publish.rs:418-434` |
  | `.codegg/codegg.jsonc` (project config) | `codegg_config` discovery + `codegg edit --project` | `crates/codegg-config/src/paths.rs:41-62`; `src/main.rs:2327-2350` |
  | `.codegg/agents/` | `codegg_core` agent definition loader (**read-only**) | `src/agent/definition.rs:514-524` |
  | `.codegg/instructions.md` | `agent::instructions` loader (**read-only**) | `src/agent/instructions.rs:170-173` |

- **Physical location (inside/outside checkout) + justification:** **INSIDE the checkout tree.**
  Every path is a plain `join` onto the project/workspace root — `FileArtifactStore::new`
  (`src/context/artifact.rs:136-140`), `ResearchService::new` (`src/research/service.rs:54`),
  `ToolProgramLedger::new` (`src/tool/tool_program_ledger.rs:94-95`),
  `ToolProgramResultStore::new` (`src/tool/tool_program_result.rs:175`),
  `ToolProgramSourceStore::new` (`src/tool/tool_program_source.rs:45`),
  `PythonSourceStore::new` (`src/python_script/source_store.rs:64`),
  `goal_artifact_dir` (`crates/codegg-core/src/goal/checkpoint.rs:6`), and the project skill
  publication root (`src/skills/publish.rs:423`). The project config path is
  `current.join(dir_name).join(format!("codegg.{}", ext))` walking **up** from cwd
  (`crates/codegg-config/src/paths.rs:46-62`), and `codegg edit --project` creates the file and
  its parent directory (`src/main.rs:2332-2350`).
  Independent confirmation that `.codegg` is expected to exist inside a project root: it is in
  the workspace directory-listing ignore list (`src/directory_listing.rs:85-95`) and in the TUI's
  `@`-completion ignore list (`src/tui/app/mod.rs:15745-15756`).
  Worst case: every member lands inside the shared checkout when the shared checkout is the
  project root.
- **Authority path:** identity is the *workspace/project root path*, not a run id. Within that:
  - context artifacts key on a stable hash of the artifact handle
    (`src/context/artifact.rs:143-146`), and evidence handles fail closed on a content
    collision rather than overwriting (`:163-175`).
  - tool-program calls key on `program_id` with a `flock`-based cross-process lock
    (`src/tool/tool_program_ledger.rs:99-127`); the lock falls back to *no lock* on non-Unix
    platforms (`:124-127`).
  - research artifacts and goal checkpoints key on their own ids
    (`src/research/service.rs:55`; `crates/codegg-core/src/goal/checkpoint.rs:16`).
  - test-run logs key on a UTC timestamp + 8 hex chars of a UUID
    (`src/test_runner/runner.rs:170-173`).
  - skill publication establishes an explicit *owned root* and refuses a target that escapes its
    context root (`src/skills/publish.rs:426-432`).
- **Lifetime / startup ordering:** mostly **during a model turn**, not before:
  `FileArtifactStore` is constructed per tool registry build
  (`src/tool/factory.rs:134-135`), and the research service is constructed from the execution
  workspace root. Two exceptions reach the checkout *before* a turn: the goal checkpoint
  (written on goal lifecycle events, `crates/codegg-core/src/goal/checkpoint.rs:9-16`) and the
  project skill publication / `codegg edit --project`, which are **human-initiated** commands.
  These family members therefore do **not** share the startup-ordering hazard of the run store
  and the legacy DB; their hazard is concurrent model-driven mutation.
- **Reachable callers:** daemon (workspace services and job paths), TUI, HTTP server, and the
  Python/tool-program tool surfaces; `codegg edit --project` and skill publication are
  human-initiated CLI paths.
- **Existing coordination (and its limits):**
  - Atomic temp-file + rename for context artifacts (`src/context/artifact.rs:176-182`), python
    sources (`src/python_script/source_store.rs:55`, `:76`) and tool-program sources
    (`src/tool/tool_program_source.rs:49-50`).
  - `flock` cross-process lock for tool-program calls
    (`src/tool/tool_program_ledger.rs:99-127`) — but it is keyed by `program_id` inside the
    workspace's own `.codegg/tool_program_calls/locks/` directory, so it coordinates CodeGG
    processes using the same program, **not** writers of other kinds to the same checkout, and it
    degrades to no lock off Unix (`:124-127`).
  - Owned-root + no-escape check for skill publication (`src/skills/publish.rs:426-432`).
  - What none of them coordinates: any of them against a checkout-level reservation, and the
    `FileArtifactStore` temp file `<hash>.json` rename against a second writer of the same handle
    from a different process (the collision check at `src/context/artifact.rs:163-175` is
    read-then-write, not a compare-and-swap, so two processes can both pass it).
- **Adversarial bypass example:** a model turn in group B, inside a shared checkout that group A
  holds, writes a context artifact via `FileArtifactStore::put`. The tool's own path
  authorization is workspace-scoped, not checkout-reservation-scoped, so the write lands at
  `<shared>/.codegg/context_artifacts/<hash>.json` while group A's agent is editing the same
  checkout. Nothing in `FileArtifactStore` (`src/context/artifact.rs:151-183`) consults a
  reservation, and no guard in `scripts/check_*` covers it — the only project-local `.codegg`
  sinks named in the change-triggered guard list are the run store and the legacy DB. A second
  variant: the tool-program ledger's `flock` protects `tool_program_calls/<program_id>` from
  other CodeGG processes, but a *different* CodeGG writer (e.g. a tool-program **source** write
  to `.codegg/tool_program_sources/`) is not covered by that lock.
- **Preliminary disposition:** **fenced-or-denied** for the written members
  (`context_artifacts`, `research`, `tool_program_*`, `python_sources`, `test-runs`, `goals`,
  `skills`, `codegg.jsonc`).
  **read-proven** for the two read-only members (`.codegg/agents/` at
  `src/agent/definition.rs:514-524`, `.codegg/instructions.md` at
  `src/agent/instructions.rs:170-173`) — they are discovered, never written by CodeGG, so they
  need no writer permit.
  They are explicitly **not** `private-scratch`: the paths are inside the checkout tree.
- **Evidence:** as listed in the table above, plus `src/tool/factory.rs:134-135`,
  `src/directory_listing.rs:85-95`, `src/tui/app/mod.rs:15745-15756`,
  `src/main.rs:2327-2350`, `crates/codegg-config/src/paths.rs:41-62`.

---

## State sink: Managed private scratch root

- **Final effect owner:** `codegg::scratch::session_scratch_dir` / `agent_run_scratch_dir` /
  `shared_scratch_dir` (`src/scratch.rs:108-145`, `:147-158`, `:160-171`).
- **Physical location (inside/outside checkout) + justification:** **OUTSIDE the checkout — and
  this is provable, not assumed.** The root is `dirs::cache_dir()` joined with the vendor and
  root names: `src/scratch.rs:126` (`let cache_root = dirs::cache_dir().ok_or(ScratchError::CacheUnavailable)?;`),
  `:129-131` (`vendor_root = cache_root.join(SCRATCH_VENDOR_NAME)`,
  `scratch_root = vendor_root.join(SCRATCH_ROOT_NAME)`,
  `sessions_root = scratch_root.join("sessions")`), then a SHA-256 of the session id
  (`:136-141`). The per-run and per-shared leaves are
  `session_root/runs/<scope>-<digest>` (`:153-157`) and `session_root/shared/<scope>-<digest>`
  (`:166-170`).
  **There is no relative/cwd fallback:** if `dirs::cache_dir()` is unavailable the function
  returns `ScratchError::CacheUnavailable` (`src/scratch.rs:126`) rather than degrading to a
  path under the process cwd. That is the specific property that makes this classification
  safe.
  Worst case: `XDG_CACHE_HOME`/`HOME` could be set to a directory that happens to be inside a
  checkout by the operator; the code has no checkout check, but it also never *derives* the
  location from the project.
- **Authority path:** identity is host-supplied, never model-supplied: "The identity is supplied
  by the host execution context, never by tool arguments" (`src/scratch.rs:104-107`).
  `ScratchId` / `AgentRunId` are typed identities (`src/scratch.rs:151`, `:164`); the shared
  locator "Authorization is deliberately checked by `AgentRunStore` before this host-only
  filesystem helper is called" (`src/scratch.rs:160-161`).
  Path construction: `scoped_resource_dir` hashes `(scope, id)` and calls
  `ensure_private_directory` + `ensure_owner_record` (`src/scratch.rs:451-461`).
- **Lifetime / startup ordering:** created on demand, per session and per delegated run; the
  root is created eagerly on first use (`src/scratch.rs:127-134`) but it is **not** a startup path
  that mutates a checkout.
- **Reachable callers:** session scratch (all sessions), delegated agent-run scratch
  (`src/scratch.rs:149-158`), and shared scratch with `ScratchId` grants
  (`src/scratch.rs:162-171`).
- **Existing coordination (and its limits):**
  - Ownership + permission enforcement: `ensure_private_parent` rejects symlinks and non-directories
    and enforces `uid` match and `mode & 0o022 == 0` (`src/scratch.rs:463-476`);
    `ensure_private_directory` rejects symlinked parents and creates with private mode
    (`:547-559`); an owner record file is written into each private directory
    (`src/scratch.rs:459`, `:535-539`).
  - The path is canonicalized before return (`src/scratch.rs:144`, `:460`).
  - What it does not coordinate: nothing about the checkout — by design, it is outside it.
- **Adversarial bypass example:** a scratch leaf is created under the user cache root, so a
  shared-checkout writer cannot reach it and a scratch writer cannot reach the checkout. The
  residual risk is *not* checkout contention but scratch confusion: two delegated runs in the same
  session share `session_root/runs/`, separated only by the `(scope, id)` hash
  (`src/scratch.rs:455-457`) and the owner record. A model that can name a `ScratchId` it does
  not own would be a bypass of the `AgentRunStore` authorization check noted at
  `src/scratch.rs:160-161` — that check is the thing to verify, and it is **not** in this audit's
  scope (it belongs to the managed-scratch line).
- **Preliminary disposition:** **private-scratch.** It is user-cache-root, uid-private,
  owner-recorded, model-cannot-choose-the-path state, and it is provably not derived from the
  project root. It should be **excluded** from checkout-writer mediation rather than fenced —
  mediating it would be a false "inside checkout" claim in the opposite direction.
- **Evidence:** `src/scratch.rs:104-107`, `:108-145`, `:126-134`, `:136-144`, `:147-158`,
  `:160-171`, `:451-461`, `:463-476`, `:535-539`, `:547-559`.

---

## State sink: User config root (and global skill / agent / memory / credential stores)

- **Final effect owner:** `codegg_core::storage::DaemonPaths`
  (`crates/codegg-core/src/storage/paths.rs:39-48` data root, `:50-56` config root,
  `:66-70` `config_root()`, `:87-90` `agents_dir()`, `:92-95` `credentials_path()`);
  `codegg_config` global config (`crates/codegg-config/src/paths.rs:64-73`);
  skill discovery (`src/agent/asset_context.rs:266-268`,
  `src/skills/registry.rs:291-305`); agent definitions
  (`src/agent/definition.rs:501-512`); memory store
  (`crates/codegg-core/src/memory/mod.rs:161-167`).
- **Physical location (inside/outside checkout) + justification:** **OUTSIDE the checkout**, with
  an explicit, named worst case. Evidence:
  - `DaemonPaths::default_config_root` = `dirs::config_dir()/codegg`, falling back to
    `dirs::home_dir()/.config/codegg` (`crates/codegg-core/src/storage/paths.rs:51-55`).
  - The global skill store is `global_root.join("codegg").join("skills")`
    (`src/skills/registry.rs:293`), where the global root is
    `default_global_discovery_root() = dirs::config_dir()` (`src/agent/asset_context.rs:266-268`).
  - The global agent store is `dirs::config_dir()/codegg/agents`
    (`src/agent/definition.rs:501-502`).
  - The memory store is `dirs::config_dir()/codegg/memory`
    (`crates/codegg-core/src/memory/mod.rs:162-165`).
  - The global config file is `dirs::config_dir()/codegg/{codegg.jsonc,codegg.json,config.json}`
    (`crates/codegg-config/src/paths.rs:64-73`).
  - **Worst case / named hole:** both roots end in
    `.unwrap_or_else(|| PathBuf::from(".codegg"))`
    (`crates/codegg-core/src/storage/paths.rs:47` for data, `:55` for config). That fallback is a
    **relative** path, so if `dirs::data_dir()`/`dirs::config_dir()` and `dirs::home_dir()` all
    fail, the "user-scoped" root resolves against the **process cwd** and lands inside whatever
    directory the process was launched in — which may be a checkout. The same relative-fallback
    pattern appears in the plugin store (`src/plugin/install.rs:35` `unwrap_or_else(|| PathBuf::from("."))`)
    and plugin data (`src/plugin/contributions.rs:166`).
- **Authority path:** `CODEGG_DATA_HOME` overrides the data root
  (`crates/codegg-core/src/storage/paths.rs:41-43`); the config root is not env-overridable in
  this struct. Identity is the config/data root, never a project path.
- **Lifetime / startup ordering:** created at config/catalog initialization, before any turn; not
  checkout-scoped and not run-scoped.
- **Reachable callers:** every CodeGG entry point (CLI, TUI, server, daemon).
- **Existing coordination (and its limits):** per-user filesystem ownership. It does **not**
  coordinate with any checkout reservation (correctly so), and it does **not** defend against the
  relative-fallback path described above.
- **Adversarial bypass example:** a process started with `HOME` unset and no `XDG_*` variables
  gets `PathBuf::from(".codegg")` as its "user config root"
  (`crates/codegg-core/src/storage/paths.rs:55`); with cwd = the shared checkout, CodeGG would
  create `.codegg/` inside the shared checkout while believing it is using user-scoped state.
  This is a *configuration-dependent* location, and the honest statement is: "outside the
  checkout under normal operation; inside the checkout in the no-HOME/no-XDG degenerate case."
- **Preliminary disposition:** **out-of-contract** for the checkout-writer policy — these are
  user-scoped state that a shared-checkout reservation should not mediate. The relative-fallback
  hole is recorded as a coverage gap below rather than silently reclassified.
- **Evidence:** `crates/codegg-core/src/storage/paths.rs:39-48`, `:50-56`, `:66-70`, `:87-90`,
  `:92-95`; `crates/codegg-config/src/paths.rs:64-73`;
  `src/agent/asset_context.rs:257-268`; `src/skills/registry.rs:291-305`;
  `src/agent/definition.rs:501-512`; `crates/codegg-core/src/memory/mod.rs:161-167`;
  `src/plugin/install.rs:33-38`; `src/plugin/contributions.rs:165-168`.

---

## State sink: Daemon catalog / session database and daemon runtime home

- **Final effect owner:** `codegg_core::storage::init_migrated_daemon_catalog` over
  `DaemonPaths` (`src/main.rs:3947-3948`, `src/main.rs:4116-4120`);
  `codegg_core::storage::DaemonPaths::catalog_db_path` (`crates/codegg-core/src/storage/paths.rs:72-75`);
  `codegg_client::LocalDaemonPaths::resolve` (`crates/codegg-client/src/paths.rs:86-91`) with
  `default_user_runtime_root` (`crates/codegg-client/src/paths.rs:161-181`).
- **Physical location (inside/outside checkout) + justification:** **OUTSIDE the checkout.**
  The catalog database is `data_root()/codegg.db` where `data_root` is
  `CODEGG_DATA_HOME` or `dirs::data_dir()/codegg` (falling back to
  `dirs::home_dir()/.local/share/codegg`)
  (`crates/codegg-core/src/storage/paths.rs:40-48`, `:72-75`) — on Linux,
  `~/.local/share/codegg/codegg.db`.
  The daemon runtime home (lock, socket, metadata, log) resolves through
  `LocalDaemonPaths::resolve` → `CODEGG_DAEMON_HOME` or `default_user_runtime_root()`
  (`crates/codegg-client/src/paths.rs:86-91`), which is
  `XDG_RUNTIME_DIR/codegg`, else `XDG_DATA_HOME/codegg`, else
  `~/.local/share/codegg` (`crates/codegg-client/src/paths.rs:161-181`).
  **Worst case / named holes:** (a) `CODEGG_DATA_HOME` (`paths.rs:41-43`) or `CODEGG_DAEMON_HOME`
  (`crates/codegg-client/src/paths.rs:87-88`) can be pointed anywhere, including inside a
  checkout; (b) the same relative `.codegg` fallback as above
  (`crates/codegg-core/src/storage/paths.rs:47`).
- **Authority path:** the daemon is a **user-scoped singleton**: the singleton lock is acquired
  *before* the socket is bound (`src/main.rs:3861-3866`), and the daemon home root is created
  with user-only permissions (`crates/codegg-client/src/paths.rs:152-158`). The daemon catalog
  carries the canonical project/workspace binding (see the legacy-import authority path at
  `crates/codegg-core/src/migration.rs:149-163`).
- **Lifetime / startup ordering:** **strictly before any model turn** — the daemon home and
  catalog are initialized as daemon startup steps 1 and 3
  (`src/main.rs:3861-3866`, `:3946-3948`). This is a startup/recovery path, and it is exactly the
  kind C004 D asks to protect. It does **not** know a run id, and it does not need to: it is
  outside the checkout, so it needs no checkout fence.
- **Reachable callers:** `codegg` (daemon-client mode), `codegg daemon`, `codegg server`, stdio
  core (`src/main.rs:4116-4120`), and every daemon-internal path through the catalog pool.
- **Existing coordination (and its limits):** singleton `flock` on `daemon.lock` (the daemon home
  root is user-scoped and permission-tightened). It does **not** coordinate with a checkout
  reservation, and it does not defend against `CODEGG_DAEMON_HOME`/`CODEGG_DATA_HOME` being
  pointed inside a checkout.
- **Adversarial bypass example:** an operator exports `CODEGG_DATA_HOME=/shared/checkout/.codegg-data`
  before starting the daemon. The daemon's catalog DB, its WAL, and (via `CODEGG_DAEMON_HOME`)
  its lock and socket all land inside the shared checkout, entirely bypassing the checkout
  reservation, while the code believes it is writing user-scoped state. This is a
  configuration-dependent location, not a code-path one.
- **Preliminary disposition:** **out-of-contract** for the checkout-writer policy (user-scoped
  daemon state, outside the checkout in normal operation), with the two named configuration holes
  recorded below.
- **Evidence:** `crates/codegg-core/src/storage/paths.rs:40-48`, `:72-75`;
  `crates/codegg-client/src/paths.rs:86-91`, `:104-107`, `:152-158`, `:161-181`;
  `src/main.rs:3861-3866`, `:3946-3948`, `:4116-4120`.

---

## State sink: Plugin store and plugin data

- **Final effect owner:** `codegg::plugin::install::plugins_dir`
  (`src/plugin/install.rs:33-38`); `codegg::plugin::contributions` plugin-data root
  (`src/plugin/contributions.rs:165-171`).
- **Physical location (inside/outside checkout) + justification:** **OUTSIDE the checkout.**
  `plugins_dir()` = `dirs::data_local_dir()/codegg/plugins` (`src/plugin/install.rs:33-38`);
  plugin data = `dirs::data_local_dir()/codegg/plugin-data/<plugin_id>`
  (`src/plugin/contributions.rs:165-171`). **Worst case:** both fall back to
  `PathBuf::from(".")` when `dirs::data_local_dir()` is unavailable
  (`src/plugin/install.rs:35`, `src/plugin/contributions.rs:166`) — a **relative** path that
  resolves against the process cwd, i.e. potentially inside a checkout. This is the same
  degenerate-environment class as the config-root fallback.
- **Authority path:** plugin id. `validate_uninstall_target`
  (`src/plugin/install.rs:428-441`) refuses a target that does not resolve inside the canonical
  `plugins_dir()`; the tests at `:616-621` and `:636-646` exercise exactly that containment, and
  `validate_relative_install_path` (`:40-52`) separately rejects traversal components in
  relative install paths.
- **Lifetime / startup ordering:** at plugin install/uninstall time — a **human-initiated** CLI
  action, not a model turn and not daemon startup.
- **Reachable callers:** `codegg plugin install|uninstall` and plugin runtime resolution.
- **Existing coordination (and its limits):** install/uninstall target containment relative to
  `plugins_dir()`; plugin data is keyed by plugin id
  (`src/plugin/contributions.rs:169-171`). Nothing coordinates with a checkout reservation, and
  the relative fallback is not guarded.
- **Adversarial bypass example:** with no `XDG_DATA_HOME`/`HOME` resolvable, `plugins_dir()`
  becomes `"./codegg/plugins"`, so `codegg plugin install` creates `./codegg/plugins` under the
  cwd. If the cwd is the shared checkout, plugin bytes land inside it with no reservation check
  and no containment root to compare against (the containment check compares against the same
  relative base, so it passes).
- **Preliminary disposition:** **out-of-contract** for the checkout-writer policy (user-scoped
  plugin state), with the relative-fallback hole recorded below.
- **Evidence:** `src/plugin/install.rs:33-38`, `:40-52`, `:428-441`, `:616-621`, `:636-646`;
  `src/plugin/contributions.rs:165-174`.

---

## State sink: Managed worktree root and worktree metadata

- **Final effect owner:** `codegg_core::worktree_service::WorktreeService`
  (`crates/codegg-core/src/worktree_service.rs:820-869` for the create/reserve path);
  `codegg_core::worktree::create_worktree_at` (`crates/codegg-core/src/worktree.rs:51`);
  the record store is `SqliteWorktreeStore` over the daemon catalog pool
  (`crates/codegg-core/src/worktree_service.rs:505`).
- **Physical location (inside/outside checkout) + justification:** **OUTSIDE the checkout in the
  production wiring, and there is an explicit native invariant that says so.**
  - Production wiring: `WorktreeService::sqlite(pool, DaemonPaths::default().data_root().join(DEFAULT_WORKTREE_DIRECTORY), ...)`
    (`src/core/runtime_deps.rs:368-375`; `DEFAULT_WORKTREE_DIRECTORY = "worktrees"` at
    `crates/codegg-core/src/worktree_service.rs:28`) → `~/.local/share/codegg/worktrees` on
    Linux, i.e. the user data root, not the checkout.
  - The managed worktree directory is then `managed_root.join(repository_id)` +
    `wt-<12 hex>` (`crates/codegg-core/src/worktree_service.rs:826-827`).
  - **Native invariant:** `if managed_root.starts_with(&root) || root.starts_with(&managed_root)`
    → `WorktreeServiceError::InvalidInput("managed worktree root must be outside the repository
    root")` (`crates/codegg-core/src/worktree_service.rs:832-836`).
  - **Worst case / named hole (important):** the *fallback* construction in
    `src/core/daemon_construct.rs:220-225` uses
    `WorktreeService::memory(PathBuf::from(".codegg/worktrees"), ...)` — a **relative** managed
    root. `canonical_or_self` (`crates/codegg-core/src/worktree_service.rs:828`, helper at
    `:1334-1336`) only canonicalizes when the path exists, and the subsequent
    `create_dir_all(&managed_root)` (`:837-841`) materializes it relative to the **process
    cwd**. Because the guard at `:832` compares an absolute `root` against a *relative*
    `managed_root`, `starts_with` is false in both directions and the "outside the repository
    root" invariant **does not fire** for the relative form. In that configuration a managed
    worktree can be created inside the daemon's cwd.
  - The worktree *records* (metadata) live in the daemon catalog SQLite pool
    (`crates/codegg-core/src/worktree_service.rs:505`), i.e. in the user-scoped catalog
    (`crates/codegg-core/src/storage/paths.rs:72-75`) — outside the checkout.
- **Authority path:** `WorktreeId::new()` + a `codegg/worktree/<12 hex>` branch
  (`crates/codegg-core/src/worktree_service.rs:821-825`); the repository is bound to the
  canonical `repository_root`; the scheduler reserves
  `exclusive:worktree-mutation` for worktree/repository metadata mutations
  (`crates/codegg-core/src/worktree_service.rs:25-27`); the per-repository lock table is
  acquired during reservation (`crates/codegg-core/src/worktree_service.rs:899`).
- **Lifetime / startup ordering:** worktree creation is a user/agent-initiated action. A
  **startup/recovery** path exists — the daemon holds a worktree reconcile handle
  (`src/core/daemon_construct.rs`, worktree reconcile wiring) and `WorktreeService::reconcile_all`
  prunes/refreshes state at daemon startup, i.e. before any model turn and without a run id.
  Exact reconcile call site: `src/core/daemon_construct.rs` (worktree reconcile handle
  construction) — `UNPROVEN:` the exact line number of the reconcile invocation was not
  re-read in this session; the handle's presence is confirmed but the precise call-site line is
  not cited here.
- **Reachable callers:** TUI worktree sidebar (`src/tui/commands/git_sidebar.rs:143`),
  daemon project listing (`src/core/daemon_projects.rs:83-88`, `:709-738`), HTTP project routes
  (`src/server/routes/project.rs:76`), the git service (`src/git_service.rs:740`), and the
  work-order coordinator (`src/core/work_order_coordinator.rs:821`, `:877`).
- **Existing coordination (and its limits):**
  - Native fence: the "outside the repository root" invariant
    (`crates/codegg-core/src/worktree_service.rs:832-836`) plus the `exclusive:worktree-mutation`
    scheduler key (`:25-27`) plus the per-repository lock (`:899`).
  - What it does **not** coordinate: the relative-root fallback bypass described above
    (`src/core/daemon_construct.rs:220-225`), and the worktree *directory* is outside the
    checkout so the native fence is about "not inside the repository", not about "not inside
    another group's reservation".
- **Adversarial bypass example:** the daemon starts with no injected `worktree_service`, so
  `src/core/daemon_construct.rs:220-225` installs the in-memory variant with managed root
  `.codegg/worktrees`. The daemon's cwd is wherever it was launched
  (the user-scoped daemon may well have been started from inside a checkout). `reserve` then
  computes `repo_root = ".codegg/worktrees/<repository_id>"` relative to that cwd, the
  "outside the repository root" guard at
  `crates/codegg-core/src/worktree_service.rs:832-836` does not fire (relative vs absolute
  `starts_with`), and `create_dir_all` (`:837-841`) materializes `<cwd>/.codegg/worktrees/...`
  — a managed worktree tree inside a checkout, with no checkout reservation consulted.
- **Preliminary disposition:** **fenced-native-candidate** for the production wiring (it already
  has a native fence asserting "managed worktree root must be outside the repository root", and
  the managed root is outside the checkout in the production path).
  The **relative-root fallback at `src/core/daemon_construct.rs:220-225` is a genuine
  `fenced-or-denied` / defect item** and should be raised as its own finding, not waved through
  as "outside checkout". The worktree *record* store is `out-of-contract` (it lives in the
  user-scoped daemon catalog).
- **Evidence:** `src/core/runtime_deps.rs:368-375`; `src/core/daemon_construct.rs:220-225`;
  `crates/codegg-core/src/worktree_service.rs:25-29`, `:505`, `:821-836`, `:837-841`, `:899`,
  `:1334-1336`; `crates/codegg-core/src/worktree.rs:51`;
  `crates/codegg-core/src/storage/paths.rs:72-75`.

---

## Inside-checkout state summary

**Provably inside a checkout (i.e. the path is constructed by joining onto the project/workspace
root, with no re-homing branch):**

1. `<workspace>/.codegg/runs` — the run store, including `index.jsonl`, `manifest.json`,
   per-run artifact directories, and the transient `index.jsonl.tmp`
   (`crates/codegg-core/src/workspace_services.rs:406`, `:418`;
   `src/tool/factory.rs:154`; `src/tui/app/mod.rs:894-896`;
   `crates/codegg-core/src/run_store.rs:1226`, `:1236`).
2. `<project>/.codegg/sessions.db` (plus its `-wal`/`-shm` siblings) — the legacy project store
   (`crates/codegg-core/src/storage/mod.rs:145`, `:166`;
   `crates/codegg-core/src/migration.rs:49`; cwd-derived at `src/main.rs:1771-1775`,
   `:1812-1816`, `:1874-1878`, `:1916-1920`, `:3066-3069`, `:4219-4224`).
3. `<workspace>/.codegg/context_artifacts/` (`src/context/artifact.rs:136-140`).
4. `<project>/.codegg/research/` (`src/research/service.rs:54`, `:79`).
5. `<workspace>/.codegg/tool_program_calls/` and `.../locks/`
   (`src/tool/tool_program_ledger.rs:94-95`).
6. `<workspace>/.codegg/tool_program_results/` (`src/tool/tool_program_result.rs:175`).
7. `<workspace>/.codegg/tool_program_sources/` (`src/tool/tool_program_source.rs:45`).
8. `<workspace>/.codegg/python_sources/` (`src/python_script/source_store.rs:64`).
9. `<workspace>/.codegg/test-runs/` (`src/test_runner/index.rs:90`; `src/test_runner/runner.rs:173`).
10. `<project>/.codegg/goals/` (`crates/codegg-core/src/goal/checkpoint.rs:6`).
11. `<project>/.codegg/skills/` (`src/skills/publish.rs:423`).
12. `<project>/.codegg/codegg.jsonc` (`crates/codegg-config/src/paths.rs:49-56`;
    `src/main.rs:2332-2350`).
13. `<project>/.codegg/agents/` and `<project>/.codegg/instructions.md` — **read-only** by CodeGG
    (`src/agent/definition.rs:514-524`; `src/agent/instructions.rs:170-173`).

**Qualified statement (not an assumption):** items 1–12 are inside a checkout **iff the
project/workspace root is that checkout**. The code never re-homes them and never consults
whether the root is a Git checkout; the "inside" property is a pure consequence of
`root.join(".codegg")`. The worst case for every one of them is a user who opens the shared
checkout as the project directory.

**Provably outside a checkout (with the code that establishes it):**

- Managed scratch root — `dirs::cache_dir()`, with a hard `CacheUnavailable` error and **no**
  relative fallback (`src/scratch.rs:126`, `:129-134`).
- User config root, global skill store, global agent store, memory store, credential store —
  `dirs::config_dir()` / `dirs::home_dir()` (`crates/codegg-core/src/storage/paths.rs:51-55`;
  `src/agent/asset_context.rs:266-268`; `src/skills/registry.rs:293`;
  `src/agent/definition.rs:501-502`; `crates/codegg-core/src/memory/mod.rs:162-165`).
- Daemon catalog DB and daemon runtime home — `dirs::data_dir()` / `XDG_RUNTIME_DIR` /
  `XDG_DATA_HOME` / `~/.local/share` (`crates/codegg-core/src/storage/paths.rs:40-48`;
  `crates/codegg-client/src/paths.rs:161-181`).
- Plugin store and plugin data — `dirs::data_local_dir()`
  (`src/plugin/install.rs:33-38`; `src/plugin/contributions.rs:165-171`).
- Managed worktree root in the production wiring — `DaemonPaths::default().data_root()/worktrees`
  (`src/core/runtime_deps.rs:368-375`; `crates/codegg-core/src/worktree_service.rs:28`), plus the
  native "must be outside the repository root" invariant
  (`crates/codegg-core/src/worktree_service.rs:832-836`).

**Every "outside" claim above carries a named configuration-dependent worst case**, listed in
Coverage gaps. None of them is a bare "it is outside" assertion.

---

## Coverage gaps

1. **Relative-path fallbacks can put "user-scoped" state inside a checkout.**
   `crates/codegg-core/src/storage/paths.rs:47` and `:55` fall back to
   `PathBuf::from(".codegg")`; `src/plugin/install.rs:35` and
   `src/plugin/contributions.rs:166` fall back to `PathBuf::from(".")`. These resolve against the
   **process cwd**. Under a normal environment they never fire; in a degenerate environment
   (`HOME` unset, no `XDG_*`) they place config/catalog/plugin state inside whatever directory
   the process was launched in. `src/scratch.rs:126` shows the correct pattern (hard error
   instead of a relative fallback); the same pattern is **not** applied to the config/data/plugin
   roots. This is a concrete, citable defect candidate for the C004/C005 line.
2. **Environment overrides can relocate "outside" state into a checkout.**
   `CODEGG_DATA_HOME` (`crates/codegg-core/src/storage/paths.rs:41-43`) and
   `CODEGG_DAEMON_HOME` (`crates/codegg-client/src/paths.rs:87-88`) are unvalidated arbitrary
   paths. No code checks that they are outside a workspace root.
3. **The managed-worktree relative-root fallback defeats the native "outside the repository
   root" fence.** `src/core/daemon_construct.rs:220-225` +
   `crates/codegg-core/src/worktree_service.rs:828`, `:832-836`, `:837-841`. The guard compares
   an absolute `root` with a relative `managed_root`, so `starts_with` is false in both
   directions and the invariant does not fire.
4. **The run store has three independent instances over one root with no cross-instance lock**
   (`src/tool/factory.rs:155`, `crates/codegg-core/src/workspace_services.rs:418`,
   `src/tui/app/mod.rs:897`; mutex at `crates/codegg-core/src/run_store.rs:1147`, `:1193-1194`,
   `:1212-1223`). The rename-vs-append lost update is provable from the code, not hypothetical.
   No test in this tree asserts cross-instance index integrity.
5. **The legacy project store still has six production callers** that create a file inside the
   cwd's checkout even for read-only commands (`src/main.rs:1775`, `:1816`, `:1878`, `:1920`,
   `:3159`, `:4224`), despite `crates/codegg-core/src/storage/mod.rs:142-143` declaring the store
   production-forbidden. No guard enforces that declaration.
6. **No project-local `.codegg` writer is covered by an existing `scripts/check_*` guard.** The
   change-triggered guard list in `AGENTS.md` covers run-store-adjacent concerns for the queue and
   scheduler, but there is no static inventory guard for `.codegg/**` writers, and the two sinks
   C004 names (run store, legacy DB) are not machine-enumerated.
7. **`UNPROVEN` — the precise call site of the daemon startup worktree reconcile.**
   The worktree reconcile handle is constructed in `src/core/daemon_construct.rs`, and
   `WorktreeService::reconcile_all` exists, but the exact invocation line was not re-read in
   this session, so it is not cited. The *existence* of a startup/recovery worktree path before
   any model turn is established; its exact line is not.
8. **`UNPROVEN` — whether the `ResearchService` and `FileArtifactStore` instances are
   constructed before or during a turn in every caller.** The constructors are proven
   (`src/research/service.rs:53-61`; `src/tool/factory.rs:134-135`), and the tool registry is
   built per session, but a complete enumeration of every construction site was not performed.
9. **`.eggplan` (repository-local administrative state) was not audited as a CodeGG-owned sink.**
   `src/work_plan_repository_binding.rs:76-79` defines `.eggplan` as a repository-local state
   root relative to the canonical workspace root, and `:181-187` confirms the CodeGG run-record
   root is `<workspace>/.codegg/runs`. The `.eggplan` tree itself is written by the Eggplan tool
   program (a model-driven writer), which belongs to the C003 native-writer line rather than to
   the CodeGG-owned state-sink list; it is flagged here so C004 does not double-count it.
10. **`src/core/daemon_project_init.rs` AGENTS.md publication was not audited here.** It is a
    project-root write inside a checkout and is named in C004's own source-evidence paragraph;
    it is outside this file's `.codegg/`-scoped sink list and is flagged rather than silently
    omitted.

---

## Cross-cutting notes

1. **The single most important correction to any "outside checkout" claim:** the run store and
   the legacy project DB are *not* outside the checkout. Both are `root.join(".codegg")` with no
   re-homing branch, and the legacy one is keyed to the **process cwd**
   (`src/main.rs:1771-1775` and the five sibling sites). C004's acceptance bullet "no false
   'outside checkout' claim" is directly satisfiable from these citations.
2. **Two startup-ordering hazards, one shared-reservation hazard.** The run store's parent
   directory is created during *workspace activation* — before a model turn, without a run id
   (`crates/codegg-core/src/workspace_services.rs:407-417`). The legacy DB is created during
   *CLI/TUI/server startup* — before a model turn, without a run id
   (`src/main.rs:3159`, `:4224`). Both are the class C004 D names. The remaining
   `.codegg/`-family writers are model-turn-scoped and therefore face the ordinary
   concurrent-writer hazard instead.
3. **The production daemon is already clean of the legacy sink.** `run_daemon` uses
   `init_migrated_daemon_catalog` over `DaemonPaths::default()` (`src/main.rs:3947-3948`), and
   stdio core does the same (`src/main.rs:4116-4120`). The legacy sink's remaining exposure is
   the six CLI/TUI/server callers listed above, plus the import tooling.
4. **A "shared checkout reservation" concept does not exist in the code at this SHA.** A search of
   `src/` for `shared checkout|shared_checkout|checkout_reservation|SharedCheckout` returned no
   matches. Therefore *every* "Existing coordination" statement above describes coordination
   against something other than a shared-checkout reservation. C004's mediation work has no
   existing fence to hook into; it must introduce one, and the two startup sinks are the
   hardest cases because they run before a run id exists.
5. **The scratch root is the one sink with a genuinely clean outside-checkout guarantee**, because
   it errors out rather than falling back to a relative path (`src/scratch.rs:126`). That is the
   pattern the config/data/plugin roots should adopt (Coverage gap 1).
6. **The atomic-write discipline is real but narrow.** Temp-file + fsync + rename protects
   individual files in the run store (`crates/codegg-core/src/run_store.rs:1235-1239`, `:1254-1259`,
   `:1554-1560`), context artifacts (`src/context/artifact.rs:176-182`) and the source stores.
   It does **not** make multi-writer sequences atomic (the run-store rename-vs-append case), and
   it does not make the *creation* of a project-local path safe (the `.codegg/` directory and
   `sessions.db` creation cases). C004's disposition should distinguish "atomic file write" from
   "checkout-level mediation".
