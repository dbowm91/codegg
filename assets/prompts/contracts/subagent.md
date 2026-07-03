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
