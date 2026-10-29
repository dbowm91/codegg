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
| daemon-effect forms | 129 |
| frontend-effect forms | 38 |
| daemon-effect forms the daemon's effect table serves | 61 |
| daemon-effect forms whose TUI arm issues a daemon request | 28 |
| daemon-effect forms whose TUI arm issues no daemon request | 40 |
| frontend-effect forms covered by the declared action set | 38 |

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
| `/init` | `—` | daemon | System | daemon → served | `src/tui/app/mod.rs:5701` | `CommandExecute` |
| `/connect` | `—` | frontend | Provider | frontend → `OpenConnectDialog` | `src/tui/app/mod.rs:5994` | `ProjectList` |
| `/connections` | `—` | frontend | Provider | frontend → `OpenConnectionSelectionDialog` | `src/tui/app/mod.rs:5998` | `ProjectList` |
| `/exit` | `—` | frontend | System | frontend → `Quit` | `src/tui/app/mod.rs:5790` | — |
| `/status` | `—` | frontend | Diagnostics | frontend → `ShowStatus` | `src/tui/app/mod.rs:6002` | — |
| `/policy` | `—` | daemon | System | daemon → served | `src/tui/app/mod.rs:6022` | `CommandExecute` |
| `/approval` | `—` | daemon | System | daemon → served | `src/tui/app/mod.rs:6037` | `CommandExecute` |
| `/sandbox` | `—` | daemon | System | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6061` | `CommandExecute` |
| `/network` | `—` | daemon | System | daemon → served | `src/tui/app/mod.rs:6087` | `CommandExecute` |
| `/themes` | `—` | frontend | System | frontend → `SetTheme` | `src/tui/app/mod.rs:6145` | `ProjectList` |
| `/help` | `—` | frontend | System | frontend → `OpenHelpDialog` | `src/tui/app/mod.rs:5794` | `ProjectList` |
| `/sessions` | `—` | frontend | Session | frontend → `OpenSessionDialog` | `src/tui/app/mod.rs:6518` | `ProjectList` |
| `/projects` | `—` | frontend | Project | frontend → `OpenProjectPicker` | `src/tui/app/mod.rs:6521` | `ProjectList` |
| `/new` | `—` | daemon | Session | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:5982` | `ProjectList`, `SessionCreate`, `SessionLoad` |
| `/stop` | `—` | daemon | Collaboration | daemon → served | `src/tui/app/mod.rs:6524` | `CommandExecute` |
| `/start` | `—` | daemon | Session | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6527` | `SessionCreate`, `SessionLoad` |
| `/share` | `—` | daemon | Session | daemon → served | `src/tui/app/mod.rs:6530` | `CommandExecute`, `ProjectList` |
| `/unshare` | `—` | daemon | Session | daemon → served | `src/tui/app/mod.rs:6556` | `CommandExecute` |
| `/rename` | `—` | frontend | Session | frontend → `ShowAdvisory` | `src/tui/app/mod.rs:6567` | — |
| `/compact` | `—` | daemon | Session | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:5989` | — |
| `/timeline` | `—` | frontend | Session | frontend → `ShowTimeline` | `src/tui/app/mod.rs:6572` | — |
| `/fork` | `—` | frontend | Session | frontend → `ShowAdvisory` | `src/tui/app/mod.rs:6699` | — |
| `/undo` | `—` | frontend | Session | frontend → `UndoLocalMessages` | `src/tui/app/mod.rs:6575` | — |
| `/redo` | `—` | frontend | Session | frontend → `RedoLocalMessages` | `src/tui/app/mod.rs:6582` | — |
| `/export` | `—` | daemon | Session | daemon → served | `src/tui/app/mod.rs:6589` | `CommandExecute` |
| `/import` | `—` | daemon | Session | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6603` | `ProjectList` |
| `/timestamps` | `—` | frontend | Session | frontend → `ToggleTimestamps` | `src/tui/app/mod.rs:6610` | — |
| `/thinking` | `—` | frontend | Session | frontend → `ToggleThinking` | `src/tui/app/mod.rs:6619` | — |
| `/reasoning` | `—` | daemon | Session | daemon → served | `src/tui/app/mod.rs:6633` | — |
| `/expand-all` | `—` | frontend | Session | frontend → `ExpandAllDisclosures` | `src/tui/app/mod.rs:6628` | — |
| `/models-refresh` | `—` | daemon | Provider | daemon → served | `src/tui/app/mod.rs:6689` | `CommandExecute` |
| `/reload` | `—` | daemon | System | daemon → served | `src/tui/app/mod.rs:5714` | `CommandExecute` |
| `/skills` | `—` | frontend | Memory | frontend → `ShowSkills` | `src/tui/app/mod.rs:5720` | — |
| `/variants` | `—` | frontend | System | frontend → `ShowAdvisory` | `src/tui/app/mod.rs:6692` | — |
| `/agents` | `—` | frontend | Agent | frontend → `OpenAgentsDialog` | `src/tui/app/mod.rs:5829` | `ProjectList` |
| `/agents` | `--all` | daemon | Agent | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:5829` | `ProjectList` |
| `/agents` | `show` | daemon | Agent | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:5829` | `ProjectList` |
| `/agents` | `diff` | daemon | Agent | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:5829` | `ProjectList` |
| `/agents` | `validate` | daemon | Agent | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:5829` | `ProjectList` |
| `/agents` | `reload` | daemon | Agent | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:5829` | `ProjectList` |
| `/agent` | `—` | daemon | Agent | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:5805` | `ProjectList` |
| `/workspaces` | `—` | frontend | Project | frontend → `ShowAdvisory` | `src/tui/app/mod.rs:6711` | — |
| `/tree` | `—` | frontend | Project | frontend → `OpenFileTreeDialog` | `src/tui/app/mod.rs:5801` | `ProjectList` |
| `/editor` | `—` | frontend | Project | frontend → `OpenExternalEditor` | `src/tui/app/mod.rs:6742` | — |
| `/open` | `—` | daemon | Project | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6745` | — |
| `/context` | `—` | frontend | Session | frontend → `OpenContextDialog` | `src/tui/app/mod.rs:6133` | `ProjectList` |
| `/cost` | `—` | frontend | Session | frontend → `OpenCostDialog` | `src/tui/app/mod.rs:6137` | `ProjectList` |
| `/usage` | `—` | frontend | Session | frontend → `OpenUsageDialog` | `src/tui/app/mod.rs:6141` | `ProjectList` |
| `/stats` | `—` | frontend | Session | frontend → `OpenStatsDialog` | `src/tui/app/mod.rs:6151` | — |
| `/queue` | `—` | frontend | Session | frontend → `OpenQueueBox` | `src/tui/app/mod.rs:5708` | — |
| `/subagent-send` | `—` | daemon | Session | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:5711` | — |
| `/tui` | `—` | frontend | System | frontend → `ToggleFullscreen` | `src/tui/app/mod.rs:6148` | — |
| `/tts` | `—` | frontend | System | frontend → `ToggleTts` | `src/tui/app/mod.rs:6154` | `NotificationSpeak` |
| `/loop` | `—` | daemon | Execution | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6758` | `ScheduleCreate` |
| `/tasks` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:6789` | `CommandExecute`, `ScheduleGet`, `WorkOrderLaneList`, `WorkOrderSummary` |
| `/task` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:6798` | `CommandExecute`, `WorkOrderLaneList`, `WorkOrderSummary` |
| `/workspace` | `—` | frontend | Project | frontend → `OpenWorkspaceDashboard` | `src/tui/app/mod.rs:6801` | `ChatChannelEnsure`, `ChatChannelPolicySet`, `ChatHistory`, `ChatPolicyGet`, `ChatProjectPolicySet`, `SessionControlRelease`, … (15) |
| `/schedules` | `—` | daemon | Agent | daemon → served | `src/tui/app/mod.rs:6808` | `CommandExecute`, `ScheduleGet` |
| `/task-del` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:6815` | `CommandExecute` |
| `/memory` | `—` | daemon | Memory | daemon → served | `src/tui/app/mod.rs:6831` | `CommandExecute`, `SessionMessagesLoad` |
| `/memory-search` | `—` | daemon | Memory | daemon → served | `src/tui/app/mod.rs:6843` | `CommandExecute`, `SessionMessagesLoad` |
| `/memory-list` | `—` | daemon | Memory | daemon → served | `src/tui/app/mod.rs:6851` | `CommandExecute`, `SessionMessagesLoad` |
| `/memory-remember` | `—` | daemon | Memory | daemon → served | `src/tui/app/mod.rs:6864` | `CommandExecute`, `SessionMessagesLoad` |
| `/memory-forget` | `—` | daemon | Memory | daemon → served | `src/tui/app/mod.rs:6872` | `CommandExecute`, `SessionMessagesLoad` |
| `/memory-consolidate` | `—` | daemon | Memory | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6880` | `SessionMessagesLoad` |
| `/habits` | `—` | daemon | Memory | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6883` | — |
| `/habit-dismiss` | `—` | daemon | Memory | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6893` | — |
| `/skill-promote` | `—` | daemon | Memory | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6907` | `ChatActionSubmit`, `ChatChannelEnsure`, `ChatChannelPolicySet`, `ChatComposingList`, `ChatComposingSet`, `ChatEdit`, … (32) |
| `/skill-proposals` | `—` | daemon | Memory | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:6969` | — |
| `/skill-proposal` | `—` | daemon | Memory | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7006` | — |
| `/goal` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:7174` | `CommandExecute`, `GoalCheckpoint`, `GoalSetBudget` |
| `/plan` | `—` | daemon | Execution | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7316` | — |
| `/state` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:7430` | — |
| `/diff` | `—` | daemon | GitReview | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:8257` | `AgentRunTranscriptRead`, `ChatChannelEnsure`, `ChatEdit`, `ChatHistory`, `ChatHistoryOlder`, `ChatRedact`, … (37) |
| `/tests` | `—` | daemon | Execution | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8266` | — |
| `/test` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:8271` | `CommandExecute`, `WorkspaceRegister` |
| `/revert` | `—` | daemon | GitReview | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8284` | — |
| `/research` | `—` | daemon | Research | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8415` | — |
| `/research-runs` | `—` | daemon | Research | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:8426` | `ProjectList` |
| `/research-open` | `—` | daemon | Research | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:8430` | `ProjectList` |
| `/research-show` | `—` | daemon | Research | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:8441` | `ProjectList` |
| `/search` | `—` | daemon | Research | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7480` | — |
| `/doctor` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7501` | — |
| `/logs` | `—` | daemon | System | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7514` | — |
| `/lsp-status` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:7527` | `CommandExecute` |
| `/nodes` | `—` | daemon | System | daemon → served | `src/tui/app/mod.rs:7531` | `CommandExecute` |
| `/lsp-previews` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7538` | — |
| `/lsp-preview` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7547` | — |
| `/lsp-preview-clear` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7568` | — |
| `/lsp-preview-refresh` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7593` | — |
| `/lsp-preview-apply` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7629` | — |
| `/review-change` | `—` | daemon | GitReview | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7624` | — |
| `/lsp-servers` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:7713` | `CommandExecute` |
| `/lsp-capabilities` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:7717` | `CommandExecute` |
| `/lsp-errors` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:7730` | `CommandExecute` |
| `/lsp-root` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:7743` | `CommandExecute` |
| `/lsp-restart` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:8148` | `CommandExecute` |
| `/lsp-stop` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:8161` | `CommandExecute` |
| `/lsp-cache-status` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:8190` | `CommandExecute` |
| `/lsp-cache-clear` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:8204` | `CommandExecute` |
| `/lsp-doctor` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7762` | — |
| `/lsp-context-diagnostics` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:7782` | `CommandExecute` |
| `/lsp-repair-local` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:7804` | `CommandExecute` |
| `/lsp-repair-hunk` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7839` | — |
| `/lsp-review-file` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:7872` | `CommandExecute` |
| `/lsp-review-diff` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7904` | — |
| `/lsp-security-review` | `—` | daemon | Diagnostics | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:7929` | — |
| `/lsp-impact` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:7961` | `CommandExecute` |
| `/lsp-test-repair` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:7996` | `CommandExecute` |
| `/lsp-interface` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:8032` | `CommandExecute` |
| `/lsp-cross-repair` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:8068` | `CommandExecute` |
| `/lsp-call-neighbors` | `—` | daemon | Diagnostics | daemon → served | `src/tui/app/mod.rs:8105` | `CommandExecute` |
| `/tool-backends` | `—` | frontend | Provider | frontend → `ShowToolBackends` | `src/tui/app/mod.rs:8236` | — |
| `/security-review` | `—` | daemon | Agent | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8479` | — |
| `/security-review-show` | `—` | daemon | Agent | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:8554` | `ProjectList` |
| `/security-review-cancel` | `—` | daemon | Agent | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8564` | — |
| `/shell-list` | `—` | frontend | Execution | frontend → `ShowShellRuns` | `src/tui/app/mod.rs:8644` | — |
| `/shell-show` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:8676` | — |
| `/shell-include` | `—` | daemon | Execution | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:8568` | `HumanShellPromotionStage` |
| `/shell-rerun` | `—` | daemon | Execution | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8612` | — |
| `/shell-kill` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:8628` | — |
| `/shell-ask` | `—` | daemon | Execution | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:8971` | `AgentRunTranscriptRead`, `ChatChannelEnsure`, `ChatEdit`, `ChatHistory`, `ChatHistoryOlder`, `ChatRedact`, … (39) |
| `/shell-expand` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:8692` | — |
| `/terminal` | `—` | daemon | Execution | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8767` | — |
| `/terminal-clear` | `—` | daemon | Execution | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:8801` | `InteractiveProcessInput` |
| `/terminal-create` | `—` | daemon | Execution | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8722` | — |
| `/terminal-list` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:8736` | `CommandExecute` |
| `/terminal-attach` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:8739` | `CommandExecute` |
| `/terminal-show` | `—` | frontend | Execution | frontend → `ShowTerminal` | `src/tui/app/mod.rs:8752` | — |
| `/terminal-focus` | `—` | frontend | Execution | frontend → `FocusTerminal` | `src/tui/app/mod.rs:8828` | — |
| `/terminal-send` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:8837` | `InteractiveProcessInput` |
| `/terminal-resize` | `—` | daemon | Execution | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8854` | — |
| `/terminal-resume` | `—` | daemon | Execution | daemon → served | `src/tui/app/mod.rs:8880` | `CommandExecute` |
| `/terminal-detach` | `—` | daemon | Execution | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8889` | — |
| `/terminal-terminate` | `—` | daemon | Execution | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8898` | — |
| `/terminal-remove` | `—` | daemon | Execution | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8907` | — |
| `/plugins` | `—` | daemon | System | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8719` | — |
| `/plugin-info` | `—` | daemon | System | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8916` | — |
| `/plugin-enable` | `—` | daemon | System | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8926` | — |
| `/plugin-disable` | `—` | daemon | System | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8936` | — |
| `/plugin-doctor` | `—` | daemon | System | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8946` | — |
| `/plugin-remove` | `—` | daemon | System | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8951` | — |
| `/plugin-install` | `—` | daemon | System | daemon → not served (the TUI keeps its local arm) | `src/tui/app/mod.rs:8961` | — |
| `/collaborators` | `—` | daemon | Collaboration | daemon → served | `src/tui/app/mod.rs:6157` | `CommandExecute`, `PresenceSnapshotGet` |
| `/collaborators` | `presence` | daemon | Collaboration | daemon → served | `src/tui/app/mod.rs:6157` | `CommandExecute`, `PresenceSnapshotGet` |
| `/team` | `—` | daemon | System | daemon → served | `src/tui/app/mod.rs:6173` | `ChatChannelPolicySet`, `ChatPolicyGet`, `ChatProjectPolicySet`, `CommandExecute`, `TeamMembershipAdd`, `TeamMembershipRevoke`, … (9) |
| `/control` | `—` | daemon | Session | daemon → served | `src/tui/app/mod.rs:6185` | `CommandExecute`, `SessionControlRelease`, `SessionControlRequest`, `SessionControlTakeover`, `SessionControlTransfer` |
| `/observe` | `—` | daemon | Collaboration | daemon → served | `src/tui/app/mod.rs:6198` | `CommandExecute`, `ProjectionResume` |
| `/stop-observing` | `—` | daemon | Collaboration | daemon → served | `src/tui/app/mod.rs:6228` | `CommandExecute` |
| `/panels` | `—` | frontend | System | frontend → `OpenPanelManager` | `src/tui/app/mod.rs:6234` | — |
| `/chat` | `—` | frontend | Collaboration | frontend → `FocusChat` | `src/tui/app/mod.rs:6246` | `ChatChannelEnsure`, `ChatChannelPolicySet`, `ChatHistory`, `ChatPolicyGet`, `ChatProjectPolicySet`, `CommandExecute`, … (15) |
| `/chat-send` | `—` | daemon | Collaboration | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6258` | `ChatChannelEnsure`, `ChatSend` |
| `/chat-reply` | `—` | daemon | Collaboration | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6271` | `ChatChannelEnsure`, `ChatSend` |
| `/chat-history` | `—` | daemon | Collaboration | daemon → served | `src/tui/app/mod.rs:6302` | `ChatChannelEnsure`, `ChatHistory`, `CommandExecute` |
| `/chat-sync` | `—` | daemon | Collaboration | daemon → served | `src/tui/app/mod.rs:6322` | `ChatChannelEnsure`, `ChatHistory`, `CommandExecute` |
| `/chat-read` | `—` | daemon | Collaboration | daemon → served | `src/tui/app/mod.rs:6334` | `CommandExecute` |
| `/chat-edit` | `—` | daemon | Collaboration | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6346` | `ChatEdit` |
| `/chat-redact` | `—` | daemon | Collaboration | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6376` | `ChatRedact` |
| `/chat-composing` | `—` | daemon | Collaboration | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6401` | `ChatComposingList`, `ChatComposingSet` |
| `/chat-action-task` | `—` | daemon | Collaboration | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6424` | `ChatActionSubmit`, `ChatChannelEnsure` |
| `/chat-action-review` | `—` | daemon | Collaboration | daemon → not served (the TUI arm issues a daemon request) | `src/tui/app/mod.rs:6460` | `ChatActionSubmit`, `ChatChannelEnsure` |
| `/chat-action-list` | `—` | daemon | Collaboration | daemon → served | `src/tui/app/mod.rs:6495` | `ChatChannelEnsure`, `ChatHistory`, `CommandExecute` |
| `/edit-undo` | `—` | daemon | Session | daemon → served | `src/tui/app/mod.rs:8293` | `CommandExecute`, `EditCheckpointUndo` |
| `/edit-reapply` | `—` | daemon | Session | daemon → served | `src/tui/app/mod.rs:8339` | `CommandExecute`, `EditCheckpointReapply` |
| `/edit-checkpoints` | `—` | daemon | Session | daemon → served | `src/tui/app/mod.rs:8385` | `CommandExecute` |
| `/tool-contracts` | `—` | frontend | Provider | frontend → `ShowToolContracts` | `src/tui/app/mod.rs:7518` | — |
| `/worktree` | `—` | frontend | System | frontend → `ShowWorktrees` | `src/tui/app/mod.rs:6716` | — |

