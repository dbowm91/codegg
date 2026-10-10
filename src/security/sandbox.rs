#![allow(clippy::type_complexity)]

//! Filesystem containment: policy, backends, and the private helper protocol.
//!
//! The module is deliberately split in three:
//!
//! * [`policy`] — a **backend-neutral** statement of what may be read,
//!   written, and denied. Owns the tool-compatible allowance sets.
//! * [`backend`] — the pluggable registry of OS containment mechanisms.
//!   Registering a new backend is one const entry.
//! * [`landlock`] / [`seatbelt`] — the two shipped backends.
//!
//! Everything else here — profiles, config, capability, enforcement, the
//! execution path, and the helper status protocol — is backend-neutral and
//! reads backend identity out of the registry rather than naming one. That
//! is what allows macOS and Linux to be served by the same containment
//! policy, and a third platform to join without another rewrite.

use crate::error::ToolError;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub mod backend;
pub mod landlock;
pub mod policy;
pub mod seatbelt;

pub use backend::{BackendEnforcement, BackendId, BACKENDS, UNCONTAINED};
pub use policy::BackendPolicy;

#[derive(Clone, Debug, Default, PartialEq)]
pub enum SandboxMode {
    #[default]
    ReadOnly,
    WorkspaceWrite,
    /// Deprecated compat alias: historic writable-roots mode whose name
    /// suggests host authority it never provided (M005).
    ///
    /// New code must use [`codegg_core::approval::SandboxProfile`] instead:
    /// `ReadOnly`/`WorkspaceWrite` map to an enabled [`SandboxConfig`],
    /// while `FullHost` means *no* `SandboxConfig` (explicit no CodeGG
    /// filesystem containment). Never construct this variant for `FullHost`.
    DangerFullAccess,
}

impl SandboxMode {
    fn is_writable(&self) -> bool {
        matches!(self, Self::WorkspaceWrite | Self::DangerFullAccess)
    }

    /// `true` only for the deprecated compat variant.
    pub fn is_deprecated_full_access(&self) -> bool {
        matches!(self, Self::DangerFullAccess)
    }

    /// Compatibility parser for legacy serialized config.
    ///
    /// `danger_full_access`/`full_host`/`fullhost` all parse to
    /// `DangerFullAccess` for readability, but callers must map the result
    /// through [`sandbox_profile_for_mode`] and treat `FullHost` as "no
    /// containment" — never silently reinterpret old writable-roots config
    /// as host authority.
    pub fn parse_compat(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "read_only" | "readonly" | "read-only" => Some(Self::ReadOnly),
            "workspace_write" | "workspace-write" => Some(Self::WorkspaceWrite),
            "danger_full_access" | "danger-full-access" | "dangerfullaccess" | "full_host"
            | "full-host" | "fullhost" => Some(Self::DangerFullAccess),
            _ => None,
        }
    }
}

/// Map a legacy [`SandboxMode`] to its M005 [`codegg_core::approval::SandboxProfile`].
///
/// `DangerFullAccess` maps to `FullHost` at the *vocabulary* level only;
/// the execution mapping differs: legacy mode behaved as writable roots,
/// while `FullHost` intentionally carries no [`SandboxConfig`]. Callers
/// must use [`sandbox_config_for_profile`] (which returns `None` for
/// `FullHost`) rather than reusing the old writable-roots construction.
pub fn sandbox_profile_for_mode(mode: &SandboxMode) -> codegg_core::approval::SandboxProfile {
    match mode {
        SandboxMode::ReadOnly => codegg_core::approval::SandboxProfile::ReadOnly,
        SandboxMode::WorkspaceWrite => codegg_core::approval::SandboxProfile::WorkspaceWrite,
        SandboxMode::DangerFullAccess => codegg_core::approval::SandboxProfile::FullHost,
    }
}

/// Map an M005 [`codegg_core::approval::SandboxProfile`] to its legacy
/// [`SandboxMode`] for readers of old config. `FullHost` maps to the
/// deprecated `DangerFullAccess` name; see [`sandbox_config_for_profile`]
/// for why the execution mapping is not 1:1.
pub fn sandbox_mode_for_profile(
    profile: codegg_core::approval::SandboxProfile,
) -> Option<SandboxMode> {
    match profile {
        codegg_core::approval::SandboxProfile::ReadOnly => Some(SandboxMode::ReadOnly),
        codegg_core::approval::SandboxProfile::WorkspaceWrite => Some(SandboxMode::WorkspaceWrite),
        // FullHost has no SandboxMode execution: it is the absence of a
        // SandboxConfig. Return None so callers cannot mistake it for a
        // writable-roots mode.
        codegg_core::approval::SandboxProfile::FullHost => None,
    }
}

/// Build the authoritative [`SandboxConfig`] for a requested M005 profile.
///
/// - `ReadOnly`/`WorkspaceWrite`: `Some` enabled config whose allowed
///   roots are exactly the canonical workspace/effective approved roots
///   (plus the minimum runtime libraries/executable requirements already
///   handled by the helper in [`SandboxConfig::launch_spec`]).
/// - `FullHost`: `None` — explicit no CodeGG filesystem containment.
///   Callers must record `FullHost` enforcement and must not fall back to
///   a writable-roots config.
pub fn sandbox_config_for_profile(
    profile: codegg_core::approval::SandboxProfile,
    workspace_root: &Path,
) -> Option<SandboxConfig> {
    sandbox_config_for_profile_with_roots(profile, &[workspace_root.to_path_buf()])
}

