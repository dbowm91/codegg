# M007-D — the command classification and its inventory

This is the classification artifact ADR-0052 asks for: every command the
daemon knows about, the **kind** of each of its argument forms, and the
route each form takes. It is the committed inventory of which daemon-effect
forms have a daemon route and which do not.

A **command** is an entry in the daemon-owned catalog. An **action** is a
frontend-local binding. The two are never the same thing, and the catalog is
never described as a set of actions.

**This file is generated.** `generate_m007d_command_classification.py`, next
to it, reads the catalog, the declared frontend-action set, the daemon's
effect table, the TUI's registry and the TUI's dispatcher, and writes every
number below. The summary table in §1 is computed from the §3 inventory the
same script writes, so the two cannot drift. Re-run it after changing any of
those sources:

```
python3 plans/implementation/desktop-frontend-ide-foundation/artifacts/\
generate_m007d_command_classification.py
```

Add `--check` to derive the artifact again and report whether it is still in
step with the tree, without writing over it:

```
python3 plans/implementation/desktop-frontend-ide-foundation/artifacts/\
generate_m007d_command_classification.py --check
```

## 1. The measured shape

| measure | count |
|---|---|
| commands | 161 |
| argument forms | 167 |
| daemon-effect forms | 124 |
| frontend-effect forms | 43 |
| daemon-effect forms the daemon's effect table serves | 73 |
| daemon-effect forms whose TUI arm issues a daemon request | 23 |
| daemon-effect forms whose TUI arm issues no daemon request | 28 |
| frontend-effect forms covered by the declared action set | 43 |

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
  it on its own operation, and audits it as a canonical `Run`. The inner
  response travels back verbatim in the result, so **both** frontends format
  the same authoritative data |
| **frontend effect** | the frontend | the declared `FrontendAction` for the
  form; it never reaches the daemon |
| **daemon effect, not served** | nobody yet | the TUI keeps its local arm;
  the catalog marks the form `daemon_served: false` so a frontend shows it as
  unavailable instead of dispatching into a refusal |

## 3. The inventory

One row per argument form. `arm` is the TUI dispatcher arm that implements
the form; `requests` are the daemon requests that arm issues, and `—` when
it issues none.