## 4. The families

The regression coverage is organised by family. A family is the catalog's
`CommandDomain`: a stable, user-facing task grouping that the catalog
already carries, so the coverage list and the catalog cannot drift apart.

| family | forms | daemon effect | frontend effect | served | issues a daemon request | no daemon request |
|---|---|---|---|---|---|---|
| Agent | 11 | 10 | 1 | 1 | 7 | 2 |
| Collaboration | 17 | 16 | 1 | 9 | 7 | 0 |
| Diagnostics | 28 | 27 | 1 | 17 | 0 | 10 |
| Execution | 29 | 26 | 3 | 13 | 4 | 9 |
| GitReview | 3 | 3 | 0 | 0 | 1 | 2 |
| Memory | 12 | 11 | 1 | 5 | 2 | 4 |
| Project | 6 | 1 | 5 | 0 | 0 | 1 |
| Provider | 5 | 1 | 4 | 1 | 0 | 0 |
| Research | 5 | 5 | 0 | 0 | 3 | 2 |
| Session | 27 | 13 | 14 | 8 | 3 | 2 |
| System | 24 | 16 | 8 | 7 | 1 | 8 |

## 5. The daemon-effect forms the daemon does not serve

These are the forms the daemon does not serve yet. They are recorded here
rather than left implicit, because the plan's Work package F asks for the
inventory of exactly this set.

