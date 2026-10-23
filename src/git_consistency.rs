//! Git read-consistency authority for the root crate.
//!
//! A Git read is only as trustworthy as the object generation it observed,
//! and a consumer that needs one stable generation has to be able to *say*
//! so at the type level rather than in prose. `egggit` supplies the
//! vocabulary — [`GitReadConsistency`], [`GitReadFact`] and
//! [`GenerationConsistent`] — but the *authority* to consume a read stays
//! with this crate, in two separate parts:
//!
//! * The **class** is a fact carried by the read. It travels with the value
//!   ([`GitReadFact`]), so a consumer cannot receive the bytes and forget what
//!   it was allowed to assume about them.
//! * The **capability** ([`LiveReadCapability`]) is an authority a consumer
//!   must *hold*. A production build of CodeGG cannot obtain it: the only
//!   constructor is compiled out of `egggit` unless its
//!   `git-live-read-test-support` feature is enabled, and the shipped binary
//!   does not enable it.
//!
//! [`admit_generation_bound`] is the fence. It turns a read fact into a
//! [`GenerationConsistent`] value, or refuses it with a typed
//! [`GitConsistencyRefusal`] that the caller cannot mistake for a warning.
//!
//! Containment and consistency stay orthogonal: a perfectly contained read
//! of a concurrently mutable repository is still a `LiveMutable` read, and
//! no amount of sandboxing turns it into a generation.

use crate::git_service::GitServiceError;

pub use egggit::consistency::{
    admit_generation_consistent, GenerationConsistent, GitConsistencyRefusal, GitReadConsistency,
    GitReadFact, LiveReadCapability,
};

/// Admit a read fact to a generation-bound consumer of this crate.
///
/// This is the root crate's fence. It delegates to
/// [`admit_generation_consistent`] and maps a refusal onto the typed
/// [`GitServiceError::Consistency`] variant, so a caller that ignores the
/// fence still cannot turn a refusal into a payload.
pub fn admit_generation_bound<T>(
    fact: GitReadFact<T>,
    capability: Option<&LiveReadCapability>,
) -> Result<GenerationConsistent<T>, GitServiceError> {
    admit_generation_consistent(fact, capability).map_err(GitServiceError::from)
}

/// The live-read capability this crate is able to obtain.
///
/// # Production posture
///
/// A production build can never obtain the capability, and this accessor is
/// the reason that is true rather than a convention:
///
/// * `LiveReadCapability`'s only constructor is compiled out of `egggit`
///   unless that crate's `git-live-read-test-support` feature is enabled.
///   The shipped binary does not enable it, so the capability cannot even be
///   named in a product build.
/// * Even where the constructor exists, the capability is minted *at the
///   point of use* by the reviewed contained live-read route (see
///   `mint_test_only_live_read_capability`) rather than handed out from
///   here, so no other caller can obtain one by asking this function.
///
/// The function therefore returns `None` in **every** build. That is the
/// single place a live route would have to change, and changing it is a
/// reviewed decision rather than a build convenience.
pub fn live_read_capability() -> Option<&'static LiveReadCapability> {
    None
}

/// Mint the live-read capability for the reviewed test-only contained
/// live-read route.
///
/// Compiled only under `linux-git-test-support`, which enables
/// `egggit/git-live-read-test-support` — the feature that compiles
/// `LiveReadCapability`'s only constructor into existence. The capability
/// therefore exists only in a test-only build of this crate, and only the
/// reviewed route may mint one.
#[cfg(feature = "linux-git-test-support")]
pub fn mint_test_only_live_read_capability() -> LiveReadCapability {
    LiveReadCapability::mint_for_test_only_route()
}

impl From<GitConsistencyRefusal> for GitServiceError {
    fn from(refusal: GitConsistencyRefusal) -> Self {
        GitServiceError::Consistency(refusal)
    }
}
