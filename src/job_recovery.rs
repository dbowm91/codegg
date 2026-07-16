use std::sync::Arc;

use codegg_core::jobs::{
    DaemonGeneration, JobStore, JobStoreError, RecoveryPolicy, RecoveryReport,
};

pub async fn recover_jobs_at_startup(
    store: Arc<dyn JobStore>,
    stale_generation: &DaemonGeneration,
    policy: &RecoveryPolicy,
) -> Result<RecoveryReport, JobStoreError> {
    store.recover_generation(stale_generation, policy).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_core::jobs::{DaemonGeneration, InMemoryJobStore, RecoveryPolicy};

    #[tokio::test(flavor = "current_thread")]
    async fn recover_delegates_to_store() {
        let store = Arc::new(InMemoryJobStore::new());
        let gen = DaemonGeneration::new();
        let policy = RecoveryPolicy::default();
        let result = recover_jobs_at_startup(store, &gen, &policy).await;
        assert!(result.is_ok());
        let report = result.unwrap();
        assert_eq!(report.interrupted_attempts, 0);
        assert_eq!(report.requeued_jobs, 0);
    }
}
