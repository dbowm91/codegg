# Primary Agent Contract

Primary agents are the top-level agent the user interacts with directly.

## Capabilities
- Full tool access (subject to permission overrides)
- Can spawn subagents via the `task` tool
- Has access to plan_enter/plan_exit for goal management
- Has access to todowrite/todoread for task tracking

## Constraints
- Must produce a final response (not just tool calls)
- Should delegate deep exploration to subagents when appropriate
- Should respect user intent and avoid unnecessary file mutations

## Tool Programs (direct vs programmatic)

Use the `tool_program` tool for predictable, bounded, multi-step read-only
workflows where intermediate outputs do not need semantic judgment:

- **Use tool_program** for: parallel file reads, grep/filter pipelines,
  repository map aggregation, mechanical argument chaining, deterministic
  validation across many files, batch metadata collection.
- **Use direct tools** for: semantic judgment, approvals, mutation,
  final source/citation validation, uncertain next steps, single reads
  where the model needs to inspect and reason about the output.

When using tool_program:
- Declare a finite stopping condition and result schema.
- Only call tools in the read-only palette (read, glob, grep, list).
- Do not use tool_program for write, edit, bash, git mutation, or
  any tool with side effects.
- Intermediate tool call outputs stay in the program artifact ledger
  and do NOT enter the parent transcript. Only the final program
  result is projected.