/// [`sandbox_config_for_profile`] with explicit approved roots (workspace
/// root plus effective approved roots, e.g. child worktree roots).
pub fn sandbox_config_for_profile_with_roots(
    profile: codegg_core::approval::SandboxProfile,
    roots: &[PathBuf],
) -> Option<SandboxConfig> {
    match profile {
        codegg_core::approval::SandboxProfile::ReadOnly => {
            let allowed = roots
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            Some(
                SandboxConfig::new()
                    .with_enabled(true)
                    .with_mode(SandboxMode::ReadOnly)
                    .with_allowed_paths(allowed),
            )
        }
        codegg_core::approval::SandboxProfile::WorkspaceWrite => {
            let allowed = roots
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            Some(
                SandboxConfig::new()
                    .with_enabled(true)
                    .with_mode(SandboxMode::WorkspaceWrite)
                    .with_allowed_paths(allowed),
            )
        }
        codegg_core::approval::SandboxProfile::FullHost => None,
    }
}

/// What OS filesystem containment this host can actually provide.
///
/// This replaces the historical implicit "availability is just a `false`"
/// with an inspectable value. A degraded (uncontained) run is a named,
/// reportable state instead of a silent lie: hosts without any backend get a
/// working tool path that says so out loud, and never a `FullHost` claim.
///
/// Which backend is available is *data*, resolved from the [`BACKENDS`]
/// registry in preference order — not a hard-coded platform branch. Adding a
/// backend therefore changes this type's payload without changing any of the
/// code that reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SandboxCapability {
    /// A backend is present and a child process can enforce it.
    Available { backend: BackendId },
    /// No OS filesystem containment is obtainable on this host. `reason`
    /// is the operator-facing platform explanation.
    Unavailable { reason: String },
}

impl SandboxCapability {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }

    /// The platform reason, or `None` when containment is available.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Available { .. } => None,
            Self::Unavailable { reason } => Some(reason.as_str()),
        }
    }

    /// The backend that will be used, or [`UNCONTAINED`] when none is.
    pub fn backend_id(&self) -> BackendId {
        match self {
            Self::Available { backend } => *backend,
            Self::Unavailable { .. } => UNCONTAINED,
        }
    }

    /// Short backend token used in enforcement descriptors and audit text.
    pub fn backend(&self) -> &'static str {
        self.backend_id().as_str()
    }

    pub fn describe(&self) -> String {
        match self {
            Self::Available { backend } => {
                let name = backend::backend(*backend)
                    .map(|entry| entry.name)
                    .unwrap_or("unknown backend");
                format!("{name} available (OS filesystem containment)")
            }
            Self::Unavailable { reason } => {
                format!("no OS filesystem containment available ({reason})")
            }
        }
    }
}

/// The host's real containment capability, probed once per call.
///
/// Probing is cheap and side-effect free, so this stays a function rather
/// than a cached global — a cached capability would go stale the moment the
/// helper binary or the kernel ABI changes.
pub fn platform_sandbox_capability() -> SandboxCapability {
    match backend::select() {
        Ok(entry) => SandboxCapability::Available { backend: entry.id },
        // Every registered backend refused; report what each one said so the
        // operator can tell "wrong kernel" from "wrong platform".
        Err(reasons) => SandboxCapability::Unavailable {
            reason: reasons
                .into_iter()
                .map(|(id, reason)| format!("{id}: {reason}"))
                .collect::<Vec<_>>()
                .join("; "),
        },
    }
}

/// How one bash dispatch will actually execute on this host.
///
/// `Unconstrained` is only ever reached when no containment was requested
/// (an explicit `FullHost` profile, or no policy at all). A constrained
/// request on a host without containment takes the explicitly named
/// [`SandboxExecutionPath::DegradedUncontained`] path — it never degrades
/// into `Unconstrained`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SandboxExecutionPath {
    /// No CodeGG filesystem containment was requested (explicit FullHost).
    Unconstrained,
    /// The child helper will enforce the policy before exec.
    Contained { backend: BackendId },
    /// Containment was requested but this host cannot provide it. The
    /// command still runs, uncontained, and must be reported to the
    /// operator and recorded distinctly in the audit/authorization trail.
    DegradedUncontained { reason: String },
}

impl SandboxExecutionPath {
    pub fn is_degraded(&self) -> bool {
        matches!(self, Self::DegradedUncontained { .. })
    }

    pub fn is_contained(&self) -> bool {
        matches!(self, Self::Contained { .. })
    }

    /// Operator-facing one-liner naming what actually happened.
    pub fn describe(&self) -> String {
        match self {
            Self::Unconstrained => {
                "no CodeGG filesystem containment (explicit FullHost)".to_string()
            }
            Self::Contained { backend } => {
                format!("filesystem contained ({backend} helper)")
            }
            Self::DegradedUncontained { reason } => {
                format!("filesystem UNCONTAINED (degraded: {reason})")
            }
        }
    }
}

