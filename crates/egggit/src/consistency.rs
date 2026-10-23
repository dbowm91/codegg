//! Typed Git read-consistency contract.
//!
//! A Git read can observe the repository under very different guarantees, and
//! the difference decides whether the result may feed a generation-bound
//! consumer (diff generation, index publication, merge-base computation).
//! Historically the repository had **no** type for this: the private object
//! view's cache made a read "immutable" or "stale" only in prose, and a
//! contained live read was indistinguishable from a snapshot at the type
//! level. A consumer therefore could not be prevented from treating a
//! possibly-torn read as a stable generation.
//!
//! This module supplies the missing vocabulary:
//!
//! * [`GitReadConsistency`] — the closed set of classes a Git read can carry.
//! * [`GitReadFact`] — a read result **together with** the class it was
//!   obtained under, so the class cannot be silently dropped.
//! * [`GenerationConsistent`] — the only shape a generation-bound consumer
//!   may accept.
//! * [`admit_generation_consistent`] — the explicit admissibility check that
//!   produces it.
//! * [`LiveReadCapability`] — the authority a consumer must **hold** to use a
//!   live-class result, on the same footing as `DestructiveRepositoryChange`
//!   gates a destructive mutation.
//!
//! Containment and consistency are orthogonal: obtaining a sandbox boundary
//! before exec is evidence about host protection only, never about
//! generation consistency. A route can be perfectly contained and still be
//! unable to claim one object generation.

use std::fmt;

/// The object-generation consistency class of a Git read.
///
/// The class describes **the object store the read observed**. It does not
/// extend to refs, the index or the worktree: the private object view
/// covers the object database only, and those other surfaces are always read
/// live. A read that consults no object store therefore observed no object
/// generation at all, which is why [`GitReadConsistency::NoObjectGeneration`]
/// exists rather than being folded into one of the other variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitReadConsistency {
    /// The read served objects from one materialized generation.
    ///
    /// The claim is bounded by the preparation that produced the generation:
    /// the bytes were observed stable while they were materialized and are
    /// not mutated in place afterwards. It says nothing about whether the
    /// repository has since moved on.
    ImmutableGeneration,
    /// The read served objects from a capture that was immutable when it was
    /// taken and may now be stale relative to the repository.
    ///
    /// The preparation cache is a best-effort mechanism with eviction, not a
    /// durability boundary, so a cache-served read is a *captured* generation
    /// rather than a current one. Reporting this class instead of
    /// [`GitReadConsistency::ImmutableGeneration`] is what keeps the receipt
    /// truthful under cache eviction.
    CapturedNotCurrent,
    /// The read served objects directly from a concurrently mutable
    /// repository.
    ///
    /// An outside writer may alter, repack or delete objects while the read
    /// runs, so the observation may be torn or invalidated. This class is
    /// inadmissible to a generation-bound consumer unless that consumer holds
    /// the [`LiveReadCapability`].
    LiveMutable,
    /// The read consulted no object store, so it observed no object
    /// generation.
    ///
    /// Such a read cannot satisfy a generation-consistent requirement: it has
    /// no generation to be consistent with.
    NoObjectGeneration,
}

impl GitReadConsistency {
    /// A stable machine name for the class.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ImmutableGeneration => "immutable_generation",
            Self::CapturedNotCurrent => "captured_not_current",
            Self::LiveMutable => "live_mutable",
            Self::NoObjectGeneration => "no_object_generation",
        }
    }

    /// Whether this class alone can satisfy a generation-consistent consumer.
    ///
    /// [`GitReadConsistency::LiveMutable`] returns `false` even though a
    /// consumer holding the [`LiveReadCapability`] may admit it: the class
    /// does not carry the authority, the capability does.
    pub fn is_generation_consistent(&self) -> bool {
        matches!(self, Self::ImmutableGeneration | Self::CapturedNotCurrent)
    }

    /// Whether the class describes bytes read from a concurrently mutable
    /// repository.
    pub fn is_live(&self) -> bool {
        matches!(self, Self::LiveMutable)
    }

    /// The weaker of two classes.
    ///
    /// A read that is assembled from several Git spawns is only as strong as
    /// its weakest part, so the composite class is computed with this
    /// function. The ordering is deliberately conservative:
    ///
    /// * `LiveMutable` is the weakest — the bytes may be torn.
    /// * `NoObjectGeneration` is next — that part of the read observed no
    ///   object generation at all, so the composite cannot claim one.
    /// * `CapturedNotCurrent` is next — one captured generation, possibly
    ///   stale.
    /// * `ImmutableGeneration` is the strongest.
    pub fn weaker(self, other: GitReadConsistency) -> GitReadConsistency {
        fn rank(class: GitReadConsistency) -> u8 {
            match class {
                GitReadConsistency::LiveMutable => 0,
                GitReadConsistency::NoObjectGeneration => 1,
                GitReadConsistency::CapturedNotCurrent => 2,
                GitReadConsistency::ImmutableGeneration => 3,
            }
        }
        if rank(self) <= rank(other) {
            self
        } else {
            other
        }
    }
}

