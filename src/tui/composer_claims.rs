//! The composer's Normal-mode claim table.
//!
//! # The fallthrough rule
//!
//! In the main buffer's **Normal** mode the composer owns a closed set of
//! letters, and this module is the single, enumerable statement of which
//! letters it owns and what each one means. The app's global binding map
//! (`src/tui/input.rs`) is consulted only for letters the composer does *not*
//! claim.
//!
//! > A claimed letter is consumed by the composer even when the composer's
//! > verb is currently a no-op. It must not silently become an unrelated app
//! > action.
//!
//! A reserved claim still **owns** the key — the fallthrough rule is the whole
//! point — so a reserved `d` cannot quietly become `OpenDiff` while
//! delete-selection is pending. `i` is the app's **only** route back to
//! Insert mode, so a reserved `i` would strand the user in Normal mode.
//!
//! # Scope
//!
//! The table applies to the **main buffer's Normal mode** only — not the
//! side-panel composers, the chat box, the Workspace dashboard's filter, the
//! dialogs, or the project picker — and not Insert mode. The claim is live
//! only when the composer holds the keyboard (`App::composer_holds_keyboard`)
//! and the *resolved* mode is Normal (`App::composer_input_mode`), never when
//! the raw `UiState::input_mode` field says Normal.
//!
//! # The one conditional claim
//!
//! `j` and `k` are claimed only when the composer's **content does not fit
//! its box**. The measure is the **fitted** content — the wrapped row count
//! against the visible row count — and not the buffer's newline count.
//!
//! The fit is a **cached layout fact**. The gate runs at key dispatch, before
//! the render pass, so the numbers come from the last layout pass rather than
//! from a second computation — two computations of one fact are two answers
//! to it, and the gate and the box must not disagree. A composer that has
//! never been laid out has no measurement, and an unmeasured fit does not
//! hold the claim: the key falls through.
//!
//! # The static guard
//!
//! `scripts/check_tui_composer_key_authority.py` reads this file and the
//! binding maps and fails when the table's invariants break: a duplicate
//! letter, a claimed letter that can still reach an app-level action, a
//! re-spelled chord that is missing from a map, a re-spelled chord that is
//! itself a claimed letter, or a capital claim that is not reachable under
//! both key encodings. The table is the guard's input, so the table is the
//! source of truth — there is deliberately no second copy of it anywhere.

use crossterm::event::{KeyCode, KeyModifiers};

/// A verb the composer can own in Normal mode.
///
/// The verb names the *capability* a key will have. Whether the capability is
/// implemented yet is recorded per claim, in [`ComposerClaim::status`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerVerb {
    /// Helix `w` — move the caret to the start of the next word.
    WordForward,
    /// Helix `b` — move the caret to the start of the previous word.
    WordBack,
    /// Helix `e` — move the caret to the end of the current word.
    WordEnd,
    /// Helix `j` — move the caret down one line.
    LineDown,
    /// Helix `k` — move the caret up one line.
    LineUp,
    /// Helix `v` — toggle the composer's select mode.
    ///
    /// A **mode**, not an action: the first press arms a character selection
    /// at the caret and enters select mode, and the next press leaves the
    /// mode and drops the selection. While the mode is on, a movement grows
    /// the selection instead of dropping it.
    SelectChars,
    /// Helix `x` — select the caret's line.
    SelectLine,
    /// Helix `d` — delete the selection.
    DeleteSelection,
    /// Helix `p` — paste.
    Paste,
    /// Helix `u` — undo.
    Undo,
    /// Helix `U` — redo.
    Redo,
    /// Helix `i` — enter insert mode before the caret.
    InsertBefore,
    /// Helix `a` — enter insert mode after the caret.
    InsertAfter,
    /// Helix `o` — open a line below the caret and enter insert mode.
    OpenLineBelow,
}