/// Decide the execution path for a configured (or absent) sandbox policy.
///
/// This is the single place that turns "containment was requested" plus the
/// host capability into a named path, so the bash tool cannot silently pick
/// containment on a host that has none.
pub fn sandbox_execution_path(config: Option<&SandboxConfig>) -> SandboxExecutionPath {
    if !config.is_some_and(|config| config.enabled) {
        return SandboxExecutionPath::Unconstrained;
    }
    match platform_sandbox_capability() {
        SandboxCapability::Available { backend } => SandboxExecutionPath::Contained { backend },
        SandboxCapability::Unavailable { reason } => {
            SandboxExecutionPath::DegradedUncontained { reason }
        }
    }
}

/// Resolve the truthful host enforcement for a requested profile (M005).
///
/// - `FullHost` → [`codegg_core::approval::SandboxEnforcement::for_full_host`].
/// - Constrained + an available backend → enforced, naming that backend.
///   ABI is unknown until launch; network is always `Unrestricted` — no
///   shipped backend isolates the network.
/// - Constrained + unavailable host → constrained-unavailable (fail-closed
///   signal; never `FullHost`).
///
/// Unchanged in substance: this reports the fact, it does not grant
/// containment. Operator policy decides deny-vs-escalate on the
/// constrained-unavailable signal.
pub fn resolve_sandbox_enforcement(
    profile: codegg_core::approval::SandboxProfile,
) -> codegg_core::approval::SandboxEnforcement {
    use codegg_core::approval::SandboxEnforcement;
    if !profile.requires_filesystem_containment() {
        return SandboxEnforcement::for_full_host();
    }
    match platform_sandbox_capability() {
        SandboxCapability::Available { backend } => {
            SandboxEnforcement::for_constrained_enforced(profile, backend.as_str(), None)
        }
        SandboxCapability::Unavailable { reason } => {
            SandboxEnforcement::for_constrained_unavailable(profile, reason)
        }
    }
}

/// Bounded escalation hint for a path outside the current constrained
/// roots. Returns a [`codegg_core::approval::SandboxEscalationRequest`]
/// describing the capability/path; callers deny or route through
/// `ApprovalRouter` rather than switching the turn to `FullHost`.
pub fn escalation_for_outside_path(
    requested_path: &str,
    profile: codegg_core::approval::SandboxProfile,
) -> codegg_core::approval::SandboxEscalationRequest {
    codegg_core::approval::SandboxEscalationRequest::new(
        requested_path,
        profile,
        "path is outside the enforced workspace roots; select FullHost explicitly if host authority is genuinely required",
    )
}

#[derive(Clone, Debug, Default)]
pub struct SandboxConfig {
    pub enabled: bool,
    pub mode: SandboxMode,
    pub allowed_paths: Vec<String>,
    pub deny_paths: Vec<String>,
}

