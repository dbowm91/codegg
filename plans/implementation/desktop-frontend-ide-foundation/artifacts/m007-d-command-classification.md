# M007-D — the command classification and its inventory

This is the classification artifact ADR-0052 asks for: every command the
daemon knows about, the **kind** of each of its argument forms, and the
route each form takes. It is the committed inventory of which daemon-effect
forms have a daemon route and which do not.

A **command** is an entry in the daemon-owned catalog. An **action** is a
frontend-local binding. The two are never the same thing, and the catalog is
never described as a set of actions.

## 1. The measured shape

| measure | count |
|---|---|
| commands | 161 |
| argument forms | 166 |
| daemon-effect forms | 134 |
| frontend-effect forms | 32 |
| daemon-effect forms the daemon's effect table serves | 39 |
| daemon-effect forms whose TUI arm already reaches a daemon request | 81 |
| frontend-effect forms covered by the declared action set | 32 |

The catalog, the declared frontend-action set and the daemon's effect table
are three different things and are proved apart by three different tests:

* `crates/codegg-core/src/command_catalog.rs` — the catalog, and its bounds.
* `crates/codegg-core/src/frontend_action.rs` — the declared frontend-action
  set, closed over the catalog's frontend-effect forms.
* `src/core/daemon_commands.rs` — the daemon's effect table, closed over the
  catalog's daemon-effect forms.

## 2. The three routes a form takes

| route | who serves it | how a frontend reaches it |
|---|---|---|
| **daemon effect, served** | the daemon | `CoreRequest::CommandExecute`; the
  daemon builds the inner request from the scope and the argument, authorizes
  it on its own operation, and audits it as a canonical `Run` |
| **frontend effect** | the frontend | the declared `FrontendAction` for the
  form; it never reaches the daemon |
| **daemon effect, not served** | nobody yet | the TUI keeps its local arm;
  the catalog marks the form `daemon_served: false` so a frontend shows it as
  unavailable instead of dispatching into a refusal |

## 3. The inventory

One row per argument form. `arm` is the TUI dispatcher arm that implements
the form; `requests` are the daemon requests that arm already reaches.

