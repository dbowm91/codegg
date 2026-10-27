#!/usr/bin/env bash
#
# verify.sh — Canonical local verification entry point for CodeGG.
#
# Usage:
#   scripts/verify.sh quick    — cheap repository sanity for ordinary iteration
#   scripts/verify.sh full     — broad maintainer/developer verification before handoff or release
#   scripts/verify.sh scoped [base-ref]
#                              — developer-local verification of only the tests a
#                                change can affect (default base: origin/main)
#   scripts/verify.sh help     — print usage
#
# Resource policy:
#   Broad Cargo commands use CARGO_BUILD_JOBS=2 by default (local
#   developer machines). Hosted CI sets CARGO_BUILD_JOBS=4 (== runner
#   vCPUs, M001 disposition in architecture/testing.md). Test execution
#   stays serial within each binary; cross-binary parallelism comes
#   from nextest profile `ci`.
#   Callers may override any env var before invoking this script.
#
# The script stops at the first failing command and returns its status.

set -euo pipefail

# ── Resolve repository root from script location ────────────────────────────
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# ── Broad-test resource contract (matches CI) ───────────────────────────────
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"

# ── Usage ───────────────────────────────────────────────────────────────────
usage() {
    cat <<EOF
Usage: scripts/verify.sh <mode>

Modes:
  quick   Cheap repository sanity for ordinary iteration.
          Developers run focused tests for changed code separately.
  full    Broad maintainer verification before handoff or release.
  scoped  Developer-local verification of only the tests a change can
          affect. Takes an optional base ref (default: origin/main).
          The hosted CI job's scope is unchanged.
  help    Print this message.

Resource policy:
  Both modes set CARGO_BUILD_JOBS=2 by default.
  Test execution uses nextest profile `ci` (serial within each binary,
  parallel across binaries); plain `cargo test` broad runs keep
  --test-threads=1.
  Callers may override via environment variables.
  cargo-nextest is required for full mode.
EOF
}