impl ComposerVerb {
    /// The Helix spelling of this verb, for documentation and for the guard's
    /// messages. It is the same key the claim table binds.
    pub const fn helix_key(self) -> char {
        match self {
            Self::WordForward => 'w',
            Self::WordBack => 'b',
            Self::WordEnd => 'e',
            Self::LineDown => 'j',
            Self::LineUp => 'k',
            Self::SelectChars => 'v',
            Self::SelectLine => 'x',
            Self::DeleteSelection => 'd',
            Self::Paste => 'p',
            Self::Undo => 'u',
            Self::Redo => 'U',
            Self::InsertBefore => 'i',
            Self::InsertAfter => 'a',
            Self::OpenLineBelow => 'o',
        }
    }
}

/// Whether a claim's verb is implemented yet.
///
/// The distinction is load-bearing for the guard: a **reserved** claim is
/// still consumed, so it must not be allowed to reach an app-level action
/// either. The status is what lets the guard tell "claimed and not yet live"
/// apart from "not claimed", and it is what a later milestone flips when its
/// verb lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimStatus {
    /// The verb is implemented and runs when the claim is live.
    Live,
    /// The verb is not implemented yet. The key is still claimed and
    /// consumed; the verb simply does nothing.
    Reserved,
}

/// The key encodings a claim answers to.
///
/// Terminals disagree about how a shifted key is reported. The app binds some
/// capital letters twice — `Char('H')` with `SHIFT`, and `Char('H')` with
/// `NONE` — because some report a shifted key without the modifier
/// (`insert_panel_direction_bindings` in `src/tui/input.rs` is the
/// established convention). A claim follows the same convention, or the verb
/// works on one terminal and silently does nothing on another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimEncodings {
    /// The bare key: `Char(c)` with no modifier.
    ///
    /// A lowercase claim accepts this encoding only. The shifted spelling of
    /// a lowercase letter is a *different* character (`Char('D')`), and it is
    /// not claimed: the claim is on `d`, not on "d with shift".
    Bare,
    /// Both encodings of a capital letter: `Char(C)` with `SHIFT` and
    /// `Char(C)` with `NONE`.
    CapitalBoth,
}

impl ClaimEncodings {
    /// Whether a key event's modifiers are one of this claim's encodings.
    ///
    /// The *character* is matched separately, against the claim's key: a
    /// lowercase claim is on `d`, not on "d with shift", and the shifted
    /// spelling of a lowercase letter is a different character.
    pub fn matches(self, modifiers: KeyModifiers) -> bool {
        match self {
            Self::Bare => modifiers == KeyModifiers::NONE,
            Self::CapitalBoth => {
                modifiers == KeyModifiers::NONE || modifiers == KeyModifiers::SHIFT
            }
        }
    }
}

/// The composer's fit, as the layout pass measured it.
///
/// The condition is a **layout** fact: it compares the rows the content
/// occupies with the rows the box shows. Both numbers come from the *same*
/// layout pass, so the claim gate and the box cannot disagree about whether
/// the content fits.
///
/// The **unknown** state is not a formality. The gate runs at key dispatch,
/// before the render pass, so a composer that has never been laid out has no
/// measurement at all — and a claim that fires on a measurement nobody can
/// see is a claim that fires in a state the user cannot see. An unknown fit
/// therefore does **not** hold a conditional claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerFit {
    /// No layout pass has measured the composer yet.
    Unknown,
    /// The layout pass measured the content and the box.
    Measured {
        /// The number of visual rows the content occupies.
        row_count: usize,
        /// The number of text rows the box shows.
        visible_rows: usize,
    },
}

impl ComposerFit {
    /// Whether the composer's content spans more than one rendered row.
    ///
    /// The measure is the **rendered row count**: the wrapped rows the content
    /// occupies. One row is not multiline; two rows are, whether or not the
    /// box shows them all. The measure is rows, never the buffer's newline
    /// count — a single logical line that wraps onto four rows is multiline,
    /// and a three-line buffer that fits on one row is not.
    ///
    /// The **unknown** state is not multiline either: the gate runs before the
    /// render pass, so a composer that has never been laid out has no
    /// measurement, and a claim that fired on one would swallow the key in a
    /// state the user cannot see.
    pub fn is_multiline(self) -> bool {
        match self {
            Self::Unknown => false,
            Self::Measured { row_count, .. } => row_count > 1,
        }
    }
}