| command | form | kind | domain | route | arm | requests |
|---|---|---|---|---|---|---|
| `/init` | `—` | daemon | System | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:5670` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (28) |
| `/connect` | `—` | frontend | Provider | frontend → `OpenConnectDialog` | `src/tui/app/mod.rs:5963` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (30) |
| `/connections` | `—` | frontend | Provider | frontend → `OpenConnectionSelectionDialog` | `src/tui/app/mod.rs:5967` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (30) |
| `/exit` | `—` | frontend | System | frontend → `Quit` | `src/tui/app/mod.rs:5759` | — |
| `/status` | `—` | daemon | Diagnostics | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:5971` | — |
| `/policy` | `—` | daemon | System | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:5991` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (28) |
| `/approval` | `—` | daemon | System | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6006` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (32) |
| `/sandbox` | `—` | daemon | System | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:6030` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (32) |
| `/network` | `—` | daemon | System | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6056` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (29) |
| `/themes` | `—` | frontend | System | frontend → `SetTheme` | `src/tui/app/mod.rs:6114` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (30) |
| `/help` | `—` | frontend | System | frontend → `OpenHelpDialog` | `src/tui/app/mod.rs:5763` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (30) |
| `/sessions` | `—` | frontend | Session | frontend → `OpenSessionDialog` | `src/tui/app/mod.rs:6487` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (30) |
| `/projects` | `—` | frontend | Project | frontend → `OpenProjectPicker` | `src/tui/app/mod.rs:6490` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (29) |
| `/new` | `—` | daemon | Session | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:5951` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (32) |
| `/stop` | `—` | daemon | Collaboration | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:6493` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (28) |
| `/start` | `—` | daemon | Session | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:6496` | `ApprovalPreferenceGet`, `ChatActionList`, `ChatCapabilities`, `ChatChannelEnsure`, … (57) |
| `/share` | `—` | daemon | Session | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6499` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (31) |
| `/unshare` | `—` | daemon | Session | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6525` | `SessionList`, `SessionUnshare` |
| `/rename` | `—` | frontend | Session | frontend → `ShowAdvisory` | `src/tui/app/mod.rs:6536` | — |
| `/compact` | `—` | daemon | Session | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:5958` | — |
| `/timeline` | `—` | frontend | Session | frontend → `ShowTimeline` | `src/tui/app/mod.rs:6541` | — |
| `/fork` | `—` | frontend | Session | frontend → `ShowAdvisory` | `src/tui/app/mod.rs:6668` | — |
| `/undo` | `—` | frontend | Session | frontend → `UndoLocalMessages` | `src/tui/app/mod.rs:6544` | — |
| `/redo` | `—` | frontend | Session | frontend → `RedoLocalMessages` | `src/tui/app/mod.rs:6551` | — |
| `/export` | `—` | daemon | Session | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6558` | `SessionExport`, `SessionList` |
| `/import` | `—` | daemon | Session | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:6572` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (30) |
| `/timestamps` | `—` | frontend | Session | frontend → `ToggleTimestamps` | `src/tui/app/mod.rs:6579` | — |
| `/thinking` | `—` | frontend | Session | frontend → `ToggleThinking` | `src/tui/app/mod.rs:6588` | — |
| `/reasoning` | `—` | daemon | Session | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6602` | `ReasoningPreferenceSet` |
| `/expand-all` | `—` | frontend | Session | frontend → `ExpandAllDisclosures` | `src/tui/app/mod.rs:6597` | — |
| `/models-refresh` | `—` | daemon | Provider | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6658` | `ModelsRefresh`, `ReasoningPreferenceSet` |
| `/reload` | `—` | daemon | System | daemon → **migrated**: the TUI executes it | `src/tui/app/mod.rs:5683` | `ApprovalPreferenceGet`, `AssetRefresh`, `DocumentChange`, `DocumentClose`, … (28) |
| `/skills` | `—` | daemon | Memory | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:5689` | — |
| `/variants` | `—` | frontend | System | frontend → `ShowAdvisory` | `src/tui/app/mod.rs:6661` | — |
| `/agents` | `—` | frontend | Agent | frontend → `OpenAgentsDialog` | `src/tui/app/mod.rs:5798` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (30) |
| `/agents` | `--all` | daemon | Agent | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:5798` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (30) |
| `/agents` | `show` | daemon | Agent | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:5798` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (30) |
| `/agents` | `diff` | daemon | Agent | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:5798` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (30) |
| `/agents` | `validate` | daemon | Agent | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:5798` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (30) |
| `/agents` | `reload` | daemon | Agent | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:5798` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (30) |
| `/agent` | `—` | daemon | Agent | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:5774` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (30) |
| `/workspaces` | `—` | frontend | Project | frontend → `ShowAdvisory` | `src/tui/app/mod.rs:6680` | — |
| `/tree` | `—` | frontend | Project | frontend → `OpenFileTreeDialog` | `src/tui/app/mod.rs:5770` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (30) |
| `/editor` | `—` | frontend | Project | frontend → `OpenExternalEditor` | `src/tui/app/mod.rs:6711` | — |
| `/open` | `—` | daemon | Project | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:6714` | `LspPreviewApply` |
| `/context` | `—` | frontend | Session | frontend → `OpenContextDialog` | `src/tui/app/mod.rs:6102` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (30) |
| `/cost` | `—` | frontend | Session | frontend → `OpenCostDialog` | `src/tui/app/mod.rs:6106` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (30) |
| `/usage` | `—` | frontend | Session | frontend → `OpenUsageDialog` | `src/tui/app/mod.rs:6110` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (30) |
| `/stats` | `—` | frontend | Session | frontend → `OpenStatsDialog` | `src/tui/app/mod.rs:6120` | — |
| `/queue` | `—` | frontend | Session | frontend → `OpenQueueBox` | `src/tui/app/mod.rs:5677` | `ReasoningPreferenceSet`, `UserPromptQueueList` |
| `/subagent-send` | `—` | daemon | Session | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:5680` | — |
| `/tui` | `—` | frontend | System | frontend → `ToggleFullscreen` | `src/tui/app/mod.rs:6117` | — |
| `/tts` | `—` | frontend | System | frontend → `ToggleTts` | `src/tui/app/mod.rs:6123` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (30) |
| `/loop` | `—` | daemon | Execution | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:6727` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (28) |
| `/tasks` | `—` | daemon | Execution | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6758` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (29) |
| `/task` | `—` | daemon | Execution | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6767` | `ApprovalPreferenceGet`, `SessionList`, `TaskModelPreferenceSet`, `WorkOrderCancel`, … (12) |
| `/workspace` | `—` | frontend | Project | frontend → `OpenWorkspaceDashboard` | `src/tui/app/mod.rs:6770` | `ApprovalPreferenceGet`, `ChatCapabilities`, `ChatChannelEnsure`, `ChatHistory`, … (34) |
| `/schedules` | `—` | daemon | Agent | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6777` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (29) |
| `/task-del` | `—` | daemon | Execution | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:6784` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (29) |
| `/memory` | `—` | daemon | Memory | daemon → **migrated**: the TUI executes it | `src/tui/app/mod.rs:6800` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (29) |
| `/memory-search` | `—` | daemon | Memory | daemon → **migrated**: the TUI executes it | `src/tui/app/mod.rs:6807` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (29) |
| `/memory-list` | `—` | daemon | Memory | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6815` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (29) |
| `/memory-remember` | `—` | daemon | Memory | daemon → **migrated**: the TUI executes it | `src/tui/app/mod.rs:6823` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (29) |
| `/memory-forget` | `—` | daemon | Memory | daemon → **migrated**: the TUI executes it | `src/tui/app/mod.rs:6831` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (29) |
| `/memory-consolidate` | `—` | daemon | Memory | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:6839` | `ReasoningPreferenceSet`, `SessionMessagesLoad` |
| `/habits` | `—` | daemon | Memory | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6842` | — |
| `/habit-dismiss` | `—` | daemon | Memory | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6852` | — |
| `/skill-promote` | `—` | daemon | Memory | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:6866` | `ApprovalPreferenceGet`, `ChatActionList`, `ChatActionSubmit`, `ChatCapabilities`, … (84) |
| `/skill-proposals` | `—` | daemon | Memory | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6928` | — |
| `/skill-proposal` | `—` | daemon | Memory | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6965` | — |
| `/goal` | `—` | daemon | Execution | daemon → **migrated**: the TUI executes it | `src/tui/app/mod.rs:7133` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (36) |
| `/plan` | `—` | daemon | Execution | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7275` | — |
| `/state` | `—` | daemon | Execution | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7389` | — |
| `/diff` | `—` | daemon | GitReview | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:8155` | `ApprovalPreferenceGet`, `ChatCapabilities`, `ChatChannelEnsure`, `ChatEdit`, … (75) |
| `/tests` | `—` | daemon | Execution | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8164` | — |
| `/test` | `—` | daemon | Execution | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:8169` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (30) |
| `/revert` | `—` | daemon | GitReview | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8182` | — |
| `/research` | `—` | daemon | Research | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8313` | — |
| `/research-runs` | `—` | daemon | Research | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:8324` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (30) |
| `/research-open` | `—` | daemon | Research | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:8328` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (30) |
| `/research-show` | `—` | daemon | Research | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:8339` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (30) |
| `/search` | `—` | daemon | Research | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7439` | — |
| `/doctor` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7460` | — |
| `/logs` | `—` | daemon | System | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:7473` | `LspPreviewApply` |
| `/lsp-status` | `—` | daemon | Diagnostics | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7486` | — |
| `/nodes` | `—` | daemon | System | daemon → **migrated**: the TUI executes it | `src/tui/app/mod.rs:7494` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (28) |
| `/lsp-previews` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7501` | — |
| `/lsp-preview` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7510` | — |
| `/lsp-preview-clear` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7527` | — |
| `/lsp-preview-refresh` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7552` | — |
| `/lsp-preview-apply` | `—` | daemon | Diagnostics | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:7584` | `LspPreviewApply`, `ReasoningPreferenceSet` |
| `/review-change` | `—` | daemon | GitReview | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:7579` | `LspPreviewApply` |
| `/lsp-servers` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7668` | — |
| `/lsp-capabilities` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7683` | — |
| `/lsp-errors` | `—` | daemon | Diagnostics | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7705` | — |
| `/lsp-root` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7726` | — |
| `/lsp-restart` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8070` | — |
| `/lsp-stop` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8086` | — |
| `/lsp-cache-status` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8104` | — |
| `/lsp-cache-clear` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8113` | — |
| `/lsp-doctor` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7740` | — |
| `/lsp-context-diagnostics` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7756` | — |
| `/lsp-repair-local` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7773` | — |
| `/lsp-repair-hunk` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7803` | — |
| `/lsp-review-file` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7832` | — |
| `/lsp-review-diff` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7859` | — |
| `/lsp-security-review` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7880` | — |
| `/lsp-impact` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7908` | — |
| `/lsp-test-repair` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7938` | — |
| `/lsp-interface` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7969` | — |
| `/lsp-cross-repair` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8000` | — |
| `/lsp-call-neighbors` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8032` | — |
| `/tool-backends` | `—` | frontend | Provider | frontend → `ShowToolBackends` | `src/tui/app/mod.rs:8134` | — |
| `/security-review` | `—` | daemon | Agent | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:8377` | `ProjectCatalogCapabilities`, `ProjectList`, `ProviderSetupList`, `ReasoningPreferenceSet`, … (5) |
| `/security-review-show` | `—` | daemon | Agent | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:8452` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (30) |
| `/security-review-cancel` | `—` | daemon | Agent | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:8462` | `EggpoolConnectionCancel`, `ReasoningPreferenceSet` |
| `/shell-list` | `—` | daemon | Execution | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8542` | — |
| `/shell-show` | `—` | daemon | Execution | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8574` | — |
| `/shell-include` | `—` | daemon | Execution | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:8466` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (32) |
| `/shell-rerun` | `—` | daemon | Execution | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8510` | — |
| `/shell-kill` | `—` | daemon | Execution | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8526` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (28) |
| `/shell-ask` | `—` | daemon | Execution | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:8869` | `ApprovalPreferenceGet`, `ChatCapabilities`, `ChatChannelEnsure`, `ChatEdit`, … (82) |
| `/shell-expand` | `—` | daemon | Execution | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8590` | — |
| `/terminal` | `—` | daemon | Execution | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:8665` | `InteractiveProcessAttach`, `InteractiveProcessCreate`, `InteractiveProcessDetach`, `InteractiveProcessInput`, … (6) |
| `/terminal-clear` | `—` | daemon | Execution | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:8699` | `InteractiveProcessAttach`, `InteractiveProcessCreate`, `InteractiveProcessDetach`, `InteractiveProcessInput`, … (6) |
| `/terminal-create` | `—` | daemon | Execution | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:8620` | `InteractiveProcessAttach`, `InteractiveProcessCreate`, `InteractiveProcessDetach`, `InteractiveProcessInput`, … (6) |
| `/terminal-list` | `—` | daemon | Execution | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8634` | `InteractiveProcessAttach`, `InteractiveProcessCreate`, `InteractiveProcessDetach`, `InteractiveProcessInput`, … (7) |
| `/terminal-attach` | `—` | daemon | Execution | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:8637` | `InteractiveProcessAttach`, `InteractiveProcessCreate`, `InteractiveProcessDetach`, `InteractiveProcessInput`, … (6) |
| `/terminal-show` | `—` | daemon | Execution | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8650` | — |
| `/terminal-focus` | `—` | frontend | Execution | frontend → `FocusTerminal` | `src/tui/app/mod.rs:8726` | — |
| `/terminal-send` | `—` | daemon | Execution | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8735` | `InteractiveProcessAttach`, `InteractiveProcessCreate`, `InteractiveProcessDetach`, `InteractiveProcessInput`, … (6) |
| `/terminal-resize` | `—` | daemon | Execution | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:8752` | `InteractiveProcessAttach`, `InteractiveProcessCreate`, `InteractiveProcessDetach`, `InteractiveProcessInput`, … (6) |
| `/terminal-resume` | `—` | daemon | Execution | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:8778` | `InteractiveProcessAttach`, `InteractiveProcessCreate`, `InteractiveProcessDetach`, `InteractiveProcessInput`, … (6) |
| `/terminal-detach` | `—` | daemon | Execution | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:8787` | `InteractiveProcessAttach`, `InteractiveProcessCreate`, `InteractiveProcessDetach`, `InteractiveProcessInput`, … (6) |
| `/terminal-terminate` | `—` | daemon | Execution | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:8796` | `InteractiveProcessAttach`, `InteractiveProcessCreate`, `InteractiveProcessDetach`, `InteractiveProcessInput`, … (7) |
| `/terminal-remove` | `—` | daemon | Execution | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:8805` | `InteractiveProcessAttach`, `InteractiveProcessCreate`, `InteractiveProcessDetach`, `InteractiveProcessInput`, … (7) |
| `/plugins` | `—` | daemon | System | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8617` | — |
| `/plugin-info` | `—` | daemon | System | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8814` | — |
| `/plugin-enable` | `—` | daemon | System | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8824` | — |
| `/plugin-disable` | `—` | daemon | System | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8834` | — |
| `/plugin-doctor` | `—` | daemon | System | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8844` | — |
| `/plugin-remove` | `—` | daemon | System | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8849` | — |
| `/plugin-install` | `—` | daemon | System | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8859` | — |
| `/collaborators` | `—` | daemon | Collaboration | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6126` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (29) |
| `/team` | `—` | daemon | System | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6142` | `ApprovalPreferenceGet`, `ChatChannelPolicySet`, `ChatPolicyGet`, `ChatProjectPolicySet`, … (41) |
| `/control` | `—` | daemon | Session | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6154` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (32) |
| `/observe` | `—` | daemon | Collaboration | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6167` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (32) |
| `/stop-observing` | `—` | daemon | Collaboration | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:6197` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (29) |
| `/panels` | `—` | frontend | System | frontend → `OpenPanelManager` | `src/tui/app/mod.rs:6203` | — |
| `/chat` | `—` | frontend | Collaboration | frontend → `FocusChat` | `src/tui/app/mod.rs:6215` | `ApprovalPreferenceGet`, `ChatCapabilities`, `ChatChannelEnsure`, `ChatHistory`, … (34) |
| `/chat-send` | `—` | daemon | Collaboration | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:6227` | `ApprovalPreferenceGet`, `ChatCapabilities`, `ChatChannelEnsure`, `ChatSend`, … (30) |
| `/chat-reply` | `—` | daemon | Collaboration | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:6240` | `ApprovalPreferenceGet`, `ChatCapabilities`, `ChatChannelEnsure`, `ChatSend`, … (30) |
| `/chat-history` | `—` | daemon | Collaboration | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:6271` | `ApprovalPreferenceGet`, `ChatCapabilities`, `ChatChannelEnsure`, `ChatHistory`, … (31) |
| `/chat-sync` | `—` | daemon | Collaboration | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:6291` | `ApprovalPreferenceGet`, `ChatCapabilities`, `ChatChannelEnsure`, `ChatHistory`, … (32) |
| `/chat-read` | `—` | daemon | Collaboration | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:6303` | `ApprovalPreferenceGet`, `ChatReadSet`, `DocumentChange`, `DocumentClose`, … (28) |
| `/chat-edit` | `—` | daemon | Collaboration | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:6315` | `ApprovalPreferenceGet`, `ChatCapabilities`, `ChatEdit`, `DocumentChange`, … (29) |
| `/chat-redact` | `—` | daemon | Collaboration | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:6345` | `ApprovalPreferenceGet`, `ChatCapabilities`, `ChatRedact`, `DocumentChange`, … (29) |
| `/chat-composing` | `—` | daemon | Collaboration | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:6370` | `ApprovalPreferenceGet`, `ChatComposingList`, `ChatComposingSet`, `DocumentChange`, … (29) |
| `/chat-action-task` | `—` | daemon | Collaboration | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:6393` | `ApprovalPreferenceGet`, `ChatActionSubmit`, `ChatCapabilities`, `ChatChannelEnsure`, … (30) |
| `/chat-action-review` | `—` | daemon | Collaboration | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:6429` | `ApprovalPreferenceGet`, `ChatActionSubmit`, `ChatCapabilities`, `ChatChannelEnsure`, … (30) |
| `/chat-action-list` | `—` | daemon | Collaboration | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:6464` | `ApprovalPreferenceGet`, `ChatActionList`, `ChatCapabilities`, `ChatChannelEnsure`, … (32) |
| `/edit-undo` | `—` | daemon | Session | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:8191` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (30) |
| `/edit-reapply` | `—` | daemon | Session | daemon → not served (the TUI arm reaches the daemon directly) | `src/tui/app/mod.rs:8237` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (30) |
| `/edit-checkpoints` | `—` | daemon | Session | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8283` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (29) |
| `/tool-contracts` | `—` | daemon | Provider | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7477` | — |
| `/worktree` | `—` | daemon | System | daemon → served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6685` | `ApprovalPreferenceGet`, `DocumentChange`, `DocumentClose`, `DocumentOpen`, … (28) |

