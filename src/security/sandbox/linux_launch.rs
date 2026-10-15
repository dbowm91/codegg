//! Linux launch contract and read-only host feature probes.
//!
//! This module describes enhanced Linux launch dimensions, but deliberately
//! registers no namespace backend. The only production plan accepted here is
//! the existing private helper with its current filesystem backend.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Demand {
    Disabled,
    Optional,
    Required,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dimension<T> {
    pub demand: Demand,
    pub requested: T,
    pub parent_ceiling: T,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilesystemView {
    pub readable_roots: Vec<PathBuf>,
    pub writable_roots: Vec<PathBuf>,
    pub protected_roots: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkIntent {
    LegacyUnrestricted,
    OfflineRequired,
    Mediated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessScope {
    Legacy,
    Namespaced,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SyscallConstraints {
    pub seccomp_profile: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResourceBudget {
    pub cpu_quota_micros: Option<u64>,
    pub memory_bytes: Option<u64>,
    pub process_count: Option<u32>,
}

/// Internal request passed through the canonical managed-process owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxLaunchRequest {
    pub filesystem: Dimension<FilesystemView>,
    /// The protected mount view is a separate, explicit opt-in. Existing
    /// callers continue to select the legacy Landlock helper.
    pub protected_mount_view: bool,
    pub network: Dimension<NetworkIntent>,
    pub process: Dimension<ProcessScope>,
    pub syscalls: Dimension<SyscallConstraints>,
    pub resources: Dimension<ResourceBudget>,
}

impl LinuxLaunchRequest {
    /// Describe today's behavior without implying that new Linux dimensions
    /// are enforced. Existing helper policy remains the filesystem authority;
    /// network stays unrestricted and no namespace/seccomp/cgroup policy is
    /// requested.
    pub fn legacy_filesystem(spec: &super::SandboxLaunchSpec) -> Self {
        let filesystem = FilesystemView {
            readable_roots: spec.read_paths.clone(),
            writable_roots: spec.write_paths.clone(),
            protected_roots: spec.deny_paths.clone(),
        };
        Self {
            filesystem: Dimension {
                demand: Demand::Required,
                requested: filesystem.clone(),
                parent_ceiling: filesystem,
            },
            protected_mount_view: spec.linux_protected_mount_view,
            network: Dimension {
                demand: Demand::Disabled,
                requested: NetworkIntent::LegacyUnrestricted,
                parent_ceiling: NetworkIntent::LegacyUnrestricted,
            },
            process: Dimension {
                demand: if spec.linux_process_isolation {
                    Demand::Required
                } else {
                    Demand::Disabled
                },
                requested: if spec.linux_process_isolation {
                    ProcessScope::Namespaced
                } else {
                    ProcessScope::Legacy
                },
                parent_ceiling: if spec.linux_process_isolation {
                    ProcessScope::Namespaced
                } else {
                    ProcessScope::Legacy
                },
            },
            syscalls: Dimension {
                demand: Demand::Disabled,
                requested: SyscallConstraints::default(),
                parent_ceiling: SyscallConstraints::default(),
            },
            resources: Dimension {
                demand: Demand::Disabled,
                requested: ResourceBudget::default(),
                parent_ceiling: ResourceBudget::default(),
            },
        }
    }

    pub fn validate_parent_ceiling(&self) -> Result<(), String> {
        let requested = &self.filesystem.requested;
        let ceiling = &self.filesystem.parent_ceiling;
        if !is_subset(&requested.readable_roots, &ceiling.readable_roots)
            || !is_subset(&requested.writable_roots, &ceiling.writable_roots)
            || !is_superset(&requested.protected_roots, &ceiling.protected_roots)
        {
            return Err("Linux filesystem request exceeds parent ceiling".into());
        }
        if !network_within_ceiling(self.network.requested, self.network.parent_ceiling)
            || !process_within_ceiling(self.process.requested, self.process.parent_ceiling)
        {
            return Err("Linux launch request exceeds parent ceiling".into());
        }
        if self.syscalls.requested != self.syscalls.parent_ceiling
            || self.resources.requested != self.resources.parent_ceiling
        {
            return Err("Linux launch request exceeds parent ceiling".into());
        }
        Ok(())
    }

    fn requires_enhanced_backend(&self) -> bool {
        self.network.demand == Demand::Required
            || self.network.requested != NetworkIntent::LegacyUnrestricted
            || self.process.demand == Demand::Required
            || self.process.requested != ProcessScope::Legacy
            || self.syscalls.demand == Demand::Required
            || self.syscalls.requested.seccomp_profile.is_some()
            || self.resources.demand == Demand::Required
            || self.resources.requested != ResourceBudget::default()
    }
}

fn is_subset(requested: &[PathBuf], ceiling: &[PathBuf]) -> bool {
    let ceiling = ceiling.iter().collect::<BTreeSet<_>>();
    requested.iter().all(|root| ceiling.contains(root))
}

fn is_superset(requested: &[PathBuf], required: &[PathBuf]) -> bool {
    let requested = requested.iter().collect::<BTreeSet<_>>();
    required.iter().all(|root| requested.contains(root))
}

fn network_within_ceiling(requested: NetworkIntent, ceiling: NetworkIntent) -> bool {
    requested == ceiling
        || matches!(
            (requested, ceiling),
            (NetworkIntent::OfflineRequired, NetworkIntent::Mediated)
        )
}

fn process_within_ceiling(requested: ProcessScope, ceiling: ProcessScope) -> bool {
    requested == ceiling
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinuxLaunchPlan {
    LegacyPrivateHelper,
    ProtectedMountView,
    ProtectedProcessIsolation,
    EnhancedUnavailable { dimensions: Vec<&'static str> },
}

pub trait LinuxLaunchAdapter {
    fn plan(&self, request: &LinuxLaunchRequest) -> Result<LinuxLaunchPlan, String>;
}

/// Production adapter: only dispatch to the existing helper. New launch
/// dimensions remain unavailable until their accepted ADR and native proof.
pub struct ProductionLinuxLaunchAdapter;

impl LinuxLaunchAdapter for ProductionLinuxLaunchAdapter {
    fn plan(&self, request: &LinuxLaunchRequest) -> Result<LinuxLaunchPlan, String> {
        request.validate_parent_ceiling()?;
        if request.protected_mount_view {
            if request.process.requested == ProcessScope::Namespaced {
                if request.network.demand != Demand::Disabled
                    || request.network.requested != NetworkIntent::LegacyUnrestricted
                    || request.syscalls.demand != Demand::Disabled
                    || request.syscalls.requested != SyscallConstraints::default()
                    || request.resources.demand != Demand::Disabled
                    || request.resources.requested != ResourceBudget::default()
                {
                    return Ok(LinuxLaunchPlan::EnhancedUnavailable {
                        dimensions: requested_enhanced_dimensions(request),
                    });
                }
                if probe_trusted_bubblewrap().state != ProbeState::Available {
                    return Ok(LinuxLaunchPlan::EnhancedUnavailable {
                        dimensions: vec!["trusted-bubblewrap"],
                    });
                }
                if super::landlock::probe_process_scopes().is_err() {
                    return Ok(LinuxLaunchPlan::EnhancedUnavailable {
                        dimensions: vec!["process-ipc-landlock-abi-6"],
                    });
                }
                if probe_seccomp_filter().state != ProbeState::Available {
                    return Ok(LinuxLaunchPlan::EnhancedUnavailable {
                        dimensions: vec!["seccomp-filter"],
                    });
                }
                return Ok(LinuxLaunchPlan::ProtectedProcessIsolation);
            }
            if request.requires_enhanced_backend() {
                return Ok(LinuxLaunchPlan::EnhancedUnavailable {
                    dimensions: requested_enhanced_dimensions(request),
                });
            }
            if probe_trusted_bubblewrap().state == ProbeState::Available {
                return Ok(LinuxLaunchPlan::ProtectedMountView);
            }
            return Ok(LinuxLaunchPlan::EnhancedUnavailable {
                dimensions: vec!["protected-filesystem-mount-view"],
            });
        }
        if request.requires_enhanced_backend() {
            return Ok(LinuxLaunchPlan::EnhancedUnavailable {
                dimensions: requested_enhanced_dimensions(request),
            });
        }
        Ok(LinuxLaunchPlan::LegacyPrivateHelper)
    }
}

fn requested_enhanced_dimensions(request: &LinuxLaunchRequest) -> Vec<&'static str> {
    let mut dimensions = Vec::new();
    if request.network.requested != NetworkIntent::LegacyUnrestricted {
        dimensions.push("network");
    }
    if request.process.requested != ProcessScope::Legacy {
        dimensions.push("process");
    }
    if request.syscalls.requested.seccomp_profile.is_some() {
        dimensions.push("syscalls");
    }
    if request.resources.requested != ResourceBudget::default() {
        dimensions.push("resources");
    }
    dimensions
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeState {
    Available,
    Unavailable,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeFact {
    pub state: ProbeState,
    pub evidence: String,
}

impl ProbeFact {
    fn available(evidence: impl Into<String>) -> Self {
        Self {
            state: ProbeState::Available,
            evidence: evidence.into(),
        }
    }

    fn unavailable(evidence: impl Into<String>) -> Self {
        Self {
            state: ProbeState::Unavailable,
            evidence: evidence.into(),
        }
    }

    fn unknown(evidence: impl Into<String>) -> Self {
        Self {
            state: ProbeState::Unknown,
            evidence: evidence.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxHostFeatures {
    pub user_namespace_creation: ProbeFact,
    pub mount_namespace: ProbeFact,
    pub pid_namespace: ProbeFact,
    pub network_namespace: ProbeFact,
    pub ipc_namespace: ProbeFact,
    pub seccomp_filter: ProbeFact,
    pub proc_mount_inputs: ProbeFact,
    pub trusted_bubblewrap: ProbeFact,
    pub landlock: ProbeFact,
    pub cgroup_v2_delegation: ProbeFact,
    pub apparmor: ProbeFact,
    pub selinux: ProbeFact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObtainedState {
    NotRequested,
    Applied,
    Unsupported,
    Failed,
}

/// Internal obtained-facts projection from the helper's private status. It
/// deliberately records only the existing filesystem backend; enhanced
/// dimensions cannot be inferred from a generic helper success frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxObtainedEnforcement {
    pub filesystem: ObtainedState,
    pub network: ObtainedState,
    pub process: ObtainedState,
    pub syscalls: ObtainedState,
    pub resources: ObtainedState,
    pub backend: Option<String>,
    pub abi: Option<u32>,
    pub guarantees: Vec<String>,
    pub limitations: Vec<String>,
}

pub fn observe_legacy_outcome(
    request: &LinuxLaunchRequest,
    outcome: &super::SandboxLaunchOutcome,
) -> Result<LinuxObtainedEnforcement, String> {
    request.validate_parent_ceiling()?;
    let (filesystem, backend, abi, guarantees, limitations) = match outcome {
        super::SandboxLaunchOutcome::Enforced {
            backend,
            abi,
            guarantees,
            limits,
        } => (
            ObtainedState::Applied,
            Some(backend.to_string()),
            *abi,
            guarantees.clone(),
            limits.clone(),
        ),
        super::SandboxLaunchOutcome::Unavailable { reason } => (
            ObtainedState::Unsupported,
            None,
            None,
            Vec::new(),
            vec![reason.clone()],
        ),
        super::SandboxLaunchOutcome::SetupError { reason }
        | super::SandboxLaunchOutcome::ExecError { reason } => (
            ObtainedState::Failed,
            None,
            None,
            Vec::new(),
            vec![reason.clone()],
        ),
        super::SandboxLaunchOutcome::BubblewrapReady => (
            ObtainedState::Failed,
            None,
            None,
            Vec::new(),
            vec!["unexpected Bubblewrap gate receipt on the legacy launch route".into()],
        ),
    };
    Ok(LinuxObtainedEnforcement {
        filesystem,
        network: ObtainedState::NotRequested,
        process: ObtainedState::NotRequested,
        syscalls: ObtainedState::NotRequested,
        resources: ObtainedState::NotRequested,
        backend,
        abi,
        guarantees,
        limitations,
    })
}

/// Read-only host discovery. These facts are diagnostic inputs only; none
/// proves that a future child launch applied or obtained containment.
pub fn probe_linux_host_features() -> LinuxHostFeatures {
    LinuxHostFeatures {
        user_namespace_creation: probe_user_namespace_creation(),
        mount_namespace: probe_namespace("mnt"),
        pid_namespace: probe_namespace("pid"),
        network_namespace: probe_namespace("net"),
        ipc_namespace: probe_namespace("ipc"),
        seccomp_filter: probe_seccomp_filter(),
        proc_mount_inputs: probe_proc_mount_inputs(),
        trusted_bubblewrap: probe_trusted_bubblewrap(),
        landlock: match super::landlock::probe() {
            Ok(()) => ProbeFact::available("Landlock ruleset creation probe succeeded"),
            Err(reason) => ProbeFact::unavailable(reason),
        },
        cgroup_v2_delegation: probe_cgroup_v2_delegation(),
        apparmor: probe_lsm(Path::new("/sys/module/apparmor")),
        selinux: probe_lsm(Path::new("/sys/fs/selinux/enforce")),
    }
}

fn probe_namespace(name: &str) -> ProbeFact {
    let path = Path::new("/proc/self/ns").join(name);
    match std::fs::metadata(&path) {
        Ok(_) => ProbeFact::unknown(format!(
            "{path:?} exists; host namespace creation is unproven"
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            ProbeFact::unavailable(format!("{path:?} is absent"))
        }
        Err(error) => ProbeFact::unknown(format!("could not inspect {path:?}: {error}")),
    }
}

fn probe_user_namespace_creation() -> ProbeFact {
    match std::fs::read_to_string("/proc/sys/user/max_user_namespaces") {
        Ok(value) => match value.trim().parse::<u64>() {
            Ok(0) => ProbeFact::unavailable("user.max_user_namespaces is zero"),
            Ok(maximum) => ProbeFact::unknown(format!(
                "user.max_user_namespaces={maximum}; host policy may still deny creation"
            )),
            Err(_) => ProbeFact::unknown("user.max_user_namespaces is not an integer"),
        },
        Err(error) => ProbeFact::unknown(format!("cannot read user namespace limit: {error}")),
    }
}

fn probe_seccomp_filter() -> ProbeFact {
    match std::fs::read_to_string("/proc/sys/kernel/seccomp/actions_avail") {
        Ok(actions) if actions.split_whitespace().any(|action| action == "filter") => {
            ProbeFact::available(
                "kernel advertises seccomp filter action; child application unproven",
            )
        }
        Ok(_) => ProbeFact::unavailable("kernel does not advertise seccomp filter action"),
        Err(error) => ProbeFact::unknown(format!("cannot inspect seccomp actions: {error}")),
    }
}

fn probe_proc_mount_inputs() -> ProbeFact {
    match std::fs::read_to_string("/proc/filesystems") {
        Ok(filesystems)
            if filesystems
                .lines()
                .any(|line| line.split_whitespace().last() == Some("proc")) =>
        {
            ProbeFact::available(
                "kernel lists proc filesystem; namespace proc construction unproven",
            )
        }
        Ok(_) => ProbeFact::unavailable("kernel does not list proc filesystem"),
        Err(error) => ProbeFact::unknown(format!("cannot inspect /proc/filesystems: {error}")),
    }
}

fn probe_trusted_bubblewrap() -> ProbeFact {
    match trusted_bubblewrap_path() {
        Ok(path) => ProbeFact::available(format!(
            "trusted system executable candidate at {}; Bubblewrap version and required option support are unqualified",
            path.display()
        )),
        Err(reason) => ProbeFact::unavailable(reason),
    }
}

/// Resolve Bubblewrap independently of PATH and caller-controlled cwd. The
/// candidate is revalidated immediately before every protected launch.
pub fn trusted_bubblewrap_path() -> Result<PathBuf, String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        for candidate in ["/usr/bin/bwrap", "/bin/bwrap", "/usr/local/bin/bwrap"] {
            let path = Path::new(candidate);
            let Ok(canonical) = std::fs::canonicalize(path) else {
                continue;
            };
            let Ok(metadata) = std::fs::metadata(&canonical) else {
                continue;
            };
            let mode = metadata.mode();
            let owner = metadata.uid();
            // A system-owned executable must not be replaceable by an
            // unprivileged group/other writer. Check every containing
            // directory as well as the file; no PATH lookup is performed.
            let secure_ancestors = canonical.ancestors().skip(1).all(|ancestor| {
                std::fs::metadata(ancestor)
                    .is_ok_and(|metadata| metadata.uid() == 0 && metadata.mode() & 0o022 == 0)
            });
            if metadata.is_file()
                && owner == 0
                && mode & 0o022 == 0
                // ADR-0031 explicitly rejects setuid helpers; accepting one
                // here could turn a namespace setup path into a privilege
                // transition even though the executable is root-owned.
                && mode & 0o6000 == 0
                && mode & 0o111 != 0
                && secure_ancestors
            {
                return Ok(canonical);
            }
            return Err(format!(
                "{} or one of its parent directories is not root-owned, non-writable, free of setuid/setgid bits, and executable",
                canonical.display()
            ));
        }
        Err("no trusted bubblewrap executable in fixed system locations".into())
    }
    #[cfg(not(unix))]
    {
        Err("bubblewrap is Linux-only".into())
    }
}

fn probe_cgroup_v2_delegation() -> ProbeFact {
    let root = Path::new("/sys/fs/cgroup");
    let controllers = match std::fs::read_to_string(root.join("cgroup.controllers")) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return ProbeFact::unavailable("cgroup v2 controllers file is absent");
        }
        Err(error) => return ProbeFact::unknown(format!("cannot inspect cgroup v2: {error}")),
    };
    let current = std::fs::read_to_string("/proc/self/cgroup");
    let path = current
        .ok()
        .and_then(|contents| {
            contents
                .lines()
                .find_map(|line| line.strip_prefix("0::"))
                .map(str::to_owned)
        })
        .map(|relative| root.join(relative.trim_start_matches('/')));
    let Some(path) = path else {
        return ProbeFact::unknown("cgroup v2 is mounted but current delegated cgroup is unknown");
    };
    let writable = std::fs::metadata(path.join("cgroup.procs"))
        .map(|metadata| !metadata.permissions().readonly())
        .unwrap_or(false);
    if writable
        && !controllers
            .split_whitespace()
            .collect::<Vec<_>>()
            .is_empty()
    {
        ProbeFact::unknown(
            "current cgroup appears writable with controllers; delegated ownership is unproven",
        )
    } else {
        ProbeFact::unavailable("cgroup v2 exists without an observable writable delegated cgroup")
    }
}

fn probe_lsm(path: &Path) -> ProbeFact {
    match std::fs::metadata(path) {
        Ok(_) => ProbeFact::available(format!(
            "{} is present; compatibility unproven",
            path.display()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            ProbeFact::unavailable(format!("{} is absent", path.display()))
        }
        Err(error) => ProbeFact::unknown(format!("cannot inspect {}: {error}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeAdapter {
        plan: Result<LinuxLaunchPlan, String>,
    }

    impl LinuxLaunchAdapter for FakeAdapter {
        fn plan(&self, request: &LinuxLaunchRequest) -> Result<LinuxLaunchPlan, String> {
            request.validate_parent_ceiling()?;
            self.plan.clone()
        }
    }

    fn legacy_request() -> LinuxLaunchRequest {
        LinuxLaunchRequest::legacy_filesystem(&super::super::SandboxLaunchSpec {
            linux_protected_mount_view: false,
            linux_process_isolation: false,
            protected_readonly_paths: Vec::new(),
            path_identities: Vec::new(),
            target: PathBuf::from("/bin/true"),
            args: Vec::new(),
            read_paths: vec![PathBuf::from("/workspace")],
            write_paths: vec![PathBuf::from("/workspace")],
            deny_paths: vec![PathBuf::from("/workspace/.git")],
        })
    }

    #[test]
    fn production_adapter_selects_only_existing_legacy_helper() {
        let plan = ProductionLinuxLaunchAdapter
            .plan(&legacy_request())
            .unwrap();
        assert_eq!(plan, LinuxLaunchPlan::LegacyPrivateHelper);
    }

    #[test]
    fn production_adapter_reports_enhanced_dimensions_unavailable() {
        let mut request = legacy_request();
        request.process.demand = Demand::Required;
        request.process.requested = ProcessScope::Namespaced;
        request.process.parent_ceiling = ProcessScope::Namespaced;
        assert_eq!(
            ProductionLinuxLaunchAdapter.plan(&request).unwrap(),
            LinuxLaunchPlan::EnhancedUnavailable {
                dimensions: vec!["process"]
            }
        );
    }

    #[test]
    fn parent_ceiling_refuses_widened_filesystem_and_process_requests() {
        let mut request = legacy_request();
        request
            .filesystem
            .requested
            .writable_roots
            .push(PathBuf::from("/etc"));
        assert!(request.validate_parent_ceiling().is_err());

        let mut request = legacy_request();
        request.filesystem.requested.protected_roots.clear();
        assert!(request.validate_parent_ceiling().is_err());

        let mut request = legacy_request();
        request.process.requested = ProcessScope::Namespaced;
        assert!(request.validate_parent_ceiling().is_err());
    }

    #[test]
    fn test_adapter_covers_success_unsupported_and_failed_setup() {
        let request = legacy_request();
        let success = FakeAdapter {
            plan: Ok(LinuxLaunchPlan::LegacyPrivateHelper),
        };
        assert_eq!(
            success.plan(&request).unwrap(),
            LinuxLaunchPlan::LegacyPrivateHelper
        );

        let unsupported = FakeAdapter {
            plan: Ok(LinuxLaunchPlan::EnhancedUnavailable {
                dimensions: vec!["network"],
            }),
        };
        assert!(matches!(
            unsupported.plan(&request).unwrap(),
            LinuxLaunchPlan::EnhancedUnavailable { .. }
        ));

        let failed = FakeAdapter {
            plan: Err("injected setup failure".into()),
        };
        assert_eq!(failed.plan(&request).unwrap_err(), "injected setup failure");
    }

    #[test]
    fn host_probe_reports_facts_without_claiming_child_enforcement() {
        let features = probe_linux_host_features();
        assert!(!features.user_namespace_creation.evidence.is_empty());
        assert!(!features.seccomp_filter.evidence.is_empty());
        assert!(
            features.seccomp_filter.evidence.contains("unproven")
                || features.seccomp_filter.state != ProbeState::Available
        );
        assert!(!features.trusted_bubblewrap.evidence.is_empty());
    }

    #[test]
    fn obtained_projection_names_only_the_filesystem_backend() {
        let request = legacy_request();
        let outcome = super::super::SandboxLaunchOutcome::Enforced {
            backend: super::super::BackendId::LANDLOCK,
            abi: Some(9),
            guarantees: vec!["filesystem path policy".into()],
            limits: vec!["no network isolation".into()],
        };
        let obtained = observe_legacy_outcome(&request, &outcome).unwrap();
        assert_eq!(obtained.filesystem, ObtainedState::Applied);
        assert_eq!(obtained.network, ObtainedState::NotRequested);
        assert_eq!(obtained.process, ObtainedState::NotRequested);
        assert_eq!(obtained.syscalls, ObtainedState::NotRequested);
        assert_eq!(obtained.resources, ObtainedState::NotRequested);
        assert_eq!(obtained.backend.as_deref(), Some("landlock"));
        assert_eq!(obtained.abi, Some(9));
        assert_eq!(obtained.guarantees, vec!["filesystem path policy"]);
        assert_eq!(obtained.limitations, vec!["no network isolation"]);
    }

    #[test]
    fn obtained_projection_distinguishes_unsupported_and_failed_setup() {
        let request = legacy_request();
        let unavailable = observe_legacy_outcome(
            &request,
            &super::super::SandboxLaunchOutcome::Unavailable {
                reason: "no backend".into(),
            },
        )
        .unwrap();
        assert_eq!(unavailable.filesystem, ObtainedState::Unsupported);
        assert_eq!(unavailable.network, ObtainedState::NotRequested);

        let failed = observe_legacy_outcome(
            &request,
            &super::super::SandboxLaunchOutcome::SetupError {
                reason: "setup failed".into(),
            },
        )
        .unwrap();
        assert_eq!(failed.filesystem, ObtainedState::Failed);
        assert_eq!(failed.backend, None);
    }
}