# ── Quick tier ──────────────────────────────────────────────────────────────
run_guards() {
    echo "==> cargo fmt --check --all"
    (cd "$REPO_ROOT" && cargo fmt --check --all)

    echo "==> python3 scripts/generate_builtin_agents.py --check"
    (cd "$REPO_ROOT" && python3 scripts/generate_builtin_agents.py --check)

    echo "==> python3 scripts/generate_subagent_names.py --check"
    (cd "$REPO_ROOT" && python3 scripts/generate_subagent_names.py --check)

    echo "==> ./scripts/check-core-boundary.sh"
    (cd "$REPO_ROOT" && ./scripts/check-core-boundary.sh)
    echo "==> ./scripts/check-client-boundary.sh"
    (cd "$REPO_ROOT" && ./scripts/check-client-boundary.sh)

    echo "==> ./scripts/check-desktop-boundary.sh"
    (cd "$REPO_ROOT" && ./scripts/check-desktop-boundary.sh)

    # M007-D: the daemon owns the command catalog (ADR-0052). The catalog
    # ownership guard proves the daemon's catalog and the TUI's command
    # surface are the same set, that only the catalog can execute a
    # command, and that no frontend holds a second catalog. The
    # frontend-action guard proves the frontend's dispatchable set is
    # declared, finite, closed and non-mutating. Both carry a self-test,
    # because a guard that cannot fail on an injected defect is not
    # evidence.
    echo "==> python3 scripts/check_command_catalog_ownership.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_command_catalog_ownership.py --self-test)
    echo "==> python3 scripts/check_command_catalog_ownership.py"
    (cd "$REPO_ROOT" && python3 scripts/check_command_catalog_ownership.py)

    echo "==> python3 scripts/check_frontend_action_set.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_frontend_action_set.py --self-test)
    echo "==> python3 scripts/check_frontend_action_set.py"
    (cd "$REPO_ROOT" && python3 scripts/check_frontend_action_set.py)

    # The desktop tree is a separate Cargo workspace that the root check
    # never compiles, so the hosted desktop qualification is the only
    # automated gate it has. A guard which is not run automatically cannot
    # protect anything, so this one runs here with its self-test: it fails
    # closed when the desktop tree falls out of that gate.
    echo "==> python3 scripts/check_desktop_gate_wiring.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_desktop_gate_wiring.py --self-test)

    echo "==> python3 scripts/check_desktop_gate_wiring.py"
    (cd "$REPO_ROOT" && python3 scripts/check_desktop_gate_wiring.py)

    # The renderer's panel layer renders the projection it is given: it calls
    # no bridge, holds no Tauri capability, reaches no filesystem or process,
    # and keeps no durable frontend-owned store. A panel that needs daemon
    # data names the existing variant or stops, so the invariant is enforced
    # rather than promised. A guard which is not run automatically cannot
    # protect anything, so this one runs here with its self-test.
    echo "==> python3 scripts/check_desktop_panel_authority.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_desktop_panel_authority.py --self-test)

    echo "==> python3 scripts/check_desktop_panel_authority.py"
    (cd "$REPO_ROOT" && python3 scripts/check_desktop_panel_authority.py)

    # The editor's text authority: the host may serve the bounded viewport
    # window it cut from the controller's own snapshot, and the renderer may
    # hold that window and no second copy of the text. The editor's DTO
    # mirror is checked here too, because the shared bindings check reads
    # the bridge module only. The guard runs with its self-test, because a
    # guard that cannot be shown to fail on an injected defect is not
    # evidence.
    echo "==> python3 scripts/check_desktop_editor_authority.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_desktop_editor_authority.py --self-test)

    echo "==> python3 scripts/check_desktop_editor_authority.py"
    (cd "$REPO_ROOT" && python3 scripts/check_desktop_editor_authority.py)

    echo "==> python3 scripts/check_sandbox_contract.py"
    (cd "$REPO_ROOT" && python3 scripts/check_sandbox_contract.py)

    echo "==> python3 scripts/check_adr_integrity.py"
    (cd "$REPO_ROOT" && python3 scripts/check_adr_integrity.py)

    echo "==> python3 scripts/check_execution_ownership.py"
    (cd "$REPO_ROOT" && python3 scripts/check_execution_ownership.py)

    # The tool-call analogue of the execution-ownership guard: the broker is
    # the only production path from a subsystem to a tool. It sits here
    # because it constrains the same question — who may execute what — for
    # tool dispatch instead of process dispatch.
    echo "==> python3 scripts/check_tool_broker_boundary.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_tool_broker_boundary.py --self-test)

    echo "==> python3 scripts/check_tool_broker_boundary.py"
    (cd "$REPO_ROOT" && python3 scripts/check_tool_broker_boundary.py)
    # The daemon's turn code resolves its workspace root from the execution
    # context, never from the process working directory: a user-scoped
    # daemon's working directory is wherever it was launched. The guard
    # covers `src/core/**` and the turn-code surfaces, plus the
    # `codegg-core` crate. A guard which is not run automatically cannot
    # protect anything, so it runs here with its self-test.
    echo "==> python3 scripts/check_daemon_cwd_usage.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_daemon_cwd_usage.py --self-test)

    echo "==> python3 scripts/check_daemon_cwd_usage.py"
    (cd "$REPO_ROOT" && python3 scripts/check_daemon_cwd_usage.py)

    # Recovery observation fields must have a production writer. The dead
    # terminal-stall API and parallel-limit knob stay removed under ADR-0040.
    echo "==> python3 scripts/check_progress_recovery_observation.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_progress_recovery_observation.py --self-test)

    echo "==> python3 scripts/check_progress_recovery_observation.py"
    (cd "$REPO_ROOT" && python3 scripts/check_progress_recovery_observation.py)

    echo "==> python3 scripts/check_headless_workspace_registry_binding.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_headless_workspace_registry_binding.py --self-test)

    echo "==> python3 scripts/check_headless_workspace_registry_binding.py"
    (cd "$REPO_ROOT" && python3 scripts/check_headless_workspace_registry_binding.py)

    # The scratch reclaim path decides on four facts — containment, real
    # directory, ownership, and the owner record — and it removes a tree only
    # when it holds no entries at all. A reclaim failure that wears the
    # provisioning text, a removal that skips the owner-record check, and a
    # report that disagrees with the validator are all silent: they are wrong
    # answers, not crashes. A guard which is not run automatically cannot
    # protect anything, so it runs here with its self-test.
    echo "==> python3 scripts/check_scratch_reclaim_invariants.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_scratch_reclaim_invariants.py --self-test)

    echo "==> python3 scripts/check_scratch_reclaim_invariants.py"
    (cd "$REPO_ROOT" && python3 scripts/check_scratch_reclaim_invariants.py)

    # The trusted Git route owns every CodeGG-owned Git child, and the
    # hostile-metadata detector is the mediation that refuses a repository
    # which declares a configuration surface the route cannot neutralise.
    # A guard which is not run automatically cannot protect anything, so
    # both the route guard and the diff-consumer guard run here with their
    # self-tests: a guard that cannot be shown to fail on an injected
    # defect is not evidence.
    echo "==> python3 scripts/check_git_forbidden_patterns.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_git_forbidden_patterns.py --self-test)

    echo "==> python3 scripts/check_git_forbidden_patterns.py"
    (cd "$REPO_ROOT" && python3 scripts/check_git_forbidden_patterns.py)

    echo "==> python3 scripts/check_git_consistency_consumers.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_git_consistency_consumers.py --self-test)

    echo "==> python3 scripts/check_git_consistency_consumers.py"
    (cd "$REPO_ROOT" && python3 scripts/check_git_consistency_consumers.py)

    # The contained managed-worktree child's writable grant is the SHARED
    # administration root, and ADR-0048 says so. The grant's precision is a
    # security boundary: a description that understates it is a description
    # the reviewer can trust too far. The guard runs with its self-test.
    echo "==> python3 scripts/check_contained_worktree_grant.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_contained_worktree_grant.py --self-test)

    echo "==> python3 scripts/check_contained_worktree_grant.py"
    (cd "$REPO_ROOT" && python3 scripts/check_contained_worktree_grant.py)

    # A delegated run's terminal state must be durable: the delegated job has
    # to subscribe for a terminal notice, or the orchestrator never learns what
    # happened to its child. The subscription's disposition and kind are part
    # of the same contract, so the guard runs with its self-test.
    echo "==> python3 scripts/check_subagent_notification_wiring.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_subagent_notification_wiring.py --self-test)

    echo "==> python3 scripts/check_subagent_notification_wiring.py"
    (cd "$REPO_ROOT" && python3 scripts/check_subagent_notification_wiring.py)

    # A delegated run's transcript must be written redacted and bounded. The
    # declared bounds and the redaction entry point are production claims, so
    # the guard runs with its self-test.
    echo "==> python3 scripts/check_delegated_transcript_store.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_delegated_transcript_store.py --self-test)

    echo "==> python3 scripts/check_delegated_transcript_store.py"
    (cd "$REPO_ROOT" && python3 scripts/check_delegated_transcript_store.py)

    # A delegated run's provider entry must be either the original attempt or
    # a claimed continuation. A provider entry reachable without a claim is a
    # continuation that was never authorized, so the guard runs with its
    # self-test.
    echo "==> python3 scripts/check_delegated_continuation_authority.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_delegated_continuation_authority.py --self-test)

    echo "==> python3 scripts/check_delegated_continuation_authority.py"
    (cd "$REPO_ROOT" && python3 scripts/check_delegated_continuation_authority.py)

    # The projection guards used to be change-triggered only. The delegated
    # run's transcript now rides the projection, so a payload that is
    # declared but never populated — or one that crosses to the wrong
    # session — is a silent failure. Each guard carries a self-test.
    echo "==> bash scripts/check_projection_disclosure.sh --self-test"
    (cd "$REPO_ROOT" && bash scripts/check_projection_disclosure.sh --self-test)

    echo "==> bash scripts/check_projection_disclosure.sh"
    (cd "$REPO_ROOT" && bash scripts/check_projection_disclosure.sh)

    echo "==> bash scripts/check_projection_publication_seam.sh --self-test"
    (cd "$REPO_ROOT" && bash scripts/check_projection_publication_seam.sh --self-test)

    echo "==> bash scripts/check_projection_publication_seam.sh"
    (cd "$REPO_ROOT" && bash scripts/check_projection_publication_seam.sh)

    echo "==> python3 scripts/check_websocket_bounds.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_websocket_bounds.py --self-test)

    echo "==> python3 scripts/check_websocket_bounds.py"
    (cd "$REPO_ROOT" && python3 scripts/check_websocket_bounds.py)
    # The contained child's argv pins core.hooksPath, so the repository's own
    # hooks cannot run inside the containment. The pin is a defence in depth,
    # not a narrowing of the grant: the shared root stays writable. A pin
    # carried by the environment would be dropped by the child's policy and
    # would be a no-op that reads like a control, so the guard refuses that
    # route by name.
    echo "==> python3 scripts/check_contained_worktree_hooks_path.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_contained_worktree_hooks_path.py --self-test)

    echo "==> python3 scripts/check_contained_worktree_hooks_path.py"
    (cd "$REPO_ROOT" && python3 scripts/check_contained_worktree_hooks_path.py)

    # The shared checkout's lease belongs to the group, and the mediation
    # authority must not fail open. Both are correctness contracts whose
    # regressions are silent, so both guards run here with their self-tests.
    echo "==> python3 scripts/check_writer_fence_completeness.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_writer_fence_completeness.py --self-test)

    echo "==> python3 scripts/check_writer_fence_completeness.py"
    (cd "$REPO_ROOT" && python3 scripts/check_writer_fence_completeness.py)

    echo "==> python3 scripts/check_checkout_mediation_authority.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_checkout_mediation_authority.py --self-test)

    echo "==> python3 scripts/check_checkout_mediation_authority.py"
    (cd "$REPO_ROOT" && python3 scripts/check_checkout_mediation_authority.py)

    echo "==> python3 scripts/check_network_intent_owner_wiring.py"
    (cd "$REPO_ROOT" && python3 scripts/check_network_intent_owner_wiring.py)

    echo "==> python3 scripts/check_tui_project_authority.py"
    (cd "$REPO_ROOT" && python3 scripts/check_tui_project_authority.py)

    echo "==> python3 scripts/check_tui_editor_text_authority.py"
    (cd "$REPO_ROOT" && python3 scripts/check_tui_editor_text_authority.py)

    echo "==> python3 scripts/check_tui_editor_key_authority.py"
    (cd "$REPO_ROOT" && python3 scripts/check_tui_editor_key_authority.py)

    echo "==> python3 scripts/check_tui_editor_leaf_authority.py"
    (cd "$REPO_ROOT" && python3 scripts/check_tui_editor_leaf_authority.py)

    echo "==> python3 scripts/check_tui_editor_completion_authority.py"
    (cd "$REPO_ROOT" && python3 scripts/check_tui_editor_completion_authority.py)

    echo "==> python3 scripts/check_tui_editor_apply_path.py"
    (cd "$REPO_ROOT" && python3 scripts/check_tui_editor_apply_path.py)

    echo "==> python3 scripts/check_tui_editor_prefix_state.py"
    (cd "$REPO_ROOT" && python3 scripts/check_tui_editor_prefix_state.py)

    echo "==> python3 scripts/check_tui_editor_doc_drift.py"
    (cd "$REPO_ROOT" && python3 scripts/check_tui_editor_doc_drift.py)

    echo "==> python3 scripts/check_tui_files_keymap_drift.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_tui_files_keymap_drift.py --self-test)

    echo "==> python3 scripts/check_tui_files_keymap_drift.py"
    (cd "$REPO_ROOT" && python3 scripts/check_tui_files_keymap_drift.py)

    echo "==> python3 scripts/check_comment_budget.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_comment_budget.py --self-test)

    echo "==> python3 scripts/check_comment_budget.py"
    (cd "$REPO_ROOT" && python3 scripts/check_comment_budget.py)

    echo "==> python3 scripts/check_tui_transport_authority.py"
    (cd "$REPO_ROOT" && python3 scripts/check_tui_transport_authority.py)

    echo "==> python3 scripts/check_tui_subagent_name_pool_isolation.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_tui_subagent_name_pool_isolation.py --self-test)
    echo "==> python3 scripts/check_tui_subagent_name_pool_isolation.py"
    (cd "$REPO_ROOT" && python3 scripts/check_tui_subagent_name_pool_isolation.py)

    echo "==> python3 scripts/check_tui_composer_key_authority.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_tui_composer_key_authority.py --self-test)
    echo "==> python3 scripts/check_tui_composer_key_authority.py"
    (cd "$REPO_ROOT" && python3 scripts/check_tui_composer_key_authority.py)

    # The published composer keymap and the implemented claim table must state
    # one rule in one wording: the guard reads the condition's label out of the
    # implementation and requires the published tables to carry it verbatim.
    echo "==> python3 scripts/check_tui_composer_keymap_drift.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_tui_composer_keymap_drift.py --self-test)
    echo "==> python3 scripts/check_tui_composer_keymap_drift.py"
    (cd "$REPO_ROOT" && python3 scripts/check_tui_composer_keymap_drift.py)

    # A command handler may not block on a runtime handle from inside the
    # runtime: the TUI event loop and the MCP CLI both run with the runtime
    # context entered, so a nested `Handle::block_on` panics and takes the
    # process down.
    echo "==> python3 scripts/check_no_nested_runtime_block_on.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_no_nested_runtime_block_on.py --self-test)
    echo "==> python3 scripts/check_no_nested_runtime_block_on.py"
    (cd "$REPO_ROOT" && python3 scripts/check_no_nested_runtime_block_on.py)
    echo "==> python3 scripts/check_command_argument_contract.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_command_argument_contract.py --self-test)
    echo "==> python3 scripts/check_command_argument_contract.py"
    (cd "$REPO_ROOT" && python3 scripts/check_command_argument_contract.py)

    # The render path derives each expensive result once: one producer for
    # the composer's wrap, one producer for the Markdown parse, and no
    # whole-buffer clone on the render path.
    echo "==> python3 scripts/check_tui_render_cost.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_tui_render_cost.py --self-test)
    echo "==> python3 scripts/check_tui_render_cost.py"
    (cd "$REPO_ROOT" && python3 scripts/check_tui_render_cost.py)

    echo "==> python3 scripts/check_daemon_listing_authority.py"
    (cd "$REPO_ROOT" && python3 scripts/check_daemon_listing_authority.py)

    echo "==> python3 scripts/check_http_route_disposition.py"
    (cd "$REPO_ROOT" && python3 scripts/check_http_route_disposition.py)

    echo "==> python3 scripts/check_writer_disposition.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_writer_disposition.py --self-test)

    echo "==> python3 scripts/check_writer_disposition.py"
    (cd "$REPO_ROOT" && python3 scripts/check_writer_disposition.py)

    echo "==> python3 scripts/check_process_denial.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_process_denial.py --self-test)

    echo "==> python3 scripts/check_process_denial.py"
    (cd "$REPO_ROOT" && python3 scripts/check_process_denial.py)

    echo "==> python3 scripts/check_shared_checkout.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_shared_checkout.py --self-test)

    echo "==> python3 scripts/check_shared_checkout.py"
    (cd "$REPO_ROOT" && python3 scripts/check_shared_checkout.py)

    echo "==> python3 scripts/check_blackboard_contract_matrix.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_blackboard_contract_matrix.py --self-test)

    echo "==> python3 scripts/check_blackboard_contract_matrix.py"
    (cd "$REPO_ROOT" && python3 scripts/check_blackboard_contract_matrix.py)

    echo "==> python3 scripts/check_audit_coverage.py"
    (cd "$REPO_ROOT" && python3 scripts/check_audit_coverage.py)

    echo "==> python3 scripts/check_queue_delivery_callsites.py"
    (cd "$REPO_ROOT" && python3 scripts/check_queue_delivery_callsites.py)

    echo "==> python3 scripts/check_queue_write_transaction_shape.py"
    (cd "$REPO_ROOT" && python3 scripts/check_queue_write_transaction_shape.py)

    echo "==> python3 scripts/check_scheduler_bypass.py"
    (cd "$REPO_ROOT" && python3 scripts/check_scheduler_bypass.py)

    echo "==> python3 scripts/check_config_merge_coverage.py"
    (cd "$REPO_ROOT" && python3 scripts/check_config_merge_coverage.py)

    echo "==> python3 scripts/check_provider_wire_boundary.py"
    (cd "$REPO_ROOT" && python3 scripts/check_provider_wire_boundary.py)

    echo "==> python3 scripts/check_provider_wire_cutover.py"
    (cd "$REPO_ROOT" && python3 scripts/check_provider_wire_cutover.py)

    echo "==> python3 scripts/check_reasoning_intent_fidelity.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_reasoning_intent_fidelity.py --self-test)
    echo "==> python3 scripts/check_reasoning_intent_fidelity.py"
    (cd "$REPO_ROOT" && python3 scripts/check_reasoning_intent_fidelity.py)

    echo "==> python3 scripts/check_reasoning_vocabulary_open.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_reasoning_vocabulary_open.py --self-test)
    echo "==> python3 scripts/check_reasoning_vocabulary_open.py"
    (cd "$REPO_ROOT" && python3 scripts/check_reasoning_vocabulary_open.py)

    echo "==> python3 scripts/check_model_profile_capability_mirror.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_model_profile_capability_mirror.py --self-test)
    echo "==> python3 scripts/check_model_profile_capability_mirror.py"
    (cd "$REPO_ROOT" && python3 scripts/check_model_profile_capability_mirror.py)

    echo "==> python3 scripts/check_tui_reasoning_surface.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_tui_reasoning_surface.py --self-test)
    echo "==> python3 scripts/check_tui_reasoning_surface.py"
    (cd "$REPO_ROOT" && python3 scripts/check_tui_reasoning_surface.py)

    echo "==> python3 scripts/check_provider_qualification.py"
    (cd "$REPO_ROOT" && python3 scripts/check_provider_qualification.py)

    echo "==> python3 scripts/check_provider_multi_surface_dispatch.py"
    (cd "$REPO_ROOT" && python3 scripts/check_provider_multi_surface_dispatch.py)

    echo "==> python3 scripts/check_openai_endpoint_composition.py"
    (cd "$REPO_ROOT" && python3 scripts/check_openai_endpoint_composition.py)

    echo "==> python3 scripts/check_provider_catalog_consistency.py"
    (cd "$REPO_ROOT" && python3 scripts/check_provider_catalog_consistency.py)

    echo "==> python3 scripts/check_no_hardcoded_models.py"
    (cd "$REPO_ROOT" && python3 scripts/check_no_hardcoded_models.py)

    echo "==> python3 scripts/check_no_hardcoded_selection_colours.py"
    (cd "$REPO_ROOT" && python3 scripts/check_no_hardcoded_selection_colours.py)

    echo "==> python3 scripts/check_provider_resilience_ownership.py"
    (cd "$REPO_ROOT" && python3 scripts/check_provider_resilience_ownership.py)

    echo "==> python3 scripts/check_eggwork_target_routing.py"
    (cd "$REPO_ROOT" && python3 scripts/check_eggwork_target_routing.py)

    echo "==> python3 scripts/check_eggwork_target_routing.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_eggwork_target_routing.py --self-test)

    echo "==> python3 scripts/check_first_party_skills.py"
    (cd "$REPO_ROOT" && python3 scripts/check_first_party_skills.py)

    echo "==> python3 scripts/check_shell_policy_test_safety.py"
    (cd "$REPO_ROOT" && python3 scripts/check_shell_policy_test_safety.py)

    echo "==> python3 scripts/check_destructive_risk_signal_coverage.py"
    (cd "$REPO_ROOT" && python3 scripts/check_destructive_risk_signal_coverage.py)

    echo "==> python3 scripts/check_destructive_risk_signal_coverage.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_destructive_risk_signal_coverage.py --self-test)

    echo "==> python3 scripts/test_shell_policy_test_safety.py"
    (cd "$REPO_ROOT" && python3 scripts/test_shell_policy_test_safety.py)

    # A difference between the dev and test cargo profiles makes the same crate
    # compile twice, so this runs before the expensive compile rather than
    # after it.
    echo "==> python3 scripts/check_cargo_profile_alignment.py"
    (cd "$REPO_ROOT" && python3 scripts/check_cargo_profile_alignment.py)
    echo "==> python3 scripts/check_cargo_profile_alignment.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_cargo_profile_alignment.py --self-test)

    # The ownership manifest is the scoped mode's input. It must agree with
    # the tree, or the scoped mode would select a scope that does not exist.
    echo "==> python3 scripts/check_test_ownership.py"
    (cd "$REPO_ROOT" && python3 scripts/check_test_ownership.py)
    echo "==> python3 scripts/check_test_ownership.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_test_ownership.py --self-test)

    # The scoped mode's selection algorithm and its `members` output format.
    # The caller reads that format with `IFS=$'\t' read -r name targets
    # features`, so a column that moves is a scope that silently changes.
    echo "==> python3 scripts/scoped_selection.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/scoped_selection.py --self-test)

    # Every nextest/CI filter that names a test target must name one that
    # exists, and the recorded integration-test count must be true. A filter
    # that names a renamed target silently stops reserving the runner for it.
    echo "==> python3 scripts/check_test_target_references.py"
    (cd "$REPO_ROOT" && python3 scripts/check_test_target_references.py)
    echo "==> python3 scripts/check_test_target_references.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_test_target_references.py --self-test)

    # Every test body must assert something checkable, and no two test bodies
    # may assert the same thing twice unless the decision table records the
    # survivor. A test that asserts nothing costs a compile-and-run cycle to
    # prove nothing; a duplicated body costs it twice.
    echo "==> python3 scripts/check_test_assertion_quality.py"
    (cd "$REPO_ROOT" && python3 scripts/check_test_assertion_quality.py)
    echo "==> python3 scripts/check_test_assertion_quality.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_test_assertion_quality.py --self-test)

    echo "==> python3 scripts/check_test_duplication.py"
    (cd "$REPO_ROOT" && python3 scripts/check_test_duplication.py)
    echo "==> python3 scripts/check_test_duplication.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_test_duplication.py --self-test)

    # The Tool Program pipeline is parse-only. The invariant used to be
    # "enforced" by three test functions that could not fail; the guard
    # replaced them with a check that can.
    echo "==> python3 scripts/check_tool_program_parse_only.py"
    (cd "$REPO_ROOT" && python3 scripts/check_tool_program_parse_only.py)
    echo "==> python3 scripts/check_tool_program_parse_only.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_tool_program_parse_only.py --self-test)

    # The domain's `JobState::is_terminal()` is the terminality authority.
    # Every projection of a job state has to agree with it, and a new
    # variant must not be added while the projections answer for the old
    # set. The guard replaced a hand-written terminality list in a query
    # that contradicted the domain.
    echo "==> python3 scripts/check_job_state_terminality_agreement.py"
    (cd "$REPO_ROOT" && python3 scripts/check_job_state_terminality_agreement.py)
    echo "==> python3 scripts/check_job_state_terminality_agreement.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_job_state_terminality_agreement.py --self-test)

    # Every registered CI test filter must still name a command the workflow
    # runs, and must select at least one test the source declares. A filter
    # that names a renamed target qualifies nothing and still reports green.
    echo "==> python3 scripts/check_test_filter_execution.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_test_filter_execution.py --self-test)

    echo "==> python3 scripts/check_test_filter_execution.py"
    (cd "$REPO_ROOT" && python3 scripts/check_test_filter_execution.py)

    # A test that re-executes its own binary must name its test by the full
    # path it has inside its own cargo target. Moving a file into a family
    # changes that path, and a name that no longer matches selects nothing
    # while still exiting 0.
    echo "==> python3 scripts/check_test_self_reexec_paths.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_test_self_reexec_paths.py --self-test)

    echo "==> python3 scripts/check_test_self_reexec_paths.py"
    (cd "$REPO_ROOT" && python3 scripts/check_test_self_reexec_paths.py)

    # Every binder of the shared integration-test helper tree binds it
    # publicly. The tree's items are `pub` and carry no blanket dead-code
    # suppression, so a private binding makes them unreachable from the
    # binding crate root -- a hosted clippy failure that only a Linux
    # runner sees.
    echo "==> python3 scripts/check_test_common_binding.py --self-test"
    (cd "$REPO_ROOT" && python3 scripts/check_test_common_binding.py --self-test)

    echo "==> python3 scripts/check_test_common_binding.py"
    (cd "$REPO_ROOT" && python3 scripts/check_test_common_binding.py)
}