## 4. The families

The regression coverage is organised by family. A family is the catalog's
`CommandDomain`: a stable, user-facing task grouping that the catalog
already carries, so the coverage list and the catalog cannot drift apart.

| family | forms | daemon effect | frontend effect | served | reaches the daemon | no route |
|---|---|---|---|---|---|---|
| Agent | 11 | 10 | 1 | 1 | 9 | 0 |
| Collaboration | 16 | 15 | 1 | 2 | 13 | 0 |
| Diagnostics | 28 | 28 | 0 | 3 | 1 | 24 |
| Execution | 29 | 28 | 1 | 11 | 14 | 3 |
| GitReview | 3 | 3 | 0 | 0 | 2 | 1 |
| Memory | 12 | 12 | 0 | 6 | 2 | 4 |
| Project | 6 | 1 | 5 | 0 | 1 | 0 |
| Provider | 5 | 2 | 3 | 2 | 0 | 0 |
| Research | 5 | 5 | 0 | 0 | 3 | 2 |
| Session | 27 | 13 | 14 | 6 | 5 | 2 |
| System | 24 | 17 | 7 | 8 | 2 | 7 |

## 5. The daemon-effect forms with no daemon route

These are the forms the daemon does not serve yet. They are recorded here
rather than left implicit, because the plan's Work package F asks for the
inventory of exactly this set.

