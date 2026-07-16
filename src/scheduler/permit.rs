//! Resource permits and RAII guards.
//!
//! A `ResourcePermit` is the abstract right to consume some
//! multidimensional resources (CPU weight, memory hint, process slot,
//! I/O weight, network slot) and one or more exclusivity keys. The
//! scheduler's admission controller hands out `ResourcePermitGuard`s;
//! when the guard is dropped (or explicitly released), the
//! reservation is released atomically.

use std::sync::Arc;

use parking_lot::Mutex;

use crate::scheduler::admission::AdmissionController;

/// Dimensions that can be reserved on the admission controller. Each
/// admission acquires all of these atomically.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PermitDimensions {
    pub cpu_weight: u32,
    pub memory_mb_hint: u64,
    pub process_slots: u16,
    pub io_weight: u32,
    pub network_slots: u16,
    pub exclusivity_keys: Vec<String>,
}

/// Token returned by the admission controller when a permit is taken.
/// It is later exchanged for a `ResourcePermitGuard` once the executor
/// is dispatched (so the admission is recorded even if the executor
/// is later unable to start).
#[derive(Debug, Clone)]
pub struct ResourcePermit {
    pub dimensions: PermitDimensions,
}

/// RAII handle that releases the reserved resources on drop. The
/// drop calls back into the admission controller so the next
/// reconciliation tick sees the released capacity.
#[derive(Debug)]
pub struct ResourcePermitGuard {
    inner: Option<Inner>,
}

#[derive(Debug)]
struct Inner {
    controller: Option<Arc<AdmissionController>>,
    dimensions: PermitDimensions,
}

impl ResourcePermitGuard {
    /// Build a guard that releases on drop. Use [`Self::new_orphan`]
    /// when the caller has not yet wrapped the controller in an
    /// `Arc`; the scheduler's `try_admit_arc` rewrites the guard
    /// to install the controller.
    pub fn new(controller: Arc<AdmissionController>, dimensions: PermitDimensions) -> Self {
        Self {
            inner: Some(Inner {
                controller: Some(controller),
                dimensions,
            }),
        }
    }

    /// Construct a guard without a controller. Drop is a no-op
    /// until [`Self::install_controller`] is called.
    pub fn new_orphan(dimensions: PermitDimensions) -> Self {
        Self {
            inner: Some(Inner {
                controller: None,
                dimensions,
            }),
        }
    }

    /// Install the controller reference. The scheduler calls this
    /// when wrapping an orphan guard returned by
    /// `AdmissionController::try_admit`.
    pub fn install_controller(&mut self, controller: Arc<AdmissionController>) {
        if let Some(inner) = self.inner.as_mut() {
            inner.controller = Some(controller);
        }
    }

    /// Detach the guard from its controller. The caller takes
    /// ownership of the dimensions and is responsible for releasing
    /// capacity (or for keeping it in use). Returns the reserved
    /// dimensions.
    pub fn detach(mut self) -> PermitDimensions {
        let inner = self.inner.take().expect("detach called once");
        // Drop runs after this returns; take clears `inner` so the
        // Drop impl sees `None` and does not double-release.
        inner.dimensions
    }

    pub fn dimensions(&self) -> &PermitDimensions {
        &self.inner.as_ref().expect("guard not detached").dimensions
    }
}

impl Drop for ResourcePermitGuard {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.take() {
            if let Some(c) = inner.controller {
                c.release(inner.dimensions);
            }
        }
    }
}

// `Mutex` re-export kept for downstream callers; silences
// "unused" warnings when no test exercises it directly.
#[allow(dead_code)]
fn _ensure_mutex_used(_m: &Arc<Mutex<()>>) {}

/// Convenience for tests: build a `PermitDimensions` from a config's
/// default per-kind resource request and the desired
/// exclusivity-keys override.
pub fn permit_from_request(
    cpu: u32,
    memory_mb: u64,
    process: u16,
    io: u32,
    network: u16,
    keys: Vec<String>,
) -> PermitDimensions {
    PermitDimensions {
        cpu_weight: cpu,
        memory_mb_hint: memory_mb,
        process_slots: process,
        io_weight: io,
        network_slots: network,
        exclusivity_keys: keys,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::config::ResolvedSchedulerConfig;

    #[test]
    fn permit_dimensions_default_is_zero() {
        let d = PermitDimensions::default();
        assert_eq!(d.cpu_weight, 0);
        assert!(d.exclusivity_keys.is_empty());
    }

    #[test]
    fn permit_from_request_helper() {
        let d = permit_from_request(2, 1024, 1, 1, 0, vec!["k".into()]);
        assert_eq!(d.cpu_weight, 2);
        assert_eq!(d.exclusivity_keys, vec!["k".to_string()]);
    }

    #[test]
    fn guard_release_on_drop() {
        let cfg = ResolvedSchedulerConfig::default();
        let controller = Arc::new(AdmissionController::new(cfg));
        {
            let dims = permit_from_request(1, 0, 1, 0, 0, vec![]);
            let permit = match controller.try_admit_arc(&dims) {
                crate::scheduler::admission::AdmissionDecision::Admitted(p) => p,
                other => panic!("expected Admitted, got {:?}", other),
            };
            assert_eq!(controller.used_process_slots(), 1);
            drop(permit);
        }
        assert_eq!(controller.used_process_slots(), 0);
    }

    #[test]
    fn guard_detach_skips_release() {
        let cfg = ResolvedSchedulerConfig::default();
        let controller = Arc::new(AdmissionController::new(cfg));
        let dims = permit_from_request(1, 0, 1, 0, 0, vec![]);
        let permit = match controller.try_admit_arc(&dims) {
            crate::scheduler::admission::AdmissionDecision::Admitted(p) => p,
            other => panic!("expected Admitted, got {:?}", other),
        };
        let _ = permit.detach();
        assert_eq!(controller.used_process_slots(), 1);
        // Released by no one — caller is responsible.
    }
}