# ── Quick tier ──────────────────────────────────────────────────────────────
run_quick() {
    echo "==> Quick verification"

    run_guards

    echo "==> python3 -m unittest discover -s dev/harness_lab/tests"
    (cd "$REPO_ROOT" && python3 -m unittest discover -s dev/harness_lab/tests)

    echo "==> CARGO_BUILD_JOBS=$CARGO_BUILD_JOBS cargo check --workspace --all-targets --locked"
    (cd "$REPO_ROOT" && cargo check --workspace --all-targets --locked)

    echo "==> Quick verification passed."
}

# ── Full tier ───────────────────────────────────────────────────────────────
run_full() {
    echo "==> Full verification"
    echo "==> Broad-test environment: CARGO_BUILD_JOBS=$CARGO_BUILD_JOBS nextest profile ci"

    if ! command -v cargo-nextest >/dev/null 2>&1; then
        echo "Error: full mode requires cargo-nextest (cargo install cargo-nextest --locked)" >&2
        exit 1
    fi

    # The scratch allocator refuses a per-user cache root another user could
    # write to, and it creates the root it depends on privately. The sweep must
    # not inherit the operator's cache root: its result has to be a property of
    # the product rather than of the machine the sweep ran on. A private
    # directory outside the workspace keeps the scratch root outside every
    # granted workspace root, exactly as the real per-user cache root is.
    sweep_cache_home="$(mktemp -d "${TMPDIR:-/tmp}/codegg-sweep-cache.XXXXXX")"
    export XDG_CACHE_HOME="$sweep_cache_home"
    trap 'rm -rf "$sweep_cache_home"' EXIT
    echo "==> Sweep cache root: $XDG_CACHE_HOME"

    # Quick checks first
    run_quick

    echo "==> CARGO_BUILD_JOBS=$CARGO_BUILD_JOBS cargo clippy --workspace --all-targets --locked -- -D warnings"
    (cd "$REPO_ROOT" && cargo clippy --workspace --all-targets --locked -- -D warnings)

    echo "==> CARGO_BUILD_JOBS=$CARGO_BUILD_JOBS cargo nextest run --workspace --locked --profile ci"
    (cd "$REPO_ROOT" && cargo nextest run --workspace --locked --profile ci)

    # M002: the feature-enabled pass is BOUNDED. It builds and runs only the
    # test targets the extra feature set actually unlocks, instead of
    # re-linking every root integration target under the new feature set.
    #
    # `--lib` is the root package's library test target, and it is NOT
    # optional: the feature-gated inline tests live there (43 gated on
    # `server` and 8 on `plugins`, measured on 2c1953347), so a bounded pass
    # that omits it silently drops 51 test functions.
    #
    # The two `--test` entries are the feature-gated families. Since the M004
    # consolidation, the nine gated integration targets are two families:
    # `lsp_test_support` (the four `lsp-test-support` targets) and
    # `server_feature` (the five `server` targets). The
    # `windows-sandbox-test-support` and `linux-git-test-support` families
    # are gated on features this pass does not enable; they are qualified by
    # the platform-specific workflows, not here.
    #
    # The `subset` profile mirrors the `ci` profile's slot bound and keeps the
    # one per-test reservation the `ci` profile carries — that test is an
    # inline library test, so it IS in this bounded set. The `ci` profile's
    # whole-binary exclusions are not carried over: none of those binaries is
    # in the bounded set.
    #
    # A gated target whose feature is not enabled is a configuration error,
    # and cargo fails loudly on an unknown `--test` name. That is the
    # intended behaviour: a silently dropped target is the failure mode this
    # milestone exists to remove.
    feature_pass_targets=(
        --lib
        --test lsp_test_support
        --test server_feature
    )
    echo "==> CARGO_BUILD_JOBS=$CARGO_BUILD_JOBS cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support --profile subset ${feature_pass_targets[*]}"
    (cd "$REPO_ROOT" && cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support --profile subset "${feature_pass_targets[@]}")

    echo "==> Full verification passed."
}