/// When a claim is live.
///
/// A claim is usually live unconditionally. The one conditional claim is
/// `j`/`k`, which the composer owns only while its **content spans more than
/// one row**; on a composer whose content is a single row, they fall through
/// to transcript scroll unchanged.
///
/// The condition is **explicit** on purpose. An implicit conditional is how a
/// leak reappears: a claim that quietly stops applying looks exactly like a
/// claim that was never made, and the letter leaks to the app action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimCondition {
    /// The claim is always live.
    Always,
    /// The claim is live only when the composer's content spans more than one
    /// rendered row — the wrapped row count exceeds one.
    ContentSpansMultipleRows,
}

impl ComposerVerb {
    /// The verb's human label — the words the help and the hint row use.
    ///
    /// The enum documents what each verb *means*; this is what it is
    /// *called*. One label per verb, in one place, so the help and the hint
    /// row cannot describe the same verb two ways.
    pub fn label(self) -> &'static str {
        match self {
            Self::WordForward => "Move to the start of the next word",
            Self::WordBack => "Move to the start of the previous word",
            Self::WordEnd => "Move to the end of the word",
            Self::LineDown => "Move down one line",
            Self::LineUp => "Move up one line",
            Self::SelectChars => "Toggle select mode; a motion then extends the selection",
            Self::SelectLine => "Select the line",
            Self::DeleteSelection => "Delete the selection",
            Self::Paste => "Paste",
            Self::Undo => "Undo the last edit",
            Self::Redo => "Redo the last edit",
            Self::InsertBefore => "Insert before the caret (enters insert mode)",
            Self::InsertAfter => "Insert after the caret (enters insert mode)",
            Self::OpenLineBelow => "Open a line below (enters insert mode)",
        }
    }
}

impl ClaimCondition {
    /// The condition in the words the help and the hint row use.
    ///
    /// `None` for an unconditional claim: the verb is available whenever the
    /// composer holds the keyboard.
    pub fn label(self) -> Option<&'static str> {
        match self {
            Self::Always => None,
            Self::ContentSpansMultipleRows => Some("composer content spans more than one row"),
        }
    }
}

/// One entry of the derived verb list: what the composer can do, rendered.
///
/// This is a **read-only view** over [`COMPOSER_CLAIMS`]. It adds no claim and
/// changes no table; it is the one place that says what the composer can do,
/// so the hint row and the help are two renderings of the same answer rather
/// than two answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComposerVerbEntry {
    /// The claimed key.
    pub key: char,
    /// The key's spelling, the way the help spells a key.
    pub key_text: &'static str,
    /// The verb's human label.
    pub label: &'static str,
    /// The condition the claim carries.
    ///
    /// The **enum**, not its label: the help needs the words
    /// ([`ClaimCondition::label`]) and the hint row needs the *verdict*
    /// ([`ClaimCondition::holds`]), and a label alone cannot answer "is this
    /// verb available right now?".
    pub condition: ClaimCondition,
}

/// The static spelling of one ASCII letter.
///
/// The help's rows carry `&'static str` keys while a claim's key is a `char`,
/// so something has to convert between the two. This is that something, and it
/// is a mechanical bijection: the table indexes the letters, so it cannot
/// spell a key differently from the character it stands for. A claim table
/// that grows a key outside the alphabet gets no spelling at all — the
/// derived list then has no entry for it, which the guard and the tests both
/// notice.
fn key_label(key: char) -> Option<&'static str> {
    const LETTERS: [&str; 52] = [
        "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r",
        "s", "t", "u", "v", "w", "x", "y", "z", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J",
        "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z",
    ];
    let index = match key {
        'a'..='z' => key as usize - 'a' as usize,
        'A'..='Z' => key as usize - 'A' as usize + 26,
        _ => return None,
    };
    LETTERS.get(index).copied()
}

