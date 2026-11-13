//! The TUI render invariants (I1–I6), executable in one place.
//!
//! `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md` §2 names
//! six presentation invariants; this module is where each one runs. Three
//! assert and pass today and live here as tests:
//!
//! * **I1** — nothing is painted outside the rect a widget was given
//!   ([`paint`]).
//! * **I2** — no panic and no blank frame at the smallest supported size
//!   ([`paint`]).
//! * **I3a** — a focused surface renders differently from an unfocused one
//!   ([`focus`]).
//!
//! Three cannot pass yet, because the defects they detect belong to later
//! milestones. Those are **measured** in [`baseline`] and recorded in
//! `docs/tui-render-invariant-baseline.toml`, which may shrink and may not
//! grow:
//!
//! * **I4** — every advertised key is handled (M006).
//! * **I5** — every rendered foreground clears `MIN_DERIVED_TEXT_RATIO`
//!   (M007).
//! * **I6** — a hint line never loses its exit key (M005).
//!
//! [`docked`] is M003's addition: a dialog adopted into a panel column must
//! leave the host's border, title and `●` alone, and a modal must never paint
//! a marker on its own frame.
//!
//! This module changes no production render path. It observes the existing
//! one through [`capture`].

pub mod baseline;
pub mod capture;
pub mod chrome;
pub mod docked;
pub mod fixtures;
pub mod focus;
pub mod paint;
pub mod vocabulary;
