//! M007 — per-workspace build admission.
//!
//! The contract under test:
//!
//! * A build-class job consumes a **counted, workspace-scoped** quota.
//! * The limit is configuration-owned; the request can never widen it.
//! * Unrelated workspaces are independent — one workspace's build load
//!   does not consume another's slots.
//! * A waiting job holds **no** permit, so it cannot deadlock a parent.
//! * A cancelled job releases only its own quota, and only after its
//!   process lifetime ends.

use codegg_core::jobs::{JobKind, KeyedQuotaRequest, ResourceClass, ResourceRequest};
use codegg_core::workspace::WorkspaceId;

use codegg::scheduler::admission::{
    AdmissionController, AdmissionDecision, BlockReason, UnschedulableReason,
};
use codegg::scheduler::config::{QuotaPolicy, ResolvedSchedulerConfig};
use codegg::scheduler::permit::{permit_from_request, permit_from_request_with_quotas};

fn config_with_build_limit(limit: u32) -> ResolvedSchedulerConfig {
    ResolvedSchedulerConfig {
        quota: QuotaPolicy {
            max_concurrent_builds_per_workspace: limit,
        },
        ..ResolvedSchedulerConfig::default()
    }
}

fn controller(limit: u32) -> std::sync::Arc<AdmissionController> {
    std::sync::Arc::new(AdmissionController::new(config_with_build_limit(limit)))
}

fn build_quota(workspace: &str) -> KeyedQuotaRequest {
    KeyedQuotaRequest::for_workspace(ResourceClass::Build, &WorkspaceId::new_unchecked(workspace))
}

#[test]
fn a_build_quota_is_scoped_to_its_workspace() {
    let request = build_quota("ws-a");
    assert_eq!(request.namespace(), Some("build"));
    assert_eq!(request.scope(), "ws-a");
    assert!(request.is_valid());
}

#[test]
fn two_builds_in_one_workspace_obey_the_configured_limit() {
    let controller = controller(1);
    let dimensions =
        permit_from_request_with_quotas(1, 512, 1, 1, 0, vec![], vec![build_quota("ws-a")]);
    let _held = match controller.try_admit_arc(&dimensions) {
        AdmissionDecision::Admitted(guard) => guard,
        other => panic!("expected Admitted, got {other:?}"),
    };
    match controller.try_admit_arc(&dimensions) {
        AdmissionDecision::TemporarilyBlocked(BlockReason::QuotaContended {
            key,
            requested,
            available,
        }) => {
            assert_eq!(key, "build:ws-a");
            assert_eq!(requested, 1);
            assert_eq!(available, 0);
        }
        other => panic!("expected QuotaContended, got {other:?}"),
    }
}

#[test]
fn unrelated_workspaces_build_concurrently() {
    let controller = controller(1);
    let first = permit_from_request_with_quotas(1, 512, 1, 1, 0, vec![], vec![build_quota("ws-a")]);
    let second =
        permit_from_request_with_quotas(1, 512, 1, 1, 0, vec![], vec![build_quota("ws-b")]);
    let _held_a = match controller.try_admit_arc(&first) {
        AdmissionDecision::Admitted(guard) => guard,
        other => panic!("expected Admitted, got {other:?}"),
    };
    let _held_b = match controller.try_admit_arc(&second) {
        AdmissionDecision::Admitted(guard) => guard,
        other => panic!("unrelated workspaces must stay concurrent, got {other:?}"),
    };
    assert_eq!(
        controller.snapshot().held_quotas.get("build:ws-a"),
        Some(&1)
    );
    assert_eq!(
        controller.snapshot().held_quotas.get("build:ws-b"),
        Some(&1)
    );
}

#[test]
fn a_raised_limit_admits_two_builds_in_one_workspace() {
    let controller = controller(2);
    let dimensions =
        permit_from_request_with_quotas(1, 512, 1, 1, 0, vec![], vec![build_quota("ws-a")]);
    let _first = match controller.try_admit_arc(&dimensions) {
        AdmissionDecision::Admitted(guard) => guard,
        other => panic!("expected Admitted, got {other:?}"),
    };
    let _second = match controller.try_admit_arc(&dimensions) {
        AdmissionDecision::Admitted(guard) => guard,
        other => panic!("a limit of two must admit two holders, got {other:?}"),
    };
}