| command | form | kind | domain | route | arm | requests |
|---|---|---|---|---|---|---|
| `/init` | `—` | daemon | System | daemon → served | `src/tui/app/mod.rs:5698` | `CommandExecute` |
| `/connect` | `—` | frontend | Provider | frontend → `OpenConnectDialog` | `src/tui/app/mod.rs:5991` | — |
| `/connections` | `—` | frontend | Provider | frontend → `OpenConnectionSelectionDialog` | `src/tui/app/mod.rs:5995` | — |
| `/exit` | `—` | frontend | System | frontend → `Quit` | `src/tui/app/mod.rs:5787` | — |
| `/status` | `—` | frontend | Diagnostics | frontend → `ShowStatus` | `src/tui/app/mod.rs:5999` | — |
| `/policy` | `—` | daemon | System | daemon → served | `src/tui/app/mod.rs:6019` | `CommandExecute` |
| `/approval` | `—` | daemon | System | daemon → served | `src/tui/app/mod.rs:6034` | `CommandExecute`, `RuntimePolicySet` |
| `/sandbox` | `—` | daemon | System | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6058` | `CommandExecute`, `RuntimePolicySet` |
| `/network` | `—` | daemon | System | daemon → served | `src/tui/app/mod.rs:6084` | `CommandExecute` |
| `/themes` | `—` | frontend | System | frontend → `SetTheme` | `src/tui/app/mod.rs:6142` | — |
| `/help` | `—` | frontend | System | frontend → `OpenHelpDialog` | `src/tui/app/mod.rs:5791` | — |
| `/sessions` | `—` | frontend | Session | frontend → `OpenSessionDialog` | `src/tui/app/mod.rs:6515` | — |
| `/projects` | `—` | frontend | Project | frontend → `OpenProjectPicker` | `src/tui/app/mod.rs:6518` | `ProjectList` |
| `/new` | `—` | daemon | Session | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:5979` | `SessionCreate` |
| `/stop` | `—` | daemon | Collaboration | daemon → served | `src/tui/app/mod.rs:6521` | `CommandExecute` |
| `/start` | `—` | daemon | Session | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6524` | `ChatChannelEnsure`, `ChatEdit`, `ChatHistory`, `ChatSend`, `CommandExecute`, `ProjectionSubscribe`, … (11) |
| `/share` | `—` | daemon | Session | daemon → served | `src/tui/app/mod.rs:6527` | `CommandExecute` |
| `/unshare` | `—` | daemon | Session | daemon → served | `src/tui/app/mod.rs:6553` | `CommandExecute` |
| `/rename` | `—` | frontend | Session | frontend → `ShowAdvisory` | `src/tui/app/mod.rs:6564` | — |
| `/compact` | `—` | frontend | Session | frontend → `CompactSession` | `src/tui/app/mod.rs:5986` | — |
| `/timeline` | `—` | frontend | Session | frontend → `ShowTimeline` | `src/tui/app/mod.rs:6569` | — |
| `/fork` | `—` | frontend | Session | frontend → `ShowAdvisory` | `src/tui/app/mod.rs:6696` | — |
| `/undo` | `—` | frontend | Session | frontend → `UndoLocalMessages` | `src/tui/app/mod.rs:6572` | — |
| `/redo` | `—` | frontend | Session | frontend → `RedoLocalMessages` | `src/tui/app/mod.rs:6579` | — |
| `/export` | `—` | daemon | Session | daemon → served | `src/tui/app/mod.rs:6586` | `CommandExecute` |
| `/import` | `—` | daemon | Session | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6600` | — |
| `/timestamps` | `—` | frontend | Session | frontend → `ToggleTimestamps` | `src/tui/app/mod.rs:6607` | — |
| `/thinking` | `—` | frontend | Session | frontend → `ToggleThinking` | `src/tui/app/mod.rs:6616` | — |
| `/reasoning` | `—` | daemon | Session | daemon → served | `src/tui/app/mod.rs:6630` | `ReasoningPreferenceSet` |
| `/expand-all` | `—` | frontend | Session | frontend → `ExpandAllDisclosures` | `src/tui/app/mod.rs:6625` | — |
| `/models-refresh` | `—` | daemon | Provider | daemon → served | `src/tui/app/mod.rs:6686` | `CommandExecute` |
| `/reload` | `—` | daemon | System | daemon → served | `src/tui/app/mod.rs:5711` | `CommandExecute` |
| `/skills` | `—` | frontend | Memory | frontend → `ShowSkills` | `src/tui/app/mod.rs:5717` | — |
| `/variants` | `—` | frontend | System | frontend → `ShowAdvisory` | `src/tui/app/mod.rs:6689` | — |
| `/agents` | `—` | frontend | Agent | frontend → `OpenAgentsDialog` | `src/tui/app/mod.rs:5826` | — |
| `/agents` | `--all` | daemon | Agent | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:5826` | — |
| `/agents` | `show` | daemon | Agent | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:5826` | — |
| `/agents` | `diff` | daemon | Agent | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:5826` | — |
| `/agents` | `validate` | daemon | Agent | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:5826` | — |
| `/agents` | `reload` | daemon | Agent | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:5826` | — |
| `/agent` | `—` | daemon | Agent | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:5802` | — |
| `/workspaces` | `—` | frontend | Project | frontend → `ShowAdvisory` | `src/tui/app/mod.rs:6708` | — |
| `/tree` | `—` | frontend | Project | frontend → `OpenFileTreeDialog` | `src/tui/app/mod.rs:5798` | — |
| `/editor` | `—` | frontend | Project | frontend → `OpenExternalEditor` | `src/tui/app/mod.rs:6739` | — |
| `/open` | `—` | daemon | Project | daemon → served | `src/tui/app/mod.rs:6742` | `CommandExecute`, `DocumentWriterAcquire` |
| `/context` | `—` | frontend | Session | frontend → `OpenContextDialog` | `src/tui/app/mod.rs:6130` | — |
| `/cost` | `—` | frontend | Session | frontend → `OpenCostDialog` | `src/tui/app/mod.rs:6134` | — |
| `/usage` | `—` | frontend | Session | frontend → `OpenUsageDialog` | `src/tui/app/mod.rs:6138` | — |
| `/stats` | `—` | frontend | Session | frontend → `OpenStatsDialog` | `src/tui/app/mod.rs:6148` | — |
| `/queue` | `—` | frontend | Session | frontend → `OpenQueueBox` | `src/tui/app/mod.rs:5705` | — |
| `/subagent-send` | `—` | daemon | Session | daemon → served | `src/tui/app/mod.rs:5708` | — |
| `/tui` | `—` | frontend | System | frontend → `ToggleFullscreen` | `src/tui/app/mod.rs:6145` | — |
| `/tts` | `—` | frontend | System | frontend → `ToggleTts` | `src/tui/app/mod.rs:6151` | `NotificationSpeak` |
| `/loop` | `—` | daemon | Execution | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6759` | `ScheduleCreate` |
| `/tasks` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:6790` | `CommandExecute`, `ScheduleGet`, `WorkOrderLaneList`, `WorkOrderSummary` |
| `/task` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:6799` | `CommandExecute`, `WorkOrderLaneList`, `WorkOrderSummary` |
| `/workspace` | `—` | frontend | Project | frontend → `OpenWorkspaceDashboard` | `src/tui/app/mod.rs:6802` | `ChatChannelEnsure`, `ChatHistory`, `CommandExecute`, `WorkspaceDashboard` |
| `/schedules` | `—` | daemon | Agent | daemon → served | `src/tui/app/mod.rs:6809` | `CommandExecute`, `ScheduleGet` |
| `/task-del` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:6816` | `CommandExecute` |
| `/memory` | `—` | daemon | Memory | daemon → served | `src/tui/app/mod.rs:6832` | `CommandExecute`, `SessionMessagesLoad` |
| `/memory-search` | `—` | daemon | Memory | daemon → served | `src/tui/app/mod.rs:6844` | `CommandExecute`, `SessionMessagesLoad` |
| `/memory-list` | `—` | daemon | Memory | daemon → served | `src/tui/app/mod.rs:6852` | `CommandExecute`, `SessionMessagesLoad` |
| `/memory-remember` | `—` | daemon | Memory | daemon → served | `src/tui/app/mod.rs:6865` | `CommandExecute`, `SessionMessagesLoad` |
| `/memory-forget` | `—` | daemon | Memory | daemon → served | `src/tui/app/mod.rs:6873` | `CommandExecute`, `SessionMessagesLoad` |
| `/memory-consolidate` | `—` | daemon | Memory | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6881` | `SessionMessagesLoad` |
| `/habits` | `—` | daemon | Memory | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6884` | — |
| `/habit-dismiss` | `—` | daemon | Memory | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6894` | — |
| `/skill-promote` | `—` | daemon | Memory | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6908` | `ChatChannelEnsure`, `ChatEdit`, `ChatHistory`, `ChatSend`, `CommandExecute`, `ProjectionSubscribe`, … (10) |
| `/skill-proposals` | `—` | daemon | Memory | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6970` | — |
| `/skill-proposal` | `—` | daemon | Memory | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7007` | — |
| `/goal` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:7175` | `CommandExecute`, `GoalCheckpoint`, `GoalClear`, `GoalDone`, `GoalFromFile`, `GoalPause`, … (9) |
| `/plan` | `—` | daemon | Execution | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7317` | — |
| `/state` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:7431` | — |
| `/diff` | `—` | daemon | GitReview | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8258` | — |
| `/tests` | `—` | daemon | Execution | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8267` | — |
| `/test` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:8272` | `CommandExecute`, `JobSubmit`, `WorkspaceRegister` |
| `/revert` | `—` | daemon | GitReview | daemon → served | `src/tui/app/mod.rs:8285` | `CommandExecute` |
| `/research` | `—` | daemon | Research | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8418` | — |
| `/research-runs` | `—` | daemon | Research | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8429` | — |
| `/research-open` | `—` | daemon | Research | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8433` | — |
| `/research-show` | `—` | daemon | Research | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8444` | — |
| `/search` | `—` | daemon | Research | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7481` | — |
| `/doctor` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7502` | — |
| `/logs` | `—` | frontend | System | frontend → `ShowLogs` | `src/tui/app/mod.rs:7515` | — |
| `/lsp-status` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:7528` | `CommandExecute` |
| `/nodes` | `—` | daemon | System | daemon → served | `src/tui/app/mod.rs:7532` | `CommandExecute` |
| `/lsp-previews` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7539` | — |
| `/lsp-preview` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7548` | — |
| `/lsp-preview-clear` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7569` | — |
| `/lsp-preview-refresh` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7594` | — |
| `/lsp-preview-apply` | `—` | daemon | Diagnostics | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:7630` | `LspPreviewApply` |
| `/review-change` | `—` | daemon | GitReview | daemon → served | `src/tui/app/mod.rs:7625` | `LspPreviewApply` |
| `/lsp-servers` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:7714` | `CommandExecute` |
| `/lsp-capabilities` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:7718` | `CommandExecute` |
| `/lsp-errors` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:7731` | `CommandExecute` |
| `/lsp-root` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:7744` | `CommandExecute` |
| `/lsp-restart` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:8149` | `CommandExecute` |
| `/lsp-stop` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:8162` | `CommandExecute` |
| `/lsp-cache-status` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:8191` | `CommandExecute` |
| `/lsp-cache-clear` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:8205` | `CommandExecute` |
| `/lsp-doctor` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7763` | — |
| `/lsp-context-diagnostics` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:7783` | `CommandExecute` |
| `/lsp-repair-local` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:7805` | `CommandExecute` |
| `/lsp-repair-hunk` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7840` | — |
| `/lsp-review-file` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:7873` | `CommandExecute` |
| `/lsp-review-diff` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7905` | — |
| `/lsp-security-review` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7930` | — |
| `/lsp-impact` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:7962` | `CommandExecute` |
| `/lsp-test-repair` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:7997` | `CommandExecute` |
| `/lsp-interface` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:8033` | `CommandExecute` |
| `/lsp-cross-repair` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:8069` | `CommandExecute` |
| `/lsp-call-neighbors` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:8106` | `CommandExecute` |
| `/tool-backends` | `—` | frontend | Provider | frontend → `ShowToolBackends` | `src/tui/app/mod.rs:8237` | — |
| `/security-review` | `—` | daemon | Agent | daemon → served | `src/tui/app/mod.rs:8482` | — |
| `/security-review-show` | `—` | daemon | Agent | daemon → served | `src/tui/app/mod.rs:8557` | — |
| `/security-review-cancel` | `—` | daemon | Agent | daemon → served | `src/tui/app/mod.rs:8567` | — |
| `/shell-list` | `—` | frontend | Execution | frontend → `ShowShellRuns` | `src/tui/app/mod.rs:8647` | — |
| `/shell-show` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:8679` | — |
| `/shell-include` | `—` | daemon | Execution | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:8571` | `HumanShellPromotionStage` |
| `/shell-rerun` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:8615` | — |
| `/shell-kill` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:8631` | `HumanShellRunCancel` |
| `/shell-ask` | `—` | daemon | Execution | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:8974` | `HumanShellPromotionStage`, `SessionPromptSubmit` |
| `/shell-expand` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:8695` | — |
| `/terminal` | `—` | daemon | Execution | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:8770` | `InteractiveProcessCreate` |
| `/terminal-clear` | `—` | daemon | Execution | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:8804` | `InteractiveProcessInput`, `InteractiveProcessResize` |
| `/terminal-create` | `—` | daemon | Execution | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:8725` | `InteractiveProcessCreate` |
| `/terminal-list` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:8739` | `CommandExecute` |
| `/terminal-attach` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:8742` | `CommandExecute` |
| `/terminal-show` | `—` | frontend | Execution | frontend → `ShowTerminal` | `src/tui/app/mod.rs:8755` | — |
| `/terminal-focus` | `—` | frontend | Execution | frontend → `FocusTerminal` | `src/tui/app/mod.rs:8831` | — |
| `/terminal-send` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:8840` | `InteractiveProcessInput`, `InteractiveProcessResize` |
| `/terminal-resize` | `—` | daemon | Execution | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:8857` | `InteractiveProcessResize` |
| `/terminal-resume` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:8883` | `CommandExecute` |
| `/terminal-detach` | `—` | daemon | Execution | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:8892` | `InteractiveProcessDetach` |
| `/terminal-terminate` | `—` | daemon | Execution | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:8901` | `InteractiveProcessTerminate` |
| `/terminal-remove` | `—` | daemon | Execution | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:8910` | `InteractiveProcessRemove` |
| `/plugins` | `—` | frontend | System | frontend → `ShowPlugins` | `src/tui/app/mod.rs:8722` | — |
| `/plugin-info` | `—` | frontend | System | frontend → `ShowPluginInfo` | `src/tui/app/mod.rs:8919` | — |
| `/plugin-enable` | `—` | daemon | System | daemon → served | `src/tui/app/mod.rs:8929` | `CommandExecute` |
| `/plugin-disable` | `—` | daemon | System | daemon → served | `src/tui/app/mod.rs:8939` | `CommandExecute` |
| `/plugin-doctor` | `—` | frontend | System | frontend → `ShowPluginDoctor` | `src/tui/app/mod.rs:8949` | — |
| `/plugin-remove` | `—` | daemon | System | daemon → served | `src/tui/app/mod.rs:8954` | `CommandExecute` |
| `/plugin-install` | `—` | daemon | System | daemon → served | `src/tui/app/mod.rs:8964` | `CommandExecute` |
| `/collaborators` | `—` | daemon | Collaboration | daemon → served | `src/tui/app/mod.rs:6154` | `CommandExecute` |
| `/collaborators` | `presence` | daemon | Collaboration | daemon → served | `src/tui/app/mod.rs:6154` | `CommandExecute` |
| `/team` | `—` | daemon | System | daemon → served | `src/tui/app/mod.rs:6170` | `ChatChannelPolicySet`, `ChatPolicyGet`, `ChatProjectPolicySet`, `CommandExecute`, `TeamMembershipAdd`, `TeamMembershipRevoke`, … (12) |
| `/control` | `—` | daemon | Session | daemon → served | `src/tui/app/mod.rs:6182` | `CommandExecute`, `SessionControlRelease`, `SessionControlRequest`, `SessionControlTakeover`, `SessionControlTransfer` |
| `/observe` | `—` | daemon | Collaboration | daemon → served | `src/tui/app/mod.rs:6195` | `CommandExecute`, `ProjectionResume`, `ProjectionSubscribe` |
| `/stop-observing` | `—` | daemon | Collaboration | daemon → served | `src/tui/app/mod.rs:6225` | `CommandExecute` |
| `/panels` | `—` | frontend | System | frontend → `OpenPanelManager` | `src/tui/app/mod.rs:6231` | — |
| `/chat` | `—` | frontend | Collaboration | frontend → `FocusChat` | `src/tui/app/mod.rs:6243` | `ChatChannelEnsure`, `ChatHistory`, `CommandExecute` |
| `/chat-send` | `—` | daemon | Collaboration | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6255` | `ChatChannelEnsure`, `ChatSend` |
| `/chat-reply` | `—` | daemon | Collaboration | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6268` | `ChatChannelEnsure`, `ChatSend` |
| `/chat-history` | `—` | daemon | Collaboration | daemon → served | `src/tui/app/mod.rs:6299` | `ChatChannelEnsure`, `ChatHistory`, `CommandExecute` |
| `/chat-sync` | `—` | daemon | Collaboration | daemon → served | `src/tui/app/mod.rs:6319` | `ChatChannelEnsure`, `ChatHistory`, `CommandExecute` |
| `/chat-read` | `—` | daemon | Collaboration | daemon → served | `src/tui/app/mod.rs:6331` | `CommandExecute` |
| `/chat-edit` | `—` | daemon | Collaboration | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6343` | `ChatEdit` |
| `/chat-redact` | `—` | daemon | Collaboration | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6373` | `ChatRedact` |
| `/chat-composing` | `—` | daemon | Collaboration | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6398` | `ChatComposingList`, `ChatComposingSet` |
| `/chat-action-task` | `—` | daemon | Collaboration | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6421` | `ChatActionSubmit`, `ChatChannelEnsure` |
| `/chat-action-review` | `—` | daemon | Collaboration | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6457` | `ChatActionSubmit`, `ChatChannelEnsure` |
| `/chat-action-list` | `—` | daemon | Collaboration | daemon → served | `src/tui/app/mod.rs:6492` | `ChatChannelEnsure`, `ChatHistory`, `CommandExecute` |
| `/edit-undo` | `—` | daemon | Session | daemon → served | `src/tui/app/mod.rs:8296` | `CommandExecute`, `EditCheckpointUndo` |
| `/edit-reapply` | `—` | daemon | Session | daemon → served | `src/tui/app/mod.rs:8342` | `CommandExecute`, `EditCheckpointReapply` |
| `/edit-checkpoints` | `—` | daemon | Session | daemon → served | `src/tui/app/mod.rs:8388` | `CommandExecute` |
| `/tool-contracts` | `—` | frontend | Provider | frontend → `ShowToolContracts` | `src/tui/app/mod.rs:7519` | — |
| `/worktree` | `—` | frontend | System | frontend → `ShowWorktrees` | `src/tui/app/mod.rs:6713` | `WorktreeList` |