/// The live claims, rendered: the derived verb list.
///
/// A **reserved** claim is not here. It owns its key — that is the
/// fallthrough rule — but there is no behaviour to describe, so advertising it
/// would teach a gesture that does nothing.
///
/// The derivation respects the claim's **condition**, so a consumer can decide
/// whether the verb is available *now*. That is what makes `j` and `k` correct
/// on a single-row composer: there they are the transcript's, and the row must
/// not claim them.
pub fn composer_verb_entries() -> Vec<ComposerVerbEntry> {
    COMPOSER_CLAIMS
        .iter()
        .filter(|claim| claim.status == ClaimStatus::Live)
        .filter_map(|claim| {
            Some(ComposerVerbEntry {
                key: claim.key,
                key_text: key_label(claim.key)?,
                label: claim.verb.label(),
                condition: claim.condition,
            })
        })
        .collect()
}

impl ClaimCondition {
    /// Whether the condition holds for a composer whose fit is `fit`.
    ///
    /// The fit is the **cached** measurement the layout pass took, not a
    /// measurement taken here: a second computation of the same fact is a
    /// second answer to it, and the gate and the box must not be able to
    /// disagree.
    pub fn holds(self, fit: ComposerFit) -> bool {
        match self {
            Self::Always => true,
            // An unknown fit does not hold. The composer has not been laid
            // out, so there is nothing to measure against, and a claim that
            // fired on an unknown fit would swallow the key in a state the
            // user cannot see.
            Self::ContentSpansMultipleRows => fit.is_multiline(),
        }
    }
}

/// One entry in the composer's claim table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComposerClaim {
    /// The claimed key.
    pub key: char,
    /// The composer verb this key maps to.
    pub verb: ComposerVerb,
    /// Whether the verb is implemented yet.
    pub status: ClaimStatus,
    /// The key encodings the claim answers to.
    pub encodings: ClaimEncodings,
    /// When the claim is live.
    pub condition: ClaimCondition,
}

impl ComposerClaim {
    /// The modifier sets this claim answers to.
    ///
    /// A lowercase claim answers to the bare key only. A capital claim
    /// answers to both, because terminals disagree about whether a shifted
    /// key carries the `SHIFT` modifier.
    pub fn claimed_keys_modifiers(&self) -> Vec<KeyModifiers> {
        match self.encodings {
            ClaimEncodings::Bare => vec![KeyModifiers::NONE],
            ClaimEncodings::CapitalBoth => vec![KeyModifiers::NONE, KeyModifiers::SHIFT],
        }
    }

    /// Whether the claim is live only under a condition.
    ///
    /// A conditional claim is the one place where a claimed letter may still
    /// be bound in the app's maps: the claim does not apply when the
    /// condition fails, and the key falls through.
    pub fn is_conditional(&self) -> bool {
        self.condition != ClaimCondition::Always
    }
}