| command | form | family | why the daemon does not serve it |
|---|---|---|---|
| `/sandbox` | `—` | System | no daemon request serves this effect |
| `/new` | `—` | Session | no daemon request serves this effect |
| `/stop` | `—` | Collaboration | no daemon request serves this effect |
| `/start` | `—` | Session | no daemon request serves this effect |
| `/compact` | `—` | Session | the daemon has no compaction request; `TuiCommand::CompactSession` has no sender |
| `/import` | `—` | Session | no daemon request serves this effect |
| `/agents` | `--all` | Agent | no daemon request serves this effect |
| `/agents` | `show` | Agent | no daemon request serves this effect |
| `/agents` | `diff` | Agent | no daemon request serves this effect |
| `/agents` | `validate` | Agent | no daemon request serves this effect |
| `/agents` | `reload` | Agent | no daemon request serves this effect |
| `/agent` | `—` | Agent | no daemon request serves this effect |
| `/open` | `—` | Project | no daemon request serves this effect |
| `/subagent-send` | `—` | Session | the effect runs a local tool or an agent turn the daemon does not own |
| `/loop` | `—` | Execution | the effect runs a local tool or an agent turn the daemon does not own |
| `/task-del` | `—` | Execution | no daemon request serves this effect |
| `/memory-consolidate` | `—` | Memory | no daemon request serves this effect |
| `/habits` | `—` | Memory | the catalogue the frontend reads is not a daemon-owned store |
| `/habit-dismiss` | `—` | Memory | no daemon request serves this effect |
| `/skill-promote` | `—` | Memory | no daemon request serves this effect |
| `/skill-proposals` | `—` | Memory | the catalogue the frontend reads is not a daemon-owned store |
| `/skill-proposal` | `—` | Memory | the catalogue the frontend reads is not a daemon-owned store |
| `/plan` | `—` | Execution | the effect runs a local tool or an agent turn the daemon does not own |
| `/diff` | `—` | GitReview | the effect runs a local tool or an agent turn the daemon does not own |
| `/tests` | `—` | Execution | the effect runs a local tool or an agent turn the daemon does not own |
| `/test` | `—` | Execution | the effect runs a local tool or an agent turn the daemon does not own |
| `/revert` | `—` | GitReview | the effect runs a local tool or an agent turn the daemon does not own |
| `/research` | `—` | Research | the effect runs a local tool or an agent turn the daemon does not own |
| `/research-runs` | `—` | Research | no daemon request serves this effect |
| `/research-open` | `—` | Research | no daemon request serves this effect |
| `/research-show` | `—` | Research | no daemon request serves this effect |
| `/search` | `—` | Research | the effect runs a local tool or an agent turn the daemon does not own |
| `/doctor` | `—` | Diagnostics | the effect runs a local tool or an agent turn the daemon does not own |
| `/logs` | `—` | System | no daemon request serves this effect |
| `/lsp-previews` | `—` | Diagnostics | the daemon has no LSP workflow request; the recipe runs in the frontend's LSP tool, which is absent in the daemon-connected configuration |
| `/lsp-preview` | `—` | Diagnostics | the daemon has no LSP workflow request; the recipe runs in the frontend's LSP tool, which is absent in the daemon-connected configuration |
| `/lsp-preview-clear` | `—` | Diagnostics | the daemon has no LSP workflow request; the recipe runs in the frontend's LSP tool, which is absent in the daemon-connected configuration |
| `/lsp-preview-refresh` | `—` | Diagnostics | the daemon has no LSP workflow request; the recipe runs in the frontend's LSP tool, which is absent in the daemon-connected configuration |
| `/lsp-preview-apply` | `—` | Diagnostics | the daemon has no LSP workflow request; the recipe runs in the frontend's LSP tool, which is absent in the daemon-connected configuration |
| `/review-change` | `—` | GitReview | no daemon request serves this effect |
| `/lsp-servers` | `—` | Diagnostics | the daemon has no LSP workflow request; the recipe runs in the frontend's LSP tool, which is absent in the daemon-connected configuration |
| `/lsp-capabilities` | `—` | Diagnostics | the daemon has no LSP workflow request; the recipe runs in the frontend's LSP tool, which is absent in the daemon-connected configuration |
| `/lsp-root` | `—` | Diagnostics | the daemon has no LSP workflow request; the recipe runs in the frontend's LSP tool, which is absent in the daemon-connected configuration |
| `/lsp-restart` | `—` | Diagnostics | the daemon has no LSP workflow request; the recipe runs in the frontend's LSP tool, which is absent in the daemon-connected configuration |
| `/lsp-stop` | `—` | Diagnostics | the daemon has no LSP workflow request; the recipe runs in the frontend's LSP tool, which is absent in the daemon-connected configuration |
| `/lsp-cache-status` | `—` | Diagnostics | the daemon has no LSP workflow request; the recipe runs in the frontend's LSP tool, which is absent in the daemon-connected configuration |
| `/lsp-cache-clear` | `—` | Diagnostics | the daemon has no LSP workflow request; the recipe runs in the frontend's LSP tool, which is absent in the daemon-connected configuration |
| `/lsp-doctor` | `—` | Diagnostics | the daemon has no LSP workflow request; the recipe runs in the frontend's LSP tool, which is absent in the daemon-connected configuration |
| `/lsp-context-diagnostics` | `—` | Diagnostics | the daemon has no LSP workflow request; the recipe runs in the frontend's LSP tool, which is absent in the daemon-connected configuration |
| `/lsp-repair-local` | `—` | Diagnostics | the daemon has no LSP workflow request; the recipe runs in the frontend's LSP tool, which is absent in the daemon-connected configuration |
| `/lsp-repair-hunk` | `—` | Diagnostics | the daemon has no LSP workflow request; the recipe runs in the frontend's LSP tool, which is absent in the daemon-connected configuration |
| `/lsp-review-file` | `—` | Diagnostics | the daemon has no LSP workflow request; the recipe runs in the frontend's LSP tool, which is absent in the daemon-connected configuration |
| `/lsp-review-diff` | `—` | Diagnostics | the daemon has no LSP workflow request; the recipe runs in the frontend's LSP tool, which is absent in the daemon-connected configuration |
| `/lsp-security-review` | `—` | Diagnostics | the daemon has no LSP workflow request; the recipe runs in the frontend's LSP tool, which is absent in the daemon-connected configuration |
| `/lsp-impact` | `—` | Diagnostics | the daemon has no LSP workflow request; the recipe runs in the frontend's LSP tool, which is absent in the daemon-connected configuration |
| `/lsp-test-repair` | `—` | Diagnostics | the daemon has no LSP workflow request; the recipe runs in the frontend's LSP tool, which is absent in the daemon-connected configuration |
| `/lsp-interface` | `—` | Diagnostics | the daemon has no LSP workflow request; the recipe runs in the frontend's LSP tool, which is absent in the daemon-connected configuration |
| `/lsp-cross-repair` | `—` | Diagnostics | the daemon has no LSP workflow request; the recipe runs in the frontend's LSP tool, which is absent in the daemon-connected configuration |
| `/lsp-call-neighbors` | `—` | Diagnostics | the daemon has no LSP workflow request; the recipe runs in the frontend's LSP tool, which is absent in the daemon-connected configuration |
| `/security-review` | `—` | Agent | no daemon request serves this effect |
| `/security-review-show` | `—` | Agent | no daemon request serves this effect |
| `/security-review-cancel` | `—` | Agent | no daemon request serves this effect |
| `/shell-include` | `—` | Execution | no daemon request serves this effect |
| `/shell-rerun` | `—` | Execution | no daemon request serves this effect |
| `/shell-ask` | `—` | Execution | no daemon request serves this effect |
| `/terminal` | `—` | Execution | no daemon request serves this effect |
| `/terminal-clear` | `—` | Execution | no daemon request serves this effect |
| `/terminal-create` | `—` | Execution | no daemon request serves this effect |
| `/terminal-attach` | `—` | Execution | no daemon request serves this effect |
| `/terminal-resize` | `—` | Execution | no daemon request serves this effect |
| `/terminal-resume` | `—` | Execution | no daemon request serves this effect |
| `/terminal-detach` | `—` | Execution | no daemon request serves this effect |
| `/terminal-terminate` | `—` | Execution | no daemon request serves this effect |
| `/terminal-remove` | `—` | Execution | no daemon request serves this effect |
| `/plugins` | `—` | System | the daemon has no plugin request surface in this milestone |
| `/plugin-info` | `—` | System | the daemon has no plugin request surface in this milestone |
| `/plugin-enable` | `—` | System | the daemon has no plugin request surface in this milestone |
| `/plugin-disable` | `—` | System | the daemon has no plugin request surface in this milestone |
| `/plugin-doctor` | `—` | System | the daemon has no plugin request surface in this milestone |
| `/plugin-remove` | `—` | System | the daemon has no plugin request surface in this milestone |
| `/plugin-install` | `—` | System | the daemon has no plugin request surface in this milestone |
| `/stop-observing` | `—` | Collaboration | no daemon request serves this effect |
| `/chat-send` | `—` | Collaboration | no daemon request serves this effect |
| `/chat-reply` | `—` | Collaboration | no daemon request serves this effect |
| `/chat-history` | `—` | Collaboration | no daemon request serves this effect |
| `/chat-sync` | `—` | Collaboration | no daemon request serves this effect |
| `/chat-read` | `—` | Collaboration | no daemon request serves this effect |
| `/chat-edit` | `—` | Collaboration | no daemon request serves this effect |
| `/chat-redact` | `—` | Collaboration | no daemon request serves this effect |
| `/chat-composing` | `—` | Collaboration | no daemon request serves this effect |
| `/chat-action-task` | `—` | Collaboration | no daemon request serves this effect |
| `/chat-action-review` | `—` | Collaboration | no daemon request serves this effect |
| `/chat-action-list` | `—` | Collaboration | no daemon request serves this effect |
| `/edit-undo` | `—` | Session | no daemon request serves this effect |
| `/edit-reapply` | `—` | Session | no daemon request serves this effect |