## 4. The families

The regression coverage is organised by family. A family is the catalog's
`CommandDomain`: a stable, user-facing task grouping that the catalog
already carries, so the coverage list and the catalog cannot drift apart.

| family | forms | daemon effect | frontend effect | served | issues a daemon request | no daemon request |
|---|---|---|---|---|---|---|
| Agent | 11 | 10 | 1 | 4 | 0 | 6 |
| Collaboration | 17 | 16 | 1 | 9 | 7 | 0 |
| Diagnostics | 28 | 27 | 1 | 17 | 1 | 9 |
| Execution | 29 | 26 | 3 | 14 | 10 | 2 |
| GitReview | 3 | 3 | 0 | 2 | 0 | 1 |
| Memory | 12 | 11 | 1 | 5 | 2 | 4 |
| Project | 6 | 1 | 5 | 1 | 0 | 0 |
| Provider | 5 | 1 | 4 | 1 | 0 | 0 |
| Research | 5 | 5 | 0 | 0 | 0 | 5 |
| Session | 27 | 12 | 15 | 9 | 2 | 1 |
| System | 24 | 12 | 12 | 11 | 1 | 0 |

## 5. The daemon-effect forms the daemon does not serve

These are the forms the daemon does not serve yet. They are recorded here
rather than left implicit, because the plan's Work package F asks for the
inventory of exactly this set.

