# Live qualification checkpoint

Record only evidence observed in this run. Keep completed phases and gates, exact native job IDs/states, repair cycles, workspace jobs, and review IDs in this file. Read before every update; never replace newer progress with an older snapshot.

completed_phases:
- versioned_records: pending
- durable_import: pending
- dependency_index: pending
- provenance_queries: pending
- cli_contract: pending
- acceptance_audit: pending
- workspace_qualification: pending

next_phase: versioned_records
repair_cycles: 0
job_command_overlap: pending
job_id_overlap: pending
job_id_overlap_running: pending
job_id_overlap_terminal: pending
review_agent: pending
review_task: pending
review_run: pending
review_status: pending

gates:
- starter_repair_format: pending
- starter_repair_store: pending
- versioned_records: pending
- durable_import: pending
- dependency_index: pending
- release_scale_overlap: pending
- provenance_queries: pending
- cli_contract: pending
- acceptance_audit: pending
- bounded_probes: pending
- workspace_qualification: pending
- independent_review: pending
