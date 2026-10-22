use codegg::scheduler::config::ResolvedSchedulerConfig;
use codegg_core::jobs::{JobKind, ResourceRequest};

#[test]
fn every_kind_has_nonzero_profile() {
    for kind in [
        JobKind::Test,
        JobKind::Build,
        JobKind::Lint,
        JobKind::Format,
        JobKind::Subagent,
        JobKind::Shell,
        JobKind::ManagedProcess,
        JobKind::Python,
        JobKind::GitRead,
        JobKind::GitMutation,
        JobKind::Maintenance,
    ] {
        let profile = ResourceRequest::for_kind(kind);
        assert!(profile.cpu_weight > 0, "{:?} must not be zero-cost", kind);
        assert!(profile.io_weight > 0, "{:?} must reserve IO", kind);
        assert!(
            profile.process_slots > 0,
            "{:?} must reserve a process slot",
            kind
        );
    }
}

#[test]
fn build_is_heaviest() {
    let build = ResourceRequest::for_kind(JobKind::Build);
    let test = ResourceRequest::for_kind(JobKind::Test);
    let lint = ResourceRequest::for_kind(JobKind::Lint);
    let format = ResourceRequest::for_kind(JobKind::Format);
    assert!(
        build.cpu_weight >= test.cpu_weight,
        "build must be at least as heavy as test"
    );
    assert!(
        build.io_weight >= test.io_weight,
        "build must be at least as heavy as test on IO"
    );
    assert!(
        test.cpu_weight >= lint.cpu_weight,
        "test must be at least as heavy as lint"
    );
    assert!(
        test.cpu_weight >= format.cpu_weight,
        "test must be at least as heavy as format"
    );
}

#[test]
fn mutation_kinds_have_exclusivity_keys() {
    assert!(ResourceRequest::for_kind(JobKind::Build)
        .exclusivity_keys
        .iter()
        .any(|k| k == "exclusive:workspace-mutation"));
    assert!(ResourceRequest::for_kind(JobKind::Format)
        .exclusivity_keys
        .iter()
        .any(|k| k == "exclusive:workspace-mutation"));
    assert!(ResourceRequest::for_kind(JobKind::GitMutation)
        .exclusivity_keys
        .iter()
        .any(|k| k == "exclusive:worktree-mutation"));
}

#[test]
fn read_kinds_have_no_exclusivity_keys() {
    assert!(ResourceRequest::for_kind(JobKind::Test)
        .exclusivity_keys
        .is_empty());
    assert!(ResourceRequest::for_kind(JobKind::Lint)
        .exclusivity_keys
        .is_empty());
    assert!(ResourceRequest::for_kind(JobKind::GitRead)
        .exclusivity_keys
        .is_empty());
}

#[test]
fn network_slots_only_for_subagent() {
    let subagent = ResourceRequest::for_kind(JobKind::Subagent);
    assert_eq!(subagent.network_slots, 1, "subagent can hit network");
    let test = ResourceRequest::for_kind(JobKind::Test);
    assert_eq!(test.network_slots, 0, "test runs in a controlled env");
    let build = ResourceRequest::for_kind(JobKind::Build);
    assert_eq!(build.network_slots, 0, "build is offline");
}

#[test]
fn profiles_are_conservative_against_default_budget() {
    let cfg = ResolvedSchedulerConfig::default();
    for kind in [
        JobKind::Test,
        JobKind::Build,
        JobKind::Lint,
        JobKind::Format,
        JobKind::Subagent,
        JobKind::ManagedProcess,
        JobKind::Shell,
        JobKind::GitMutation,
    ] {
        let profile = ResourceRequest::for_kind(kind);
        assert!(
            profile.cpu_weight * 2 <= cfg.resources.max_cpu_weight,
            "{:?} reserves more than half of the cpu budget",
            kind
        );
        assert!(
            profile.memory_mb_hint * 2 <= cfg.resources.max_memory_mb_hint,
            "{:?} reserves more than half of the memory budget",
            kind
        );
        assert!(
            profile.io_weight * 2 <= cfg.resources.max_io_weight,
            "{:?} reserves more than half of the io budget",
            kind
        );
        assert!(
            (profile.process_slots as u32) * 2 <= cfg.resources.max_process_slots,
            "{:?} reserves more than half of the process slots budget",
            kind
        );
    }
}

#[test]
fn subagent_distinct_from_test() {
    let sub = ResourceRequest::for_kind(JobKind::Subagent);
    let test = ResourceRequest::for_kind(JobKind::Test);
    assert_ne!(sub, test, "subagent profile must differ from test");
    assert_ne!(sub.network_slots, test.network_slots);
}

#[test]
fn scheduler_dispatched_kinds_are_distinguishable() {
    // The admission controller must distinguish every kind it actually
    // routes.  The following kinds intentionally share a profile:
    //
    //   AgentTurn / Subagent / Research  (all network-capable agent work)
    //   Shell / ManagedProcess           (same match arm in for_kind)
    //
    // FINDING: GitRead and Maintenance also share an identical profile
    // (cpu=1, mem=128, proc=1, io=1, network=0, exclusivity=[]).
    // This is harmless today because Maintenance jobs never enter the
    // scheduler queue, but a future scheduler audit tool that keys on
    // profile equality would conflate them.  Tracked as a follow-up.
    let dispatched = [
        JobKind::Test,
        JobKind::Build,
        JobKind::Lint,
        JobKind::Format,
        JobKind::Subagent,
        JobKind::Shell,
        JobKind::ManagedProcess,
        JobKind::Python,
        JobKind::GitRead,
        JobKind::GitMutation,
    ];
    let profiles: Vec<(JobKind, ResourceRequest)> = dispatched
        .iter()
        .map(|&k| (k, ResourceRequest::for_kind(k)))
        .collect();
    // Allow the intentional Shell/ManagedProcess pair to share.
    let intentional_shared = [(JobKind::Shell, JobKind::ManagedProcess)];
    for i in 0..profiles.len() {
        for j in (i + 1)..profiles.len() {
            let (ki, pi) = &profiles[i];
            let (kj, pj) = &profiles[j];
            if pi == pj
                && !intentional_shared
                    .iter()
                    .any(|&(a, b)| (*ki == a && *kj == b) || (*ki == b && *kj == a))
            {
                panic!(
                    "{:?} and {:?} have identical resource profiles — \
                     a scheduler audit cannot distinguish them",
                    ki, kj
                );
            }
        }
    }
}