| command | form | family | the requests its TUI arm issues |
|---|---|---|---|
| `/sandbox` | `—` | System | `CommandExecute` |
| `/new` | `—` | Session | `ProjectList`, `SessionCreate`, `SessionLoad` |
| `/start` | `—` | Session | `SessionCreate`, `SessionLoad` |
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
| `/skill-promote` | `—` | Memory | `ChatActionSubmit`, `ChatChannelEnsure`, `ChatChannelPolicySet`, `ChatComposingList`, `ChatComposingSet`, `ChatEdit`, … (32) |
| `/skill-proposals` | `—` | Memory | — |
| `/skill-proposal` | `—` | Memory | — |
| `/plan` | `—` | Execution | — |
| `/diff` | `—` | GitReview | `AgentRunTranscriptRead`, `ChatChannelEnsure`, `ChatEdit`, `ChatHistory`, `ChatHistoryOlder`, `ChatRedact`, … (37) |
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
| `/lsp-doctor` | `—` | Diagnostics | — |
| `/lsp-repair-hunk` | `—` | Diagnostics | — |
| `/lsp-review-diff` | `—` | Diagnostics | — |
| `/lsp-security-review` | `—` | Diagnostics | — |
| `/security-review` | `—` | Agent | — |
| `/security-review-show` | `—` | Agent | `ProjectList` |
| `/security-review-cancel` | `—` | Agent | — |
| `/shell-include` | `—` | Execution | `HumanShellPromotionStage` |
| `/shell-rerun` | `—` | Execution | — |
| `/shell-ask` | `—` | Execution | `AgentRunTranscriptRead`, `ChatChannelEnsure`, `ChatEdit`, `ChatHistory`, `ChatHistoryOlder`, `ChatRedact`, … (39) |
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
| `/policy` | `—` | `execution_policy_get` |
| `/init` | `—` | `project_init_draft_get` |
| `/reasoning` | `—` | `reasoning_preference_get` |
| `/reload` | `—` | `asset_refresh` |
| `/schedules` | `—` | `schedule_list` |
| `/share` | `—` | `session_share` |
| `/shell-expand` | `—` | `human_shell_run_read` |
| `/shell-kill` | `—` | `human_shell_run_cancel` |
| `/shell-show` | `—` | `human_shell_run_read` |
| `/state` | `—` | `active_goal_load` |
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