#[test]
fn a_request_above_the_limit_is_impossible_not_merely_blocked() {
    let controller = controller(1);
    let dimensions = permit_from_request_with_quotas(
        1,
        512,
        1,
        1,
        0,
        vec![],
        vec![KeyedQuotaRequest::new("build:ws-a", 4)],
    );
    match controller.try_admit_arc(&dimensions) {
        AdmissionDecision::Impossible(UnschedulableReason::QuotaExceedsLimit {
            key,
            requested,
            limit,
        }) => {
            assert_eq!(key, "build:ws-a");
            assert_eq!(requested, 4);
            assert_eq!(limit, 1);
        }
        other => panic!("expected QuotaExceedsLimit, got {other:?}"),
    }
}

#[test]
fn an_unknown_quota_namespace_is_refused_rather_than_ignored() {
    let controller = controller(1);
    let dimensions = permit_from_request_with_quotas(
        1,
        512,
        1,
        1,
        0,
        vec![],
        vec![KeyedQuotaRequest::new("cache:ws-a", 1)],
    );
    match controller.try_admit_arc(&dimensions) {
        AdmissionDecision::Impossible(UnschedulableReason::UnknownQuotaNamespace { namespace }) => {
            assert_eq!(namespace, "cache");
        }
        other => panic!("expected UnknownQuotaNamespace, got {other:?}"),
    }
}

#[test]
fn a_malformed_quota_request_is_refused() {
    let controller = controller(1);
    let dimensions = permit_from_request_with_quotas(
        1,
        512,
        1,
        1,
        0,
        vec![],
        vec![KeyedQuotaRequest::new("build", 1)],
    );
    match controller.try_admit_arc(&dimensions) {
        AdmissionDecision::Impossible(UnschedulableReason::UnknownQuotaNamespace { .. }) => {}
        other => panic!("a malformed key must be refused, got {other:?}"),
    }
}

#[test]
fn a_waiting_job_holds_no_permit() {
    let controller = controller(1);
    let dimensions =
        permit_from_request_with_quotas(1, 512, 1, 1, 0, vec![], vec![build_quota("ws-a")]);
    let _held = match controller.try_admit_arc(&dimensions) {
        AdmissionDecision::Admitted(guard) => guard,
        other => panic!("expected Admitted, got {other:?}"),
    };
    // The second job is blocked on the quota. It must not have consumed
    // any process slot, CPU weight or memory: a waiting job that holds a
    // permit is exactly the deadlock the milestone forbids.
    let before = controller.snapshot();
    assert!(matches!(
        controller.try_admit_arc(&dimensions),
        AdmissionDecision::TemporarilyBlocked(BlockReason::QuotaContended { .. })
    ));
    let after = controller.snapshot();
    assert_eq!(before.used_process, after.used_process);
    assert_eq!(before.used_cpu, after.used_cpu);
    assert_eq!(before.used_memory, after.used_memory);
    assert_eq!(before.held_quotas, after.held_quotas);
}

#[test]
fn cancelling_a_waiting_job_releases_only_its_own_queue_request() {
    let controller = controller(1);
    let dimensions =
        permit_from_request_with_quotas(1, 512, 1, 1, 0, vec![], vec![build_quota("ws-a")]);
    let held = match controller.try_admit_arc(&dimensions) {
        AdmissionDecision::Admitted(guard) => guard,
        other => panic!("expected Admitted, got {other:?}"),
    };
    // A second, waiting job never reserved anything: dropping the
    // blocked attempt must not touch the running job's quota.
    assert!(matches!(
        controller.try_admit_arc(&dimensions),
        AdmissionDecision::TemporarilyBlocked(BlockReason::QuotaContended { .. })
    ));
    assert_eq!(
        controller.snapshot().held_quotas.get("build:ws-a"),
        Some(&1)
    );
    drop(held);
    assert!(controller.snapshot().held_quotas.is_empty());
}

#[test]
fn a_cancelled_running_job_releases_its_quota_when_the_permit_drops() {
    let controller = controller(1);
    let dimensions =
        permit_from_request_with_quotas(1, 512, 1, 1, 0, vec![], vec![build_quota("ws-a")]);
    let held = match controller.try_admit_arc(&dimensions) {
        AdmissionDecision::Admitted(guard) => guard,
        other => panic!("expected Admitted, got {other:?}"),
    };
    assert_eq!(
        controller.snapshot().held_quotas.get("build:ws-a"),
        Some(&1)
    );
    // The permit guard is dropped when the executor's process lifetime
    // ends, which is the only point at which the quota is returned.
    drop(held);
    assert!(controller.snapshot().held_quotas.is_empty());
    // And the scope is available again.
    assert!(matches!(
        controller.try_admit_arc(&dimensions),
        AdmissionDecision::Admitted(_)
    ));
}

