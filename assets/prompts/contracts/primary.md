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