| command | form | family | the requests its TUI arm issues |
|---|---|---|---|
| `/sandbox` | `—` | System | `CommandExecute`, `RuntimePolicySet` |
| `/new` | `—` | Session | `SessionCreate` |
| `/start` | `—` | Session | `ChatChannelEnsure`, `ChatEdit`, `ChatHistory`, `ChatSend`, `CommandExecute`, `ProjectionSubscribe`, … (11) |
| `/import` | `—` | Session | — |
| `/agents` | `--all` | Agent | — |
| `/agents` | `show` | Agent | — |
| `/agents` | `diff` | Agent | — |
| `/agents` | `validate` | Agent | — |
| `/agents` | `reload` | Agent | — |
| `/agent` | `—` | Agent | — |
| `/loop` | `—` | Execution | `ScheduleCreate` |
| `/memory-consolidate` | `—` | Memory | `SessionMessagesLoad` |
| `/habits` | `—` | Memory | — |
| `/habit-dismiss` | `—` | Memory | — |
| `/skill-promote` | `—` | Memory | `ChatChannelEnsure`, `ChatEdit`, `ChatHistory`, `ChatSend`, `CommandExecute`, `ProjectionSubscribe`, … (10) |
| `/skill-proposals` | `—` | Memory | — |
| `/skill-proposal` | `—` | Memory | — |
| `/plan` | `—` | Execution | — |
| `/diff` | `—` | GitReview | — |
| `/tests` | `—` | Execution | — |
| `/research` | `—` | Research | — |
| `/research-runs` | `—` | Research | — |
| `/research-open` | `—` | Research | — |
| `/research-show` | `—` | Research | — |
| `/search` | `—` | Research | — |
| `/doctor` | `—` | Diagnostics | — |
| `/lsp-previews` | `—` | Diagnostics | — |
| `/lsp-preview` | `—` | Diagnostics | — |
| `/lsp-preview-clear` | `—` | Diagnostics | — |
| `/lsp-preview-refresh` | `—` | Diagnostics | — |
| `/lsp-preview-apply` | `—` | Diagnostics | `LspPreviewApply` |
| `/lsp-doctor` | `—` | Diagnostics | — |
| `/lsp-repair-hunk` | `—` | Diagnostics | — |
| `/lsp-review-diff` | `—` | Diagnostics | — |
| `/lsp-security-review` | `—` | Diagnostics | — |
| `/shell-include` | `—` | Execution | `HumanShellPromotionStage` |
| `/shell-ask` | `—` | Execution | `HumanShellPromotionStage`, `SessionPromptSubmit` |
| `/terminal` | `—` | Execution | `InteractiveProcessCreate` |
| `/terminal-clear` | `—` | Execution | `InteractiveProcessInput`, `InteractiveProcessResize` |
| `/terminal-create` | `—` | Execution | `InteractiveProcessCreate` |
| `/terminal-resize` | `—` | Execution | `InteractiveProcessResize` |
| `/terminal-detach` | `—` | Execution | `InteractiveProcessDetach` |
| `/terminal-terminate` | `—` | Execution | `InteractiveProcessTerminate` |
| `/terminal-remove` | `—` | Execution | `InteractiveProcessRemove` |
| `/chat-send` | `—` | Collaboration | `ChatChannelEnsure`, `ChatSend` |
| `/chat-reply` | `—` | Collaboration | `ChatChannelEnsure`, `ChatSend` |
| `/chat-edit` | `—` | Collaboration | `ChatEdit` |
| `/chat-redact` | `—` | Collaboration | `ChatRedact` |
| `/chat-composing` | `—` | Collaboration | `ChatComposingList`, `ChatComposingSet` |
| `/chat-action-task` | `—` | Collaboration | `ChatActionSubmit`, `ChatChannelEnsure` |
| `/chat-action-review` | `—` | Collaboration | `ChatActionSubmit`, `ChatChannelEnsure` |

