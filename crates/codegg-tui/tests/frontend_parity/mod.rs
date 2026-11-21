#[path = "../../../../tests/frontend_parity/mod.rs"]
mod canonical_fixture;
pub use canonical_fixture::{CanonicalState, Golden, IdentifiedState, NamedState};

#[path = "tui_reconstruction.inc"]
pub mod tui_reconstruction;