#[test]
fn a_job_without_a_quota_is_unaffected_by_the_build_limit() {
    let controller = controller(1);
    let dimensions = permit_from_request(1, 512, 1, 1, 0, vec![]);
    let _first = match controller.try_admit_arc(&dimensions) {
        AdmissionDecision::Admitted(guard) => guard,
        other => panic!("expected Admitted, got {other:?}"),
    };
    let _second = match controller.try_admit_arc(&dimensions) {
        AdmissionDecision::Admitted(guard) => guard,
        other => panic!("an unsupported kind must retain its existing admission, got {other:?}"),
    };
    assert!(controller.snapshot().held_quotas.is_empty());
}

#[test]
fn the_build_class_replaces_the_legacy_global_exclusivity_key() {
    let request = ResourceRequest::for_kind(JobKind::Build);
    assert!(
        !request
            .exclusivity_keys
            .iter()
            .any(|key| key == "exclusive:workspace-mutation"),
        "the legacy global key must not survive: it serialized unrelated workspaces"
    );
    assert_eq!(
        request.keyed_quotas.len(),
        1,
        "a build must carry exactly the build class"
    );
    assert_eq!(
        request.keyed_quotas[0].namespace(),
        Some("build"),
        "the class is named but not yet scoped"
    );
    assert_eq!(request.keyed_quotas[0].scope(), "");
}

#[test]
fn only_build_and_format_are_build_class() {
    assert_eq!(
        ResourceClass::for_job_kind(JobKind::Build),
        Some(ResourceClass::Build)
    );
    assert_eq!(
        ResourceClass::for_job_kind(JobKind::Format),
        Some(ResourceClass::Build)
    );
    for kind in [
        JobKind::AgentTurn,
        JobKind::Subagent,
        JobKind::Test,
        JobKind::Lint,
        JobKind::Shell,
        JobKind::GitMutation,
    ] {
        assert_eq!(
            ResourceClass::for_job_kind(kind),
            None,
            "{kind:?} must not be classified as a build"
        );
        let request = ResourceRequest::for_kind(kind);
        assert!(
            request.keyed_quotas.is_empty(),
            "{kind:?} must not carry a quota"
        );
    }
}

#[test]
fn the_build_class_scope_is_bound_to_the_canonical_workspace() {
    // The submission boundary binds the scope. The bound key is what
    // makes two workspaces independent.
    let workspace = WorkspaceId::new_unchecked("ws-77");
    let request = KeyedQuotaRequest::for_workspace(ResourceClass::Build, &workspace);
    assert_eq!(request.key, "build:ws-77");
    assert_ne!(request.key, build_quota("ws-other").key);
}

#[test]
fn an_unbound_class_request_still_carries_a_bound() {
    // A request that never reached the submission boundary keeps the
    // namespace but has no scope. It is counted in one shared bucket
    // rather than escaping the bound.
    let unbound = KeyedQuotaRequest::unbound(ResourceClass::Build);
    assert_eq!(unbound.namespace(), Some("build"));
    assert!(unbound.is_valid());
    assert_ne!(unbound.key, build_quota("ws-a").key);
}

#[test]
fn the_default_build_limit_is_one() {
    assert_eq!(
        ResolvedSchedulerConfig::default()
            .quota
            .max_concurrent_builds_per_workspace,
        1
    );
}

#[test]
fn a_zero_build_limit_is_refused_by_validation() {
    let mut config = ResolvedSchedulerConfig::default();
    config.quota.max_concurrent_builds_per_workspace = 0;
    assert!(config.validate().is_err());
}

#[test]
fn an_admitted_build_consumes_its_scope_and_no_other() {
    let controller = controller(1);
    let dimensions =
        permit_from_request_with_quotas(1, 512, 1, 1, 0, vec![], vec![build_quota("ws-a")]);
    let _held = match controller.try_admit_arc(&dimensions) {
        AdmissionDecision::Admitted(guard) => guard,
        other => panic!("expected Admitted, got {other:?}"),
    };
    let snapshot = controller.snapshot();
    assert_eq!(snapshot.held_quotas.len(), 1);
    assert_eq!(snapshot.held_quotas.get("build:ws-a"), Some(&1));
    assert!(!snapshot
        .held_keys
        .keys()
        .any(|key| key.contains("workspace-mutation")));
}
