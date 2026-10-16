# Command Intent Post-Closure Corrective Addendum

Status: active — C001 aligns the `git remote -v` test and architecture description with the existing read-only classifier

Controlling process: `plans/003-planning-process.md#7-corrective-passes`
Owning module: `architecture/command_intent.md`

## Trigger

Hosted CI run `38844497889` reached `command_intent::tests::git_remote_v_is_mutating`
and failed because `classify_command("git remote -v")` returned
`GitReadOnly`, while the stale test expected `GitMutating`. The classifier's
`classify_git_remote` branch treats `-v` as a read-only remote listing. The
production behavior is correct; the test comment and the architecture
description were stale.

## Corrective scope

C001 renames and corrects the test to assert read-only classification, updates
the architecture description, and runs the canonical hosted CI suite. It does
not alter command routing or classification behavior. `git remote show
origin` retains its existing conservative fallback and test.

## Closure

Close after hosted `CI / verify` passes on the exact change head with the full
workspace suite completed. Record the run and test counts in
`plans/closure/command-intent-post-closure-corrective/001-status.md` and update
the planning registry. Do not edit historical closures.

