//! Frontend-neutral UTF-8 document snapshots and deterministic byte edits.
//!
//! Revisions begin at zero. Empty transactions are successful no-ops and do
//! not advance the revision. Ranges refer to the pre-transaction text and must
//! be sorted, disjoint UTF-8 byte ranges.

mod buffer;
mod edit;
mod error;
mod position;

pub use buffer::{DocumentBuffer, DocumentRevision, DocumentSnapshot};
pub use edit::{AppliedTransaction, DocumentLimits, TextEdit, TextRange, TextTransaction};
pub use error::{DocumentError, Result};
pub use position::BytePosition;