**39 of the 129 daemon-effect forms are migrated.**
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
| the declared frontend-effect set | the action that serves a **catalog command form** | `crates/codegg-core/src/frontend_action.rs` — 35 `FrontendAction` variants, keyed by `(command_id, form)` | the palette, through `action_for_form(command_id, form)` |
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

**19 are misclassified and 21 are real missing
capability**, out of 40 daemon-effect forms whose TUI arm issues
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
| `/lsp-doctor` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-repair-hunk` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-review-diff` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
| `/lsp-security-review` | Diagnostics | missing capability | the authoritative language server is daemon-owned, and the daemon has no request for this surface |
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

The accounting is complete: **129 daemon-effect forms = 61 served + 28 mechanical-unserved + 40 with no daemon request**, and every one of them is named.

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
| `/skill-promote` | Memory | subsystem move | the arm drives 32 daemon requests; moving it is a subsystem, not a table row |
| `/diff` | GitReview | subsystem move | the arm drives 37 daemon requests; moving it is a subsystem, not a table row |
| `/research-runs` | Research | misclassified | the TUI's own `ResearchService` over the local filesystem; the catalog's kind is wrong and the corrected kind is `FrontendEffect` |
| `/research-open` | Research | misclassified | the TUI's own `ResearchService` over the local filesystem; the catalog's kind is wrong and the corrected kind is `FrontendEffect` |
| `/research-show` | Research | misclassified | the TUI's own `ResearchService` over the local filesystem; the catalog's kind is wrong and the corrected kind is `FrontendEffect` |
| `/security-review-show` | Agent | not mechanical | the arm shows the TUI's cached review result, and the daemon has no request that reads one — the same missing surface as `/security-review` (M007-H) |
| `/shell-include` | Execution | not mechanical | `HumanShellPromotionStage` carries three identifiers beside the session, and the argument's convention has room for one |
| `/shell-ask` | Execution | subsystem move | the arm drives 39 daemon requests; moving it is a subsystem, not a table row |
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