## 6. What the daemon serves

The daemon's effect table, one row per served form. The argument convention
is uniform: the first whitespace-separated word of the command's argument
is the identifier, and the remainder is the payload.

| command | form | the daemon request it builds |
|---|---|---|
| `/approval` | `—` | `approval_preference_get` |
| `/chat-action-list` | `—` | `chat_action_list` |
| `/chat-history` | `—` | `chat_history_older` |
| `/chat-read` | `—` | `chat_read_set` |
| `/chat-sync` | `—` | `chat_sync` |
| `/collaborators` | `—` | `team_principal_list` |
| `/collaborators` | `presence` | `presence_snapshot_get` |
| `/control` | `—` | `session_control_get` |
| `/edit-checkpoints` | `—` | `edit_checkpoint_list` |
| `/edit-reapply` | `—` | `edit_checkpoint_reapply_latest` |
| `/edit-undo` | `—` | `edit_checkpoint_undo_latest` |
| `/export` | `—` | `session_export` |
| `/goal` | `—` | `goal_show` |
| `/lsp-cache-clear` | `—` | `lsp_control` |
| `/lsp-cache-status` | `—` | `lsp_control` |
| `/lsp-call-neighbors` | `—` | `—` |
| `/lsp-capabilities` | `—` | `lsp_server_status_get` |
| `/lsp-context-diagnostics` | `—` | `—` |
| `/lsp-cross-repair` | `—` | `—` |
| `/lsp-errors` | `—` | `lsp_server_status_get` |
| `/lsp-impact` | `—` | `—` |
| `/lsp-interface` | `—` | `—` |
| `/lsp-repair-local` | `—` | `—` |
| `/lsp-restart` | `—` | `lsp_control` |
| `/lsp-review-file` | `—` | `—` |
| `/lsp-root` | `—` | `—` |
| `/lsp-servers` | `—` | `lsp_server_status_get` |
| `/lsp-status` | `—` | `lsp_server_status_get` |
| `/lsp-stop` | `—` | `lsp_control` |
| `/lsp-test-repair` | `—` | `—` |
| `/memory` | `—` | `memory_list` |
| `/memory-forget` | `—` | `memory_forget` |
| `/memory-list` | `—` | `memory_list` |
| `/memory-remember` | `—` | `memory_remember` |
| `/memory-search` | `—` | `memory_search` |
| `/models-refresh` | `—` | `models_refresh` |
| `/network` | `—` | `network_intent_set` |
| `/nodes` | `—` | `node_telemetry` |
| `/observe` | `—` | `projection_subscribe` |
| `/open` | `—` | `document_open` |
| `/plugin-disable` | `—` | `—` |
| `/plugin-enable` | `—` | `—` |
| `/plugin-install` | `—` | `—` |
| `/plugin-remove` | `—` | `—` |
| `/policy` | `—` | `execution_policy_get` |
| `/init` | `—` | `project_init_draft_get` |
| `/reasoning` | `—` | `reasoning_preference_get` |
| `/reload` | `—` | `asset_refresh` |
| `/revert` | `—` | `git_revert` |
| `/review-change` | `—` | `lsp_preview_read` |
| `/schedules` | `—` | `schedule_list` |
| `/security-review` | `—` | `security_review_start` |
| `/security-review-cancel` | `—` | `turn_cancel` |
| `/security-review-show` | `—` | `security_review_read` |
| `/share` | `—` | `session_share` |
| `/shell-expand` | `—` | `human_shell_run_read` |
| `/shell-kill` | `—` | `human_shell_run_cancel` |
| `/shell-rerun` | `—` | `human_shell_run_list` |
| `/shell-show` | `—` | `human_shell_run_read` |
| `/state` | `—` | `active_goal_load` |
| `/stop` | `—` | `turn_cancel` |
| `/stop-observing` | `—` | `projection_unsubscribe` |
| `/subagent-send` | `—` | `turn_steer` |
| `/task` | `—` | `work_order_list` |
| `/task-del` | `—` | `schedule_delete` |
| `/tasks` | `—` | `work_order_list` |
| `/team` | `—` | `team_membership_list` |
| `/terminal-attach` | `—` | `interactive_process_attach` |
| `/terminal-list` | `—` | `interactive_process_list` |
| `/terminal-resume` | `—` | `interactive_process_resume` |
| `/terminal-send` | `—` | `interactive_process_input` |
| `/test` | `—` | `job_wait` |
| `/unshare` | `—` | `session_unshare` |