impl SandboxConfig {
    pub fn new() -> Self {
        Self {
            enabled: false,
            mode: SandboxMode::default(),
            allowed_paths: Vec::new(),
            deny_paths: Vec::new(),
        }
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn with_mode(mut self, mode: SandboxMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn with_allowed_paths(mut self, paths: Vec<String>) -> Self {
        self.allowed_paths = paths;
        self
    }

    pub fn with_deny_paths(mut self, paths: Vec<String>) -> Self {
        self.deny_paths = paths;
        self
    }

    /// Whether this host can enforce filesystem containment at all.
    /// Prefer [`platform_sandbox_capability`] when the reason matters.
    pub fn is_available() -> bool {
        platform_sandbox_capability().is_available()
    }

    pub fn enforce(&self) -> Result<(), ToolError> {
        if self.enabled {
            return Err(ToolError::Permission(
                "sandbox enforcement is child-process-only; launch through the sandbox helper"
                    .to_string(),
            ));
        }
        Ok(())
    }

    /// Construct the bounded child-launch description used by the private
    /// helper. All paths are resolved before the child starts; missing rules
    /// are policy errors, never silently skipped.
    ///
    /// The result is **backend-neutral**: it names paths, not a mechanism.
    /// [`policy`] builds the tool-compatible allowance sets and [`backend`]
    /// decides which OS facility renders them.
    pub fn launch_spec(
        &self,
        target: impl AsRef<Path>,
        args: &[String],
        cwd: Option<&Path>,
    ) -> Result<SandboxLaunchSpec, ToolError> {
        if !self.enabled {
            return Err(ToolError::Permission(
                "cannot build a sandbox launch spec for a disabled sandbox".to_string(),
            ));
        }
        let target = resolve_executable(target.as_ref()).ok_or_else(|| {
            ToolError::Permission(format!(
                "sandbox target could not be resolved: {}",
                target.as_ref().display()
            ))
        })?;
        let roots = if self.allowed_paths.is_empty() {
            vec![cwd
                .ok_or_else(|| ToolError::Permission("sandbox cwd is required".to_string()))?
                .to_path_buf()]
        } else {
            self.allowed_paths
                .iter()
                .map(|raw| {
                    std::fs::canonicalize(raw).map_err(|e| {
                        ToolError::Permission(format!(
                            "sandbox path '{raw}' could not resolve: {e}"
                        ))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        let write_roots = roots.clone();
        let mut read_paths = roots;
        read_paths.push(target.clone());
        // Tool-compatible read allowances: without these a sandboxed command
        // cannot resolve a hostname, read its own toolchain, or find its
        // package cache, and every invocation fails. See `policy`.
        read_paths.extend(policy::tool_read_allowances());
        let write_paths = if self.mode.is_writable() {
            let mut writes = write_roots;
            writes.extend(policy::tool_write_allowances());
            writes
        } else {
            Vec::new()
        };
        // Device nodes are writable under every profile, including
        // ReadOnly: they hold no persistent state, and without them every
        // `> /dev/null` and `2>&1` fails. See `policy::device_write_allowances`.
        let mut write_paths = write_paths;
        write_paths.extend(policy::device_write_allowances());
        // Operator-declared denies are config strings; the backend's own
        // sensitive-path set is paths. Both land in one list.
        //
        // Canonicalized like the allow sets, and deliberately best-effort: a
        // deny path that does not exist denies nothing, so dropping it is
        // correct, whereas an uncanonicalized deny is a rule the backend
        // cannot match (`/etc/sudoers` vs the resolved `/private/etc/...`).
        let mut deny_paths: Vec<PathBuf> = self
            .deny_paths
            .iter()
            .filter_map(|raw| std::fs::canonicalize(raw).ok())
            .collect();
        deny_paths.extend(policy::sensitive_deny_paths());
        Ok(SandboxLaunchSpec {
            target,
            args: args.to_vec(),
            read_paths,
            write_paths,
            deny_paths,
        })
    }
}

/// Private, bounded launch description consumed by `codegg-sandbox-helper`.
/// It is local process plumbing, not a daemon or public wire protocol.
///
/// Paths only — which OS facility enforces them is chosen by [`backend`]
/// at apply time, so the same spec serves Landlock and Seatbelt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxLaunchSpec {
    pub target: PathBuf,
    pub args: Vec<String>,
    pub read_paths: Vec<PathBuf>,
    pub write_paths: Vec<PathBuf>,
    /// Roots never readable or writable, regardless of profile. Redundant
    /// under a deny-first backend, load-bearing under an allow-default one.
    #[serde(default)]
    pub deny_paths: Vec<PathBuf>,
}

/// One-shot helper status. `Enforced` is a setup event; the other variants
/// are terminal events. The parent accepts `Enforced` followed by EOF for a
/// successful target exec, or `Enforced` followed by `ExecError` when exec
/// returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SandboxLaunchOutcome {
    /// Containment is in force. `abi` is the backend's reported version and
    /// is `None` for mechanisms without one (Seatbelt). `guarantees` and
    /// `limits` are the obtained facts, so a backend can never be read as
    /// stronger than it is.
    Enforced {
        backend: BackendId,
        abi: Option<u32>,
        guarantees: Vec<String>,
        limits: Vec<String>,
    },
    Unavailable {
        reason: String,
    },
    SetupError {
        reason: String,
    },
    ExecError {
        reason: String,
    },
}

impl SandboxLaunchOutcome {
    /// Construct the success outcome from a backend's own report.
    pub fn enforced(backend: BackendId, enforcement: BackendEnforcement) -> Self {
        Self::Enforced {
            backend,
            abi: enforcement.abi,
            guarantees: enforcement.guarantees,
            limits: enforcement.limits,
        }
    }

    /// The backend that produced containment, when it was produced.
    pub fn backend(&self) -> Option<BackendId> {
        match self {
            Self::Enforced { backend, .. } => Some(*backend),
            _ => None,
        }
    }
}

/// Version for the private helper status frame. This is local process
/// plumbing, not a public or durable protocol.
pub const SANDBOX_STATUS_VERSION: u8 = 1;
pub const MAX_SANDBOX_STATUS_BYTES: usize = 16 * 1024;
pub const MAX_SANDBOX_SPEC_BYTES: usize = 64 * 1024;

#[cfg(unix)]
pub const SANDBOX_STATUS_FD: i32 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxStatusFrame {
    pub version: u8,
    pub outcome: SandboxLaunchOutcome,
}

/// Encode one bounded, length-prefixed status frame.
pub fn encode_sandbox_status(outcome: SandboxLaunchOutcome) -> Result<Vec<u8>, String> {
    let payload = serde_json::to_vec(&SandboxStatusFrame {
        version: SANDBOX_STATUS_VERSION,
        outcome,
    })
    .map_err(|error| format!("encode sandbox status: {error}"))?;
    let frame_len = 4usize
        .checked_add(payload.len())
        .ok_or_else(|| "sandbox status frame length overflowed".to_string())?;
    if frame_len > MAX_SANDBOX_STATUS_BYTES {
        return Err("sandbox status frame exceeds 16 KiB".to_string());
    }
    let payload_len = u32::try_from(payload.len())
        .map_err(|_| "sandbox status payload length exceeds u32".to_string())?;
    let mut frame = Vec::with_capacity(frame_len);
    frame.extend_from_slice(&payload_len.to_be_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

/// Decode the complete private status stream and enforce its small state
/// machine. A target that writes to the channel, a helper that emits a
/// duplicate terminal state, or a truncated/oversized stream fails closed.
pub fn decode_sandbox_status(bytes: &[u8]) -> Result<SandboxLaunchOutcome, String> {
    if bytes.is_empty() {
        return Err("sandbox helper produced no status frame".to_string());
    }
    if bytes.len() > MAX_SANDBOX_STATUS_BYTES {
        return Err("sandbox status stream exceeds 16 KiB".to_string());
    }

    let mut cursor = 0usize;
    // The setup frame is remembered whole (not just its ABI) so the backend
    // identity, guarantees, and limits survive to the caller.
    let mut setup: Option<SandboxLaunchOutcome> = None;
    let mut setup_seen: Option<()> = None;
    let mut terminal = None;
    while cursor < bytes.len() {
        let length_end = cursor
            .checked_add(4)
            .ok_or_else(|| "sandbox status length overflowed".to_string())?;
        if length_end > bytes.len() {
            return Err("sandbox status frame has a truncated length prefix".to_string());
        }
        let payload_len = u32::from_be_bytes(
            bytes[cursor..length_end]
                .try_into()
                .map_err(|_| "sandbox status length prefix is invalid".to_string())?,
        ) as usize;
        if payload_len == 0 || payload_len > MAX_SANDBOX_STATUS_BYTES - 4 {
            return Err("sandbox status frame has an invalid length".to_string());
        }
        cursor = length_end;
        let payload_end = cursor
            .checked_add(payload_len)
            .ok_or_else(|| "sandbox status payload length overflowed".to_string())?;
        if payload_end > bytes.len() {
            return Err("sandbox status frame is truncated".to_string());
        }
        let frame: SandboxStatusFrame = serde_json::from_slice(&bytes[cursor..payload_end])
            .map_err(|error| format!("sandbox status frame is malformed: {error}"))?;
        if frame.version != SANDBOX_STATUS_VERSION {
            return Err(format!(
                "unsupported sandbox status version {}",
                frame.version
            ));
        }
        cursor = payload_end;

        match &frame.outcome {
            SandboxLaunchOutcome::Enforced { .. } => {
                if setup_seen.replace(()).is_some() || terminal.is_some() {
                    return Err("sandbox helper produced a duplicate setup status".to_string());
                }
                setup = Some(frame.outcome.clone());
            }
            terminal_outcome @ (SandboxLaunchOutcome::Unavailable { .. }
            | SandboxLaunchOutcome::SetupError { .. }
            | SandboxLaunchOutcome::ExecError { .. }) => {
                if terminal.is_some() {
                    return Err("sandbox helper produced duplicate terminal status".to_string());
                }
                if setup_seen.is_some()
                    && !matches!(terminal_outcome, SandboxLaunchOutcome::ExecError { .. })
                {
                    return Err("sandbox helper produced a terminal status after setup".to_string());
                }
                if matches!(terminal_outcome, SandboxLaunchOutcome::ExecError { .. })
                    && setup_seen.is_none()
                {
                    return Err("sandbox exec failure was reported before setup".to_string());
                }
                terminal = Some(frame.outcome.clone());
            }
        }
    }

    if let Some(outcome) = terminal {
        if setup_seen.is_none() && matches!(outcome, SandboxLaunchOutcome::ExecError { .. }) {
            return Err("sandbox exec failure had no enforced setup".to_string());
        }
        return Ok(outcome);
    }
    setup.ok_or_else(|| "sandbox helper produced no terminal status".to_string())
}

/// Return the private helper executable from the installation-owned sibling
/// location. Inherited environment, PATH, and cwd are deliberately not part
/// of this resolution rule.
///
/// The strict trust rule lives in [`crate::install`]; this wrapper preserves
/// the historical `trusted sandbox helper` error wording while sharing the
/// canonical installation-directory and sibling validation.
pub fn sandbox_helper_path() -> Result<PathBuf, String> {
    crate::install::trusted_sandbox_helper_path()
        .map_err(|e| e.replace("installation-owned", "trusted sandbox helper"))
}

#[allow(dead_code)]
pub(crate) fn resolve_trusted_helper(current: &Path) -> Result<PathBuf, String> {
    crate::install::trusted_sandbox_helper_path_for(current)
        .map_err(|e| e.replace("installation-owned", "trusted sandbox helper"))
}

fn resolve_executable(path: &Path) -> Option<PathBuf> {
    if path.is_absolute() {
        return path.canonicalize().ok();
    }
    std::env::var_os("PATH").and_then(|path_var| {
        std::env::split_paths(&path_var)
            .map(|dir| dir.join(path))
            .find(|candidate| candidate.is_file())
            .and_then(|candidate| candidate.canonicalize().ok())
    })
}

/// Probe the Landlock backend specifically.
///
/// Retained for callers that report on one named mechanism (the install
/// doctor) rather than asking "can this host contain anything?". Host-wide
/// capability must use [`platform_sandbox_capability`], which consults every
/// registered backend.
pub fn probe_landlock() -> Result<(), String> {
    landlock::probe()
}

/// Apply the Landlock backend. Prefer [`apply_backend`], which picks the
/// host's backend for you.
pub fn apply_landlock(spec: &SandboxLaunchSpec) -> Result<u32, String> {
    landlock::apply(spec)?
        .abi
        .ok_or_else(|| "Landlock reported no effective ABI".to_string())
}

/// Apply `spec` with the host's selected backend.
///
/// Runs only inside the one-shot helper. Fails closed: an unavailable host
/// or a setup error is an error, never a silent downgrade to an uncontained
/// exec.
pub fn apply_backend(spec: &SandboxLaunchSpec) -> Result<SandboxLaunchOutcome, String> {
    let selected = backend::select().map_err(|reasons| {
        reasons
            .into_iter()
            .map(|(id, reason)| format!("{id}: {reason}"))
            .collect::<Vec<_>>()
            .join("; ")
    })?;
    let enforcement = (selected.apply)(spec)
        .map_err(|reason| format!("{} setup failed: {reason}", selected.id))?;
    Ok(SandboxLaunchOutcome::enforced(selected.id, enforcement))
}

struct CachedPaths {
    paths: Vec<PathBuf>,
    timestamp: Instant,
}

static CANONICAL_PATHS_CACHE: Mutex<
    Option<(HashMap<Vec<String>, CachedPaths>, VecDeque<Vec<String>>)>,
> = Mutex::new(None);

const MAX_CACHE_ENTRIES: usize = 100;
const CACHE_TTL: Duration = Duration::from_secs(300);

fn get_canonical_paths(allowed_paths: &[String]) -> Vec<PathBuf> {
    let mut cache = CANONICAL_PATHS_CACHE.lock().unwrap_or_else(|poisoned| {
        tracing::warn!("canonical path cache mutex was poisoned; resetting the cache");
        let mut cache = poisoned.into_inner();
        *cache = None;
        cache
    });
    if cache.is_none() {
        *cache = Some((HashMap::new(), VecDeque::new()));
    }
    let (cache_map, cache_order) = cache.as_mut().unwrap();

    if cache_map.is_empty() || cache_order.is_empty() {
        cache_order.clear();
    } else if let Some(oldest_key) = cache_order.front() {
        if let Some(cached) = cache_map.get(oldest_key) {
            if cached.timestamp.elapsed() > CACHE_TTL {
                cache_map.clear();
                cache_order.clear();
            }
        }
    }

    while cache_order.len() >= MAX_CACHE_ENTRIES {
        if let Some(oldest_key) = cache_order.pop_front() {
            cache_map.remove(&oldest_key);
        }
    }

    if let Some(cached) = cache_map.get(allowed_paths) {
        return cached.paths.clone();
    }

    let canonical: Vec<PathBuf> = allowed_paths
        .iter()
        .filter_map(|p| std::fs::canonicalize(p).ok())
        .collect();

    cache_map.insert(
        allowed_paths.to_vec(),
        CachedPaths {
            paths: canonical.clone(),
            timestamp: Instant::now(),
        },
    );
    cache_order.push_back(allowed_paths.to_vec());
    canonical
}

pub fn validate_path_safety(path: &Path, allowed_paths: &[String]) -> Result<(), ToolError> {
    if path
        .symlink_metadata()
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(ToolError::Permission(format!(
            "path '{}' is a symlink",
            path.display()
        )));
    }

    let canonical = std::fs::canonicalize(path).map_err(|_| {
        ToolError::Permission(format!("path '{}' could not be resolved", path.display()))
    })?;

    let allowed_canonical = get_canonical_paths(allowed_paths);
    for allowed in &allowed_canonical {
        if canonical.starts_with(allowed) {
            return Ok(());
        }
    }

    Err(ToolError::Permission(format!(
        "path '{}' is not in allowed paths",
        path.display()
    )))
}

pub fn get_default_allowed_paths() -> Vec<String> {
    let mut paths = Vec::new();

    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.to_string_lossy().to_string());
    }

    if let Ok(home) = std::env::var("HOME") {
        let home_path = Path::new(&home);
        if home_path.exists() {
            paths.push(format!("{}/.config", home));
            paths.push(format!("{}/.local/share", home));
        }
    }

    if let Some(config) = dirs::config_dir() {
        paths.push(config.to_string_lossy().to_string());
    }

    if let Some(data) = dirs::data_dir() {
        paths.push(data.to_string_lossy().to_string());
    }

    paths
}

/// Paths the sandbox treats as sensitive, as strings.
///
/// Compatibility wrapper over [`policy::sensitive_deny_paths`], which is the
/// real owner. The previous hard-coded list (`/etc`, `/var`, `/dev`, `/home`)
/// was Linux-shaped and far too coarse: now that `deny_paths` is genuinely
/// enforced rather than merely recorded, denying all of `/etc` would block
/// `/etc/hosts` and `/etc/resolv.conf`, and denying all of `/var` would block
/// the per-user temporary directory on both platforms. The curated set names
/// credential material instead of whole subtrees.
pub fn get_sensitive_paths() -> Vec<String> {
    policy::sensitive_deny_paths()
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn landlock_access_keeps_directory_rights_for_directories() {
        use landlock::{AccessFs, ABI};

        let directory = tempfile::tempdir().expect("directory fixture");
        let access =
            landlock_access_for_path(directory.path(), AccessFs::from_read(ABI::V1), ABI::V1)
                .expect("directory classification");

        assert!(access.contains(AccessFs::ReadDir));
        assert!(access.contains(AccessFs::ReadFile));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn landlock_access_removes_directory_rights_for_regular_files() {
        use landlock::{AccessFs, ABI};

        let file = tempfile::NamedTempFile::new().expect("file fixture");
        let access = landlock_access_for_path(file.path(), AccessFs::from_read(ABI::V1), ABI::V1)
            .expect("regular-file classification");

        assert!(!access.contains(AccessFs::ReadDir));
        assert!(access.contains(AccessFs::ReadFile));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn landlock_access_removes_directory_rights_for_special_files() {
        use landlock::{Access, AccessFs, ABI};

        let path = Path::new("/dev/null");
        if !path.exists() {
            return;
        }
        let access = landlock_access_for_path(path, AccessFs::from_all(ABI::V1), ABI::V1)
            .expect("special-file classification");

        assert!(!access.contains(AccessFs::ReadDir));
        assert!(access.contains(AccessFs::ReadFile));
        assert!(access.contains(AccessFs::WriteFile));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn landlock_access_fails_closed_when_path_cannot_be_classified() {
        use landlock::{AccessFs, ABI};

        let path = Path::new("/definitely/missing/codegg-sandbox-path");
        let error = landlock_access_for_path(path, AccessFs::from_read(ABI::V1), ABI::V1)
            .expect_err("missing path classification must fail");

        assert!(error.contains("classify sandbox path"));
        assert!(error.contains(path.to_string_lossy().as_ref()));
    }

    #[test]
    fn test_sandbox_config_default() {
        let config = SandboxConfig::new();
        assert!(!config.enabled);
        assert!(config.allowed_paths.is_empty());
    }

    #[test]
    fn enabled_enforcement_cannot_restrict_the_parent() {
        let config = SandboxConfig::new().with_enabled(true);
        let error = config
            .enforce()
            .expect_err("enabled enforcement must be child-only");
        assert!(error.to_string().contains("child-process-only"));
    }

    #[test]
    fn launch_spec_maps_workspace_write_to_write_roots() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let config = SandboxConfig::new()
            .with_enabled(true)
            .with_mode(SandboxMode::WorkspaceWrite)
            .with_allowed_paths(vec![temp_dir.path().to_string_lossy().to_string()]);
        let spec = config
            .launch_spec(
                "sh",
                &["-c".to_string(), "true".to_string()],
                Some(temp_dir.path()),
            )
            .expect("spec should be constructed");
        let canonical = temp_dir.path().canonicalize().expect("canonical temp dir");
        assert!(spec.read_paths.iter().any(|path| path == &canonical));
        assert!(spec.write_paths.iter().any(|path| path == &canonical));
        assert!(spec.args.contains(&"-c".to_string()));
    }

    #[test]
    fn test_validate_path_safety() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let temp_path = temp_dir.path().join("test");
        std::fs::create_dir_all(&temp_path).expect("temp path should be created");

        let allowed = vec![
            temp_dir.path().to_string_lossy().to_string(),
            "/home/user/project".to_string(),
        ];
        let result = validate_path_safety(&temp_path, &allowed);
        assert!(
            result.is_ok(),
            "path inside temp_dir should be allowed: {:?}",
            result
        );

        let result = validate_path_safety(Path::new("/etc/passwd"), &allowed);
        assert!(result.is_err(), "path outside allowed should be rejected");
    }

    #[test]
    fn test_validate_path_safety_with_symlink() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let real = temp_dir.path().join("real");
        let link = temp_dir.path().join("link");
        std::fs::create_dir_all(&real).expect("real dir should be created");

        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, &link).expect("symlink should be created");

        #[cfg(not(unix))]
        {
            return;
        }

        let allowed = vec![temp_dir.path().to_string_lossy().to_string()];
        let result = validate_path_safety(&link, &allowed);
        assert!(
            result.is_err(),
            "symlink in path should be rejected: {:?}",
            result
        );
    }

    #[cfg(unix)]
    #[test]
    fn trusted_helper_resolution_ignores_inherited_override() {
        use std::os::unix::fs::PermissionsExt;

        let install = tempfile::tempdir().expect("installation directory");
        let executable = install.path().join("codegg");
        let helper = install.path().join("codegg-sandbox-helper");
        std::fs::write(&executable, b"codegg").expect("executable fixture");
        std::fs::write(&helper, b"helper").expect("helper fixture");
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755))
            .expect("helper executable permissions");

        let variable = ["CODEGG", "SANDBOX", "HELPER"].join("_");
        let substitution = install.path().join("substituted-helper");
        std::fs::write(&substitution, b"substitution").expect("substitution fixture");
        std::env::set_var(&variable, &substitution);
        let resolved = resolve_trusted_helper(&executable).expect("trusted sibling helper");
        std::env::remove_var(variable);

        assert_eq!(resolved, helper.canonicalize().expect("canonical helper"));
    }

    #[test]
    fn m005_profile_maps_to_truthful_sandbox_config() {
        use codegg_core::approval::SandboxProfile;
        let dir = tempfile::tempdir().expect("workspace fixture");
        // WorkspaceWrite builds an enabled writable config over the root.
        let write = sandbox_config_for_profile(SandboxProfile::WorkspaceWrite, dir.path())
            .expect("workspace_write must build a config");
        assert!(write.enabled);
        assert!(matches!(write.mode, SandboxMode::WorkspaceWrite));
        // ReadOnly builds an enabled non-writable config.
        let read = sandbox_config_for_profile(SandboxProfile::ReadOnly, dir.path())
            .expect("read_only must build a config");
        assert!(read.enabled);
        assert!(matches!(read.mode, SandboxMode::ReadOnly));
        // FullHost intentionally builds no containment.
        assert!(
            sandbox_config_for_profile(SandboxProfile::FullHost, dir.path()).is_none(),
            "FullHost must not build a SandboxConfig"
        );
        // WorkspaceWrite and FullHost are distinct execution properties.
        assert_ne!(
            sandbox_config_for_profile(SandboxProfile::WorkspaceWrite, dir.path())
                .map(|c| c.enabled),
            sandbox_config_for_profile(SandboxProfile::FullHost, dir.path()).map(|c| c.enabled)
        );
    }

    #[test]
    fn m005_legacy_danger_full_access_is_compat_only() {
        // Legacy string still parses for readability, but maps to FullHost
        // vocabulary while execution stays "no config".
        let mode = SandboxMode::parse_compat("danger_full_access").expect("compat parse");
        assert!(mode.is_deprecated_full_access());
        assert_eq!(
            sandbox_profile_for_mode(&mode),
            codegg_core::approval::SandboxProfile::FullHost
        );
        assert!(
            sandbox_mode_for_profile(codegg_core::approval::SandboxProfile::FullHost).is_none()
        );
    }

    #[test]
    fn platform_capability_matches_is_available_and_carries_a_reason() {
        let capability = platform_sandbox_capability();
        assert_eq!(
            capability.is_available(),
            SandboxConfig::is_available(),
            "capability and legacy boolean must never disagree"
        );
        match &capability {
            SandboxCapability::Available { backend } => {
                assert!(capability.reason().is_none());
                assert_eq!(capability.backend_id(), *backend);
                // An available backend must be one the registry actually
                // ships; otherwise capability is naming something real only
                // by accident.
                assert!(
                    backend::backend(*backend).is_some(),
                    "an available backend must be registered"
                );
            }
            SandboxCapability::Unavailable { reason } => {
                // An unsupported host must be able to say *why*, out loud.
                assert!(!reason.is_empty(), "unavailable must carry a reason");
                assert_eq!(capability.reason(), Some(reason.as_str()));
                assert_eq!(capability.backend(), "uncontained");
            }
        }
        assert!(capability
            .describe()
            .contains(if capability.is_available() {
                "available (OS filesystem containment)"
            } else {
                "no OS filesystem containment available"
            }));
    }

    #[test]
    fn no_policy_never_takes_the_degraded_path() {
        // Absent or disabled config means containment was never requested;
        // that is explicit FullHost semantics, not a degraded run.
        assert_eq!(
            sandbox_execution_path(None),
            SandboxExecutionPath::Unconstrained
        );
        let disabled = SandboxConfig::new().with_enabled(false);
        assert_eq!(
            sandbox_execution_path(Some(&disabled)),
            SandboxExecutionPath::Unconstrained
        );
    }

    #[test]
    fn enabled_policy_selects_path_from_real_host_capability() {
        let enabled = SandboxConfig::new().with_enabled(true);
        let path = sandbox_execution_path(Some(&enabled));
        match platform_sandbox_capability() {
            SandboxCapability::Available { backend } => {
                assert_eq!(path, SandboxExecutionPath::Contained { backend });
                assert!(path.is_contained());
                assert!(!path.is_degraded());
            }
            SandboxCapability::Unavailable { reason } => {
                assert_eq!(
                    path,
                    SandboxExecutionPath::DegradedUncontained {
                        reason: reason.clone()
                    }
                );
                assert!(path.is_degraded());
                assert!(!path.is_contained());
                // The degraded path must name itself and its reason.
                assert!(path.describe().contains("UNCONTAINED"));
                assert!(path.describe().contains(&reason));
                // It must never masquerade as the explicit FullHost path.
                assert_ne!(path, SandboxExecutionPath::Unconstrained);
            }
        }
    }

    #[test]
    fn m005_enforcement_resolution_is_truthful_per_host() {
        use codegg_core::approval::SandboxProfile;
        let full = resolve_sandbox_enforcement(SandboxProfile::FullHost);
        assert!(full.is_full_host());
        let constrained = resolve_sandbox_enforcement(SandboxProfile::WorkspaceWrite);
        // Network is always unrestricted for shell (no backend).
        assert!(matches!(
            constrained.network,
            codegg_core::approval::NetworkEnforcement::Unrestricted
        ));
        if SandboxConfig::is_available() {
            assert!(constrained.is_enforced());
        } else {
            assert!(!constrained.is_enforced());
            assert!(!constrained.is_full_host());
        }
    }

    #[test]
    fn status_decoder_rejects_malformed_duplicate_and_oversized_frames() {
        let enforced = encode_sandbox_status(SandboxLaunchOutcome::enforced(
            BackendId::LANDLOCK,
            BackendEnforcement::new(Some(9), &["guarantee"], &["limit"]),
        ))
        .expect("enforced frame");
        let setup = encode_sandbox_status(SandboxLaunchOutcome::SetupError {
            reason: "bad rule".to_string(),
        })
        .expect("setup frame");
        assert!(decode_sandbox_status(&enforced[..enforced.len() - 1]).is_err());
        assert!(decode_sandbox_status(&[enforced.clone(), enforced.clone()].concat()).is_err());
        assert!(decode_sandbox_status(&[enforced, setup].concat()).is_err());
        assert!(decode_sandbox_status(&vec![0_u8; MAX_SANDBOX_STATUS_BYTES + 1]).is_err());
    }
}
