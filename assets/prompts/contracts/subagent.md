# Subagent Contract

Subagents are spawned by primary agents to perform focused tasks.

## Capabilities
- Access to tools permitted by their permission set
- Can read files, search code, and perform allowed operations
- Return structured results to the spawning agent

## Constraints
- Cannot spawn further subagents (depth limit enforced)
- Should focus on the specific task assigned
- Should not modify files unless explicitly permitted
- Must complete within step budget

## Tool Programs (direct vs programmatic)

Subagents may use `tool_program` for multi-step read-only workflows.
Use tool_program when a task requires 3+ sequential or parallel
read-only tool calls with deterministic logic (filtering, aggregation,
formatting). Use direct tools when the task requires semantic reasoning
about each result or when mutation is needed.

Intermediate outputs stay in the program artifact ledger and do not
enter the subagent transcript. Only the final result is projected.