## 6. What the daemon serves

The daemon's effect table, one row per served form. The argument convention
is uniform: the first whitespace-separated word of the command's argument
is the identifier, and the remainder is the payload.

| command | form | the daemon request it builds |
|---|---|---|
| `/reload` | `—` | `asset_refresh` |
| `/project_init` | `—` | `project_init_draft_get` |
| `/policy` | `—` | `execution_policy_get` |
| `/approval` | `—` | `approval_preference_get` |
| `/reasoning` | `—` | `reasoning_preference_get` |
| `/network` | `—` | `network_intent_set` |
| `/skills` | `—` | `skill_catalogue_list` |
| `/models_refresh` | `—` | `models_refresh` |
| `/nodes` | `—` | `node_telemetry` |
| `/worktree` | `—` | `managed_worktree_list` |
| `/tool_contracts` | `—` | `tool_program_list` |
| `/share` | `—` | `session_share` |
| `/unshare` | `—` | `session_unshare` |
| `/export` | `—` | `session_export` |
| `/status` | `—` | `session_lifecycle_get` |
| `/tasks` | `—` | `work_order_list` |
| `/task` | `—` | `work_order_list` |
| `/schedules` | `—` | `schedule_list` |
| `/goal` | `—` | `goal_show` |
| `/state` | `—` | `active_goal_load` |
| `/memory` | `—` | `memory_list` |
| `/memory_list` | `—` | `memory_list` |
| `/memory_search` | `—` | `memory_search` |
| `/memory_remember` | `—` | `memory_remember` |
| `/memory_forget` | `—` | `memory_forget` |
| `/collaborators` | `—` | `team_principal_list` |
| `/team` | `—` | `team_membership_list` |
| `/control` | `—` | `session_control_get` |
| `/observe` | `—` | `projection_subscribe` |
| `/edit_checkpoints` | `—` | `edit_checkpoint_list` |
| `/shell_list` | `—` | `human_shell_run_capabilities` |
| `/shell_show` | `—` | `human_shell_run_read` |
| `/shell_expand` | `—` | `human_shell_run_read` |
| `/shell_kill` | `—` | `human_shell_run_cancel` |
| `/terminal_list` | `—` | `interactive_process_list` |
| `/terminal_show` | `—` | `interactive_process_list` |
| `/terminal_send` | `—` | `interactive_process_input` |
| `/lsp_status` | `—` | `lsp_diagnostics_get` |
| `/lsp_errors` | `—` | `lsp_diagnostics_get` |

