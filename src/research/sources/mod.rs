pub mod advisory;
pub mod crates_io;
pub mod docs_rs;
pub mod eggsearch;
pub mod github;
pub mod local_repo;
pub mod url;

use std::future::Future;
use std::pin::Pin;

use crate::research::error::Result;
use crate::research::types::*;

pub trait ResearchSourceAdapter: Send + Sync {
    fn collect<'a>(
        &'a self,
        request: &'a ResearchRequest,
        plan: &'a ResearchPlan,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<SourceRecord>>> + Send + 'a>>;

    /// Collect records together with structured retrieval status when the
    /// adapter can report it. Existing adapters retain their established
    /// behavior through this default implementation.
    fn collect_with_receipt<'a>(
        &'a self,
        request: &'a ResearchRequest,
        plan: &'a ResearchPlan,
    ) -> Pin<Box<dyn Future<Output = Result<SourceCollection>> + Send + 'a>> {
        Box::pin(async move {
            self.collect(request, plan)
                .await
                .map(|sources| SourceCollection {
                    sources,
                    receipts: Vec::new(),
                })
        })
    }

    fn name(&self) -> &'static str;
}
