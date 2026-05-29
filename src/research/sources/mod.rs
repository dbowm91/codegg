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
    fn name(&self) -> &'static str;
}