## 7. The TUI migration

A daemon-effect command is *migrated* when the TUI executes it through
`CoreRequest::CommandExecute` and the daemon's inner response feeds the
arm's existing presentation code. The observable outcome does not change:
the daemon owns the effect and the authoritative data, and the frontend
keeps the formatting.

| command | family | the TUI effect call |
|---|---|---|
| `/goal` | Execution | `CoreRequest::CommandExecute` (`goal`, form `—`) |
| `/memory` | Memory | `CoreRequest::CommandExecute` (`memory`, form `—`) |
| `/memory-forget` | Memory | `CoreRequest::CommandExecute` (`memory_forget`, form `—`) |
| `/memory-remember` | Memory | `CoreRequest::CommandExecute` (`memory_remember`, form `—`) |
| `/memory-search` | Memory | `CoreRequest::CommandExecute` (`memory_search`, form `—`) |
| `/nodes` | System | `CoreRequest::CommandExecute` (`nodes`, form `—`) |
| `/reload` | System | `CoreRequest::CommandExecute` (`reload`, form `—`) |

**7 of the 134 daemon-effect forms are migrated.**
The rest keep their local arm, and each one is listed above with the route
it takes. The migration is one site per command: the request construction
becomes a `CommandExecute` and the response is unwrapped, so the
presentation code around it is unchanged.