/// The composer's claim table.
///
/// Closed by construction: every letter here has exactly one owner, and a
/// letter that is not here is owned by the app's binding map.
///
/// The three letters that used to belong to app-level actions — `x`
/// (stop/resume the turn), `d` (open diff) and `i` (focus prompt) — are here
/// because ADR-0042 gave them to the composer. The app-level capabilities
/// moved to chords that are not letters; the capabilities themselves remain.
/// `space` is **not** here: help keeps it, and the Helix copy gesture is the
/// second step of that chord rather than a composer letter. `g` is not here
/// either, so `GoToTop` keeps it. `h` and `l` are not here: the existing
/// actions already move the composer caret, which is exactly Helix `h` and
/// `l`.
///
/// That last sentence is a **premise about the app's binding maps**, and a
/// premise is only as good as the check on it. `h` and `l` were bound in the
/// vim map and in no other, and `vim_mode` defaults to false — so in the
/// default configuration the composer drew a caret that no letter could
/// move. The letters are now bound in both maps, and
/// `scripts/check_tui_composer_key_authority.py` rule G9 fails if the two
/// maps ever disagree about a caret motion again.
///
/// `j` and `k` are the one conditional pair. They are claimed only while the
/// composer's **content spans more than one row** — the wrapped row count
/// exceeds one. On a composer whose content is a single row they fall
/// through to transcript scroll, which is why their app-level binding
/// survives; and they come back on their own as soon as the content grows
/// back to more than one row.
///
/// ## Which claims are live
///
/// A claim is **live** when the composer owns the key *and* the verb does
/// something. A **reserved** claim still owns the key — that is the
/// fallthrough rule, and it is why a reserved `d` cannot quietly become
/// `OpenDiff` — but the verb itself is pending.
///
/// The live set grew in M004, when the motion verbs landed — `w`, `b`, `e`,
/// `j` and `k` joined `i` — and again in M005, when the selection verbs
/// landed: `v`, `x`, `d` and `p`. M006 closed the table: `u`, `U`, `a` and `o`
/// joined them, and every claim is live. The status column stays because the
/// distinction is what lets the guard tell "claimed and not yet live" apart
/// from "not claimed" — and the next milestone that adds a verb will use it
/// again.
pub const COMPOSER_CLAIMS: &[ComposerClaim] = &[
    ComposerClaim {
        key: 'a',
        verb: ComposerVerb::InsertAfter,
        // **Live**, and the reason the vim map's bare `a` is gone.
        //
        // ADR-0042 part 4 lists `a` among the letters the composer owns, but
        // its conflict table does not mention it — a gap in an accepted ADR,
        // recorded in the roadmap rather than papered over by editing it.
        status: ClaimStatus::Live,
        encodings: ClaimEncodings::Bare,
        condition: ClaimCondition::Always,
    },
    ComposerClaim {
        key: 'b',
        verb: ComposerVerb::WordBack,
        status: ClaimStatus::Live,
        encodings: ClaimEncodings::Bare,
        condition: ClaimCondition::Always,
    },
    ComposerClaim {
        key: 'd',
        verb: ComposerVerb::DeleteSelection,
        status: ClaimStatus::Live,
        encodings: ClaimEncodings::Bare,
        condition: ClaimCondition::Always,
    },
    ComposerClaim {
        key: 'e',
        verb: ComposerVerb::WordEnd,
        status: ClaimStatus::Live,
        encodings: ClaimEncodings::Bare,
        condition: ClaimCondition::Always,
    },
    ComposerClaim {
        key: 'i',
        verb: ComposerVerb::InsertBefore,
        // **Live**, and the app's only route back to Insert mode.
        //
        // `i` is the app's *only* route back to Insert mode: the Normal-mode
        // gesture that switches the composer from Normal to Insert is not a
        // binding at all — it is the `None` arm of the key chain, reached
        // when the key resolves to no action. Claiming `i` and leaving the
        // verb reserved would consume the key and do nothing, and the user
        // would press `Esc` and never be able to type again.
        //
        // The transition is not a new capability: Helix `i` is "insert before
        // the caret", the caret stays where it is, and the next character is
        // typed at it — which is exactly what the app's gesture did. The
        // claim moves the app's existing transition into the table, and
        // M006 refines it (insert-after, open-line-below, and the selection
        // interaction).
        status: ClaimStatus::Live,
        encodings: ClaimEncodings::Bare,
        condition: ClaimCondition::Always,
    },
    ComposerClaim {
        key: 'j',
        verb: ComposerVerb::LineDown,
        status: ClaimStatus::Live,
        encodings: ClaimEncodings::Bare,
        condition: ClaimCondition::ContentSpansMultipleRows,
    },
    ComposerClaim {
        key: 'k',
        verb: ComposerVerb::LineUp,
        status: ClaimStatus::Live,
        encodings: ClaimEncodings::Bare,
        condition: ClaimCondition::ContentSpansMultipleRows,
    },
    ComposerClaim {
        key: 'o',
        verb: ComposerVerb::OpenLineBelow,
        status: ClaimStatus::Live,
        encodings: ClaimEncodings::Bare,
        condition: ClaimCondition::Always,
    },
    ComposerClaim {
        key: 'p',
        verb: ComposerVerb::Paste,
        status: ClaimStatus::Live,
        encodings: ClaimEncodings::Bare,
        condition: ClaimCondition::Always,
    },
    ComposerClaim {
        key: 'u',
        verb: ComposerVerb::Undo,
        status: ClaimStatus::Live,
        encodings: ClaimEncodings::Bare,
        condition: ClaimCondition::Always,
    },
    ComposerClaim {
        key: 'U',
        verb: ComposerVerb::Redo,
        status: ClaimStatus::Live,
        encodings: ClaimEncodings::CapitalBoth,
        condition: ClaimCondition::Always,
    },
    ComposerClaim {
        key: 'v',
        verb: ComposerVerb::SelectChars,
        status: ClaimStatus::Live,
        encodings: ClaimEncodings::Bare,
        condition: ClaimCondition::Always,
    },
    ComposerClaim {
        key: 'w',
        verb: ComposerVerb::WordForward,
        status: ClaimStatus::Live,
        encodings: ClaimEncodings::Bare,
        condition: ClaimCondition::Always,
    },
    ComposerClaim {
        key: 'x',
        verb: ComposerVerb::SelectLine,
        status: ClaimStatus::Live,
        encodings: ClaimEncodings::Bare,
        condition: ClaimCondition::Always,
    },
];