impl fmt::Display for GitReadConsistency {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A Git read result together with the consistency class it was obtained
/// under.
///
/// The class travels with the value so that a consumer cannot receive a read
/// and silently forget what it was allowed to assume about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitReadFact<T> {
    value: T,
    class: GitReadConsistency,
}

impl<T> GitReadFact<T> {
    /// Pair a read result with the class it was obtained under.
    pub fn new(value: T, class: GitReadConsistency) -> Self {
        Self { value, class }
    }

    /// The read result.
    pub fn value(&self) -> &T {
        &self.value
    }

    /// Consume the fact and take the read result.
    pub fn into_value(self) -> T {
        self.value
    }

    /// The consistency class this read was obtained under.
    pub fn class(&self) -> GitReadConsistency {
        self.class
    }

    /// Map the read result, preserving the consistency class.
    pub fn map<U>(self, mapper: impl FnOnce(T) -> U) -> GitReadFact<U> {
        GitReadFact {
            value: mapper(self.value),
            class: self.class,
        }
    }
}

/// Why a read result was refused for a generation-consistent consumer.
///
/// This is a typed refusal with a stable machine name, so a caller can
/// distinguish "the read was live and you did not hold the capability" from
/// "the read never observed an object generation".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitConsistencyRefusal {
    /// A live-class result reached a generation-consistent consumer that did
    /// not hold the [`LiveReadCapability`].
    LiveClassWithoutCapability { class: GitReadConsistency },
    /// A read that observed no object generation reached a
    /// generation-consistent consumer.
    NoObjectGeneration,
}

impl GitConsistencyRefusal {
    /// A stable machine name for the refusal.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LiveClassWithoutCapability { .. } => "live_class_without_capability",
            Self::NoObjectGeneration => "no_object_generation",
        }
    }
}

impl fmt::Display for GitConsistencyRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LiveClassWithoutCapability { class } => write!(
                formatter,
                "live Git read ({class}) is inadmissible to a generation-consistent consumer \
                 without the live-read capability"
            ),
            Self::NoObjectGeneration => formatter.write_str(
                "Git read observed no object generation and cannot satisfy a \
                 generation-consistent consumer",
            ),
        }
    }
}

impl std::error::Error for GitConsistencyRefusal {}

/// A read result admitted for a consumer whose contract requires one stable
/// object generation.
///
/// The only way to obtain this type is [`admit_generation_consistent`], so a
/// caller cannot hand a generation-bound consumer a value whose class was
/// never checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationConsistent<T> {
    value: T,
    class: GitReadConsistency,
}

impl<T> GenerationConsistent<T> {
    /// The admitted read result.
    pub fn value(&self) -> &T {
        &self.value
    }

    /// Consume the admission and take the read result.
    pub fn into_value(self) -> T {
        self.value
    }

    /// The consistency class the read was admitted under.
    pub fn class(&self) -> GitReadConsistency {
        self.class
    }
}

/// The capability a consumer must hold to use a live-class Git read result.
///
/// This is an **authority**, not a label: on the same footing as
/// `DestructiveRepositoryChange` gates a destructive mutation. It exists so
/// that "may this consumer use a live read?" has a typed answer that is
/// checked at the boundary rather than remembered by convention.
///
/// The constructor is compiled only under the crate's test-only support
/// feature. A production build of `egggit` therefore has **no** way to
/// obtain the capability, and the generation-consistency fence cannot be
/// relaxed in the shipped binary. Obtaining it in a product build requires a
/// separate, explicitly accepted decision — the one ADR-0044 proposes and
/// has not been given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveReadCapability {
    _private: (),
}

impl LiveReadCapability {
    /// Mint the capability for the separately reviewed contained live-read
    /// route.
    ///
    /// Compiled only under the `git-live-read-test-support` feature.
    #[cfg(feature = "git-live-read-test-support")]
    pub fn mint_for_test_only_route() -> Self {
        Self { _private: () }
    }
}