The regression coverage is organised by family and lives in
`src/tui/command_route_tests.rs` (the TUI's side) and
`tests/core_daemon/command_catalog_m007d.inc` (the daemon's side).
The family list is the coverage's own constant, so a migrated command
with no test fails its family.

## 8. The recorded discrepancies

* **`/compact` keeps its current behaviour.** The arm toasts and the daemon
  has no compaction request, so the form is in the "no daemon route" set
  above. The behaviour is unchanged and the gap is directed to a separate
  corrective milestone.
* **The composer's `/` completion overlay deviates.** The composer's
  completion overlay lists the TUI's own registry; the daemon's catalog is
  what the palette reads. A user is expected to use the `mod+shift+p`
  palette for the daemon's catalog. The overlay is recorded here, not
  changed.
* **`command_run` is uninstrumented.** The effect's own operation is audited;
  the command dispatch is audited as the canonical `Run`. A second
  instrumentation point on the dispatch would audit the same decision twice.

## 9. Two namespaces, and how the palette resolves an action id

The declared frontend-effect set and the desktop's keymap both hold
"actions", and they are **two different namespaces**. The plan's
terminology amendment (plan 016) makes an *action* a frontend-local
binding; it does not make every action id mean the same thing.

| namespace | what an id names | where it lives | who resolves it |
|---|---|---|---|
| the declared frontend-effect set | the action that serves a **catalog command form** | `crates/codegg-core/src/frontend_action.rs` — 29 `FrontendAction` variants, keyed by `(command_id, form)` | the palette, through `action_for_form(command_id, form)` |
| the desktop keymap | a **gesture** the desktop dispatches | `apps/desktop/src/keymap.ts` — the editor's 22 `editor.*` ids, the palette's `command.palette.open`, and the rest | the desktop's one dispatch, in `App.tsx` |

**How the palette resolves an action id.** It does not resolve a keymap id.
Every row the palette shows comes from the **daemon's catalog**, so the
palette's own resolution is:

1. the typed line selects a catalog **command** and one of its argument
   **forms**;
2. a frontend-effect form resolves to its declared `FrontendAction` through
   `action_for_form(command_id, form)` — the catalog's identity, never a
   string compared against a keymap id;
3. a daemon-effect form resolves to `CoreRequest::CommandExecute`, or is
   shown as unavailable when the daemon's catalog says `daemon_served:
   false`.

**What happens when an id is in the keymap but not in the declared set.**
Nothing: the two sets are never joined. A keymap id such as
`editor.cursor.left` is an editor gesture, not a command effect, and the
palette never sees it — the palette's rows come from the catalog, and the
catalog holds no editor gesture. A keymap id such as `command.palette.open`
opens the palette and is not itself a command effect either. There is no
path by which a keymap id could be dispatched as a command's effect, so a
new editor action id can never be mistaken for a command, and a new
command can never be mistaken for an editor gesture.

The editor's commands go through the daemon's **document path** — a
different set of `CoreRequest` variants from `CommandExecute` — which is
the same separation seen from the other side.