/// The claim that owns a key event, if any.
///
/// This is the *lookup*, not the dispatch: it says which claim owns the key,
/// and the caller decides whether the claim is live (the composer must hold
/// the keyboard and the resolved mode must be Normal).
pub fn claim_for(modifiers: KeyModifiers, code: KeyCode) -> Option<&'static ComposerClaim> {
    let KeyCode::Char(c) = code else {
        return None;
    };
    COMPOSER_CLAIMS
        .iter()
        .find(|claim| claim.key == c && claim.encodings.matches(modifiers))
}

/// Whether the composer would claim this key event, given the composer's fit.
///
/// The caller owns the *surface* decision — whether the composer holds the
/// keyboard at all, and whether the resolved mode is Normal. This function
/// answers the narrower question: given that the composer is the target, does
/// this key belong to the composer?
pub fn claims_key(modifiers: KeyModifiers, code: KeyCode, fit: ComposerFit) -> bool {
    claim_for(modifiers, code).is_some_and(|claim| claim.condition.holds(fit))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn the_table_is_closed_and_single_owner() {
        let mut seen = BTreeSet::new();
        for claim in COMPOSER_CLAIMS {
            assert!(
                seen.insert(claim.key),
                "the letter {:?} is claimed twice",
                claim.key
            );
            assert!(
                claim.key.is_ascii_alphabetic(),
                "the claim {:?} is not a letter",
                claim.key
            );
        }
        assert_eq!(seen.len(), COMPOSER_CLAIMS.len());
    }

    #[test]
    fn every_claim_names_a_verb_and_the_verb_agrees_with_the_key() {
        for claim in COMPOSER_CLAIMS {
            assert_eq!(
                claim.verb.helix_key(),
                claim.key,
                "the claim for {:?} names a verb whose Helix key is {:?}",
                claim.key,
                claim.verb.helix_key()
            );
        }
    }

    #[test]
    fn a_capital_claim_answers_to_both_encodings() {
        let claim = COMPOSER_CLAIMS
            .iter()
            .find(|claim| claim.key == 'U')
            .expect("the table claims `U` for redo");
        assert_eq!(claim.encodings, ClaimEncodings::CapitalBoth);
        assert!(claim_for(KeyModifiers::NONE, KeyCode::Char('U')).is_some());
        assert!(claim_for(KeyModifiers::SHIFT, KeyCode::Char('U')).is_some());
    }

    #[test]
    fn a_lowercase_claim_answers_to_the_bare_encoding_only() {
        let claim = COMPOSER_CLAIMS
            .iter()
            .find(|claim| claim.key == 'd')
            .expect("the table claims `d`");
        assert_eq!(claim.encodings, ClaimEncodings::Bare);
        assert!(claim_for(KeyModifiers::NONE, KeyCode::Char('d')).is_some());
        assert!(claim_for(KeyModifiers::SHIFT, KeyCode::Char('d')).is_none());
        // The shifted spelling of a lowercase letter is a different character.
        assert!(claim_for(KeyModifiers::SHIFT, KeyCode::Char('D')).is_none());
    }

    #[test]
    fn a_modified_key_is_never_claimed() {
        for modifiers in [
            KeyModifiers::CONTROL,
            KeyModifiers::ALT,
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        ] {
            for claim in COMPOSER_CLAIMS {
                assert!(
                    claim_for(modifiers, KeyCode::Char(claim.key)).is_none(),
                    "{:?} must not be claimed under {modifiers:?}",
                    claim.key
                );
            }
        }
    }

    #[test]
    fn every_claim_is_live() {
        // M006 was the milestone that made the table whole: the motions
        // (M004), the selection verbs (M005), and the undo/redo and insert
        // transitions (M006) all landed, so no claim is left reserved.
        //
        // A reserved claim is not a harmless placeholder — it is a key the
        // composer swallows and does nothing with. The table having no
        // reserved row left is the point.
        let reserved: Vec<char> = COMPOSER_CLAIMS
            .iter()
            .filter(|claim| claim.status == ClaimStatus::Reserved)
            .map(|claim| claim.key)
            .collect();
        assert!(
            reserved.is_empty(),
            "these claims are still reserved: {reserved:?}"
        );
    }

    #[test]
    fn the_table_is_the_adrs_fixed_letter_set() {
        // ADR-0042 part 4 gives the composer a closed set of Normal-mode
        // letters. M006 completed the set with `a`; the table must not grow
        // past it, and must not quietly drop one.
        let keys: Vec<char> = COMPOSER_CLAIMS.iter().map(|claim| claim.key).collect();
        assert_eq!(
            keys,
            vec!['a', 'b', 'd', 'e', 'i', 'j', 'k', 'o', 'p', 'u', 'U', 'v', 'w', 'x']
        );
    }

    #[test]
    fn the_derived_list_has_one_entry_per_live_claim() {
        let entries = composer_verb_entries();
        let live: Vec<char> = COMPOSER_CLAIMS
            .iter()
            .filter(|claim| claim.status == ClaimStatus::Live)
            .map(|claim| claim.key)
            .collect();
        let derived: Vec<char> = entries.iter().map(|entry| entry.key).collect();
        assert_eq!(
            derived, live,
            "the derived list and the claim table disagree about which verbs are live"
        );
    }

    #[test]
    fn the_derived_list_carries_every_claims_condition() {
        for entry in composer_verb_entries() {
            let claim = COMPOSER_CLAIMS
                .iter()
                .find(|claim| claim.key == entry.key)
                .expect("a derived entry with no claim");
            assert_eq!(
                entry.condition, claim.condition,
                "the derived entry for {:?} lost its condition",
                entry.key
            );
        }
    }

    #[test]
    fn the_conditional_verbs_are_the_ones_the_condition_gates() {
        let conditional: Vec<(char, ClaimCondition)> = composer_verb_entries()
            .iter()
            .filter(|entry| entry.condition != ClaimCondition::Always)
            .map(|entry| (entry.key, entry.condition))
            .collect();
        assert_eq!(
            conditional,
            vec![
                ('j', ClaimCondition::ContentSpansMultipleRows),
                ('k', ClaimCondition::ContentSpansMultipleRows)
            ],
            "only the line motions are conditional; the rest are available whenever \
             the composer holds the keyboard"
        );
    }

    #[test]
    fn every_live_claim_has_a_label() {
        for entry in composer_verb_entries() {
            assert!(
                !entry.label.is_empty(),
                "the verb for {:?} has no label; the help could not describe it",
                entry.key
            );
        }
    }

    /// A composer holding one rendered row is not multiline.
    ///
    /// The measure is the **rendered row count**: a composer whose content
    /// occupies a single row has no row below the caret for a vertical motion
    /// to reach, so the transcript keeps the key.
    #[test]
    fn a_composer_with_one_row_is_not_multiline() {
        let one_row = ComposerFit::Measured {
            row_count: 1,
            visible_rows: 3,
        };

        assert!(
            !one_row.is_multiline(),
            "a composer whose content occupies one row is not multiline"
        );
        assert!(
            !ClaimCondition::ContentSpansMultipleRows.holds(one_row),
            "the multiline condition must not hold for a one-row composer"
        );
    }

    /// Several rendered rows are multiline — whether or not the box shows them.
    ///
    /// The fitting case is the one the old condition got wrong: three rows in
    /// a three-row box **fits**, and it is still multiline, so the vertical
    /// keys belong to the composer. The overflowing case is the one the
    /// 2026-10-23 amendment was recorded for: a composer whose content does
    /// not fit is necessarily multiline, so it keeps them too.
    #[test]
    fn a_composer_with_several_rows_is_multiline() {
        let fits = ComposerFit::Measured {
            row_count: 3,
            visible_rows: 3,
        };
        let overflows = ComposerFit::Measured {
            row_count: 4,
            visible_rows: 3,
        };

        assert!(
            fits.is_multiline(),
            "three rows in a three-row box fits, and it is still multiline"
        );
        assert!(
            overflows.is_multiline(),
            "four rows in a three-row box does not fit, and it is multiline too"
        );
        assert!(
            ClaimCondition::ContentSpansMultipleRows.holds(fits),
            "the multiline condition must hold for a composer whose content fits"
        );
        assert!(
            ClaimCondition::ContentSpansMultipleRows.holds(overflows),
            "the multiline condition must hold for a composer whose content overflows"
        );
    }

    /// The multiline condition gates the line motions and nothing else.
    ///
    /// The claim set is unchanged — the same letters, the same spellings. What
    /// the condition changed is the **boundary** at which `j` and `k` become
    /// the composer's.
    #[test]
    fn the_multiline_condition_gates_j_and_k_only() {
        let multiline = ComposerFit::Measured {
            row_count: 3,
            visible_rows: 3,
        };
        for claim in COMPOSER_CLAIMS {
            if claim.condition == ClaimCondition::ContentSpansMultipleRows {
                assert!(
                    claim.condition.holds(multiline),
                    "{:?} must be live on a multiline composer",
                    claim.key
                );
            } else {
                assert!(
                    claim.condition.holds(ComposerFit::Unknown),
                    "{:?} must be live whatever the content's fit",
                    claim.key
                );
            }
        }
    }

    /// An unmeasured fit holds nothing.
    ///
    /// The gate runs before the render pass, so a composer that has never been
    /// laid out has no measurement. A claim that fired on an unknown fit would
    /// swallow the key in a state the user cannot see — the fall-through is
    /// the safe answer, because the transcript's scroll is a visible action and
    /// a swallowed key is not.
    #[test]
    fn an_unknown_fit_does_not_hold_the_condition() {
        assert!(
            !ClaimCondition::ContentSpansMultipleRows.holds(ComposerFit::Unknown),
            "an unmeasured composer must not claim the key"
        );
        assert!(
            ClaimCondition::Always.holds(ComposerFit::Unknown),
            "an unconditional claim does not depend on the fit"
        );
    }

    /// The label names the multiline rule, in the words the published tables
    /// use.
    ///
    /// The label is the **shared vocabulary** between the implementation and
    /// the published keymaps: `scripts/check_tui_composer_keymap_drift.py`
    /// reads it out of this function and requires the published surfaces to
    /// carry it verbatim.
    #[test]
    fn the_condition_label_names_the_multiline_rule() {
        assert_eq!(
            ClaimCondition::ContentSpansMultipleRows.label(),
            Some("composer content spans more than one row"),
            "the label is the user-facing statement of the condition"
        );
        assert_eq!(
            ClaimCondition::Always.label(),
            None,
            "an unconditional claim has no condition to state"
        );
    }
}