/// Admit a read fact to a consumer whose contract requires one stable object
/// generation.
///
/// * [`GitReadConsistency::ImmutableGeneration`] and
///   [`GitReadConsistency::CapturedNotCurrent`] are admitted: both served
///   objects from one captured generation, and a generation-bound consumer
///   needs internal consistency of those bytes. Currency is a separate
///   concern, handled by compare-and-swap at the publication boundary.
/// * [`GitReadConsistency::LiveMutable`] is admitted **only** when the caller
///   holds the [`LiveReadCapability`]. Without it the read is refused, not
///   warned about.
/// * [`GitReadConsistency::NoObjectGeneration`] is refused: there is no
///   generation to be consistent with.
pub fn admit_generation_consistent<T>(
    fact: GitReadFact<T>,
    capability: Option<&LiveReadCapability>,
) -> Result<GenerationConsistent<T>, GitConsistencyRefusal> {
    let class = fact.class;
    match class {
        GitReadConsistency::ImmutableGeneration | GitReadConsistency::CapturedNotCurrent => {
            Ok(GenerationConsistent {
                value: fact.into_value(),
                class,
            })
        }
        GitReadConsistency::LiveMutable => match capability {
            Some(_) => Ok(GenerationConsistent {
                value: fact.into_value(),
                class,
            }),
            None => Err(GitConsistencyRefusal::LiveClassWithoutCapability { class }),
        },
        GitReadConsistency::NoObjectGeneration => Err(GitConsistencyRefusal::NoObjectGeneration),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_names_are_stable_and_distinct() {
        let names = [
            GitReadConsistency::ImmutableGeneration.as_str(),
            GitReadConsistency::CapturedNotCurrent.as_str(),
            GitReadConsistency::LiveMutable.as_str(),
            GitReadConsistency::NoObjectGeneration.as_str(),
        ];
        let mut unique = names.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), names.len(), "class names must be distinct");
        assert_eq!(names[0], "immutable_generation");
        assert_eq!(names[1], "captured_not_current");
        assert_eq!(names[2], "live_mutable");
        assert_eq!(names[3], "no_object_generation");
    }

    #[test]
    fn only_captured_classes_are_generation_consistent() {
        assert!(GitReadConsistency::ImmutableGeneration.is_generation_consistent());
        assert!(GitReadConsistency::CapturedNotCurrent.is_generation_consistent());
        assert!(!GitReadConsistency::LiveMutable.is_generation_consistent());
        assert!(!GitReadConsistency::NoObjectGeneration.is_generation_consistent());
    }

    #[test]
    fn a_live_read_is_refused_for_a_generation_consistent_consumer() {
        let fact = GitReadFact::new("payload", GitReadConsistency::LiveMutable);
        let refusal =
            admit_generation_consistent(fact, None).expect_err("a live read must be refused");
        assert_eq!(
            refusal,
            GitConsistencyRefusal::LiveClassWithoutCapability {
                class: GitReadConsistency::LiveMutable
            }
        );
        assert_eq!(refusal.as_str(), "live_class_without_capability");
    }

    #[test]
    fn a_read_without_an_object_generation_is_refused() {
        let fact = GitReadFact::new("payload", GitReadConsistency::NoObjectGeneration);
        let refusal = admit_generation_consistent(fact, None)
            .expect_err("a read with no generation must be refused");
        assert_eq!(refusal, GitConsistencyRefusal::NoObjectGeneration);
        assert_eq!(refusal.as_str(), "no_object_generation");
    }

    #[test]
    fn captured_classes_are_admitted() {
        for class in [
            GitReadConsistency::ImmutableGeneration,
            GitReadConsistency::CapturedNotCurrent,
        ] {
            let fact = GitReadFact::new(7u32, class);
            let admitted = admit_generation_consistent(fact, None)
                .expect("a captured generation must be admitted");
            assert_eq!(admitted.class(), class);
            assert_eq!(*admitted.value(), 7);
        }
    }

    #[test]
    fn mapping_preserves_the_class() {
        let fact = GitReadFact::new(2u32, GitReadConsistency::CapturedNotCurrent);
        let mapped = fact.map(|value| value * 3);
        assert_eq!(mapped.class(), GitReadConsistency::CapturedNotCurrent);
        assert_eq!(*mapped.value(), 6);
    }

    #[test]
    fn the_weaker_class_wins_for_a_composite_read() {
        use GitReadConsistency::*;
        assert_eq!(LiveMutable.weaker(ImmutableGeneration), LiveMutable);
        assert_eq!(ImmutableGeneration.weaker(LiveMutable), LiveMutable);
        assert_eq!(
            NoObjectGeneration.weaker(CapturedNotCurrent),
            NoObjectGeneration
        );
        assert_eq!(
            CapturedNotCurrent.weaker(ImmutableGeneration),
            CapturedNotCurrent
        );
        assert_eq!(
            ImmutableGeneration.weaker(ImmutableGeneration),
            ImmutableGeneration
        );
    }
}