## 7. The TUI migration

A daemon-effect command is *migrated* when the TUI executes it through
`CoreRequest::CommandExecute` and the daemon's inner response feeds the
arm's existing presentation code. The observable outcome does not change:
the daemon owns the effect and the authoritative data, and the frontend
keeps the formatting.

| command | family | the TUI effect call |
|---|---|---|
| `/approval` | System | `CoreRequest::CommandExecute` (`approval`) |
| `/chat-action-list` | Collaboration | `CoreRequest::CommandExecute` (`chat_action_list`) |
| `/chat-history` | Collaboration | `CoreRequest::CommandExecute` (`chat_history`) |
| `/chat-read` | Collaboration | `CoreRequest::CommandExecute` (`chat_read`) |
| `/chat-sync` | Collaboration | `CoreRequest::CommandExecute` (`chat_sync`) |
| `/collaborators` | Collaboration | `CoreRequest::CommandExecute` (`collaborators`) |
| `/control` | Session | `CoreRequest::CommandExecute` (`control`) |
| `/edit-checkpoints` | Session | `CoreRequest::CommandExecute` (`edit_checkpoints`) |
| `/edit-reapply` | Session | `CoreRequest::CommandExecute` (`edit_reapply`) |
| `/edit-undo` | Session | `CoreRequest::CommandExecute` (`edit_undo`) |
| `/export` | Session | `CoreRequest::CommandExecute` (`export`) |
| `/goal` | Execution | `CoreRequest::CommandExecute` (`goal`) |
| `/memory` | Memory | `CoreRequest::CommandExecute` (`memory`) |
| `/memory-forget` | Memory | `CoreRequest::CommandExecute` (`memory_forget`) |
| `/memory-list` | Memory | `CoreRequest::CommandExecute` (`memory_list`) |
| `/memory-remember` | Memory | `CoreRequest::CommandExecute` (`memory_remember`) |
| `/memory-search` | Memory | `CoreRequest::CommandExecute` (`memory_search`) |
| `/models-refresh` | Provider | `CoreRequest::CommandExecute` (`models_refresh`) |
| `/network` | System | `CoreRequest::CommandExecute` (`network`) |
| `/nodes` | System | `CoreRequest::CommandExecute` (`nodes`) |
| `/observe` | Collaboration | `CoreRequest::CommandExecute` (`observe`) |
| `/policy` | System | `CoreRequest::CommandExecute` (`policy`) |
| `/init` | System | `CoreRequest::CommandExecute` (`project_init`) |
| `/reasoning` | Session | `CoreRequest::CommandExecute` (`reasoning`) |
| `/reload` | System | `CoreRequest::CommandExecute` (`reload`) |
| `/schedules` | Agent | `CoreRequest::CommandExecute` (`schedules`) |
| `/share` | Session | `CoreRequest::CommandExecute` (`share`) |
| `/state` | Execution | `CoreRequest::CommandExecute` (`state`) |
| `/stop` | Collaboration | `CoreRequest::CommandExecute` (`stop`) |
| `/stop-observing` | Collaboration | `CoreRequest::CommandExecute` (`stop_observing`) |
| `/task` | Execution | `CoreRequest::CommandExecute` (`task`) |
| `/task-del` | Execution | `CoreRequest::CommandExecute` (`task_del`) |
| `/tasks` | Execution | `CoreRequest::CommandExecute` (`tasks`) |
| `/team` | System | `CoreRequest::CommandExecute` (`team`) |
| `/terminal-attach` | Execution | `CoreRequest::CommandExecute` (`terminal_attach`) |
| `/terminal-list` | Execution | `CoreRequest::CommandExecute` (`terminal_list`) |
| `/terminal-resume` | Execution | `CoreRequest::CommandExecute` (`terminal_resume`) |
| `/test` | Execution | `CoreRequest::CommandExecute` (`test`) |
| `/unshare` | Session | `CoreRequest::CommandExecute` (`unshare`) |

**39 of the 124 daemon-effect forms are migrated.**
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
| the declared frontend-effect set | the action that serves a **catalog command form** | `crates/codegg-core/src/frontend_action.rs` — 40 `FrontendAction` variants, keyed by `(command_id, form)` | the palette, through `action_for_form(command_id, form)` |
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

## 10. The classification of the forms the daemon does not serve

Every daemon-effect form the daemon does not serve is classified below, in
two groups. The forms whose TUI arm issues **no** daemon request are one
of two things:

