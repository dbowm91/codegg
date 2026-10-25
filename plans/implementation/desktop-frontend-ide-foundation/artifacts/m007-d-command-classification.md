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
| argument forms | 166 |
| daemon-effect forms | 134 |
| frontend-effect forms | 32 |
| daemon-effect forms the daemon's effect table serves | 51 |
| daemon-effect forms whose TUI arm issues a daemon request | 28 |
| daemon-effect forms whose TUI arm issues no daemon request | 55 |
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
| `/init` | `—` | daemon | System | daemon → served | `src/tui/app/mod.rs:5670` | — |
| `/connect` | `—` | frontend | Provider | frontend → `OpenConnectDialog` | `src/tui/app/mod.rs:5963` | `ProjectList` |
| `/connections` | `—` | frontend | Provider | frontend → `OpenConnectionSelectionDialog` | `src/tui/app/mod.rs:5967` | `ProjectList` |
| `/exit` | `—` | frontend | System | frontend → `Quit` | `src/tui/app/mod.rs:5759` | — |
| `/status` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:5971` | — |
| `/policy` | `—` | daemon | System | daemon → served | `src/tui/app/mod.rs:5991` | `ExecutionPolicyGet` |
| `/approval` | `—` | daemon | System | daemon → served | `src/tui/app/mod.rs:6006` | `ExecutionPolicyGet` |
| `/sandbox` | `—` | daemon | System | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6030` | `ExecutionPolicyGet` |
| `/network` | `—` | daemon | System | daemon → served | `src/tui/app/mod.rs:6056` | `ExecutionPolicyGet`, `NetworkIntentSet` |
| `/themes` | `—` | frontend | System | frontend → `SetTheme` | `src/tui/app/mod.rs:6114` | `ProjectList` |
| `/help` | `—` | frontend | System | frontend → `OpenHelpDialog` | `src/tui/app/mod.rs:5763` | `ProjectList` |
| `/sessions` | `—` | frontend | Session | frontend → `OpenSessionDialog` | `src/tui/app/mod.rs:6487` | `ProjectList` |
| `/projects` | `—` | frontend | Project | frontend → `OpenProjectPicker` | `src/tui/app/mod.rs:6490` | `ProjectList` |
| `/new` | `—` | daemon | Session | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:5951` | `ProjectList`, `SessionCreate` |
| `/stop` | `—` | daemon | Collaboration | daemon → served | `src/tui/app/mod.rs:6493` | — |
| `/start` | `—` | daemon | Session | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6496` | `SessionCreate` |
| `/share` | `—` | daemon | Session | daemon → served | `src/tui/app/mod.rs:6499` | `ProjectList`, `SessionShare` |
| `/unshare` | `—` | daemon | Session | daemon → served | `src/tui/app/mod.rs:6525` | `SessionUnshare` |
| `/rename` | `—` | frontend | Session | frontend → `ShowAdvisory` | `src/tui/app/mod.rs:6536` | — |
| `/compact` | `—` | daemon | Session | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:5958` | — |
| `/timeline` | `—` | frontend | Session | frontend → `ShowTimeline` | `src/tui/app/mod.rs:6541` | — |
| `/fork` | `—` | frontend | Session | frontend → `ShowAdvisory` | `src/tui/app/mod.rs:6668` | — |
| `/undo` | `—` | frontend | Session | frontend → `UndoLocalMessages` | `src/tui/app/mod.rs:6544` | — |
| `/redo` | `—` | frontend | Session | frontend → `RedoLocalMessages` | `src/tui/app/mod.rs:6551` | — |
| `/export` | `—` | daemon | Session | daemon → served | `src/tui/app/mod.rs:6558` | `SessionExport` |
| `/import` | `—` | daemon | Session | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6572` | `ProjectList` |
| `/timestamps` | `—` | frontend | Session | frontend → `ToggleTimestamps` | `src/tui/app/mod.rs:6579` | — |
| `/thinking` | `—` | frontend | Session | frontend → `ToggleThinking` | `src/tui/app/mod.rs:6588` | — |
| `/reasoning` | `—` | daemon | Session | daemon → served | `src/tui/app/mod.rs:6602` | — |
| `/expand-all` | `—` | frontend | Session | frontend → `ExpandAllDisclosures` | `src/tui/app/mod.rs:6597` | — |
| `/models-refresh` | `—` | daemon | Provider | daemon → served | `src/tui/app/mod.rs:6658` | — |
| `/reload` | `—` | daemon | System | daemon → served | `src/tui/app/mod.rs:5683` | — |
| `/skills` | `—` | daemon | Memory | daemon → served | `src/tui/app/mod.rs:5689` | — |
| `/variants` | `—` | frontend | System | frontend → `ShowAdvisory` | `src/tui/app/mod.rs:6661` | — |
| `/agents` | `—` | frontend | Agent | frontend → `OpenAgentsDialog` | `src/tui/app/mod.rs:5798` | `ProjectList` |
| `/agents` | `--all` | daemon | Agent | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:5798` | `ProjectList` |
| `/agents` | `show` | daemon | Agent | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:5798` | `ProjectList` |
| `/agents` | `diff` | daemon | Agent | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:5798` | `ProjectList` |
| `/agents` | `validate` | daemon | Agent | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:5798` | `ProjectList` |
| `/agents` | `reload` | daemon | Agent | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:5798` | `ProjectList` |
| `/agent` | `—` | daemon | Agent | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:5774` | `ProjectList` |
| `/workspaces` | `—` | frontend | Project | frontend → `ShowAdvisory` | `src/tui/app/mod.rs:6680` | — |
| `/tree` | `—` | frontend | Project | frontend → `OpenFileTreeDialog` | `src/tui/app/mod.rs:5770` | `ProjectList` |
| `/editor` | `—` | frontend | Project | frontend → `OpenExternalEditor` | `src/tui/app/mod.rs:6711` | — |
| `/open` | `—` | daemon | Project | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6714` | — |
| `/context` | `—` | frontend | Session | frontend → `OpenContextDialog` | `src/tui/app/mod.rs:6102` | `ProjectList` |
| `/cost` | `—` | frontend | Session | frontend → `OpenCostDialog` | `src/tui/app/mod.rs:6106` | `ProjectList` |
| `/usage` | `—` | frontend | Session | frontend → `OpenUsageDialog` | `src/tui/app/mod.rs:6110` | `ProjectList` |
| `/stats` | `—` | frontend | Session | frontend → `OpenStatsDialog` | `src/tui/app/mod.rs:6120` | — |
| `/queue` | `—` | frontend | Session | frontend → `OpenQueueBox` | `src/tui/app/mod.rs:5677` | — |
| `/subagent-send` | `—` | daemon | Session | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:5680` | — |
| `/tui` | `—` | frontend | System | frontend → `ToggleFullscreen` | `src/tui/app/mod.rs:6117` | — |
| `/tts` | `—` | frontend | System | frontend → `ToggleTts` | `src/tui/app/mod.rs:6123` | `NotificationSpeak` |
| `/loop` | `—` | daemon | Execution | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6727` | `ScheduleCreate` |
| `/tasks` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:6758` | `ScheduleGet`, `ScheduleList`, `WorkOrderLaneList`, `WorkOrderList`, `WorkOrderSummary` |
| `/task` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:6767` | `WorkOrderLaneList`, `WorkOrderList`, `WorkOrderSummary` |
| `/workspace` | `—` | frontend | Project | frontend → `OpenWorkspaceDashboard` | `src/tui/app/mod.rs:6770` | `ChatChannelEnsure`, `ChatChannelPolicySet`, `ChatHistory`, `ChatPolicyGet`, `ChatProjectPolicySet`, `PresenceSnapshotGet`, … (19) |
| `/schedules` | `—` | daemon | Agent | daemon → served | `src/tui/app/mod.rs:6777` | `ScheduleGet`, `ScheduleList` |
| `/task-del` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:6784` | `ScheduleList` |
| `/memory` | `—` | daemon | Memory | daemon → served | `src/tui/app/mod.rs:6800` | `SessionMessagesLoad` |
| `/memory-search` | `—` | daemon | Memory | daemon → served | `src/tui/app/mod.rs:6812` | `SessionMessagesLoad` |
| `/memory-list` | `—` | daemon | Memory | daemon → served | `src/tui/app/mod.rs:6820` | `SessionMessagesLoad` |
| `/memory-remember` | `—` | daemon | Memory | daemon → served | `src/tui/app/mod.rs:6833` | `SessionMessagesLoad` |
| `/memory-forget` | `—` | daemon | Memory | daemon → served | `src/tui/app/mod.rs:6841` | `SessionMessagesLoad` |
| `/memory-consolidate` | `—` | daemon | Memory | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6849` | `SessionMessagesLoad` |
| `/habits` | `—` | daemon | Memory | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6852` | — |
| `/habit-dismiss` | `—` | daemon | Memory | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6862` | — |
| `/skill-promote` | `—` | daemon | Memory | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6876` | `ChatActionSubmit`, `ChatChannelEnsure`, `ChatChannelPolicySet`, `ChatComposingList`, `ChatComposingSet`, `ChatEdit`, … (38) |
| `/skill-proposals` | `—` | daemon | Memory | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6938` | — |
| `/skill-proposal` | `—` | daemon | Memory | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6975` | — |
| `/goal` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:7143` | `GoalCheckpoint`, `GoalSetBudget` |
| `/plan` | `—` | daemon | Execution | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7285` | — |
| `/state` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:7399` | — |
| `/diff` | `—` | daemon | GitReview | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:8165` | `ChatChannelEnsure`, `ChatEdit`, `ChatHistory`, `ChatHistoryOlder`, `ChatRedact`, `ChatSend`, … (38) |
| `/tests` | `—` | daemon | Execution | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8174` | — |
| `/test` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:8179` | `WorkspaceRegister` |
| `/revert` | `—` | daemon | GitReview | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8192` | — |
| `/research` | `—` | daemon | Research | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8323` | — |
| `/research-runs` | `—` | daemon | Research | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:8334` | `ProjectList` |
| `/research-open` | `—` | daemon | Research | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:8338` | `ProjectList` |
| `/research-show` | `—` | daemon | Research | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:8349` | `ProjectList` |
| `/search` | `—` | daemon | Research | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7449` | — |
| `/doctor` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7470` | — |
| `/logs` | `—` | daemon | System | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7483` | — |
| `/lsp-status` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:7496` | — |
| `/nodes` | `—` | daemon | System | daemon → served | `src/tui/app/mod.rs:7504` | — |
| `/lsp-previews` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7511` | — |
| `/lsp-preview` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7520` | — |
| `/lsp-preview-clear` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7537` | — |
| `/lsp-preview-refresh` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7562` | — |
| `/lsp-preview-apply` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7594` | — |
| `/review-change` | `—` | daemon | GitReview | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7589` | — |
| `/lsp-servers` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7678` | — |
| `/lsp-capabilities` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7693` | — |
| `/lsp-errors` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:7715` | — |
| `/lsp-root` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7736` | — |
| `/lsp-restart` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8080` | — |
| `/lsp-stop` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8096` | — |
| `/lsp-cache-status` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8114` | — |
| `/lsp-cache-clear` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8123` | — |
| `/lsp-doctor` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7750` | — |
| `/lsp-context-diagnostics` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7766` | — |
| `/lsp-repair-local` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7783` | — |
| `/lsp-repair-hunk` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7813` | — |
| `/lsp-review-file` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7842` | — |
| `/lsp-review-diff` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7869` | — |
| `/lsp-security-review` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7890` | — |
| `/lsp-impact` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7918` | — |
| `/lsp-test-repair` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7948` | — |
| `/lsp-interface` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7979` | — |
| `/lsp-cross-repair` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8010` | — |
| `/lsp-call-neighbors` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8042` | — |
| `/tool-backends` | `—` | frontend | Provider | frontend → `ShowToolBackends` | `src/tui/app/mod.rs:8144` | — |
| `/security-review` | `—` | daemon | Agent | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8387` | — |
| `/security-review-show` | `—` | daemon | Agent | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:8462` | `ProjectList` |
| `/security-review-cancel` | `—` | daemon | Agent | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8472` | — |
| `/shell-list` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:8552` | — |
| `/shell-show` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:8584` | — |
| `/shell-include` | `—` | daemon | Execution | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:8476` | `HumanShellPromotionStage` |
| `/shell-rerun` | `—` | daemon | Execution | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8520` | — |
| `/shell-kill` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:8536` | — |
| `/shell-ask` | `—` | daemon | Execution | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:8879` | `ChatChannelEnsure`, `ChatEdit`, `ChatHistory`, `ChatHistoryOlder`, `ChatRedact`, `ChatSend`, … (43) |
| `/shell-expand` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:8600` | — |
| `/terminal` | `—` | daemon | Execution | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8675` | — |
| `/terminal-clear` | `—` | daemon | Execution | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:8709` | `InteractiveProcessInput` |
| `/terminal-create` | `—` | daemon | Execution | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8630` | — |
| `/terminal-list` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:8644` | `InteractiveProcessList` |
| `/terminal-attach` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:8647` | — |
| `/terminal-show` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:8660` | — |
| `/terminal-focus` | `—` | frontend | Execution | frontend → `FocusTerminal` | `src/tui/app/mod.rs:8736` | — |
| `/terminal-send` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:8745` | `InteractiveProcessInput` |
| `/terminal-resize` | `—` | daemon | Execution | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8762` | — |
| `/terminal-resume` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:8788` | — |
| `/terminal-detach` | `—` | daemon | Execution | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8797` | — |
| `/terminal-terminate` | `—` | daemon | Execution | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8806` | — |
| `/terminal-remove` | `—` | daemon | Execution | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8815` | — |
| `/plugins` | `—` | daemon | System | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8627` | — |
| `/plugin-info` | `—` | daemon | System | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8824` | — |
| `/plugin-enable` | `—` | daemon | System | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8834` | — |
| `/plugin-disable` | `—` | daemon | System | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8844` | — |
| `/plugin-doctor` | `—` | daemon | System | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8854` | — |
| `/plugin-remove` | `—` | daemon | System | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8859` | — |
| `/plugin-install` | `—` | daemon | System | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8869` | — |
| `/collaborators` | `—` | daemon | Collaboration | daemon → served | `src/tui/app/mod.rs:6126` | `PresenceSnapshotGet` |
| `/team` | `—` | daemon | System | daemon → served | `src/tui/app/mod.rs:6142` | `ChatChannelPolicySet`, `ChatPolicyGet`, `ChatProjectPolicySet`, `TeamMembershipAdd`, `TeamMembershipList`, `TeamMembershipRevoke`, … (10) |
| `/control` | `—` | daemon | Session | daemon → served | `src/tui/app/mod.rs:6154` | `SessionControlGet`, `SessionControlRelease`, `SessionControlRequest`, `SessionControlTakeover`, `SessionControlTransfer` |
| `/observe` | `—` | daemon | Collaboration | daemon → served | `src/tui/app/mod.rs:6167` | `ProjectionResume` |
| `/stop-observing` | `—` | daemon | Collaboration | daemon → served | `src/tui/app/mod.rs:6197` | — |
| `/panels` | `—` | frontend | System | frontend → `OpenPanelManager` | `src/tui/app/mod.rs:6203` | — |
| `/chat` | `—` | frontend | Collaboration | frontend → `FocusChat` | `src/tui/app/mod.rs:6215` | `ChatChannelEnsure`, `ChatChannelPolicySet`, `ChatHistory`, `ChatPolicyGet`, `ChatProjectPolicySet`, `PresenceSnapshotGet`, … (18) |
| `/chat-send` | `—` | daemon | Collaboration | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6227` | `ChatChannelEnsure`, `ChatSend` |
| `/chat-reply` | `—` | daemon | Collaboration | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6240` | `ChatChannelEnsure`, `ChatSend` |
| `/chat-history` | `—` | daemon | Collaboration | daemon → served | `src/tui/app/mod.rs:6271` | `ChatChannelEnsure`, `ChatHistory` |
| `/chat-sync` | `—` | daemon | Collaboration | daemon → served | `src/tui/app/mod.rs:6291` | `ChatChannelEnsure`, `ChatHistory` |
| `/chat-read` | `—` | daemon | Collaboration | daemon → served | `src/tui/app/mod.rs:6303` | — |
| `/chat-edit` | `—` | daemon | Collaboration | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6315` | `ChatEdit` |
| `/chat-redact` | `—` | daemon | Collaboration | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6345` | `ChatRedact` |
| `/chat-composing` | `—` | daemon | Collaboration | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6370` | `ChatComposingList`, `ChatComposingSet` |
| `/chat-action-task` | `—` | daemon | Collaboration | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6393` | `ChatActionSubmit`, `ChatChannelEnsure` |
| `/chat-action-review` | `—` | daemon | Collaboration | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6429` | `ChatActionSubmit`, `ChatChannelEnsure` |
| `/chat-action-list` | `—` | daemon | Collaboration | daemon → served | `src/tui/app/mod.rs:6464` | `ChatChannelEnsure`, `ChatHistory` |
| `/edit-undo` | `—` | daemon | Session | daemon → served | `src/tui/app/mod.rs:8201` | `EditCheckpointUndo` |
| `/edit-reapply` | `—` | daemon | Session | daemon → served | `src/tui/app/mod.rs:8247` | `EditCheckpointReapply` |
| `/edit-checkpoints` | `—` | daemon | Session | daemon → served | `src/tui/app/mod.rs:8293` | `EditCheckpointList` |
| `/tool-contracts` | `—` | daemon | Provider | daemon → served | `src/tui/app/mod.rs:7487` | — |
| `/worktree` | `—` | daemon | System | daemon → served | `src/tui/app/mod.rs:6685` | — |

## 4. The families

The regression coverage is organised by family. A family is the catalog's
`CommandDomain`: a stable, user-facing task grouping that the catalog
already carries, so the coverage list and the catalog cannot drift apart.

| family | forms | daemon effect | frontend effect | served | issues a daemon request | no daemon request |
|---|---|---|---|---|---|---|
| Agent | 11 | 10 | 1 | 1 | 7 | 2 |
| Collaboration | 16 | 15 | 1 | 8 | 7 | 0 |
| Diagnostics | 28 | 28 | 0 | 3 | 0 | 25 |
| Execution | 29 | 28 | 1 | 15 | 4 | 9 |
| GitReview | 3 | 3 | 0 | 0 | 1 | 2 |
| Memory | 12 | 12 | 0 | 6 | 2 | 4 |
| Project | 6 | 1 | 5 | 0 | 0 | 1 |
| Provider | 5 | 2 | 3 | 2 | 0 | 0 |
| Research | 5 | 5 | 0 | 0 | 3 | 2 |
| Session | 27 | 13 | 14 | 8 | 3 | 2 |
| System | 24 | 17 | 7 | 8 | 1 | 8 |

## 5. The daemon-effect forms the daemon does not serve

These are the forms the daemon does not serve yet. They are recorded here
rather than left implicit, because the plan's Work package F asks for the
inventory of exactly this set.

| command | form | family | the requests its TUI arm issues |
|---|---|---|---|
| `/sandbox` | `—` | System | `ExecutionPolicyGet` |
| `/new` | `—` | Session | `ProjectList`, `SessionCreate` |
| `/start` | `—` | Session | `SessionCreate` |
| `/compact` | `—` | Session | — |
| `/import` | `—` | Session | `ProjectList` |
| `/agents` | `--all` | Agent | `ProjectList` |
| `/agents` | `show` | Agent | `ProjectList` |
| `/agents` | `diff` | Agent | `ProjectList` |
| `/agents` | `validate` | Agent | `ProjectList` |
| `/agents` | `reload` | Agent | `ProjectList` |
| `/agent` | `—` | Agent | `ProjectList` |
| `/open` | `—` | Project | — |
| `/subagent-send` | `—` | Session | — |
| `/loop` | `—` | Execution | `ScheduleCreate` |
| `/memory-consolidate` | `—` | Memory | `SessionMessagesLoad` |
| `/habits` | `—` | Memory | — |
| `/habit-dismiss` | `—` | Memory | — |
| `/skill-promote` | `—` | Memory | `ChatActionSubmit`, `ChatChannelEnsure`, `ChatChannelPolicySet`, `ChatComposingList`, `ChatComposingSet`, `ChatEdit`, … (38) |
| `/skill-proposals` | `—` | Memory | — |
| `/skill-proposal` | `—` | Memory | — |
| `/plan` | `—` | Execution | — |
| `/diff` | `—` | GitReview | `ChatChannelEnsure`, `ChatEdit`, `ChatHistory`, `ChatHistoryOlder`, `ChatRedact`, `ChatSend`, … (38) |
| `/tests` | `—` | Execution | — |
| `/revert` | `—` | GitReview | — |
| `/research` | `—` | Research | — |
| `/research-runs` | `—` | Research | `ProjectList` |
| `/research-open` | `—` | Research | `ProjectList` |
| `/research-show` | `—` | Research | `ProjectList` |
| `/search` | `—` | Research | — |
| `/doctor` | `—` | Diagnostics | — |
| `/logs` | `—` | System | — |
| `/lsp-previews` | `—` | Diagnostics | — |
| `/lsp-preview` | `—` | Diagnostics | — |
| `/lsp-preview-clear` | `—` | Diagnostics | — |
| `/lsp-preview-refresh` | `—` | Diagnostics | — |
| `/lsp-preview-apply` | `—` | Diagnostics | — |
| `/review-change` | `—` | GitReview | — |
| `/lsp-servers` | `—` | Diagnostics | — |
| `/lsp-capabilities` | `—` | Diagnostics | — |
| `/lsp-root` | `—` | Diagnostics | — |
| `/lsp-restart` | `—` | Diagnostics | — |
| `/lsp-stop` | `—` | Diagnostics | — |
| `/lsp-cache-status` | `—` | Diagnostics | — |
| `/lsp-cache-clear` | `—` | Diagnostics | — |
| `/lsp-doctor` | `—` | Diagnostics | — |
| `/lsp-context-diagnostics` | `—` | Diagnostics | — |
| `/lsp-repair-local` | `—` | Diagnostics | — |
| `/lsp-repair-hunk` | `—` | Diagnostics | — |
| `/lsp-review-file` | `—` | Diagnostics | — |
| `/lsp-review-diff` | `—` | Diagnostics | — |
| `/lsp-security-review` | `—` | Diagnostics | — |
| `/lsp-impact` | `—` | Diagnostics | — |
| `/lsp-test-repair` | `—` | Diagnostics | — |
| `/lsp-interface` | `—` | Diagnostics | — |
| `/lsp-cross-repair` | `—` | Diagnostics | — |
| `/lsp-call-neighbors` | `—` | Diagnostics | — |
| `/security-review` | `—` | Agent | — |
| `/security-review-show` | `—` | Agent | `ProjectList` |
| `/security-review-cancel` | `—` | Agent | — |
| `/shell-include` | `—` | Execution | `HumanShellPromotionStage` |
| `/shell-rerun` | `—` | Execution | — |
| `/shell-ask` | `—` | Execution | `ChatChannelEnsure`, `ChatEdit`, `ChatHistory`, `ChatHistoryOlder`, `ChatRedact`, `ChatSend`, … (43) |
| `/terminal` | `—` | Execution | — |
| `/terminal-clear` | `—` | Execution | `InteractiveProcessInput` |
| `/terminal-create` | `—` | Execution | — |
| `/terminal-resize` | `—` | Execution | — |
| `/terminal-detach` | `—` | Execution | — |
| `/terminal-terminate` | `—` | Execution | — |
| `/terminal-remove` | `—` | Execution | — |
| `/plugins` | `—` | System | — |
| `/plugin-info` | `—` | System | — |
| `/plugin-enable` | `—` | System | — |
| `/plugin-disable` | `—` | System | — |
| `/plugin-doctor` | `—` | System | — |
| `/plugin-remove` | `—` | System | — |
| `/plugin-install` | `—` | System | — |
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
| `/control` | `—` | `session_control_get` |
| `/edit-checkpoints` | `—` | `edit_checkpoint_list` |
| `/edit-reapply` | `—` | `edit_checkpoint_reapply_latest` |
| `/edit-undo` | `—` | `edit_checkpoint_undo_latest` |
| `/export` | `—` | `session_export` |
| `/goal` | `—` | `goal_show` |
| `/lsp-errors` | `—` | `lsp_diagnostics_get` |
| `/lsp-status` | `—` | `lsp_diagnostics_get` |
| `/memory` | `—` | `memory_list` |
| `/memory-forget` | `—` | `memory_forget` |
| `/memory-list` | `—` | `memory_list` |
| `/memory-remember` | `—` | `memory_remember` |
| `/memory-search` | `—` | `memory_search` |
| `/models-refresh` | `—` | `models_refresh` |
| `/network` | `—` | `network_intent_set` |
| `/nodes` | `—` | `node_telemetry` |
| `/observe` | `—` | `projection_subscribe` |
| `/policy` | `—` | `execution_policy_get` |
| `/init` | `—` | `project_init_draft_get` |
| `/reasoning` | `—` | `reasoning_preference_get` |
| `/reload` | `—` | `asset_refresh` |
| `/schedules` | `—` | `schedule_list` |
| `/share` | `—` | `session_share` |
| `/shell-expand` | `—` | `human_shell_run_read` |
| `/shell-kill` | `—` | `human_shell_run_cancel` |
| `/shell-list` | `—` | `human_shell_run_capabilities` |
| `/shell-show` | `—` | `human_shell_run_read` |
| `/skills` | `—` | `skill_catalogue_list` |
| `/state` | `—` | `active_goal_load` |
| `/status` | `—` | `session_lifecycle_get` |
| `/stop` | `—` | `turn_cancel` |
| `/stop-observing` | `—` | `projection_unsubscribe` |
| `/task` | `—` | `work_order_list` |
| `/task-del` | `—` | `schedule_delete` |
| `/tasks` | `—` | `work_order_list` |
| `/team` | `—` | `team_membership_list` |
| `/terminal-attach` | `—` | `interactive_process_attach` |
| `/terminal-list` | `—` | `interactive_process_list` |
| `/terminal-resume` | `—` | `interactive_process_resume` |
| `/terminal-send` | `—` | `interactive_process_input` |
| `/terminal-show` | `—` | `interactive_process_list` |
| `/test` | `—` | `job_wait` |
| `/tool-contracts` | `—` | `tool_program_list` |
| `/unshare` | `—` | `session_unshare` |
| `/worktree` | `—` | `managed_worktree_list` |

## 7. The TUI migration

A daemon-effect command is *migrated* when the TUI executes it through
`CoreRequest::CommandExecute` and the daemon's inner response feeds the
arm's existing presentation code. The observable outcome does not change:
the daemon owns the effect and the authoritative data, and the frontend
keeps the formatting.

| command | family | the TUI effect call |
|---|---|---|
| `/chat-action-list` | Collaboration | `CoreRequest::CommandExecute` (`chat_action_list`) |
| `/chat-history` | Collaboration | `CoreRequest::CommandExecute` (`chat_history`) |
| `/chat-read` | Collaboration | `CoreRequest::CommandExecute` (`chat_read`) |
| `/chat-sync` | Collaboration | `CoreRequest::CommandExecute` (`chat_sync`) |
| `/edit-reapply` | Session | `CoreRequest::CommandExecute` (`edit_reapply`) |
| `/edit-undo` | Session | `CoreRequest::CommandExecute` (`edit_undo`) |
| `/goal` | Execution | `CoreRequest::CommandExecute` (`goal`) |
| `/memory` | Memory | `CoreRequest::CommandExecute` (`memory`) |
| `/memory-forget` | Memory | `CoreRequest::CommandExecute` (`memory_forget`) |
| `/memory-list` | Memory | `CoreRequest::CommandExecute` (`memory_list`) |
| `/memory-remember` | Memory | `CoreRequest::CommandExecute` (`memory_remember`) |
| `/memory-search` | Memory | `CoreRequest::CommandExecute` (`memory_search`) |
| `/nodes` | System | `CoreRequest::CommandExecute` (`nodes`) |
| `/reload` | System | `CoreRequest::CommandExecute` (`reload`) |
| `/stop` | Collaboration | `CoreRequest::CommandExecute` (`stop`) |
| `/stop-observing` | Collaboration | `CoreRequest::CommandExecute` (`stop_observing`) |
| `/task-del` | Execution | `CoreRequest::CommandExecute` (`task_del`) |
| `/terminal-attach` | Execution | `CoreRequest::CommandExecute` (`terminal_attach`) |
| `/terminal-resume` | Execution | `CoreRequest::CommandExecute` (`terminal_resume`) |
| `/test` | Execution | `CoreRequest::CommandExecute` (`test`) |

**20 of the 134 daemon-effect forms are migrated.**
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

**19 are misclassified and 36 are real missing
capability**, out of 55 daemon-effect forms whose TUI arm issues
no daemon request.

The kind correction is **not applied here**: a frontend-effect form needs a
declared `FrontendAction`, and the declared set's model is a UI action the
frontend performs. 24 new actions and 28 catalog
kind changes are their own change, recorded as a follow-up rather than
smuggled into this milestone.

| command | family | verdict | why |
|---|---|---|---|
| `/compact` | Session | missing capability | compaction is a daemon-owned session operation; the daemon has no compaction request |
| `/open` | Project | missing capability | opening a document is daemon-owned workspace state |
| `/subagent-send` | Session | missing capability | steering a subagent is a daemon-owned turn operation |
| `/habits` | Memory | misclassified | the TUI's own `HabitStore` on the local filesystem — not the daemon's memory store |
| `/habit-dismiss` | Memory | misclassified | the TUI's own `HabitStore` on the local filesystem |
| `/skill-proposals` | Memory | misclassified | the TUI's own `SkillPromotionStore` on the local filesystem |
| `/skill-proposal` | Memory | misclassified | the TUI's own `SkillPromotionStore` on the local filesystem |
| `/plan` | Execution | missing capability | the plan list is daemon-owned work-plan state; the daemon has no plan request |
| `/tests` | Execution | missing capability | the suite run is daemon-owned job state; the daemon has no test-suite request |
| `/revert` | GitReview | missing capability | the daemon owns the git mutation; it exposes it as a tool, not as a request |
| `/research` | Research | missing capability | the research turn is daemon-owned agent-loop work |
| `/search` | Research | misclassified | the TUI searches its own transcript buffer |
| `/doctor` | Diagnostics | misclassified | a frontend-local diagnostic routine |
| `/logs` | System | missing capability | the daemon writes the log file and the frontend reads it out of band; the daemon has no log-tail request |
| `/lsp-previews` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-preview` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-preview-clear` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-preview-refresh` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-preview-apply` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/review-change` | GitReview | missing capability | the change-review workflow is a daemon-owned agent turn |
| `/lsp-servers` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-capabilities` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-root` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-restart` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-stop` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-cache-status` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-cache-clear` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-doctor` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-context-diagnostics` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-repair-local` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-repair-hunk` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-review-file` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-review-diff` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-security-review` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-impact` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-test-repair` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-interface` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-cross-repair` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-call-neighbors` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/security-review` | Agent | missing capability | the security-review workflow is a daemon-owned agent turn |
| `/security-review-cancel` | Agent | missing capability | cancelling a security-review run is a daemon-owned turn cancellation |
| `/shell-rerun` | Execution | missing capability | re-running a human-shell run is daemon-owned job state |
| `/terminal` | Execution | misclassified | the TUI's own interactive terminal |
| `/terminal-create` | Execution | misclassified | the TUI's own interactive terminal |
| `/terminal-resize` | Execution | misclassified | the TUI's own interactive terminal |
| `/terminal-detach` | Execution | misclassified | the TUI's own interactive terminal |
| `/terminal-terminate` | Execution | misclassified | the TUI's own interactive terminal |
| `/terminal-remove` | Execution | misclassified | the TUI's own interactive terminal |
| `/plugins` | System | misclassified | the TUI's own `PluginManager` over a plugin service the TUI creates; the daemon has no plugin manager |
| `/plugin-info` | System | misclassified | the TUI's own `PluginManager` |
| `/plugin-enable` | System | misclassified | the TUI's own `PluginManager` |
| `/plugin-disable` | System | misclassified | the TUI's own `PluginManager` |
| `/plugin-doctor` | System | misclassified | the TUI's own `PluginManager` |
| `/plugin-remove` | System | misclassified | the TUI's own `PluginManager` |
| `/plugin-install` | System | misclassified | the TUI's own `PluginManager` |

### The follow-ups

| follow-up | surface it needs | ADR |
|---|---|---|
| M007-F | the daemon's LSP command surface: a preview list, a server list,
  capabilities, root, restart and stop, a cache status and clear, a doctor, and
  the repair, review, impact, test-repair, interface, cross-repair and
  call-neighbours workflows. The daemon's `egglsp` instance is authoritative and
  the TUI's `LspTool` is a second, non-authoritative one. The catalog declares
  26 `/lsp-*` forms; the daemon serves 2 of them and does not serve the other
  24. **Which behaviour refuses where:** the TUI's own `/lsp-*` arms refuse
  locally, with the toast `LSP not available`, because `app.lsp_tool` is `None`
  — the TUI never builds a second `LspTool` in production, and only the tests
  populate it. The one arm that would reach the daemon (`/lsp-preview-apply`)
  reaches it only *after* the same `lsp_tool` check, so it refuses too. The
  desktop palette shows the same forms as unavailable because the daemon does
  not serve them. Both refusals state the same fact — the authoritative
  language server is the daemon's — so the palette's unavailable row is
  correct, not a gap. | **yes** — the authoritative language server's command
  surface is a new ownership boundary, and ADR-0052 does not cover it |
| M007-G | the daemon's plugin catalogue: list, info, enable, disable, doctor,
  remove and install over the plugin registry the daemon actually loads. The
  TUI's `PluginManager` is its own instance and the daemon has none. | **yes** —
  the plugin registry's owner is a new ownership boundary |
| M007-H | the daemon's missing command surfaces: compaction, subagent steer,
  the plan list, the test suite, a git revert, the research turn, the security-
  review and change-review workflows, opening a document, re-running a human-
  shell run, and a log tail. Each is an existing request family the daemon has
  not grown yet. | no — these extend request families that already exist |
| M007-I | the three **subsystem moves**: the git-review diff view, the
  skill-promotion workflow, and the human-shell ask workflow. Each is a
  workflow of dozens of daemon requests rather than one effect, so moving
  it is a milestone with its own ownership question — which daemon owns
  the workflow's state, and whether the frontend keeps any of it. | **yes** —
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

**9 misclassified, 16 not mechanical, 3 subsystem move**, out of 28 daemon-effect forms whose TUI arm issues a
daemon request but which the daemon does not serve.

The accounting is complete: **134 daemon-effect forms = 51 served + 28 mechanical-unserved + 55 with no daemon request**, and every one of them is named.

| command | family | verdict | why |
|---|---|---|---|
| `/sandbox` | System | not mechanical | the daemon effect is issued by the confirmation dialog's own submit path, and `RuntimePolicySet` carries the cached policy's `expected_revision`; the empty-argument arm is a local selector |
| `/new` | Session | not mechanical | `SessionCreate` carries a filesystem `directory` that the scope cannot derive |
| `/start` | Session | not mechanical | the arm's effect is a turn submission through the agent loop's own prompt path, and a `SessionCreate` when no session is bound; neither is a request the scope and an argument can address |
| `/import` | Session | not mechanical | the arm opens the import dialog; the daemon request is issued by the dialog's own submit path, not by the command |
| `/agents` | Agent | misclassified | the TUI lists its own agent registry (`app.agent_state`); the catalog's kind is wrong and the corrected kind is `FrontendEffect` |
| `/agents` | Agent | misclassified | the TUI lists its own agent registry (`app.agent_state`); the catalog's kind is wrong and the corrected kind is `FrontendEffect` |
| `/agents` | Agent | misclassified | the TUI lists its own agent registry (`app.agent_state`); the catalog's kind is wrong and the corrected kind is `FrontendEffect` |
| `/agents` | Agent | misclassified | the TUI lists its own agent registry (`app.agent_state`); the catalog's kind is wrong and the corrected kind is `FrontendEffect` |
| `/agents` | Agent | misclassified | the TUI lists its own agent registry (`app.agent_state`); the catalog's kind is wrong and the corrected kind is `FrontendEffect` |
| `/agent` | Agent | misclassified | the TUI selects from its own agent registry (`app.agent_state`); the catalog's kind is wrong and the corrected kind is `FrontendEffect` |
| `/loop` | Execution | not mechanical | `ScheduleCreate` carries a structured spec (kind, job template, overlap and missed-run policies, labels) that the scope cannot derive |
| `/memory-consolidate` | Memory | not mechanical | the daemon request reads the session's messages; the effect itself is a write into the TUI's own memory store, which the daemon does not own |
| `/skill-promote` | Memory | subsystem move | the arm drives 44 daemon requests; moving it is a subsystem, not a table row |
| `/diff` | GitReview | subsystem move | the arm drives 40 daemon requests; moving it is a subsystem, not a table row |
| `/research-runs` | Research | misclassified | the TUI's own `ResearchService` over the local filesystem; the catalog's kind is wrong and the corrected kind is `FrontendEffect` |
| `/research-open` | Research | misclassified | the TUI's own `ResearchService` over the local filesystem; the catalog's kind is wrong and the corrected kind is `FrontendEffect` |
| `/research-show` | Research | misclassified | the TUI's own `ResearchService` over the local filesystem; the catalog's kind is wrong and the corrected kind is `FrontendEffect` |
| `/security-review-show` | Agent | not mechanical | the arm shows the TUI's cached review result, and the daemon has no request that reads one — the same missing surface as `/security-review` (M007-H) |
| `/shell-include` | Execution | not mechanical | `HumanShellPromotionStage` carries three identifiers beside the session, and the argument's convention has room for one |
| `/shell-ask` | Execution | subsystem move | the arm drives 45 daemon requests; moving it is a subsystem, not a table row |
| `/terminal-clear` | Execution | not mechanical | the request carries a byte payload the TUI composes — the pending input — and the argument's convention cannot carry bytes |
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
