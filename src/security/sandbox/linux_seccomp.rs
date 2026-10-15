//! Reviewed Linux syscall denylist compiled with seccompiler.
//!
//! Bubblewrap installs these cBPF instructions after namespace/mount setup
//! and before target execution. The default remains allow so ordinary
//! developer tools keep their syscall surface; namespace changes, tracing,
//! kernel attack-surface interfaces, and privileged host operations are
//! denied with ENOSYS so glibc can fall back from clone3 to clone. Network
//! policy is owned by M005, not this filter.

use std::collections::BTreeMap;
use std::convert::TryInto;

use seccompiler::{
    BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
    SeccompRule, TargetArch,
};

const MAX_FILTER_BYTES: usize = 32 * 1024;

pub fn compile_filter() -> Result<Vec<u8>, String> {
    let architecture: TargetArch = std::env::consts::ARCH
        .try_into()
        .map_err(|error| format!("unsupported seccomp architecture: {error}"))?;
    let mut rules = BTreeMap::<i64, Vec<SeccompRule>>::new();

    for syscall in [
        libc::SYS_unshare,
        libc::SYS_setns,
        libc::SYS_mount,
        libc::SYS_umount2,
        libc::SYS_pivot_root,
        libc::SYS_ptrace,
        libc::SYS_process_vm_readv,
        libc::SYS_process_vm_writev,
        libc::SYS_bpf,
        libc::SYS_perf_event_open,
        libc::SYS_keyctl,
        libc::SYS_add_key,
        libc::SYS_request_key,
        libc::SYS_userfaultfd,
        libc::SYS_open_by_handle_at,
        libc::SYS_init_module,
        libc::SYS_finit_module,
        libc::SYS_delete_module,
        libc::SYS_kexec_load,
        libc::SYS_reboot,
        libc::SYS_swapon,
        libc::SYS_swapoff,
        libc::SYS_syslog,
        libc::SYS_acct,
        libc::SYS_io_uring_setup,
        libc::SYS_io_uring_enter,
        libc::SYS_io_uring_register,
        // ENOSYS makes libc fall back to clone on runtimes that support both.
        libc::SYS_clone3,
    ] {
        rules.insert(syscall as i64, Vec::new());
    }

    // `clone` remains available for ordinary fork, process, and thread
    // creation, but any namespace-creation flag is denied. clone3 is denied
    // above because seccomp cannot safely dereference its userspace struct.
    let namespace_flags = [
        libc::CLONE_NEWNS,
        libc::CLONE_NEWCGROUP,
        libc::CLONE_NEWUTS,
        libc::CLONE_NEWIPC,
        libc::CLONE_NEWUSER,
        libc::CLONE_NEWPID,
        libc::CLONE_NEWNET,
    ];
    let clone_rules = namespace_flags
        .into_iter()
        .map(|flag| {
            SeccompRule::new(vec![SeccompCondition::new(
                0,
                SeccompCmpArgLen::Dword,
                SeccompCmpOp::MaskedEq(flag as u64),
                flag as u64,
            )
            .map_err(|error| format!("compile clone namespace condition: {error}"))?])
            .map_err(|error| format!("compile clone namespace rule: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    rules.insert(libc::SYS_clone as i64, clone_rules);

    let filter = SeccompFilter::new(
        rules,
        SeccompAction::Allow,
        SeccompAction::Errno(libc::ENOSYS as u32),
        architecture,
    )
    .map_err(|error| format!("construct seccomp filter: {error}"))?;
    let program: BpfProgram = filter
        .try_into()
        .map_err(|error| format!("compile seccomp filter: {error}"))?;
    if program.is_empty() || program.len().saturating_mul(8) > MAX_FILTER_BYTES {
        return Err("compiled seccomp filter is empty or exceeds its 32 KiB bound".into());
    }

    let mut bytes = Vec::with_capacity(program.len() * 8);
    for instruction in program {
        bytes.extend_from_slice(&instruction.code.to_ne_bytes());
        bytes.push(instruction.jt);
        bytes.push(instruction.jf);
        bytes.extend_from_slice(&instruction.k.to_ne_bytes());
    }
    if bytes.is_empty() || bytes.len() > MAX_FILTER_BYTES || bytes.len() % 8 != 0 {
        return Err("serialized seccomp filter violates its bounded cBPF format".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_compiles_arch_checked_bounded_cbpf() {
        let bytes = compile_filter().expect("compile fixed M004 seccomp policy");
        assert!(!bytes.is_empty());
        assert!(bytes.len() <= MAX_FILTER_BYTES);
        assert_eq!(bytes.len() % 8, 0);
    }
}