# ── Scoped tier ─────────────────────────────────────────────────────────────
#
# Developer-local only. The hosted CI job's scope is unchanged and stays
# unchanged: `quick`, `full`, and the hosted job all run the whole suite.
#
# The scope is computed from `docs/test-ownership.toml` by
# `scripts/scoped_selection.py`. It is a *selector*: it never decides that a
# test does not need to run. A changed file with no mapping widens the
# affected member to its full surface, and a changed file outside every
# member widens the scope to the whole workspace. It never silently skips.
run_scoped() {
    local base="${1:-origin/main}"

    echo "==> Scoped verification (base: $base)"

    if ! command -v cargo-nextest >/dev/null 2>&1; then
        echo "Error: scoped mode requires cargo-nextest (cargo install cargo-nextest --locked)" >&2
        exit 1
    fi

    # The guards still run exactly as they do in `quick` (plan invariant I9).
    # Scoping composes with the change-trigger routing; it does not replace
    # or reorder it.
    run_guards

    local selection
    if ! selection="$(python3 "$REPO_ROOT/scripts/scoped_selection.py" --base "$base" --format json)"; then
        echo "Error: the scoped selection could not be determined. Refusing to run." >&2
        exit 1
    fi

    echo "==> Scope"
    printf '%s\n' "$selection" | python3 "$REPO_ROOT/scripts/scoped_selection.py" --base "$base" --format text

    # The workspace-wide widening case: run the ordinary workspace test pass.
    if printf '%s' "$selection" | grep -q '"widen_workspace": true'; then
        echo "==> Scope widened to the whole workspace; running the workspace-wide pass"
        echo "==> CARGO_BUILD_JOBS=$CARGO_BUILD_JOBS cargo nextest run --workspace --locked --profile ci"
        (cd "$REPO_ROOT" && cargo nextest run --workspace --locked --profile ci)
        echo "==> Scoped verification passed."
        return 0
    fi

    # Otherwise: one Nextest invocation per affected member, so that a
    # `--test` name is only ever resolved against the package that owns it.
    # The member's library test target is always included — an affected
    # member contributes it even when no integration target matched.
    #
    # A target gated behind `required-features` cannot be built without its
    # features, so the features the selected targets are gated on are
    # enabled for that member's invocation. Naming a gated target without
    # enabling its feature is a cargo error, not a silent skip.
    local member_line name targets features
    while IFS=$'\t' read -r name targets features; do
        [ -n "$name" ] || continue
        local -a args=(--lib)
        local -a feature_args=()
        local target
        for target in $targets; do
            args+=(--test "$target")
        done
        if [ -n "$features" ]; then
            feature_args=(--features "$features")
        fi

        # `${arr[@]+"${arr[@]}"}"` is the portable-safe expansion of a possibly
        # empty array. Under `set -u`, plain `"${arr[@]}"` on an empty array is
        # an "unbound variable" error in bash 3.2 — which is the shell macOS
        # ships, and which this script runs under. `feature_args` is empty
        # whenever the selected targets need no cargo feature, which is the
        # common case, so the plain expansion would break the scoped mode on
        # every unfeatured selection.
        echo "==> CARGO_BUILD_JOBS=$CARGO_BUILD_JOBS cargo nextest run -p $name --locked --profile ci ${feature_args[*]+"${feature_args[*]}"} ${args[*]}"
        (cd "$REPO_ROOT" && cargo nextest run -p "$name" --locked --profile ci "${feature_args[@]+"${feature_args[@]}"}" "${args[@]}")
    done < <(python3 "$REPO_ROOT/scripts/scoped_selection.py" --base "$base" --format members)

    echo "==> Scoped verification passed."
}

# ── Main ────────────────────────────────────────────────────────────────────
case "${1:-}" in
    quick)
        run_quick
        ;;
    full)
        run_full
        ;;
    scoped)
        shift
        run_scoped "${1:-origin/main}"
        ;;
    help|--help|-h)
        usage
        ;;
    *)
        echo "Error: unknown mode '${1:-}'" >&2
        echo "" >&2
        usage >&2
        exit 1
        ;;
esac