* **misclassified** — the form is not really a daemon effect: it does not
  mutate daemon-owned state, does not read daemon-owned state as its
  result, and does not drive a daemon-mediated process. The catalog's kind
  is wrong, and the corrected kind is named.
* **missing capability** — the classification is right and the daemon
  genuinely lacks the surface. That is new product capability, not
  migration work, and it belongs to the follow-up named here.

The forms whose TUI arm **does** issue a daemon request are classified in
the second table below, under a different set of verdicts.

**17 are misclassified and 11 are real missing
capability**, out of 28 daemon-effect forms whose TUI arm issues
no daemon request.

The kind correction is **not applied here**: a frontend-effect form needs a
declared `FrontendAction`, and the declared set's model is a UI action the
frontend performs. 13 new actions and 17 catalog
kind changes are their own change, recorded as a follow-up rather than
smuggled into this milestone.

| command | family | verdict | why |
|---|---|---|---|
| `/import` | Session | misclassified | the arm only opens the import dialog (`src/tui/app/mod.rs:6603`); the fork itself travels as the daemon's own `SessionFork` request, so the command form is a `FrontendEffect` and the catalog's kind is wrong |
| `/agents` | Agent | misclassified | the TUI lists its own agent registry (`app.agent_state`); the catalog's kind is wrong and the corrected kind is `FrontendEffect` |
| `/agents` | Agent | misclassified | the TUI lists its own agent registry (`app.agent_state`); the catalog's kind is wrong and the corrected kind is `FrontendEffect` |
| `/agents` | Agent | misclassified | the TUI lists its own agent registry (`app.agent_state`); the catalog's kind is wrong and the corrected kind is `FrontendEffect` |
| `/agents` | Agent | misclassified | the TUI lists its own agent registry (`app.agent_state`); the catalog's kind is wrong and the corrected kind is `FrontendEffect` |
| `/agents` | Agent | misclassified | the TUI lists its own agent registry (`app.agent_state`); the catalog's kind is wrong and the corrected kind is `FrontendEffect` |
| `/agent` | Agent | misclassified | the TUI selects from its own agent registry (`app.agent_state`); the catalog's kind is wrong and the corrected kind is `FrontendEffect` |
| `/habits` | Memory | misclassified | the TUI's own `HabitStore` on the local filesystem — not the daemon's memory store |
| `/habit-dismiss` | Memory | misclassified | the TUI's own `HabitStore` on the local filesystem |
| `/skill-proposals` | Memory | misclassified | the TUI's own `SkillPromotionStore` on the local filesystem |
| `/skill-proposal` | Memory | misclassified | the TUI's own `SkillPromotionStore` on the local filesystem |
| `/plan` | Execution | missing capability | the plan list is daemon-owned work-plan state; the daemon has no plan request |
| `/diff` | GitReview | misclassified | the arm's effect is the frontend's own: one local `git` subprocess and one file read, with the git root read through the read-only `egggit` crate — no daemon-mediated process is driven |
| `/tests` | Execution | missing capability | the suite run is daemon-owned job state; the daemon has no test-suite request |
| `/research` | Research | missing capability | the research turn is daemon-owned agent-loop work |
| `/research-runs` | Research | misclassified | the TUI's own `ResearchService` over the local filesystem; the catalog's kind is wrong and the corrected kind is `FrontendEffect` |
| `/research-open` | Research | misclassified | the TUI's own `ResearchService` over the local filesystem; the catalog's kind is wrong and the corrected kind is `FrontendEffect` |
| `/research-show` | Research | misclassified | the TUI's own `ResearchService` over the local filesystem; the catalog's kind is wrong and the corrected kind is `FrontendEffect` |
| `/search` | Research | misclassified | the TUI searches its own transcript buffer |
| `/doctor` | Diagnostics | misclassified | a frontend-local diagnostic routine |
| `/lsp-previews` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-preview` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-preview-clear` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-preview-refresh` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-doctor` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-repair-hunk` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-review-diff` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-security-review` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |

### The follow-ups

| follow-up | surface it needs | ADR |
|---|---|---|
| M007-F | the daemon's LSP command surface: a preview list, a server list,
  capabilities, root, restart and stop, a cache status and clear, a doctor, and
  the repair, review, impact, test-repair, interface, cross-repair and
  call-neighbours workflows. The daemon's `egglsp` instance is authoritative and
  the TUI's `LspTool` is a second, non-authoritative one in standalone mode.
  The catalog declares
  26 `/lsp-*` forms; the daemon serves 17 of them and
  leaves 9 unavailable in daemon-connected mode until their
  typed handlers are implemented. The unavailable forms are
  `/lsp-previews`, `/lsp-preview`, `/lsp-preview-clear`, `/lsp-preview-refresh`, `/lsp-preview-apply`, `/lsp-doctor`, `/lsp-repair-hunk`, `/lsp-review-diff`, `/lsp-security-review`. The unavailable palette rows accurately describe the
  daemon's current command surface, and the inventory above lists every form's
  route. M007-F is closed under accepted ADR-0056. | **yes** — the authoritative language
  server's command surface is a new ownership boundary, and ADR-0052 does not
  cover it; ADR-0056 records the accepted decision |
| M007-G | the daemon's plugin catalogue: list, info, enable, disable, doctor,
  remove and install over the plugin registry the daemon actually loads. The
  TUI's `PluginManager` is its own instance and the daemon has none. | **yes** —
  the plugin registry's owner is a new ownership boundary |
| M007-H | the daemon's missing command surfaces: 3 of them —
  `/plan`, `/research`, `/tests`. Each is a surface the
  daemon has not exposed as a command. | no — these extend request
  families that already exist |
| M007-I | the 2 **subsystem moves**:
  `/shell-ask`, `/skill-promote`. Each is a workflow of
  more than one daemon request rather than one effect, so moving it is a
  milestone with its own ownership question — which daemon owns the
  workflow's state, and whether the frontend keeps any of it. | **yes** —
  each is a new ownership boundary, and ADR-0052 does not cover it |

### The forms whose TUI arm issues a daemon request

The remaining daemon-effect forms the daemon does not serve, and whose TUI
arm issues a daemon request anyway, are one of four things — the fifth,
**migrated**, is a form the daemon already serves and is counted above:

* **mechanical** — the effect request's fields are the scope's plus at most
  one argument, so the daemon's effect table can build it. The encoding is
  named, so the row is a decision and not a guess. The argument's convention
  is one token, or a token pair whose second token is a number — `<channel>`,
  `<channel> <seq>`, `<handle> <from_seq>`.
* **not mechanical** — the effect request carries a value that convention
  cannot produce: free text, a structured payload, a list, a boolean, or a
  second effect beside the first. A runtime *scalar* is not disqualifying —
  the argument carries it.
* **subsystem move** — the arm is a subsystem in its own right. Moving it is
  a milestone, and it is named in the follow-up table above.
* **misclassified** — the arm's effect is the frontend's own, so the
  catalog's kind is wrong and the corrected kind is named.

**3 mechanical, 18 not mechanical, 2 subsystem move**, out of 23 daemon-effect forms whose TUI arm issues a
daemon request but which the daemon does not serve.

The accounting is complete: **124 daemon-effect forms = 73 served + 23 mechanical-unserved + 28 with no daemon request**, and every one of them is named.

| command | family | verdict | why |
|---|---|---|---|
| `/sandbox` | System | not mechanical | the daemon effect is issued by the confirmation dialog's own submit path, and `RuntimePolicySet` carries the cached policy's `expected_revision`; the empty-argument arm is a local selector |
| `/new` | Session | not mechanical | `SessionCreate` carries a filesystem `directory` that the scope cannot derive |
| `/start` | Session | not mechanical | the arm's effect is a turn submission through the agent loop's own prompt path, and a `SessionCreate` when no session is bound; neither is a request the scope and an argument can address |
| `/loop` | Execution | not mechanical | `ScheduleCreate` carries a structured spec (kind, job template, overlap and missed-run policies, labels) that the scope cannot derive |
| `/memory-consolidate` | Memory | not mechanical | the daemon request reads the session's messages; the effect itself is a write into the TUI's own memory store, which the daemon does not own |
| `/skill-promote` | Memory | subsystem move | the arm drives 10 daemon requests; moving it is a subsystem, not a table row |
| `/lsp-preview-apply` | Diagnostics | not mechanical | the arm builds the whole `LspPreviewApply` request — the candidate's title, kind, provenance and patches — and the argument's convention cannot carry it |
| `/shell-include` | Execution | not mechanical | `HumanShellPromotionStage` carries three identifiers beside the session, and the argument's convention has room for one |
| `/shell-ask` | Execution | subsystem move | the arm drives 2 daemon requests; moving it is a subsystem, not a table row |
| `/terminal` | Execution | not mechanical | `InteractiveProcessCreate` carries the whole create request — the workspace, the argv, the cwd, the environment overrides and the viewport — and the argument's convention cannot carry it |
| `/terminal-clear` | Execution | not mechanical | the request carries a byte payload the TUI composes — the pending input — and the argument's convention cannot carry bytes |
| `/terminal-create` | Execution | not mechanical | `InteractiveProcessCreate` carries the whole create request — the workspace, the argv, the cwd, the environment overrides and the viewport — and the argument's convention cannot carry it |
| `/terminal-resize` | Execution | not mechanical | `InteractiveProcessResize` carries three values — the attachment id, the columns and the rows — and the argument's convention carries a handle and at most one number |
| `/terminal-detach` | Execution | mechanical | `InteractiveProcessDetach` takes one identifier — the resolved attachment id — which travels as the argument |
| `/terminal-terminate` | Execution | mechanical | `InteractiveProcessTerminate` takes one identifier — the resolved attachment id — which travels as the argument |
| `/terminal-remove` | Execution | mechanical | `InteractiveProcessRemove` takes one identifier — the resolved attachment id — which travels as the argument |
| `/chat-send` | Collaboration | not mechanical | the body is free text, and the request also carries mentions, references and a runtime idempotency key — none of which the argument's convention can carry |
| `/chat-reply` | Collaboration | not mechanical | the body is free text, and the request also carries the reply's thread root, mentions, references and a runtime idempotency key |
| `/chat-edit` | Collaboration | not mechanical | the request carries four values — the channel, the message id, the revision and a free-text body — and the argument's convention has room for two |
| `/chat-redact` | Collaboration | not mechanical | the request carries four values — the channel, the message id, the revision and a free-text reason — and the argument's convention has room for two |
| `/chat-composing` | Collaboration | not mechanical | the arm has two effects (the composing list and the composing set), and the set's flag is a boolean the argument's convention cannot carry |
| `/chat-action-task` | Collaboration | not mechanical | the action is a structured payload assembled from the arguments, and the request carries a runtime idempotency key |
| `/chat-action-review` | Collaboration | not mechanical | the action is a structured payload assembled from the arguments, and the request carries a runtime idempotency key |

## 11. The settled items

* **The description bound is re-baselined from 200 to 256 bytes.** The
  catalog's own longest descriptions are 217 bytes (`/control`) and 211
  (`/security-review`), so a 200-byte ceiling refuses a command the plan
  classifies. The code's bound is
  `crates/codegg-core/src/command_catalog.rs` —
  `bounds::MAX_DESCRIPTION_BYTES = 256` — and plan 014's protocol table
  records the re-baselining with this evidence.
* **The `core_daemon` family's recursion limit is raised to 512.** The
  daemon's request future genuinely grew a layer: the command executor and
  the shared family router sit inside it. The `Send` proof in that test
  crate exceeded the default budget of 128, so
  `tests/core_daemon/mod.rs` declares `#![recursion_limit = "512"]`. It is
  a real type recursion and not a low budget, and only `core_daemon` needed
  it.
* **`/compact` keeps its current behaviour** and stays in the inventory. The
  daemon has no compaction request — `TuiCommand::CompactSession` has no
  sender — so the form has no daemon route. The behaviour is unchanged and
  the gap is directed to a separate corrective milestone.
