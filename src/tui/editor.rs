//! M006-A: TUI editor presentation model.
//!
//! This module owns **frontend presentation state only**: cursor, anchor,
//! viewport, buffer mode, and a bounded undo/redo history. It holds **no
//! document text**.
//!
//! Every offset recorded here is a position into the replica owned by
//! [`DocumentController`]. Text is read on demand through
//! `DocumentController::try_snapshot()` and dropped at the end of the
//! call that needed it; nothing in this module, in
//! [`TuiDocumentPresentation`](super::document_session::TuiDocumentPresentation),
//! or in [`EditorState`](super::app::state::EditorState) may store it.
//!
//! `scripts/check_tui_editor_text_authority.py` enforces that statically.
//!
//! ## One keyboard axis
//!
//! Which region of the editor route owns keyboard input is **not** a question
//! this module answers, and that is the fix.
//!
//! This used to be a second axis beside [`EditorMode`], and the two could
//! disagree: opening a file defaulted to `Composer`, which made
//! `handle_editor_key` claim nothing except `Ctrl+E` while the bare-text gate
//! swallowed every printable character. The file rendered and no key reached
//! it, which read as "the editor does not work" rather than as two
//! independent toggles.
//!
//! One axis now answers it, and it is the one every other surface already
//! shows as `NOR`/`INS`: [`crate::tui::input::InputMode`]. Insert puts the
//! keyboard in the document; Normal leaves the Helix verbs the buffer and
//! hands everything else to the bottom input box — the same contract
//! `/terminal` already runs on. See `commands::editor::handle_editor_key`.
//!
//! ## Hard wrap
//!
//! M006-A ships hard wrap with horizontal scrolling. One logical line maps
//! to exactly one screen row, which is what makes the line-number gutter,
//! the current-line highlight, and cursor arithmetic deterministic. Soft
//! wrap changes the line-to-row mapping and is deferred; see the M006-A
//! closure record.

use codegg_document::{BytePosition, DocumentSnapshot, TextEdit, TextTransaction};
use codegg_protocol::document::MAX_DOCUMENT_EDITS;
use codegg_protocol::lsp::LspReadOperation;

use super::document_session::TuiDocumentPresentation;
use crate::tui::editor_effect::EditorPendingEffect;
use crate::tui::editor_lsp::EditorDiagnosticStep;
use crate::tui::editor_match;
use crate::tui::helix_selection::{Selection, SelectionMode, SelectionState};

/// Maximum retained undo/redo entries per document attachment.
pub const MAX_EDITOR_UNDO_DEPTH: usize = 256;

/// Maximum retained undo/redo payload bytes per document attachment.
///
/// Bounds the inverse transactions a long editing session can retain. Old
/// entries are dropped from the bottom of the stack first, so the most
/// recent history is always the history that survives.
pub const MAX_EDITOR_UNDO_BYTES: usize = 4 * 1024 * 1024;

/// Maximum width of the pending-prefix indicator, in bytes.
///
/// This bounds what the editor can *put on screen*, not what it can hold.
/// [`EditorPrefix`] is a single enum value, so the state machine cannot grow
/// past one variant however many leaves are registered against it; the only
/// thing left to bound is the rendered text. Every
/// [`EditorPrefix::display`] is asserted to fit by
/// `every_pending_prefix_display_fits_the_indicator_bound`, so adding a
/// grammar that needs a longer label fails a test instead of silently
/// truncating into an indistinguishable blank.
pub const MAX_EDITOR_PENDING_PREFIX: usize = 5;

/// The multi-key grammar the editor is waiting on.
///
/// **Grammar rule: one pending prefix, one terminal key consumed.** A key
/// arriving while a prefix is armed either matches a leaf or disarms it;
/// nothing consumes a second key, and no prefix is ever armed on top of
/// another. The find prefixes keep the rule but change what the terminal
/// key *is*: `f` takes the next character as its target rather than as a
/// leaf, which is still exactly one key and still always disarms. A prefix
/// is also disarmed outright whenever the thing it was waiting for stops
/// being the thing on screen — opening a different document, leaving the
/// editor route, moving focus to the tree pane, or a reconciliation
/// rewriting the text under it. A prefix that outlives its target is a
/// gesture applied to the wrong buffer, which is the "relative path
/// resolved against the wrong root" defect in gesture form.
///
/// Adding a grammar means adding a variant here and its leaf in
/// `commands::editor::resolve_pending_prefix`, never a new field on the
/// presentation. That is the whole point of the type: `pending_command` and
/// `yank_prefix_pending` were one flag per feature, which is why Helix's
/// `[`/`]` grammars each wanted another one.
///
/// This module is the presentation model and deliberately knows nothing about
/// key events, so a leaf is named by the glyph it completes rather than by a
/// `KeyCode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorPrefix {
    /// Nothing pending: every key resolves against the keymap.
    #[default]
    Idle,
    /// Helix goto mode, armed by `g`. Leaves: [`GOTO_LEAVES`].
    Goto,
    /// Helix space mode, armed by `space`. Leaves: [`SPACE_LEAVES`].
    Space,
    /// Helix's unimpaired `[` prefix. Leaves: [`OPEN_BRACKET_LEAVES`].
    OpenBracket,
    /// Helix's unimpaired `]` prefix. Leaves: [`CLOSE_BRACKET_LEAVES`].
    CloseBracket,
    /// Helix `f` — waiting for the character to find.
    FindNext,
    /// Helix `F` — waiting for the character to find.
    FindPrevious,
    /// Helix `t` — waiting for the character to find.
    FindTillNext,
    /// Helix `T` — waiting for the character to find.
    FindTillPrevious,
    /// Helix `r` — waiting for the character to replace with.
    ///
    /// The second capture grammar, and it is here for exactly the reason the
    /// four find prefixes are: `r` has no single-key spelling, and the key it
    /// takes next is **data**. See [`Self::is_capture`] and [`Self::leaves`].
    ReplaceChar,
    /// Helix `m` — the match-mode prefix. Leaves: [`MATCH_LEAVES`].
    ///
    /// The first level of a **two-level** grammar. Helix's `m` takes a
    /// command (`m m`, `m s`, `m a`) and then, for most of those commands, an
    /// argument (`m s (`, `m a w`). One field still carries that: the second
    /// level is [`Self::MatchArg`], whose payload says which half of the
    /// gesture is pending.
    Match,
    /// Helix's `m` **argument** — the second level of the `m` grammar.
    ///
    /// A **capture**: the next key is the argument as *data*, not a command.
    /// The payload rides in the variant, so the presentation still has
    /// exactly one prefix field and the prefix-state guard stays green.
    ///
    /// [`MatchArg::SurroundReplaceTo`] is the one variant that carries a
    /// character, and it is there because `m r` takes **two**: the first
    /// character is held while the second is waited for, and a single
    /// `EditorPrefix` value has to be able to say which half is pending.
    MatchArg(MatchArg),
}

/// What the `m` grammar is waiting for.
///
/// The second level of Helix's two-level `m`. Every variant except
/// [`Self::SurroundReplaceTo`] is waiting for its **first** character; the
/// replace is the one gesture that takes two, and its second half carries the
/// first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchArg {
    /// `m s` — surround the selection with the captured character.
    SurroundAdd,
    /// `m d` — remove the captured character's pair around the selection.
    SurroundDelete,
    /// `m r` — waiting for the **first** character of the replacement pair.
    ///
    /// On capture it arms [`Self::SurroundReplaceTo`], which takes the
    /// second.
    SurroundReplaceFrom,
    /// `m r` — the first character is captured; waiting for the second.
    SurroundReplaceTo(char),
    /// `m a` — select **around** the captured object.
    AroundObject,
    /// `m i` — select **inside** the captured object.
    InnerObject,
}

impl MatchArg {
    /// The command this argument belongs to, as the `m`-grammar's second
    /// level names it.
    ///
    /// The label the pending indicator shows, and the only thing that
    /// distinguishes the two halves of `m r` from each other: both are "the
    /// replace argument", and only the payload says which half is pending.
    pub fn command(self) -> &'static str {
        match self {
            MatchArg::SurroundAdd => "m s",
            MatchArg::SurroundDelete => "m d",
            MatchArg::SurroundReplaceFrom | MatchArg::SurroundReplaceTo(_) => "m r",
            MatchArg::AroundObject => "m a",
            MatchArg::InnerObject => "m i",
        }
    }

    /// Whether this argument is drawn from a **fixed vocabulary** rather than
    /// from any character.
    ///
    /// The surround arguments take any character — Helix's surround accepts
    /// one, and the quotes need a character that is both an opening and a
    /// closing delimiter. The object arguments take one of five, and a
    /// character outside that set is not a slightly-wrong object, it is not an
    /// object at all.
    pub fn is_restricted(self) -> bool {
        matches!(self, MatchArg::AroundObject | MatchArg::InnerObject)
    }
}

impl EditorPrefix {
    /// The find motion this prefix is waiting to complete, when it is one.
    ///
    /// These four are the one place the grammar takes its next key as
    /// **data** rather than as a leaf glyph: after `f`, the character typed
    /// is the target, not a command. That is a categorically different job
    /// from `gg`'s second `g`, which is a command — and it is why
    /// [`Self::leaves`] is empty for all of them.
    /// [`find_target_char`][target] does the capture; the machine still owns
    /// the disarm.
    ///
    /// [target]: crate::tui::commands::editor::find_target_char
    pub fn find_kind(self) -> Option<FindKind> {
        match self {
            EditorPrefix::FindNext => Some(FindKind::Next),
            EditorPrefix::FindPrevious => Some(FindKind::Previous),
            EditorPrefix::FindTillNext => Some(FindKind::TillNext),
            EditorPrefix::FindTillPrevious => Some(FindKind::TillPrevious),
            _ => None,
        }
    }

    /// Whether this prefix waits for a target character rather than a leaf.
    pub fn is_find(self) -> bool {
        self.find_kind().is_some()
    }

    /// Whether this prefix waits for a character to be *supplied*.
    ///
    /// `f`/`t`/`F`/`T` take a character to search for and `r` takes a
    /// character to write. Both take their next key as **data**, which is a
    /// categorically different job from `gg`'s second `g` — that one is a
    /// command, and a lookup in the leaf table is the right way to resolve
    /// it. A capture has no leaves at all, so [`Self::leaves`] is empty for
    /// every variant this covers.
    ///
    /// One answer rather than two (`is_find` plus a `== ReplaceChar` test at
    /// each call site) so a future capture grammar cannot be added to the
    /// dispatch without also being added here — which is what would leave it
    /// resolving an armed prefix down the leaf lookup and out to the composer.
    pub fn is_capture(self) -> bool {
        self.is_find() || self == EditorPrefix::ReplaceChar || self.match_arg().is_some()
    }

    /// The `m` argument this prefix is waiting for, when it is one.
    ///
    /// The classifier for the `m` grammar's second level, beside
    /// [`Self::find_kind`] for the find grammars' first level. One answer
    /// rather than a `==` test at each call site, so a third argument grammar
    /// cannot arrive without being added here — which is what would leave it
    /// resolving an armed prefix down the leaf lookup and out to the
    /// composer.
    pub fn match_arg(self) -> Option<MatchArg> {
        match self {
            EditorPrefix::MatchArg(arg) => Some(arg),
            _ => None,
        }
    }

    /// Whether this prefix is waiting for the character `r` replaces with.
    pub fn is_replace_char(self) -> bool {
        self == EditorPrefix::ReplaceChar
    }

    /// The prefix `glyph` arms, when `glyph` is one of the find keys.
    ///
    /// The one table mapping `f`/`t`/`F`/`T` onto their kinds, so an arm
    /// cannot arm `FindPrevious` for `T`.
    pub fn armed_by(glyph: char) -> Option<Self> {
        match glyph {
            'f' => Some(EditorPrefix::FindNext),
            'F' => Some(EditorPrefix::FindPrevious),
            't' => Some(EditorPrefix::FindTillNext),
            'T' => Some(EditorPrefix::FindTillPrevious),
            _ => None,
        }
    }

    /// Whether a key is currently being waited on.
    pub fn is_pending(self) -> bool {
        self != EditorPrefix::Idle
    }

    /// What the editor shows for an armed prefix, or `""` when idle.
    ///
    /// This is the whole grammar's rendering surface, so it is a closed set
    /// rather than accumulated text. `space` renders as the word rather than
    /// the space character it is bound to: a literal space would put nothing
    /// on screen, and a pending prefix the user cannot see is a prefix they
    /// cannot predict. The find prefixes show their key for the same reason
    /// — an armed `f` that drew nothing would be indistinguishable from a
    /// dropped keypress.
    ///
    /// **One label is not a closed set.** The `m r` gesture takes two
    /// characters, and the second half has to say which half it is: the
    /// indicator shows the character already captured, so `m r (` reads
    /// `"m r ("` rather than the same `"m r"` the first half shows. A reader
    /// who cannot tell the halves apart cannot predict what the next key
    /// does, and a pending prefix they cannot predict is a prefix they will
    /// get wrong. The captured character is a single glyph, not accumulated
    /// text — the label is still bounded by
    /// [`MAX_EDITOR_PENDING_PREFIX`].
    pub fn display(self) -> std::borrow::Cow<'static, str> {
        match self {
            EditorPrefix::Idle => "".into(),
            EditorPrefix::Goto => "g".into(),
            EditorPrefix::Space => "space".into(),
            EditorPrefix::OpenBracket => "[".into(),
            EditorPrefix::CloseBracket => "]".into(),
            EditorPrefix::FindNext => "f".into(),
            EditorPrefix::FindPrevious => "F".into(),
            EditorPrefix::FindTillNext => "t".into(),
            EditorPrefix::FindTillPrevious => "T".into(),
            EditorPrefix::ReplaceChar => "r".into(),
            EditorPrefix::Match => "m".into(),
            EditorPrefix::MatchArg(MatchArg::SurroundReplaceTo(from)) => {
                format!("{} {}", MatchArg::SurroundReplaceFrom.command(), from).into()
            }
            EditorPrefix::MatchArg(arg) => arg.command().into(),
        }
    }

    /// Every glyph that completes this prefix, with what it runs.
    ///
    /// A **table**, not a single glyph. `leaf()` used to return
    /// `Option<char>` and it worked only because goto mode had exactly one
    /// leaf (`gg`) and space mode exactly one (`space y`). Helix's goto mode
    /// has twenty, and space mode has thirty; expressing those as one `Some`
    /// per prefix would mean either a special case per prefix or a second
    /// mechanism beside this one — which is the regression M002 deleted.
    ///
    /// The table is the whole answer to "what does this prefix accept", so a
    /// new grammar grows a row here rather than an arm in the dispatch layer.
    /// [`commands::editor::resolve_pending_prefix`] looks the glyph up and runs
    /// the action; an unlisted glyph takes the mismatch path, which
    /// [`Self::swallows_unmatched`] decides.
    ///
    /// The find prefixes return an empty slice on purpose: their next key is a
    /// *target character*, not a leaf, so there is no glyph that could
    /// complete them. `r` is empty for the same reason — it takes the
    /// character to write, not a key that means "replace".
    ///
    /// Every empty row above is a **capture** ([`Self::is_capture`]); a prefix
    /// with leaves is one the dispatch resolves by glyph lookup. That claim
    /// used to be false of `[`/`]` — they sat in this arm too, unarmed, until
    /// M006 gave them [`OPEN_BRACKET_LEAVES`] and [`CLOSE_BRACKET_LEAVES`].
    pub fn leaves(self) -> &'static [PrefixLeaf] {
        match self {
            EditorPrefix::Goto => GOTO_LEAVES,
            EditorPrefix::Space => SPACE_LEAVES,
            EditorPrefix::OpenBracket => OPEN_BRACKET_LEAVES,
            EditorPrefix::CloseBracket => CLOSE_BRACKET_LEAVES,
            EditorPrefix::Match => MATCH_LEAVES,
            EditorPrefix::Idle
            | EditorPrefix::FindNext
            | EditorPrefix::FindPrevious
            | EditorPrefix::FindTillNext
            | EditorPrefix::FindTillPrevious
            | EditorPrefix::ReplaceChar
            | EditorPrefix::MatchArg(_) => &[],
        }
    }

    /// Whether an unmatched key is consumed with the prefix or resolved next.
    ///
    /// `Goto` swallows, because that is the baseline's `gg` behavior and
    /// Helix's: the second key belongs to the chord whether or not it
    /// completes it. The unimpaired prefixes retry, because swallowing there
    /// would mean a `[` followed by a letter lost that letter on its way to
    /// the composer — and their leaves are still two milestones away, so a
    /// prefix that cannot be escaped is worse than one that occasionally
    /// passes a key through.
    ///
    /// The find prefixes retry too, and for the sharper reason: their next
    /// key is a target character, so there is no such thing as a key that
    /// "matches". A key that cannot be a target — an arrow, `Esc`, one of
    /// the two modal openers — is handed back for normal resolution rather
    /// than eaten, which is what keeps `f` from swallowing a keystroke on
    /// its way to the composer.
    ///
    /// `r` retries for that same reason and no other one: `r` then `Left`
    /// moves the cursor one character left rather than replacing nothing and
    /// eating the arrow.
    ///
    /// `m` retries too, and for the sharper reason still: Helix swallows an
    /// unrecognised key after `m`, but the editor-wide rule is that a prefix
    /// which does not recognise the next key returns it to the composer
    /// rather than eating it. A swallowed `m z` would type nothing at all,
    /// which is the M002 failure mode this milestone exists to end.
    pub fn swallows_unmatched(self) -> bool {
        self == EditorPrefix::Goto
    }
}

/// What one completed prefix gesture does.
///
/// The distinction is not decoration: a motion resolves against the document
/// and returns nothing the App layer has to act on, while a yank has to hand a
/// byte range back out to the owner of the clipboard. Collapsing them into
/// "the leaf is an [`EditorCommand`]" would have meant inventing a command
/// whose resolution is a side effect, which is the same reason the buffer
/// reports [`BufferKey::YankRange`] rather than calling the clipboard itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefixAction {
    /// Run this through the shared Helix motion path.
    Motion(EditorCommand),
    /// Report the held selection's byte range; the App layer owns the copy.
    YankSelection,
    /// Run this through the shared **edit** path — the same one `x`, `d` and
    /// `y` use.
    ///
    /// A third arm rather than a bool on [`Self::Motion`], because what a leaf
    /// *is* decides whether it can move the cursor at all. `]Space` inserts a
    /// newline: putting it in the motion arm would have made `EditorCommand`'s
    /// name a lie and routed it into `editor_motion`, which is the one place
    /// in the dispatch that promises never to touch text.
    Edit(EditorCommand),
    /// Ask the editor surface for something, or open the diagnostics panel.
    ///
    /// A fourth arm rather than an `EditorCommand` because an effect leaf
    /// moves no cursor and edits no text: it is a request, and putting it in
    /// the motion or edit arm would have made a leaf that cannot change the
    /// buffer look like one that does.
    ///
    /// The effect is *named* here, not performed. Resolving a leaf has only
    /// `&mut EditorState`; a read needs the App's core client. See
    /// [`EditorPendingEffect`].
    Effect(EditorPendingEffect),
    /// Arm this **capture** — the second level of a two-level grammar.
    ///
    /// The `m` grammar's commands take their argument as data, exactly as
    /// `f`/`t`/`F`/`T`/`r` do, so the leaf that completes `m s` does not run a
    /// verb: it says *which argument is being waited for*, and the argument
    /// itself is the next key.
    ///
    /// A fifth arm rather than a bool on [`Self::Motion`], because the
    /// difference is a whole second level: the motion arm promises to move
    /// the cursor, and arming a capture moves nothing.
    ///
    /// The payload is a `Copy` enum, so this arm does **not** cost
    /// [`PrefixAction`] its `Copy` — which is what keeps the leaf tables
    /// `const`, and every future leaf payload const-constructible.
    Capture(MatchArg),
}

/// One row of a prefix's leaf table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrefixLeaf {
    /// The unmodified glyph that completes the prefix.
    ///
    /// Unmodified because terminals disagree about how capitals are encoded,
    /// and the dispatch layer already owns that reconciliation: it accepts a
    /// capital as either `Char('G')` + `SHIFT` or `Char('G')` + `NONE`. This
    /// module knows nothing about key events.
    pub glyph: char,
    /// What the completed gesture does.
    pub action: PrefixAction,
}

/// Goto mode's leaves, against `https://docs.helix-editor.com/keymap.html`.
///
/// Only the leaves this editor can *honour* are listed. Helix binds twenty;
/// the omissions are deliberate and each is a capability this surface does
/// not have, not an oversight:
///
/// - `gf`/`ga`/`gm`/`gn`/`gp`/`.` — files and alternate buffers. Multi-document
///   attachment is out of this roadmap's scope.
/// - `gt`/`gc`/`gb` — top/middle/bottom **of the screen**. Excluded because
///   this editor resolves motions against [`NOMINAL_VISIBLE_LINES`], a fixed
///   guess rather than the real viewport, so "the top of the screen" would be a
///   line that is not the top of the screen. Binding them now would ship a key
///   that lands one place and looks broken; the honest fix is to thread the
///   real viewport through first.
/// - `g|` — goto column, which needs the same count entry point as Helix's
///   `G <n>`; this editor's `G` takes no count today.
///
/// `ge` and the bare `G` both land on the last line here, which is Helix's
/// own outcome too: with no count argument `G` is `goto_line` to the end of
/// the file, exactly what `ge` is.
pub const GOTO_LEAVES: &[PrefixLeaf] = &[
    PrefixLeaf {
        glyph: 'g',
        action: PrefixAction::Motion(EditorCommand::BufferStart),
    },
    // `gs` — the leaf this milestone exists for. There is no other key that
    // reaches first non-whitespace: `0` is line start and `$` is line end, so
    // on an indented file every placement went through column zero.
    PrefixLeaf {
        glyph: 's',
        action: PrefixAction::Motion(EditorCommand::FirstNonWhitespace),
    },
    PrefixLeaf {
        glyph: 'e',
        action: PrefixAction::Motion(EditorCommand::BufferEnd),
    },
    PrefixLeaf {
        glyph: 'h',
        action: PrefixAction::Motion(EditorCommand::LineStart),
    },
    PrefixLeaf {
        glyph: 'l',
        action: PrefixAction::Motion(EditorCommand::LineEnd),
    },
    // `gj`/`gk` are Helix's *textual* line motions, against `j`/`k`'s
    // visual ones. This editor renders one screen row per logical line and
    // has no soft-wrap display-line model, so the two are the same move —
    // and the leaves deliberately resolve to the same commands rather than
    // to a near-duplicate that could later drift apart.
    PrefixLeaf {
        glyph: 'j',
        action: PrefixAction::Motion(EditorCommand::MoveDown),
    },
    PrefixLeaf {
        glyph: 'k',
        action: PrefixAction::Motion(EditorCommand::MoveUp),
    },
    // `gd`/`gi`/`gr` — LSP definition/implementation/references. M013 owns
    // these; they were previously a silent `Swallowed` no-op.
    PrefixLeaf {
        glyph: 'd',
        action: PrefixAction::Effect(EditorPendingEffect::Read(LspReadOperation::Definition)),
    },
    PrefixLeaf {
        glyph: 'i',
        action: PrefixAction::Effect(EditorPendingEffect::Read(LspReadOperation::Implementation)),
    },
    PrefixLeaf {
        glyph: 'r',
        action: PrefixAction::Effect(EditorPendingEffect::Read(LspReadOperation::References)),
    },
    // `gy` — type definition. Bound to an explanation rather than left
    // unbound, because `egg lsp` does not implement `textDocument/
    // typeDefinition` and offering the operation in the protocol would be a
    // promise the daemon cannot keep. An unbound `gy` would be
    // indistinguishable from a key that never arrived; this one says why.
    PrefixLeaf {
        glyph: 'y',
        action: PrefixAction::Effect(EditorPendingEffect::Unavailable(
            "go to type definition: no language server implements it in this build",
        )),
    },
];

/// Space mode's leaves, against `https://docs.helix-editor.com/keymap.html`.
///
/// Helix binds about twenty; the omissions are each a capability this surface
/// does not have:
///
/// - `space r` (rename) and `space a` (code action) — the **preview** halves
///   are on the read surface (`LspReadOperation::RenamePreview`), but applying
///   either is an LSP *write*, and write authorization for an LSP-driven
///   mutation has no accepted ADR. Binding them to something that cannot
///   apply would be worse than absent.
/// - `space D` / `[D` — workspace diagnostics. The daemon publishes
///   per-project diagnostics, but there is no server capability behind a
///   separate "workspace diagnostics" request, so this would be a second name
///   for the panel `space d` already opens.
/// - `space w` / `space W` — window/workspace switching, out of scope.
///
/// `]`/`[` diagnostic stepping is already bound as bare keys, so the
/// navigation half of Helix's diagnostics pair needed no new row.
pub const SPACE_LEAVES: &[PrefixLeaf] = &[
    PrefixLeaf {
        glyph: 'y',
        action: PrefixAction::YankSelection,
    },
    PrefixLeaf {
        glyph: 'k',
        action: PrefixAction::Effect(EditorPendingEffect::Read(LspReadOperation::Hover)),
    },
    PrefixLeaf {
        glyph: 's',
        action: PrefixAction::Effect(EditorPendingEffect::Read(LspReadOperation::DocumentSymbols)),
    },
    // Workspace symbols carry the token under the cursor, exactly as F7 does;
    // a read with no query is not a request the daemon can answer.
    PrefixLeaf {
        glyph: 'S',
        action: PrefixAction::Effect(EditorPendingEffect::Read(
            LspReadOperation::WorkspaceSymbols,
        )),
    },
    PrefixLeaf {
        glyph: 'd',
        action: PrefixAction::Effect(EditorPendingEffect::OpenDiagnostics),
    },
    // Helix's "select references" highlights them in the buffer. There is no
    // highlight-overlay authority on the read surface yet, so this is the
    // references read — the part of the gesture that can be honoured.
    PrefixLeaf {
        glyph: 'h',
        action: PrefixAction::Effect(EditorPendingEffect::Read(LspReadOperation::References)),
    },
    // `space f` — Helix's file picker. Multi-document attachment is out of
    // scope, so the closest honest reading is this document's symbols.
    PrefixLeaf {
        glyph: 'f',
        action: PrefixAction::Effect(EditorPendingEffect::Read(LspReadOperation::DocumentSymbols)),
    },
    // `space F` — switch to file picker. Same missing capability as `space f`,
    // and unlike `space f` there is no document-local reading of it.
    PrefixLeaf {
        glyph: 'F',
        action: PrefixAction::Effect(EditorPendingEffect::Unavailable(
            "file picker: multi-document attachment is not available in this build",
        )),
    },
];

/// `[` mode's leaves: the **non-ts subset** of Helix's unimpaired `[` grammar.
///
/// M006's scope, against `https://docs.helix-editor.com/keymap.html`. Helix
/// binds about thirty glyphs after `[`; four are here and the omissions are
/// each a capability this repository has no data source for, not a shrug:
///
/// - `[q`/`[Q`/`[f`/`[F`/`[p`/`[m`/`[n`/`[%`/`[i`/`[c`/`[s`/`[t`/`[d`/`[b`
///   (`find_definition`, `find_type_definition`, `find_parent`, `find_mod`
///   (workspace search), `find_nth_occurrence`, `find_percent`, `find_occurrence`,
///   `find_todo`, `find_file`, `find_global`, `switch`, `find_toggle_select`,
///   `find_config`, `find_diagnostic`, `find_last_modified_file`,
///   `find_last_modification`) — the whole of unimpaired's "find" family sits
///   on VCS change sets, workspace-wide search, symbol definitions and
///   diagnostics. None of those is reachable here today, and binding a key to
///   a placeholder would be a key that does nothing.
/// - `[a`/`[v`/`[w`/`[e` — quote/word/word-occurrence/table. Same list, and a
///   partial implementation of any of them is worse than none: `[w` that
///   matched a *word* while `[W` was unbound would look like a bug.
/// - `[d`/`[D`/`[g`/`[G` — diagnostics and change navigation. Unimpaired
///   change navigation has **no reachable data source**: `egggit`'s
///   `ChangedFile`/`FileDiff` carry no line ranges, and `DiffHunk` sits behind
///   a private widget method. That is recorded as forward-looking work rather
///   than bound to a key that could only ever report nothing.
///
/// Every glyph listed is `Ctrl`-free and unmodified, which is what
/// [`PrefixLeaf::glyph`] requires and what `resolve_pending_prefix` looks up.
pub const OPEN_BRACKET_LEAVES: &[PrefixLeaf] = &[
    // `[Space` — a newline *above* the cursor's line. Deliberately not
    // `EditorCommand::OpenLineAbove`: Helix's `open_above` hands the keyboard
    // to the document, and the two are different verbs there too
    // (`open_above` "Open new line above selection" vs `add_newline_above`
    // "Add newline above"). The cursor stays put — Helix's
    // `add_newline_impl` moves no selection, and staying put is the point:
    // `]Space ]p` splits a paragraph without the cursor jumping away first.
    PrefixLeaf {
        glyph: ' ',
        action: PrefixAction::Edit(EditorCommand::AddNewlineAbove),
    },
    PrefixLeaf {
        glyph: 'p',
        action: PrefixAction::Motion(EditorCommand::PrevParagraph),
    },
    // `[d` — jump to the previous diagnostic.
    PrefixLeaf {
        glyph: 'd',
        action: PrefixAction::Effect(EditorPendingEffect::DiagnosticStep(
            EditorDiagnosticStep::Backward,
        )),
    },
];

/// `]` mode's leaves: the non-ts subset of Helix's unimpaired `]` grammar.
///
/// Same scope, same four omissions and the same reason as
/// [`OPEN_BRACKET_LEAVES`], mirrored left for right.
///
/// The `Space` row is also the precedence test in its own right: `Space` is
/// the *arming* glyph of a second prefix, so resolving `]` + `Space` proves
/// `resolve_pending_prefix` gets to look a glyph up before any bare-key arm
/// asks whether that glyph arms something.
pub const CLOSE_BRACKET_LEAVES: &[PrefixLeaf] = &[
    PrefixLeaf {
        glyph: ' ',
        action: PrefixAction::Edit(EditorCommand::AddNewlineBelow),
    },
    PrefixLeaf {
        glyph: 'p',
        action: PrefixAction::Motion(EditorCommand::NextParagraph),
    },
    // `]d` — jump to the next diagnostic. `]` is forward throughout this
    // layer, and Helix pairs `[d`/`]d` with the same bracket that arms them,
    // so the leaf does not carry a direction the prefix has not already said.
    PrefixLeaf {
        glyph: 'd',
        action: PrefixAction::Effect(EditorPendingEffect::DiagnosticStep(
            EditorDiagnosticStep::Forward,
        )),
    },
];

/// `m` mode's leaves: the *commands* of Helix's two-level `m` grammar.
///
/// The first level. Each leaf either runs a verb (`m m`) or says **which
/// argument the second level is waiting for** (`m s`, `m r`, `m d`, `m a`,
/// `m i`) — the argument itself is the key after it, taken as data.
///
/// The two-level grammar is Helix's, and the machine is one field wide, so the
/// second level rides in [`EditorPrefix::MatchArg`]'s payload rather than in a
/// second field on the presentation. See the `EditorPrefix` docs.
///
/// The object kinds Helix marks `TS` — `f`, `t`, `a`, `c`, `T`, `x` — are
/// **not** here. They need a syntax tree, which is milestone M014's slice and
/// is gated on ADR-0034; a variant nothing constructs is a claim, not a hook.
pub const MATCH_LEAVES: &[PrefixLeaf] = &[
    // `m m` — jump to the mate of the pair under the cursor.
    PrefixLeaf {
        glyph: 'm',
        action: PrefixAction::Motion(EditorCommand::MatchBracket),
    },
    PrefixLeaf {
        glyph: 's',
        action: PrefixAction::Capture(MatchArg::SurroundAdd),
    },
    PrefixLeaf {
        glyph: 'r',
        action: PrefixAction::Capture(MatchArg::SurroundReplaceFrom),
    },
    PrefixLeaf {
        glyph: 'd',
        action: PrefixAction::Capture(MatchArg::SurroundDelete),
    },
    PrefixLeaf {
        glyph: 'a',
        action: PrefixAction::Capture(MatchArg::AroundObject),
    },
    PrefixLeaf {
        glyph: 'i',
        action: PrefixAction::Capture(MatchArg::InnerObject),
    },
];

/// Minimum display columns reserved for the horizontal scroll indicator.
const MIN_EDITOR_TEXT_COLUMNS: u16 = 1;

/// Maximum source bytes examined for one visible logical line in one frame.
///
/// C002: the renderer never materializes a whole logical line larger than
/// this. A line beyond the cap renders from its prefix and is visibly marked
/// truncated; cursor and selection byte offsets stay exact, but horizontal
/// motion past the prefix end clamps to the prefix. Ordinary short lines
/// render exactly as before.
pub const MAX_RENDER_LINE_BYTES: usize = 32 * 1024;

/// Maximum source bytes examined across all visible lines in one frame.
///
/// C002: `longest_visible_width` stops measuring once the frame total reaches
/// this, so a tall viewport of huge lines cannot make one frame expensive.
/// The per-line cap above applies first; this caps their sum.
pub const MAX_FRAME_SOURCE_BYTES: usize = 256 * 1024;

/// Visible suffix marking a huge line rendered from its prefix.
///
/// The renderer appends this (warning-styled) instead of silently showing a
/// prefix as if it were the whole line.
pub const TRUNCATED_LINE_SUFFIX: &str = " …[too long]";

/// Maximum length of the `:` command line.
///
/// The line names one command and at most one argument, so a short bound is
/// the whole grammar. Anything longer is refused at the keystroke rather
/// than buffered, so this field can never grow into a second text buffer —
/// the same reasoning that bounds
/// [`MAX_EDITOR_PENDING_PREFIX`](super::editor::MAX_EDITOR_PENDING_PREFIX).
pub const MAX_EDITOR_COMMAND_LINE: usize = 64;

/// Maximum number of `:` completions offered at once.
///
/// A bounded list keeps the overlay cheap to render every frame and stops a
/// one-character query from producing an unbounded candidate set.
pub const MAX_EDITOR_COMMAND_MATCHES: usize = 8;

/// Maximum length of the `/` search line.
///
/// Same reasoning as [`MAX_EDITOR_COMMAND_LINE`]: a search needle is a query,
/// never document text, so it is refused at the keystroke rather than
/// buffered into something unbounded.
pub const MAX_EDITOR_SEARCH_LINE: usize = 128;

/// Maximum number of retained search matches.
///
/// Bounded so a one-character query in a large file cannot turn the
/// highlight set into an unbounded list. Past the bound the search still
/// works — it reports the matches it kept and says the rest were dropped,
/// rather than silently pretending the document contains only those.
pub const MAX_EDITOR_SEARCH_MATCHES: usize = 2_000;

/// Bytes of the document a `/` search will scan.
///
/// A search is an interactive keystroke, so its cost is paid on the frame
/// the user is waiting for. Beyond this bound the search reports that it
/// covered only the leading window rather than hanging on a huge file.
pub const MAX_EDITOR_SEARCH_BYTES: usize = 2 * 1024 * 1024;

/// One resolved `/` search outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchOutcome {
    /// Retained match start offsets, ascending.
    pub matches: Vec<usize>,
    /// Whether matches beyond [`MAX_EDITOR_SEARCH_MATCHES`] existed and were
    /// dropped. Disclosed rather than hidden: a truncated match list that
    /// looks complete is the same defect as unvalidated LSP coverage.
    pub truncated: bool,
    /// Whether the scan stopped at [`MAX_EDITOR_SEARCH_BYTES`] before the
    /// end of the document.
    pub bounded: bool,
}

/// Find every occurrence of `needle` in `snapshot`, bounded.
///
/// Offsets are absolute UTF-8 byte offsets into the replica and are always
/// character boundaries, because they are handed straight to the renderer
/// and to the cursor. The full ascending list is returned rather than a
/// slice starting at the cursor: `n` and `N` index it directly, so the
/// cursor's position is part of what the *caller* asks for
/// ([`next_match_index`]) instead of a post-hoc adjustment here.
pub fn find_matches(snapshot: &DocumentSnapshot, needle: &str) -> SearchOutcome {
    if needle.is_empty() {
        return SearchOutcome {
            matches: Vec::new(),
            truncated: false,
            bounded: false,
        };
    }
    let total = snapshot.len_bytes();
    // Cut the scanned window at a character boundary, and never report the
    // bound as reached when the document already fits inside it.
    let mut window = total.min(MAX_EDITOR_SEARCH_BYTES);
    while window < total && !snapshot.is_byte_boundary(window) {
        window += 1;
    }
    let bounded = window < total;
    let Ok(haystack) = snapshot.read_bytes(0, window) else {
        return SearchOutcome {
            matches: Vec::new(),
            truncated: false,
            bounded,
        };
    };
    let mut matches = Vec::new();
    let mut truncated = false;
    for (offset, _) in haystack.match_indices(needle) {
        if offset + needle.len() > window {
            break;
        }
        if matches.len() == MAX_EDITOR_SEARCH_MATCHES {
            truncated = true;
            break;
        }
        matches.push(offset);
    }
    SearchOutcome {
        matches,
        truncated,
        bounded,
    }
}

// ── Character-finding motions (Helix `f` / `F` / `t` / `T`) ──────────────────

/// Bytes one find-char motion scans away from the cursor.
///
/// Deliberately the same window a `/` search uses rather than a second
/// number to reason about: both are paid on the keystroke, so both need the
/// same reason to stop. Unlike `/`, the window is measured **from the
/// cursor** — `[cursor, cursor + MAX)` forward, `[cursor - MAX, cursor)`
/// backward — because a find jumps *toward* a target rather than
/// enumerating the document.
pub const MAX_EDITOR_FIND_BYTES: usize = MAX_EDITOR_SEARCH_BYTES;

/// Which way a find-char motion travels, and what Helix calls it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindKind {
    /// Helix `f` — find next char.
    Next,
    /// Helix `F` — find previous char.
    Previous,
    /// Helix `t` — find till next char.
    TillNext,
    /// Helix `T` — find till previous char.
    TillPrevious,
}

impl FindKind {
    /// Which direction this motion travels.
    pub fn forward(self) -> bool {
        matches!(self, Self::Next | Self::TillNext)
    }

    /// The `EditorCommand` this motion is, given its target.
    pub fn command(self, target: char) -> EditorCommand {
        match self {
            Self::Next => EditorCommand::FindNext(target),
            Self::Previous => EditorCommand::FindPrevious(target),
            Self::TillNext => EditorCommand::FindTillNext(target),
            Self::TillPrevious => EditorCommand::FindTillPrevious(target),
        }
    }
}

/// One resolved find-char motion: what to look for, and which way.
///
/// The repeatable-motion record is this type rather than an
/// `Option<EditorCommand>` so that "repeat only find motions" is a property
/// the compiler enforces. `Alt-.` is Helix's *repeat last motion*, and
/// Helix keeps a single slot that every motion overwrites — which is why
/// `j` between two finds is what disarms the repeat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FindMotion {
    pub kind: FindKind,
    pub target: char,
}

impl FindMotion {
    /// Scan for this motion's target from `offset`.
    pub fn outcome(&self, snapshot: &DocumentSnapshot, offset: usize) -> FindOutcome {
        find_char(snapshot, offset, self.target, self.kind.forward())
    }

    /// The `EditorCommand` this motion is.
    pub fn command(self) -> EditorCommand {
        self.kind.command(self.target)
    }
}

/// Where one find-char scan stopped, and why it stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindOutcome {
    /// The target is at this byte offset, always on a character boundary.
    Found(usize),
    /// The whole document was scanned in that direction and the target does
    /// not occur in it.
    Absent,
    /// The scan stopped at [`MAX_EDITOR_FIND_BYTES`] first, so the target
    /// may well exist past it.
    ///
    /// Disclosed rather than folded into `Absent`, for the same reason
    /// [`SearchOutcome::bounded`] is: a result that stopped early and one
    /// that looked everywhere are different facts, and reporting them the
    /// same way is a search that lies by omission.
    Bounded,
}

/// Why the last find-char motion did not land, if it did not.
///
/// Disclosed in the status row rather than swallowed. A motion that stops
/// because the character is not there and a motion that stops because the
/// bound was reached otherwise read identically — as a keypress that was
/// simply dropped — and they call for different answers from the reader
/// (type something else vs. jump through `/`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FindNote {
    /// The last find motion landed, or no find motion has run.
    #[default]
    None,
    /// The whole document was scanned; the character does not occur in that
    /// direction.
    Absent(char),
    /// The scan stopped at [`MAX_EDITOR_FIND_BYTES`] first.
    Bounded(char),
}

impl FindNote {
    /// The note for a completed scan.
    pub fn from_outcome(outcome: FindOutcome, target: char) -> Self {
        match outcome {
            FindOutcome::Found(_) => FindNote::None,
            FindOutcome::Absent => FindNote::Absent(target),
            FindOutcome::Bounded => FindNote::Bounded(target),
        }
    }

    /// Status-row text, or `""` when the last find landed.
    pub fn label(self) -> String {
        match self {
            FindNote::None => String::new(),
            FindNote::Absent(target) => format!("no {target:?} in that direction"),
            FindNote::Bounded(target) => format!(
                "no {target:?} within the {mib} MiB scan bound",
                mib = MAX_EDITOR_FIND_BYTES / (1024 * 1024)
            ),
        }
    }
}

/// Bytes read per step of a find-char scan.
///
/// The window is bounded at [`MAX_EDITOR_FIND_BYTES`], but materializing all
/// of it on one keystroke would make `f` pay a 2 MiB copy for a jump the
/// user expects to be instant. Chunks are read with a small overlap — the
/// widest target minus one byte — so a multi-byte target straddling a chunk
/// edge is still wholly inside one of them.
///
/// Overlapping means a target can be seen twice. That is harmless: the first
/// occurrence is returned and the scan stops, and UTF-8 is
/// self-synchronizing, so a valid encoding of `target` appears in the byte
/// stream only where that character actually starts — never at the tail of
/// some other character.
const FIND_SCAN_CHUNK: usize = 8 * 1024;

/// The largest character boundary at or before `offset`, never below `floor`.
fn boundary_at_or_before(snapshot: &DocumentSnapshot, offset: usize, floor: usize) -> usize {
    let mut candidate = offset;
    while candidate > floor && !snapshot.is_byte_boundary(candidate) {
        candidate -= 1;
    }
    candidate
}

/// The smallest character boundary at or after `offset`, never above
/// `ceiling`.
fn boundary_at_or_after(snapshot: &DocumentSnapshot, offset: usize, ceiling: usize) -> usize {
    let mut candidate = offset;
    while candidate < ceiling && !snapshot.is_byte_boundary(candidate) {
        candidate += 1;
    }
    candidate
}

/// Find `target` from `offset`, forward or backward, **across line
/// boundaries**.
///
/// Helix calls out the difference from vi in a single line — *"`f`, `F`,
/// `t` and `T` are not confined to the current line"* — because the current
/// line is what an implementation reaches for by habit. Nothing here knows
/// which line the cursor is on; this is the whole reason the cross-line
/// tests exist rather than being obvious from the code.
///
/// The scan starts **strictly past** the cursor in the direction of travel,
/// so `f x` from a cursor already sitting on `x` finds the *next* `x`
/// rather than standing still, and `F x` likewise looks strictly behind.
///
/// `DocumentSnapshot::read_bytes` refuses a range that does not start and end
/// on character boundaries, which is why this reads in aligned chunks rather
/// than a byte at a time. Reading per byte would not merely be slow: a
/// single `read_bytes(offset, offset + 1)` into the middle of a multi-byte
/// character is an error, so the byte at which a two-byte target begins
/// would be *unreadable*, and the scan would walk past every occurrence of
/// it. That was the first implementation of this function and it silently
/// failed on every document containing a non-ASCII character.
pub fn find_char(
    snapshot: &DocumentSnapshot,
    offset: usize,
    target: char,
    forward: bool,
) -> FindOutcome {
    // The widest match minus one byte, so a target straddling a chunk edge is
    // still wholly inside the next chunk. `str::match_indices` yields the
    // *byte* offset of each match, so no encoding is materialized here.
    let overlap = target.len_utf8() - 1;
    let total = snapshot.len_bytes();
    let cursor = clamp_offset(snapshot, offset);

    if forward {
        let bound = cursor.saturating_add(MAX_EDITOR_FIND_BYTES).min(total);
        let mut at = next_char_boundary(snapshot, cursor).unwrap_or(total);
        while at < bound {
            let ceiling = (at + FIND_SCAN_CHUNK).min(bound);
            let last_chunk = ceiling == bound;
            let hi = boundary_at_or_before(snapshot, ceiling, at);
            if hi == at {
                break;
            }
            let Ok(chunk) = snapshot.read_bytes(at, hi) else {
                // A window we could not read is a window we did not cover,
                // which is exactly what `Bounded` says.
                return FindOutcome::Bounded;
            };
            if let Some((found, _)) = chunk.match_indices(target).next() {
                let at_found = at + found;
                // A target whose last byte is past the bound lies outside the
                // window, so reporting it would be reporting something the
                // bound promised not to look at.
                if at_found + target.len_utf8() <= bound {
                    return FindOutcome::Found(at_found);
                }
            }
            if last_chunk {
                break;
            }
            at = hi - overlap;
        }
        if bound < total {
            FindOutcome::Bounded
        } else {
            FindOutcome::Absent
        }
    } else {
        let bound = cursor.saturating_sub(MAX_EDITOR_FIND_BYTES);
        let mut at = cursor;
        while at > bound {
            let floor = at.saturating_sub(FIND_SCAN_CHUNK).max(bound);
            let last_chunk = floor == bound;
            let lo = boundary_at_or_after(snapshot, floor, at);
            if lo >= at {
                break;
            }
            let Ok(chunk) = snapshot.read_bytes(lo, at) else {
                return FindOutcome::Bounded;
            };
            if let Some(found) = chunk.rfind(target) {
                let at_found = lo + found;
                if at_found >= bound {
                    return FindOutcome::Found(at_found);
                }
            }
            if last_chunk {
                break;
            }
            at = lo + overlap;
        }
        if bound > 0 {
            FindOutcome::Bounded
        } else {
            FindOutcome::Absent
        }
    }
}

/// The word surrounding `offset`, for seeding a `/` search.
///
/// Returns `None` when the cursor is not inside a word, so `/` on
/// whitespace opens an empty line rather than a search for a space.
pub fn word_at(snapshot: &DocumentSnapshot, offset: usize) -> Option<String> {
    let offset = clamp_offset(snapshot, offset);
    let line = cursor_line(snapshot, offset);
    let start = line_start(snapshot, line)?;
    let end = line_content_end(snapshot, line).unwrap_or(start);
    let Ok(line_text) = snapshot.read_bytes(start, end) else {
        return None;
    };
    let column = offset.saturating_sub(start);
    if column > line_text.len() || !line_text.is_char_boundary(column) {
        return None;
    }
    let is_word = |ch: char| ch.is_alphanumeric() || ch == '_';
    // The cursor has to be *on* a word, not merely before one. Scanning
    // outward from a position with no scan-backward guard returned the whole
    // line: on the space after `let`, the backward sweep found `let`, the
    // forward sweep broke at that same space, and the gap between them was
    // the entire rest of the line — so `/` on whitespace seeded a search for
    // a whole row instead of opening empty.
    //
    // The test is on the character *at* the cursor, not the one before it.
    // Checking backward meant the space at the end of `let` was judged by the
    // `t` behind it and accepted.
    let under = line_text[column..].chars().next()?;
    if !is_word(under) {
        return None;
    }
    let mut begin = 0usize;
    let mut finish = line_text.len();
    for (index, ch) in line_text[..column].char_indices().rev() {
        if is_word(ch) {
            begin = index;
        } else {
            break;
        }
    }
    for (index, ch) in line_text[column..].char_indices() {
        if is_word(ch) {
            finish = column + index + ch.len_utf8();
        } else {
            break;
        }
    }
    begin = begin.min(column);
    finish = finish.max(column);
    if begin >= finish {
        return None;
    }
    Some(line_text[begin..finish].to_string())
}

/// Index of the first match at or after `cursor`, wrapping to the first.
///
/// Inclusive on purpose, and this is the difference from
/// [`next_match_index`]. Typing `/` while the cursor already sits on a match
/// must leave you **on** that match; a strict `>` would make every keystroke
/// of a live search hop you to the next occurrence, so refining a needle
/// you are looking at would keep running away from you.
pub fn first_match_index(matches: &[usize], cursor: usize) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    match matches.iter().position(|offset| *offset >= cursor) {
        Some(index) => Some(index),
        None => Some(0),
    }
}

/// Index of the first match strictly after `cursor`, wrapping to the first.
///
/// Strict, because this is `n`: pressing it must move you *off* the match you
/// are on, or the first press would look like the key did nothing.
///
/// Wrapping is deliberate: a search that stopped at the last match would
/// make `n` look broken exactly when the user has found what they were
/// looking for and wants to come back to it.
pub fn next_match_index(matches: &[usize], cursor: usize) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    match matches.iter().position(|offset| *offset > cursor) {
        Some(index) => Some(index),
        None => Some(0),
    }
}

/// Index of the last match strictly before `cursor`, wrapping to the last.
pub fn previous_match_index(matches: &[usize], cursor: usize) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    match matches.iter().rposition(|offset| *offset < cursor) {
        Some(index) => Some(index),
        None => Some(matches.len() - 1),
    }
}

/// One `:` command the editor can execute.
///
/// Kept as data rather than a `match` arm at the call site so the palette,
/// the parser, and the help line all read the *same* list: a command that
/// cannot be discovered cannot be executed by muscle memory alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorCommandName {
    /// `:w` — write the buffer to disk.
    Write,
    /// `:q` — close the document.
    Quit,
    /// `:wq` / `:x` — write, then close.
    WriteQuit,
    /// `:q!` — close, discarding the local draft.
    ForceQuit,
    /// `:e` — reload the buffer from disk.
    Reload,
    /// `:wa` — write every open document (one today).
    WriteAll,
    /// `:new` — create a new document from a template.
    New,
    /// `:open <path>` — open a workspace-relative path.
    Open,
    /// `:complete` — request language-server completions at the cursor.
    Complete,
    /// `:split-regex <pattern>` — `S`, split each selection on regex matches.
    ///
    /// A **command** rather than a prompt so the pattern has somewhere to be
    /// typed with no new input mode and no new prefix to disarm. `S` opens
    /// this command's line already filled in, so the chord is a shortcut for
    /// the command rather than a second way to spell it.
    SplitRegex,
    /// `:keep-regex <pattern>` — `K`, keep the selections that match.
    KeepRegex,
    /// `:remove-regex <pattern>` — `Alt-K`, remove the selections that match.
    RemoveRegex,
}

impl EditorCommandName {
    /// The canonical spelling, shown in the palette and matched first.
    pub fn spec(self) -> &'static str {
        match self {
            Self::Write => "w",
            Self::Quit => "q",
            Self::WriteQuit => "wq",
            Self::ForceQuit => "q!",
            Self::Reload => "e",
            Self::WriteAll => "wa",
            Self::New => "new",
            Self::Open => "open",
            Self::Complete => "complete",
            Self::SplitRegex => "split-regex",
            Self::KeepRegex => "keep-regex",
            Self::RemoveRegex => "remove-regex",
        }
    }

    /// One-line help shown next to the palette entry.
    pub fn help(self) -> &'static str {
        match self {
            Self::Write => "write the buffer to disk",
            Self::Quit => "close this document",
            Self::WriteQuit => "write, then close",
            Self::ForceQuit => "close and discard unsaved changes",
            Self::Reload => "reload from disk, discarding the draft",
            Self::WriteAll => "write all open documents",
            Self::New => "create a new file from a template",
            Self::Open => "open a workspace-relative path",
            Self::Complete => "complete the word at the cursor",
            Self::SplitRegex => "split each selection on regex matches",
            Self::KeepRegex => "keep the selections matching a regex",
            Self::RemoveRegex => "remove the selections matching a regex",
        }
    }

    /// Whether the command consumes a trailing argument.
    ///
    /// Drives the fuzzy palette: an argument-taking command is still a valid
    /// completion while the user is mid-argument, and the palette narrows to
    /// the spec only once the line is complete enough to run.
    pub fn takes_argument(self) -> bool {
        matches!(
            self,
            Self::Open | Self::SplitRegex | Self::KeepRegex | Self::RemoveRegex
        )
    }

    /// Every command, in palette order.
    ///
    /// One list, so a new command is discoverable the moment it is
    /// executable rather than only after a help line is also updated.
    pub const ALL: &'static [EditorCommandName] = &[
        Self::Write,
        Self::Quit,
        Self::WriteQuit,
        Self::ForceQuit,
        Self::Reload,
        Self::WriteAll,
        Self::New,
        Self::Open,
        Self::Complete,
        Self::SplitRegex,
        Self::KeepRegex,
        Self::RemoveRegex,
    ];
}

/// One palette row: the command plus the match quality that ordered it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorCommandMatch {
    pub command: EditorCommandName,
    /// Lower is better. Ties break on [`EditorCommandName::spec`] so the
    /// ordering is deterministic across runs.
    pub score: usize,
}

/// Resolve a complete `:` line (without the leading `:`) to a command and
/// its argument.
///
/// Returns `None` for an unknown name. An extra argument is refused for a
/// command that takes none, so `:wq now` is an error rather than a silent
/// `:wq` — a mistyped command must never do half of what was asked.
pub fn parse_editor_command(line: &str) -> Option<(EditorCommandName, Option<String>)> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (name, argument) = match trimmed.split_once(char::is_whitespace) {
        Some((name, rest)) => (name, Some(rest.trim())),
        None => (trimmed, None),
    };
    let command = match name {
        "w" => EditorCommandName::Write,
        "q" => EditorCommandName::Quit,
        "wq" | "x" => EditorCommandName::WriteQuit,
        "q!" => EditorCommandName::ForceQuit,
        "e" => EditorCommandName::Reload,
        "wa" | "wall" => EditorCommandName::WriteAll,
        "new" => EditorCommandName::New,
        "open" | "o" => EditorCommandName::Open,
        "complete" => EditorCommandName::Complete,
        "split-regex" => EditorCommandName::SplitRegex,
        "keep-regex" => EditorCommandName::KeepRegex,
        "remove-regex" => EditorCommandName::RemoveRegex,
        _ => return None,
    };
    let argument = argument.filter(|value| !value.is_empty());
    if argument.is_some() && !command.takes_argument() {
        return None;
    }
    Some((command, argument.map(str::to_string)))
}

/// Fuzzy completions for a partially typed `:` line.
///
/// Matches the command name only — never the argument — because the palette
/// answers "which command did I mean", and `:open <TAB>` must complete the
/// command while the path is still being typed.
///
/// An **exact prefix always outranks a mere subsequence**. `fuzzy_score` is
/// a subsequence matcher, so the query `w` matches `new` (through its `w`)
/// with a *better* raw score than it gives `w` itself; left uncorrected, the
/// one-key query for `w` would rank `new` first. A prefix match is the
/// stronger signal, so it is ranked ahead of every non-prefix match
/// regardless of score, and only then ordered by score.
pub fn complete_editor_command(query: &str) -> Vec<EditorCommandMatch> {
    let query = query.trim();
    // An empty query is the discovery path, and it is served in *declared*
    // order — not alphabetically — so the registry reads as a curated menu
    // rather than an index. Sorting it by score would reorder the list for
    // no benefit. The whole registry is offered, not the query bound: a
    // command the empty palette hides is undiscoverable, and the registry
    // itself is the bound.
    if query.is_empty() {
        return EditorCommandName::ALL
            .iter()
            .map(|command| EditorCommandMatch {
                command: *command,
                score: 0,
            })
            .collect();
    }
    // Two tiers, because a prefix match is a categorically stronger signal
    // than a subsequence match: `fuzzy_score` is a subsequence matcher, so
    // the query `w` matches `new` (through its `w`) as well as `w` itself.
    // A flat score would let a subsequence outrank the exact prefix the user
    // is typing. `PREFIX_TIER` therefore sits below any subsequence score.
    const PREFIX_TIER: usize = 0;
    const SUBSEQUENCE_TIER: usize = 1_000_000;

    let mut matches: Vec<EditorCommandMatch> = EditorCommandName::ALL
        .iter()
        .filter_map(|command| {
            let spec = command.spec();
            if spec.starts_with(query) {
                return Some(EditorCommandMatch {
                    command: *command,
                    // Within the tier, the shortest prefix wins: query `w`
                    // ranks `w` ahead of `wq` and `wa`.
                    score: PREFIX_TIER + spec.len(),
                });
            }
            // `fuzzy_score` returns 0 for a total miss *and* for an empty
            // query, so a non-matching command must be filtered by the score
            // rather than merely sorted by it.
            let score = crate::util::fuzzy::fuzzy_score(query, spec);
            (score > 0).then_some(EditorCommandMatch {
                command: *command,
                score: SUBSEQUENCE_TIER + score,
            })
        })
        .collect();
    matches.sort_by(|left, right| {
        left.score
            .cmp(&right.score)
            .then_with(|| left.command.spec().cmp(right.command.spec()))
    });
    matches.truncate(MAX_EDITOR_COMMAND_MATCHES);
    matches
}

/// Buffer edit mode. `Normal` is a vi-style command mode over the buffer;
/// `Insert` routes printable keys into the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorMode {
    #[default]
    Normal,
    Insert,
}

/// A local, reversible edit produced by a presentation-level command.
///
/// `transaction` is what the frontend submits to
/// `DocumentController::apply_local`; `inverse` restores the pre-edit local
/// text. Both are derived from the snapshot the caller already holds, so
/// constructing an `EditorEdit` never copies the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorEdit {
    /// Forward transaction to submit through the controller.
    pub transaction: TextTransaction,
    /// Inverse transaction that restores the pre-edit text.
    pub inverse: TextTransaction,
    /// Retained payload bytes, for the undo memory bound.
    pub bytes: usize,
    /// Cursor position to adopt once the controller accepts the edit.
    pub cursor_after: usize,
    /// Whether the controller accepting this edit should keep the editor in
    /// insert mode (open-line commands, insertion entry).
    pub stay_insert: bool,
}

/// One reversible step in the frontend-local undo history.
///
/// An entry retains **both** directions, so one entry is what undo and redo
/// each apply. Reusing the same transaction for both would make redo
/// re-apply the undo, silently reverting the user's re-edit.
///
/// **The entry never moves.** The history is one ordered list and a cursor
/// names the position in it, so "undo" and "redo" are the same movement in
/// opposite directions rather than a transfer between two stacks. That is
/// what makes the "both directions" invariant structural: there is no
/// direction in which an entry can lose its other half.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorUndoEntry {
    /// Transaction that produces the newer text. Redo applies this.
    pub forward: TextTransaction,
    /// Transaction that restores the older text. Undo applies this.
    pub inverse: TextTransaction,
    /// Cursor byte offset observed before the transaction was applied.
    pub cursor_before: usize,
    /// The selection set observed before the transaction was applied.
    ///
    /// Positions only, never text — the same rule the cursor follows, and the
    /// same reason: this struct is frontend presentation state and is held by
    /// the bounded undo history. Restoring it is what makes `u` after a
    /// multi-cursor edit put the reader's cursors back rather than dropping
    /// them on the floor.
    pub selections_before: Vec<crate::tui::helix_selection::Selection<EditorPoint>>,
    /// Retained payload bytes across both directions, for the memory bound.
    pub bytes: usize,
}

/// One open insert session: the edits since Insert mode was entered.
///
/// Helix commits an undo checkpoint when insert mode is escaped, so a
/// typing burst is **one** undo step rather than one per keystroke. This
/// is the frontend-local half of that: the session accumulates the edits
/// and hands them over as a single [`EditorUndoEntry`].
///
/// **Bounded transactions, never retained text.** The session keeps the
/// individual [`TextTransaction`]s it was given, not a copy of the text
/// they produced, and the merged insertion is built once, at commit time.
/// The text-authority guard rejects a retained copy of the document, and
/// this is the shape that respects it.
///
/// **A contiguous run of insertions, and nothing else.** An edit joins the
/// session when it is a pure insertion at the end of the run so far. A
/// deletion, a replacement, or an insertion anywhere else ends the run: the
/// session is committed as one entry and the new edit starts a fresh one.
/// That restriction is what makes the merged entry exact. A transaction's
/// edits are all expressed in the coordinates of one base text, so two
/// insertions at the same place cannot both be expressed in it, and a run
/// that is not one contiguous insertion has no single merged form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsertSession {
    /// Byte offset in the session's starting text where the run begins.
    base_offset: usize,
    /// The run's edits, in order. Each is a single empty-range insertion.
    edits: Vec<TextTransaction>,
    /// Bytes the run has inserted, which is also the length of the run.
    inserted_len: usize,
    /// Retained payload bytes across the run, for the memory bound.
    bytes: usize,
    /// Cursor byte offset observed before the run's first edit.
    cursor_before: usize,
    /// The selection set observed before the run's first edit.
    selections_before: Vec<crate::tui::helix_selection::Selection<EditorPoint>>,
}

/// A resolved, document-relative edit intent from a normal-mode key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorCommand {
    MoveLeft,
    MoveRight,
    MoveUp,
    MoveDown,
    LineStart,
    LineEnd,
    BufferStart,
    BufferEnd,
    /// Helix `gs` — the first non-whitespace byte on the cursor's line.
    FirstNonWhitespace,
    WordForward,
    WordBackward,
    /// Helix `e` — the last byte of the word at or after the cursor.
    WordEnd,
    /// Helix `W` — whitespace-delimited run, against [`WordForward`]'s
    /// byte-classified word.
    LongWordForward,
    /// Helix `B` — see [`Self::LongWordForward`].
    LongWordBackward,
    /// Helix `E` — see [`Self::LongWordForward`].
    LongWordEnd,
    HalfPageDown,
    HalfPageUp,
    /// Helix `]p` — the first non-whitespace byte of the next paragraph.
    ///
    /// A variant of its own rather than a flag on [`Self::MoveDown`]:
    /// paragraphs are not lines, and the answer this gives ("there is none,
    /// and here is why") is a different kind of fact from any other motion's.
    /// See [`ParagraphMotion`] for the rule and [`next_paragraph_start`] for
    /// the scanner.
    NextParagraph,
    /// Helix `[p` — see [`Self::NextParagraph`].
    PrevParagraph,
    /// Helix `f {char}` — the next occurrence, spanning line boundaries.
    FindNext(char),
    /// Helix `F {char}` — the previous occurrence, spanning line boundaries.
    FindPrevious(char),
    /// Helix `t {char}` — see [`Self::FindNext`]; the two differ in Helix's
    /// *selection*, not in where the cursor lands.
    FindTillNext(char),
    /// Helix `T {char}` — see [`Self::FindPrevious`].
    FindTillPrevious(char),
    /// Helix `m m` — jump to the mate of the pair under the cursor.
    ///
    /// A **motion**, so it belongs beside the other motions rather than in
    /// the edit path: it moves the cursor and changes no byte.
    ///
    /// The mate is found with the **delimiter-pair table**, not a syntax
    /// tree: Helix marks `m m` as `TS`, and CodeGG's is a deliberate
    /// pair-table match. See the `editor_match` module.
    MatchBracket,
    InsertBefore,
    InsertAfter,
    InsertLineStart,
    InsertLineEnd,
    OpenLineBelow,
    OpenLineAbove,
    /// Helix `]Space` — insert a newline *below* the cursor's line, leaving
    /// the cursor where it is and the mode Normal.
    ///
    /// Not [`Self::OpenLineBelow`], which is Helix's `o`: that one also
    /// inserts a newline below, but it hands the keyboard to the document
    /// afterwards. Helix keeps them apart (`open_below` "Open new line below
    /// selection" vs `add_newline_below` "Add newline below"), and collapsing
    /// them here would have made `]Space` silently enter Insert.
    AddNewlineBelow,
    /// Helix `[Space` — see [`Self::AddNewlineBelow`].
    AddNewlineAbove,
    DeleteChar,
    DeleteLine,
    DeleteToLineEnd,
    DeleteWord,
}

impl EditorCommand {
    /// This command as a find motion, when it is one.
    ///
    /// The single place that answers it, so "which motions are repeatable
    /// by `Alt-.`" cannot be answered two slightly different ways by the
    /// recorder and the dispatcher.
    pub fn find_motion(self) -> Option<FindMotion> {
        let (kind, target) = match self {
            Self::FindNext(target) => (FindKind::Next, target),
            Self::FindPrevious(target) => (FindKind::Previous, target),
            Self::FindTillNext(target) => (FindKind::TillNext, target),
            Self::FindTillPrevious(target) => (FindKind::TillPrevious, target),
            _ => return None,
        };
        Some(FindMotion { kind, target })
    }

    /// Whether this command is a find motion.
    pub fn is_find_motion(self) -> bool {
        self.find_motion().is_some()
    }

    /// This command as an unimpaired paragraph motion, when it is one.
    ///
    /// The single place that answers it, for the same reason
    /// [`Self::find_motion`] is the single place for finds: `apply_motion`
    /// dispatches on this, and `note_motion` decides whether `Alt-.` should
    /// remember the command, and those two must not be able to disagree about
    /// which commands are unimpaired.
    pub fn paragraph_motion(self) -> Option<ParagraphMotion> {
        match self {
            Self::NextParagraph => Some(ParagraphMotion { forward: true }),
            Self::PrevParagraph => Some(ParagraphMotion { forward: false }),
            _ => None,
        }
    }

    /// Whether this command is an unimpaired paragraph motion.
    pub fn is_paragraph_motion(self) -> bool {
        self.paragraph_motion().is_some()
    }
}

/// Which way a [`ParagraphMotion`] looks.
///
/// A `bool` field rather than two variants because there is nothing else to
/// distinguish: the rule is one scanner read in one of two directions, and a
/// two-variant enum would invite a third variant that means neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParagraphMotion {
    /// `true` for `]p`, `false` for `[p`.
    pub forward: bool,
}

/// What a paragraph scan concluded.
///
/// Three answers because a bounded scan has a third outcome that `Option`
/// cannot express: "I stopped looking". Collapsing that into `None` would
/// report "there is no next paragraph" about a document that has one, which is
/// the worst of the three answers — a fact the user has no way to distrust.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParagraphOutcome {
    /// The byte offset of the target paragraph's first non-whitespace byte.
    Landed(usize),
    /// The scan reached the end of the buffer without finding a paragraph.
    NoParagraph,
    /// The scan hit [`MAX_PARAGRAPH_SCAN_BYTES`] first. Whether a paragraph
    /// exists past that point is *unknown*, and is reported as unknown.
    Bounded,
}

impl ParagraphMotion {
    /// The command this motion is entered from.
    pub fn command(self) -> EditorCommand {
        if self.forward {
            EditorCommand::NextParagraph
        } else {
            EditorCommand::PrevParagraph
        }
    }

    /// Resolve this motion against `from`.
    pub fn outcome(self, snapshot: &DocumentSnapshot, from: usize) -> ParagraphOutcome {
        if self.forward {
            next_paragraph_start(snapshot, from)
        } else {
            prev_paragraph_start(snapshot, from)
        }
    }

    /// The disclosure for an outcome that moved nothing.
    ///
    /// `None` when the scan landed, because landing needs no explanation.
    pub fn note(self, outcome: ParagraphOutcome) -> Option<ParagraphNote> {
        match outcome {
            ParagraphOutcome::Landed(_) => None,
            ParagraphOutcome::NoParagraph => Some(if self.forward {
                ParagraphNote::NoNextParagraph
            } else {
                ParagraphNote::NoPreviousParagraph
            }),
            ParagraphOutcome::Bounded => Some(ParagraphNote::ScanBounded),
        }
    }
}

/// Why the last unimpaired paragraph motion did not land.
///
/// The paragraph analogue of [`FindNote`]: both exist because a motion that
/// scans can *fail*, and a failure the status row cannot see is a keypress
/// that looks like a dropped one. `None` is the default rather than a variant
/// so every path that never runs a paragraph motion needs no write at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParagraphNote {
    /// The last paragraph motion landed; nothing to disclose.
    #[default]
    None,
    /// `]p` with no paragraph after the cursor.
    NoNextParagraph,
    /// `[p` with no paragraph before the cursor.
    NoPreviousParagraph,
    /// The scan hit its byte budget before it could answer.
    ScanBounded,
}

impl ParagraphNote {
    /// The words the status row shows, or `None` when there is nothing to say.
    pub fn label(self) -> Option<String> {
        match self {
            ParagraphNote::None => None,
            ParagraphNote::NoNextParagraph => {
                Some("no next paragraph — ]p reached the end".to_string())
            }
            ParagraphNote::NoPreviousParagraph => {
                Some("no previous paragraph — [p reached the start".to_string())
            }
            ParagraphNote::ScanBounded => Some(format!(
                "paragraph scan stopped after {MAX_PARAGRAPH_SCAN_BYTES} bytes — \
                 the answer is not known"
            )),
        }
    }
}

/// Why the last bracket-match motion did not land, if it did not.
///
/// The bracket analogue of [`FindNote`] and [`ParagraphNote`], and for the
/// same reason: `m m` can fail in three distinguishable ways, and a reader
/// who cannot tell them apart sees a keypress that was dropped. "There is no
/// bracket here" and "the bracket here has no mate" call for different
/// answers — one says *move the cursor*, the other says *the code is
/// unbalanced* — so they are not one refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MatchNote {
    /// The last bracket motion landed, or none has run.
    #[default]
    None,
    /// No delimiter is under the cursor.
    NoBracket,
    /// A delimiter is under the cursor, but no mate was found for it.
    Unbalanced,
    /// The scan hit its byte budget before it could answer.
    ScanBounded,
}

impl MatchNote {
    /// The status-row text, or `None` when there is nothing to say.
    pub fn label(self) -> Option<String> {
        match self {
            MatchNote::None => None,
            MatchNote::NoBracket => Some("no bracket under the cursor".to_string()),
            MatchNote::Unbalanced => Some("no matching bracket".to_string()),
            MatchNote::ScanBounded => Some(format!(
                "bracket scan stopped after {MAX_SET_VERB_READ_BYTES} bytes — \
                 the answer is not known"
            )),
        }
    }
}

// ── Offset safety ────────────────────────────────────────────────────────────

/// Clamp `offset` into `0..=len_bytes` and onto a UTF-8 character boundary.
///
/// A non-boundary offset is at most three bytes past the preceding
/// boundary in valid UTF-8, so this walks back at most three steps.
pub fn clamp_offset(snapshot: &DocumentSnapshot, offset: usize) -> usize {
    let mut candidate = offset.min(snapshot.len_bytes());
    while candidate > 0 && !snapshot.is_byte_boundary(candidate) {
        candidate -= 1;
    }
    candidate
}

/// Next character boundary strictly after `offset`, or the end of the
/// document. Returns `None` only when `offset` is already the end.
pub fn next_char_boundary(snapshot: &DocumentSnapshot, offset: usize) -> Option<usize> {
    let mut candidate = clamp_offset(snapshot, offset).saturating_add(1);
    while candidate < snapshot.len_bytes() {
        if snapshot.is_byte_boundary(candidate) {
            return Some(candidate);
        }
        candidate += 1;
    }
    None
}

/// Previous character boundary strictly before `offset`.
pub fn prev_char_boundary(snapshot: &DocumentSnapshot, offset: usize) -> usize {
    let mut candidate = clamp_offset(snapshot, offset);
    while candidate > 0 {
        candidate -= 1;
        if snapshot.is_byte_boundary(candidate) {
            return candidate;
        }
    }
    0
}

/// Zero-based line containing `offset`, clamped to a valid line.
pub fn cursor_line(snapshot: &DocumentSnapshot, offset: usize) -> usize {
    let offset = clamp_offset(snapshot, offset);
    snapshot
        .byte_to_position(offset)
        .map(|position| position.line)
        .unwrap_or_else(|_| snapshot.len_lines().saturating_sub(1))
}

/// Byte offset where `line` starts.
pub fn line_start(snapshot: &DocumentSnapshot, line: usize) -> Option<usize> {
    snapshot.line_range(line).ok().map(|range| range.start)
}

/// Byte length of `line` including its terminator, without reading content.
///
/// Cheap: `line_range` alone, so the renderer can decide the bounded path
/// before examining a single content byte.
pub fn line_byte_len(snapshot: &DocumentSnapshot, line: usize) -> Option<usize> {
    snapshot
        .line_range(line)
        .ok()
        .map(|range| range.end.saturating_sub(range.start))
}

/// Whether `line` exceeds the per-line render budget and will be shown
/// truncated. Cheap: length only, no content read.
pub fn line_is_truncated(snapshot: &DocumentSnapshot, line: usize) -> bool {
    line_byte_len(snapshot, line).is_some_and(|len| len > MAX_RENDER_LINE_BYTES)
}

/// Walk `offset` back onto a UTF-8 character boundary (at most three steps
/// in valid UTF-8). Used to clip a budget window without splitting a char.
fn boundary_back(snapshot: &DocumentSnapshot, offset: usize) -> usize {
    let mut candidate = offset.min(snapshot.len_bytes());
    while candidate > 0 && !snapshot.is_byte_boundary(candidate) {
        candidate -= 1;
    }
    candidate
}

/// Exclusive end of the bounded prefix of `start..end`: at most
/// `MAX_RENDER_LINE_BYTES` past `start`, adjusted back to a character
/// boundary so the caller can read it directly.
pub fn bounded_prefix_end(snapshot: &DocumentSnapshot, start: usize, end: usize) -> usize {
    let capped = start.saturating_add(MAX_RENDER_LINE_BYTES).min(end);
    if capped >= end {
        return end;
    }
    boundary_back(snapshot, capped).max(start)
}

/// Byte offset of the last content byte on `line`, excluding the line
/// terminator. Equals the line start for an empty line.
///
/// C002: inspects at most the last two bytes rather than reading the whole
/// line, so a multi-megabyte line costs a constant here.
pub fn line_content_end(snapshot: &DocumentSnapshot, line: usize) -> Option<usize> {
    let range = snapshot.line_range(line).ok()?;
    if range.start >= range.end {
        return Some(range.start);
    }
    // The terminator, if any, is `\n`, `\r`, or `\r\n` at the very end.
    let mut tail_start = range.end.saturating_sub(2).max(range.start);
    while tail_start < range.end && !snapshot.is_byte_boundary(tail_start) {
        tail_start += 1;
    }
    let tail = snapshot.read_bytes(tail_start, range.end).ok()?;
    let terminator = if tail.ends_with("\r\n") {
        2
    } else if tail.ends_with('\n') || tail.ends_with('\r') {
        1
    } else {
        0
    };
    Some(range.end.saturating_sub(terminator))
}

/// Byte offset of the end of the last line, excluding any terminator.
pub fn buffer_content_end(snapshot: &DocumentSnapshot) -> usize {
    let last = snapshot.len_lines().saturating_sub(1);
    line_content_end(snapshot, last).unwrap_or_else(|| snapshot.len_bytes())
}

/// Zero-based index of the document's last line.
///
/// A document always has at least one line, so this never underflows; it is
/// the single answer to "where does the file end" that paging and viewport
/// arithmetic share.
pub fn last_line_of(snapshot: &DocumentSnapshot) -> usize {
    snapshot.len_lines().saturating_sub(1)
}

/// Byte offset of the first non-whitespace byte on `line`.
///
/// Helix `gs`. A blank line resolves to its own start, so `gs` round-trips
/// with `0` rather than appearing stuck; a whitespace-only line resolves to
/// that line's [`line_content_end`], which is where `$` already sits.
///
/// **Bounded, like [`line_content_end`]:** the scan stops at the end of the
/// line's *rendered* prefix ([`bounded_prefix_end`], i.e.
/// [`MAX_RENDER_LINE_BYTES`]), because that is already the horizon past which
/// the cursor cannot be shown and horizontal motion cannot reach. A line
/// indented past that bound resolves to the last visible byte rather than
/// scanning megabytes to find a character the user cannot see — and such a
/// line is already rendered with [`TRUNCATED_LINE_SUFFIX`], so the answer is
/// disclosed on screen rather than silently wrong.
///
/// Whitespace is **ASCII** only, matching [`is_word_byte`]'s treatment of
/// every byte `>= 0x80` as content: a byte-at-a-time scan cannot tell a
/// non-breaking space from any other multi-byte character without decoding it,
/// and indentation is ASCII in every source language this editor targets.
pub fn first_non_whitespace(snapshot: &DocumentSnapshot, line: usize) -> Option<usize> {
    let start = line_start(snapshot, line)?;
    let content_end = line_content_end(snapshot, line)?;
    let limit = bounded_prefix_end(snapshot, start, content_end);
    let mut cursor = start;
    while cursor < limit {
        // An unreadable character ends the scan rather than being classified: a
        // character that cannot be seen is not one to skip over.
        match leading_byte(snapshot, cursor) {
            Some(byte) if byte.is_ascii_whitespace() => {}
            Some(_) => return Some(cursor),
            None => return Some(cursor),
        }
        cursor = next_char_boundary(snapshot, cursor).unwrap_or(limit);
    }
    Some(limit)
}

/// Maximum source bytes one paragraph scan may examine, across every line it
/// looks at.
///
/// C002 discipline: the per-line cap in [`bounded_prefix_end`] bounds *one*
/// line, and a paragraph walk visits lines until it finds a boundary — so on a
/// 200 MB file with no blank lines the per-line bound alone would still be a
/// 200 MB scan behind one keystroke. This is the total budget that makes the
/// walk a constant.
///
/// The value is the frame's own source budget rather than a new number, so the
/// worst case is "one keystroke costs what one screenful of the document
/// costs". Exhausting it is a third answer ([`ParagraphOutcome::Bounded`]), not
/// a silent `None`.
pub const MAX_PARAGRAPH_SCAN_BYTES: usize = MAX_FRAME_SOURCE_BYTES;

/// Blankness of one line, plus what answering it cost.
///
/// The cost is carried out rather than returned bare because [`next_paragraph_start`]
/// has to charge a shared budget across every line it inspects, and a boolean
/// alone cannot say how much it spent.
fn line_blankness(snapshot: &DocumentSnapshot, line: usize) -> (bool, usize) {
    let (Some(start), Some(content_end)) =
        (line_start(snapshot, line), line_content_end(snapshot, line))
    else {
        return (false, 0);
    };
    let limit = bounded_prefix_end(snapshot, start, content_end);
    let mut cursor = start;
    while cursor < limit {
        match leading_byte(snapshot, cursor) {
            // An unreadable character ends the scan and the line counts as
            // content: a byte nobody can read is not evidence of blankness.
            Some(byte) if byte.is_ascii_whitespace() => {}
            Some(_) => return (false, limit - start),
            None => return (false, limit - start),
        }
        cursor = next_char_boundary(snapshot, cursor).unwrap_or(limit);
    }
    // Every byte in the window was whitespace, but the line continues past it.
    // Whether the *rest* is whitespace is unknown, so this line is content —
    // the answer that cannot invent a paragraph boundary that is not there.
    // A multi-megabyte indented line is nearly always content, so this is
    // also the answer that is nearly always right.
    (limit >= content_end, limit - start)
}

/// Whether `line` is blank: whitespace only, ASCII, excluding its terminator.
pub fn line_is_blank(snapshot: &DocumentSnapshot, line: usize) -> bool {
    line_blankness(snapshot, line).0
}

/// First non-whitespace byte of the next paragraph after `from`, for Helix `]p`.
///
/// Three answers, and `Option` could not carry them: `NoParagraph` is a real
/// answer, and `Bounded` is "the budget ran out first", which is the one this
/// must not let a caller confuse with the other.
///
/// The rule, in one line: **leave whatever paragraph or separator the cursor
/// is on, cross one blank run, and land on the first non-whitespace byte of
/// the content line after it.** Both cases collapse to that because starting
/// inside a paragraph and starting on a blank one both need the same two
/// skips — forward over content, then forward over blanks.
pub fn next_paragraph_start(snapshot: &DocumentSnapshot, from: usize) -> ParagraphOutcome {
    let mut budget = MAX_PARAGRAPH_SCAN_BYTES;
    let last = last_line_of(snapshot);
    let mut line = cursor_line(snapshot, from);

    // Leave the current paragraph, if the cursor is inside one. A *blank*
    // line ends this walk: it is the separator we are about to cross, and
    // stepping over it here would skip a whole paragraph.
    loop {
        let (blank, spent) = line_blankness(snapshot, line);
        budget = budget.saturating_sub(spent + 1);
        if blank || line >= last {
            break;
        }
        line += 1;
    }
    // Cross the separator. A file that is one paragraph has nothing here.
    if budget == 0 {
        return ParagraphOutcome::Bounded;
    }
    while line <= last {
        let (blank, spent) = line_blankness(snapshot, line);
        budget = budget.saturating_sub(spent + 1);
        if !blank {
            return ParagraphOutcome::Landed(
                first_non_whitespace(snapshot, line)
                    .unwrap_or(line_start(snapshot, line).unwrap_or(0)),
            );
        }
        if line == last {
            break;
        }
        line += 1;
    }
    ParagraphOutcome::NoParagraph
}

/// First non-whitespace byte of the previous paragraph before `from`, for `[p`.
///
/// The mirror of [`next_paragraph_start`] and the same three answers. It never
/// leaves the cursor standing still: a cursor inside paragraph *n* lands on
/// paragraph *n-1*'s first non-whitespace byte, and a cursor on a blank line
/// lands on the paragraph that ends just above the blank run.
pub fn prev_paragraph_start(snapshot: &DocumentSnapshot, from: usize) -> ParagraphOutcome {
    let mut budget = MAX_PARAGRAPH_SCAN_BYTES;
    let mut line = cursor_line(snapshot, from);

    // Walk back to the top of whatever the cursor is sitting in. On a blank
    // line that is the top of the blank run; on content, the paragraph's own
    // first line.
    loop {
        if budget == 0 {
            return ParagraphOutcome::Bounded;
        }
        if line == 0 {
            return ParagraphOutcome::NoParagraph;
        }
        let (blank, spent) = line_blankness(snapshot, line - 1);
        budget = budget.saturating_sub(spent + 1);
        if blank {
            break;
        }
        line -= 1;
    }

    // On a blank line: the paragraph above the run is the previous one.
    // On content: line is this paragraph's first line, so `line - 1` is either
    // the separator or nothing at all.
    if line == 0 {
        return ParagraphOutcome::NoParagraph;
    }
    let mut prev = line - 1;
    loop {
        let (blank, spent) = line_blankness(snapshot, prev);
        budget = budget.saturating_sub(spent + 1);
        if budget == 0 {
            return ParagraphOutcome::Bounded;
        }
        if !blank {
            break;
        }
        if prev == 0 {
            return ParagraphOutcome::NoParagraph;
        }
        prev -= 1;
    }
    // `prev` is the previous paragraph's last line; walk to its first. The
    // landing is its **first non-whitespace** byte, not its start — on an
    // indented paragraph those are different offsets, and `]p`/`[p` are
    // defined by the former.
    loop {
        if budget == 0 {
            return ParagraphOutcome::Bounded;
        }
        if prev == 0 {
            return ParagraphOutcome::Landed(
                first_non_whitespace(snapshot, prev)
                    .or_else(|| line_start(snapshot, prev))
                    .unwrap_or(0),
            );
        }
        let (blank, spent) = line_blankness(snapshot, prev - 1);
        budget = budget.saturating_sub(spent + 1);
        if blank {
            return ParagraphOutcome::Landed(
                first_non_whitespace(snapshot, prev)
                    .or_else(|| line_start(snapshot, prev))
                    .unwrap_or(0),
            );
        }
        prev -= 1;
    }
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

/// Whether `byte` belongs to a whitespace-delimited run, for Helix's `W`/`B`/`E`.
///
/// **Deliberately not [`is_word_byte`].** `is_word_byte`'s `>= 0x80` clause
/// exists so the byte-at-a-time `w`/`b`/`e` scan classifies the interior of a
/// multi-byte character correctly; reusing it here would make every non-ASCII
/// byte and every ASCII punctuation mark part of the same word as its
/// neighbours, which collapses `W` onto `w` on precisely the text where the
/// difference matters. Whitespace versus not is the whole of `WORD`.
fn is_long_word_byte(byte: u8) -> bool {
    !byte.is_ascii_whitespace()
}

/// The leading byte of the character that starts at `offset`.
///
/// **Not** `read_bytes(offset, offset + 1)`. That range needs a character
/// boundary at *both* ends, so it is `Err` for every byte of a multi-byte
/// character — its **first** byte as much as its interior. A scanner that
/// treated that as "not a word byte" walked straight past every non-ASCII
/// character: on `日本語 text`, `w` from offset 0 landed on the end of the
/// document instead of on `text`, and `e` did the same. It was invisible
/// because ASCII single-byte characters are the only kind that read back.
///
/// `None` means the offset is not readable as a character start at all, which
/// is also what a caller gets at the end of the document; callers scanning
/// forward only ask at offsets they have already advanced onto a boundary.
fn leading_byte(snapshot: &DocumentSnapshot, offset: usize) -> Option<u8> {
    let end = next_char_boundary(snapshot, offset).unwrap_or_else(|| snapshot.len_bytes());
    snapshot
        .read_bytes(offset, end)
        .ok()
        .and_then(|text| text.as_bytes().first().copied())
}

/// Start of the next word at or after `offset`.
pub fn word_forward(snapshot: &DocumentSnapshot, offset: usize) -> usize {
    let mut cursor = clamp_offset(snapshot, offset);
    let end = snapshot.len_bytes();
    while cursor < end {
        if !is_word_byte(leading_byte(snapshot, cursor).unwrap_or(0)) {
            cursor += 1;
            continue;
        }
        while cursor < end && is_word_byte(leading_byte(snapshot, cursor).unwrap_or(0)) {
            cursor = next_char_boundary(snapshot, cursor).unwrap_or(end);
        }
        if cursor >= end {
            return buffer_content_end(snapshot);
        }
        // Skip the separator run, then report the next word start.
        while cursor < end && !is_word_byte(leading_byte(snapshot, cursor).unwrap_or(0)) {
            cursor = next_char_boundary(snapshot, cursor).unwrap_or(end);
        }
        return cursor;
    }
    buffer_content_end(snapshot)
}

/// Start of the previous word strictly before `offset`.
pub fn word_backward(snapshot: &DocumentSnapshot, offset: usize) -> usize {
    let mut cursor = clamp_offset(snapshot, offset);
    while cursor > 0 {
        let previous = prev_char_boundary(snapshot, cursor);
        if is_word_byte(leading_byte(snapshot, previous).unwrap_or(0)) {
            break;
        }
        cursor = previous;
    }
    while cursor > 0 {
        let previous = prev_char_boundary(snapshot, cursor);
        if !is_word_byte(leading_byte(snapshot, previous).unwrap_or(0)) {
            break;
        }
        cursor = previous;
    }
    cursor
}

/// Byte offset of the character at the end of the word at or after `offset`.
///
/// Helix `e`. When the cursor is not already on word content it first steps to
/// the next word, so `e` from whitespace lands on a word rather than standing
/// still.
///
/// **Not** the terminal panel's `e`. That surface scans a `&str` by column
/// index over scrollback rather than by byte offset over a document, so it
/// carries its own `next_word_end`; the two agree on ASCII and were never the
/// same code.
///
/// It steps **a whole character at a time** rather than probing `offset + 1`.
/// The probe was the original design on the reasoning that `is_word_byte`
/// treats any byte `>= 0x80` as content, so walking through the interior of a
/// multi-byte character would still classify it correctly — that reasoning is
/// false, because a one-byte read inside a multi-byte character is not
/// readable at all, so the interior could never be classified. Worse, the
/// final `.unwrap_or(cursor)` then had nowhere to advance to on the last
/// character of a document, which is a spin.
pub fn word_end(snapshot: &DocumentSnapshot, offset: usize) -> usize {
    let end = snapshot.len_bytes();
    let mut cursor = clamp_offset(snapshot, offset);
    if !is_word_byte(leading_byte(snapshot, cursor).unwrap_or(0)) {
        cursor = word_forward(snapshot, cursor);
        if cursor >= end || !is_word_byte(leading_byte(snapshot, cursor).unwrap_or(0)) {
            return buffer_content_end(snapshot);
        }
    }
    while let Some(next) = next_char_boundary(snapshot, cursor) {
        match leading_byte(snapshot, next) {
            Some(byte) if is_word_byte(byte) => cursor = next,
            _ => break,
        }
    }
    cursor
}

/// Start of the next whitespace-delimited run at or after `offset`.
///
/// Helix `W`. The same three-phase scan as [`word_forward`] — leave the
/// separator, cross the run, cross the next separator run — with
/// [`is_long_word_byte`] as the classifier. On `foo.bar` this lands on `foo`
/// then `.bar`, where `w` lands on `foo` then `bar`.
pub fn long_word_forward(snapshot: &DocumentSnapshot, offset: usize) -> usize {
    let mut cursor = clamp_offset(snapshot, offset);
    let end = snapshot.len_bytes();
    while cursor < end {
        // Separators are ASCII by definition here, so stepping one byte at a
        // time across them cannot land inside a multi-byte character.
        if !is_long_word_byte(leading_byte(snapshot, cursor).unwrap_or(0)) {
            cursor += 1;
            continue;
        }
        while cursor < end && is_long_word_byte(leading_byte(snapshot, cursor).unwrap_or(0)) {
            cursor = next_char_boundary(snapshot, cursor).unwrap_or(end);
        }
        if cursor >= end {
            return buffer_content_end(snapshot);
        }
        while cursor < end && !is_long_word_byte(leading_byte(snapshot, cursor).unwrap_or(0)) {
            cursor = next_char_boundary(snapshot, cursor).unwrap_or(end);
        }
        return cursor;
    }
    buffer_content_end(snapshot)
}

/// Start of the previous whitespace-delimited run strictly before `offset`.
///
/// Helix `B`.
pub fn long_word_backward(snapshot: &DocumentSnapshot, offset: usize) -> usize {
    let mut cursor = clamp_offset(snapshot, offset);
    while cursor > 0 {
        let previous = prev_char_boundary(snapshot, cursor);
        if is_long_word_byte(leading_byte(snapshot, previous).unwrap_or(0)) {
            break;
        }
        cursor = previous;
    }
    while cursor > 0 {
        let previous = prev_char_boundary(snapshot, cursor);
        if !is_long_word_byte(leading_byte(snapshot, previous).unwrap_or(0)) {
            break;
        }
        cursor = previous;
    }
    cursor
}

/// Byte offset of the character at the end of the whitespace-delimited run at
/// or after `offset`.
///
/// Helix `E`. Mirrors [`word_end`]'s "step to the next run first" rule, so `E`
/// from whitespace lands on a run rather than standing still, and its whole-
/// character stepping for the same reason.
pub fn long_word_end(snapshot: &DocumentSnapshot, offset: usize) -> usize {
    let end = snapshot.len_bytes();
    let mut cursor = clamp_offset(snapshot, offset);
    if !is_long_word_byte(leading_byte(snapshot, cursor).unwrap_or(0)) {
        cursor = long_word_forward(snapshot, cursor);
        if cursor >= end || !is_long_word_byte(leading_byte(snapshot, cursor).unwrap_or(0)) {
            return buffer_content_end(snapshot);
        }
    }
    while let Some(next) = next_char_boundary(snapshot, cursor) {
        match leading_byte(snapshot, next) {
            Some(byte) if is_long_word_byte(byte) => cursor = next,
            _ => break,
        }
    }
    cursor
}

/// Move `offset` by `delta` lines, preserving the display column as closely
/// as the target line allows.
///
/// `desired_column` is the column a vertical run should aim for, which is what
/// makes `j` onto a short line and back up return to the original column
/// instead of the short line's. `None` means "no run in progress", so the
/// current cursor column is used and becomes the target — see
/// [`TuiDocumentPresentation::desired_column`].
///
/// The column is clamped into the target line, and [`offset_on_line`] clamps it
/// again onto a character boundary, so a stale column recorded against older
/// text cannot produce an offset outside the line or mid-codepoint.
pub fn move_vertical(
    snapshot: &DocumentSnapshot,
    offset: usize,
    delta: isize,
    desired_column: Option<usize>,
) -> usize {
    let offset = clamp_offset(snapshot, offset);
    let last_line = snapshot.len_lines().saturating_sub(1);
    let current = cursor_line(snapshot, offset);
    let column = desired_column
        .unwrap_or_else(|| offset.saturating_sub(line_start(snapshot, current).unwrap_or(0)));
    let target = if delta.is_negative() {
        current.saturating_sub(delta.unsigned_abs())
    } else {
        current.saturating_add(delta as usize).min(last_line)
    };
    offset_on_line(snapshot, target, column)
}

/// A position in the document as a Helix selection point.
///
/// The editor's counterpart to the terminal's viewport cell: a byte offset
/// paired with the line it sits on, because [`SelectionPoint`] separates the
/// line from the point so whole-line selection can answer "which lines does
/// this span" without knowing how wide a line is. The line is redundant with
/// the offset for every *motion*, but it is not derivable at selection time —
/// `x` growth walks to the next line's content start, and resolving that
/// needs the line anyway.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct EditorPoint {
    /// UTF-8 byte offset into the local replica.
    pub byte: usize,
    /// Zero-based line containing `byte`.
    pub line: usize,
}

impl crate::tui::helix_selection::SelectionPoint for EditorPoint {
    type Line = usize;

    fn line(&self) -> usize {
        self.line
    }
}

/// Resolve a byte offset into a [`SelectionPoint`].
///
/// Clamped and boundary-aligned first, so a stale offset recorded against
/// older text can never produce a point the renderer would slice
/// mid-codepoint.
pub fn editor_point(snapshot: &DocumentSnapshot, byte: usize) -> EditorPoint {
    let byte = clamp_offset(snapshot, byte);
    EditorPoint {
        byte,
        line: cursor_line(snapshot, byte),
    }
}

/// The text objects the `m` grammar can name.
///
/// Five, and five is the whole of Helix's **parser-free** set. The
/// syntax-node objects (`f`, `t`, `a`, `c`, `T`, `x`) need a grammar this
/// editor does not have, and they are milestone M014's slice — a variant
/// nothing constructs is a claim, not a hook, so they are not here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectKind {
    /// `w` — the word under the cursor.
    ///
    /// The byte classification is [`is_word_byte`]'s, so a CJK run is one
    /// word and the object agrees with the `w` motion the reader just used
    /// to get there.
    Word,
    /// `W` — the whitespace-delimited run under the cursor.
    LongWord,
    /// `p` — the paragraph under the cursor.
    Paragraph,
    /// A **delimiter pair**, named by either of its delimiters.
    ///
    /// The pair is the one in the delimiter table — see the `editor_match`
    /// module — so `(` and `)` name the same pair.
    Pair(char),
    /// `m` — the **innermost** pair enclosing the cursor.
    ClosestPair,
}

impl ObjectKind {
    /// The object a character names, when it names one.
    ///
    /// The fixed vocabulary the `m a` / `m i` arguments are drawn from. A
    /// character outside it is not a slightly-wrong object, it is not an
    /// object at all, and the argument capture hands it back rather than
    /// guessing.
    pub fn from_char(ch: char) -> Option<Self> {
        match ch {
            'w' => Some(ObjectKind::Word),
            'W' => Some(ObjectKind::LongWord),
            'p' => Some(ObjectKind::Paragraph),
            'm' => Some(ObjectKind::ClosestPair),
            other => crate::tui::editor_match::pair_for(other).map(|_| ObjectKind::Pair(other)),
        }
    }
}

/// One Helix selection verb, resolved against a document.
///
/// No longer `Copy`: the regex verbs carry the pattern they were given, and a
/// verb that owns a `String` is not a unit value. Every call site constructed
/// one and passed it straight through, so nothing had to be rewritten around
/// it — but the boundary had to move honestly rather than keep a `Copy` impl
/// that only held for the fourteen variants that do not carry text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelixVerb {
    /// `v` — toggle character selection at the cursor.
    SelectChar,
    /// `x` — select the cursor's line, or grow the held line selection by one.
    SelectLine,
    /// `d` — delete the held selection. Claims the key even with nothing
    /// selected, because an unclaimed `d` escapes to the global `OpenDiff`
    /// binding.
    DeleteSelection,
    /// `y` — yank the held selection to the system clipboard.
    YankSelection,
    /// Any deliberate motion: extends a character selection, leaves a line
    /// selection alone, and disarms line growth.
    Moved(EditorCommand),
    /// `;` — collapse the set to a single cursor at the primary's head.
    CollapseSelection,
    /// `Alt-;` — swap every member's anchor and head.
    FlipSelections,
    /// `Alt-:` — make every member forward.
    EnsureSelectionsForward,
    /// `%` — select the entire file as one primary member.
    SelectAll,
    /// `,` — keep only the primary.
    KeepPrimarySelection,
    /// `Alt-,` — remove the primary, keeping the rest.
    RemovePrimarySelection,
    /// `C` / `Alt-C` — copy every member onto the next / previous line.
    ///
    /// A **signed** line delta rather than two variants, so the "which line"
    /// question is answered once. `1` is `C`, `-1` is `Alt-C`.
    CopySelectionOnLine { line_delta: isize },
    /// `(` / `)` — move which member is the primary.
    ///
    /// A member delta, wrapped. Focus only: no byte in the buffer moves, which
    /// is what separates it from [`Self::RotateSelectionContents`].
    RotateSelections { member_delta: isize },
    /// `Alt-(` / `Alt-)` — rotate the members' *contents* around the set.
    ///
    /// A buffer edit. The two `Rotate*` verbs are the most easily confused
    /// pair in Helix's keymap and the difference is not subtle: this one moves
    /// text and leaves every member's identity alone.
    RotateSelectionContents { member_delta: isize },
    /// `&` — pad every member to the widest member's display width.
    AlignSelections,
    /// `Alt-minus` — merge every member into one spanning the whole set.
    MergeSelections,
    /// `Alt-_` — merge adjacent members only, leaving gaps alone.
    MergeConsecutiveSelections,
    /// `S` — split every member on the matches of a regex.
    ///
    /// The pattern travels with the verb rather than being read from the
    /// command line later: the match happens here, against the bytes the
    /// reader was looking at when they pressed `S`.
    SplitSelection { pattern: String },
    /// `K` — keep only the members whose text matches a regex.
    KeepSelections { pattern: String },
    /// `Alt-K` — remove the members whose text matches a regex.
    RemoveSelections { pattern: String },
    /// `m a` / `m i` — select **around** or **inside** a text object.
    ///
    /// A selection-shaping verb: it sets the selection set and edits no
    /// byte, so it goes through the plain verb path and pushes no undo
    /// entry.
    SelectObject { kind: ObjectKind, around: bool },
    /// `m s` — surround the held selection with a character.
    ///
    /// The character is the delimiter to add *as a pair*: `(` adds `(` and
    /// `)`, and a character that is both an opening and a closing delimiter
    /// (a quote) adds itself twice.
    SurroundAdd { ch: char },
    /// `m d` — remove a character's pair around the held selection.
    SurroundDelete { ch: char },
    /// `m r` — replace a character's pair around the held selection.
    ///
    /// The **outermost** enclosing pair of `from` is replaced, so the
    /// replaced text keeps its inner pairs intact: `(a(b))` with `m r ([`
    /// becomes `[a(b)]`.
    SurroundReplace { from: char, to: char },
}

/// A selection-shaping motion that `Alt-.` can repeat.
///
/// Separate from [`EditorCommand`] because these are not cursor motions in the
/// sense `Alt-.` originally meant: `C` does not walk the cursor, it *adds*
/// one, and `%` selects rather than moves. Recording them as ordinary
/// `EditorCommand`s would have made `note_motion` erase them, since every
/// `EditorCommand` motion clears the repeat slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionMotion {
    /// `C` — add a cursor on the next line.
    CopyOntoNextLine,
    /// `Alt-C` — add a cursor on the previous line.
    CopyOntoPreviousLine,
    /// `%` — select the entire file.
    SelectAll,
}

impl SelectionMotion {
    /// The verb this motion repeats.
    pub fn verb(self) -> HelixVerb {
        match self {
            Self::CopyOntoNextLine => HelixVerb::CopySelectionOnLine { line_delta: 1 },
            Self::CopyOntoPreviousLine => HelixVerb::CopySelectionOnLine { line_delta: -1 },
            Self::SelectAll => HelixVerb::SelectAll,
        }
    }
}

/// The **primary** selection resolved to a byte range.
///
/// Character mode is **inclusive of the head character**, matching
/// [`helix_selection::SelectionMode::Char`] and the terminal: `v` then `l`
/// selects two characters, not one-and-a-bit. Line mode spans whole lines
/// including their terminators, so deleting the selection removes the lines
/// rather than leaving blank ones behind.
///
/// Both endpoints are clamped to the document and to character boundaries, so
/// a stale selection recorded against older text can never panic or slice
/// mid-codepoint during rendering.
///
/// **Primary-scoped**, deliberately: with the selection set this answers for
/// the distinguished member, which is what every pre-set caller meant. A
/// caller that acts on the *whole* set wants [`selection_ranges`].
pub fn selection_range(
    presentation: &TuiDocumentPresentation,
    snapshot: &DocumentSnapshot,
) -> Option<std::ops::Range<usize>> {
    let selection = *presentation.selections.get(presentation.primary_index)?;
    member_range(snapshot, &selection)
}

/// Every member's range, merged and in document order.
///
/// Merged, not merely sorted, because the two are the same requirement here:
/// a transaction's edits must ascend and must not overlap
/// (`codegg_document`'s validator rejects the second outright and
/// [`invert`] is only exact for the first). Two cursors selecting the same
/// word, or a set whose members touch, therefore produce one range rather
/// than an invalid transaction or a double-applied deletion.
///
/// Members that resolve to nothing — an empty range at the end of the
/// document — drop out rather than contributing an empty edit.
pub fn selection_ranges(
    presentation: &TuiDocumentPresentation,
    snapshot: &DocumentSnapshot,
) -> Vec<std::ops::Range<usize>> {
    merge_ranges(
        presentation
            .selections
            .iter()
            .filter_map(|selection| member_range(snapshot, selection))
            .collect(),
    )
}

/// Sort `ranges` ascending and collapse every overlapping or touching pair.
///
/// Touching counts as overlapping: `a..b` and `b..c` delete the same bytes as
/// `a..c`, and collapsing them keeps the edit count — and the 256-edit budget
/// — proportional to distinct regions rather than to cursor count.
pub fn merge_ranges(mut ranges: Vec<std::ops::Range<usize>>) -> Vec<std::ops::Range<usize>> {
    ranges.retain(|range| range.start < range.end);
    ranges.sort_by_key(|range| range.start);
    let mut merged: Vec<std::ops::Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    merged
}

/// One member's range, or `None` when it covers nothing.
fn member_range(
    snapshot: &DocumentSnapshot,
    selection: &crate::tui::helix_selection::Selection<EditorPoint>,
) -> Option<std::ops::Range<usize>> {
    let (first, last) = selection.bounds();
    let (start, end) = match selection.mode {
        crate::tui::helix_selection::SelectionMode::Char => {
            let start = clamp_offset(snapshot, first.byte);
            // `char_end`, not a bare `next_char_boundary(..).unwrap_or(last.byte)`:
            // at the very end of a document there is no next character, and the
            // bare fallback left `end == last.byte`, which **drops the head
            // character** from a selection that reaches it. `v` on the final
            // character of a file then resolved to an empty range, so `d` on it
            // deleted nothing and the reader saw a highlight that would not go.
            // `char_end` is the same answer the rest of the module already
            // uses for "one character from here", including at the end.
            let end = char_end(snapshot, last.byte);
            (start, end)
        }
        crate::tui::helix_selection::SelectionMode::Line => {
            let last_line = snapshot.len_lines().saturating_sub(1);
            let first_line = first.line.min(last_line);
            let final_line = last.line.min(last_line);
            let start = line_start(snapshot, first_line).unwrap_or(0);
            // `final_line + 1` is the start of the following line, which for
            // the last line is the end of the buffer. That is what makes a
            // whole-line delete leave no trailing blank line.
            let end = line_start(snapshot, final_line + 1)
                .unwrap_or_else(|| snapshot.len_bytes())
                .min(snapshot.len_bytes());
            (start, end)
        }
    };
    (start < end).then_some(start..end)
}

/// Apply one Helix verb.
///
/// The **rules** live in [`crate::tui::helix_selection`]; this owns the
/// geometry only, which is what the shared module deliberately does not know:
/// which byte starts the next line, and which offset a motion resolved to.
/// Restoring the state machine per verb rather than re-deriving `v`/`x` here
/// is what keeps the editor and `/terminal` agreeing about what `x` does.
///
/// The whole set is restored and written back, so a verb applies to every
/// member. `cursor_byte` remains the primary's head — there is no second
/// cursor concept, which is why the viewport, LSP reads, and undo entries all
/// keep addressing one offset.
pub fn apply_helix_verb(
    snapshot: &DocumentSnapshot,
    presentation: &mut TuiDocumentPresentation,
    verb: HelixVerb,
) -> HelixVerbOutcome {
    let mut state = SelectionState::restore_set(
        editor_point(snapshot, presentation.cursor_byte),
        presentation.selections.clone(),
        presentation.primary_index,
    );
    let mut outcome = HelixVerbOutcome::Handled;
    let mut scrolled_to = presentation.viewport_line;
    // Only a find motion writes a record; `None` means this verb moved the
    // cursor and deliberately left the repeatable-motion slot alone.
    let mut find_record: Option<(Option<FindMotion>, FindNote)> = None;
    let mut paragraph_record: Option<ParagraphNote> = None;
    let mut match_record: Option<MatchNote> = None;
    // Seeded from the real presentation so a verb that never resolves a
    // motion — `v`, `x` — leaves the column exactly as it found it.
    let mut probe_desired_column = presentation.desired_column;
    match verb {
        HelixVerb::SelectChar => state.toggle_char(),
        HelixVerb::SelectLine => {
            // `x` is an action, not a mode: the second press grows the
            // selection by exactly one line, and only while nothing has moved
            // since. That condition belongs to the shared machine because it
            // is the same one the terminal uses.
            //
            // Growth is per member: members on different lines must each walk
            // to *their* next line, so the surface resolves one point per
            // member rather than one point for the set.
            if state.line_select_extends() {
                state.grow_all_to(|selection| {
                    editor_point(
                        snapshot,
                        next_line_content_start(snapshot, selection.head.line),
                    )
                });
            } else {
                state.select_line();
            }
        }
        // Delete and yank are *effects* over a range the machine describes,
        // not transitions of the selection, so they read the range and drop
        // the selection outright. Both still claim the key with nothing held:
        // an unclaimed `d` escapes to the global `OpenDiff` binding, and an
        // unclaimed `y` reads as a dead key.
        //
        // A **set**, because Helix's `d` deletes every selection at once, and
        // merging happens in `selection_ranges` rather than here so the
        // overlap rule lives in one place.
        //
        // The selection is **left in place** for the caller to consume. That
        // is what lets the undo entry record the set the reader actually had:
        // clearing here would mean `u` after a multi-cursor delete restored the
        // text and dropped the cursors on the floor. The caller owns clearing
        // because the caller is the layer that knows whether an edit landed.
        HelixVerb::DeleteSelection => {
            let ranges = selection_ranges(presentation, snapshot);
            return if ranges.is_empty() {
                HelixVerbOutcome::Handled
            } else {
                HelixVerbOutcome::DeleteRanges(ranges)
            };
        }
        HelixVerb::YankSelection => {
            let ranges = selection_ranges(presentation, snapshot);
            return if ranges.is_empty() {
                HelixVerbOutcome::Handled
            } else {
                HelixVerbOutcome::YankRanges(ranges)
            };
        }
        HelixVerb::Moved(command) => {
            // Resolved against a throwaway presentation so the real one is
            // written exactly once, below, through the shared machine — which
            // is what decides whether this move extends a character selection
            // or only disarms line growth.
            //
            // The find record and its disclosure ride along with the cursor
            // and the scroll for the same reason: a find motion is the one
            // motion that also writes state about the *attempt*, and it
            // resolves on the probe. Both are seeded from the real
            // presentation first so a non-find motion — which writes neither
            // — cannot erase a record the dispatcher deliberately kept.
            let mut probe = TuiDocumentPresentation {
                viewport_line: presentation.viewport_line,
                cursor_byte: presentation.cursor_byte,
                desired_column: presentation.desired_column,
                last_find: presentation.last_find,
                find_note: presentation.find_note,
                // Same reason as the two above: `]p` reached through `Alt-.`
                // resolves on the probe, so a disclosure left on the probe
                // would be a refusal the repeat could not explain.
                paragraph_note: presentation.paragraph_note,
                // Same reason as the two above: the bracket note describes
                // the motion that just ran, and a motion that resolves on the
                // probe must not leave a stale one behind.
                match_note: presentation.match_note,
                ..TuiDocumentPresentation::default()
            };
            let next = apply_motion(&mut probe, snapshot, command, NOMINAL_HELIX_PAGES);
            // The scroll has to come back with the cursor: it was resolved on
            // the probe, and dropping it is what made a second `Ctrl+d` land on
            // the same line as the first.
            scrolled_to = probe.viewport_line;
            probe_desired_column = probe.desired_column;
            state.move_to(editor_point(snapshot, next));
            outcome = HelixVerbOutcome::Moved(next);
            find_record = Some((probe.last_find, probe.find_note));
            paragraph_record = Some(probe.paragraph_note);
            match_record = Some(probe.match_note);
        }
        // ── Set verbs ────────────────────────────────────────────────────
        //
        // All of them are answered against `state`, so each runs through the
        // same restore/write-back pair this function already does and none of
        // them can leave the presentation holding a set the rules would not
        // have produced. The mutating three return before the write-back: an
        // edit invalidates every stored offset, so writing the set back and
        // then replacing the text would leave members pointing at a document
        // that no longer has those bytes. `submit_edit` clears them instead.
        HelixVerb::CollapseSelection => {
            if !state.collapse_to_primary() {
                return SetVerbOutcome::NothingToDo("nothing selected".to_string()).into();
            }
        }
        HelixVerb::FlipSelections => {
            if !state.flip_all() {
                return SetVerbOutcome::NothingToDo("nothing selected".to_string()).into();
            }
        }
        HelixVerb::EnsureSelectionsForward => {
            if !state.ensure_forward() {
                return SetVerbOutcome::NothingToDo(
                    "every selection is already forward".to_string(),
                )
                .into();
            }
        }
        HelixVerb::SelectAll => select_all(snapshot, &mut state),
        HelixVerb::KeepPrimarySelection => {
            if !state.keep_primary() {
                return SetVerbOutcome::NothingToDo("only one selection is held".to_string())
                    .into();
            }
        }
        HelixVerb::RemovePrimarySelection => {
            if !state.remove_primary() {
                return SetVerbOutcome::NothingToDo("nothing selected".to_string()).into();
            }
        }
        HelixVerb::CopySelectionOnLine { line_delta } => {
            if let SetVerbOutcome::NothingToDo(reason) =
                copy_selection_on_line(snapshot, &mut state, line_delta)
            {
                return HelixVerbOutcome::NothingToDo(reason);
            }
        }
        HelixVerb::RotateSelections { member_delta } => {
            if !state.rotate_primary(member_delta) {
                return SetVerbOutcome::NothingToDo("only one selection is held".to_string())
                    .into();
            }
        }
        HelixVerb::MergeSelections => {
            if !state.merge_all() {
                return SetVerbOutcome::NothingToDo("nothing to merge".to_string()).into();
            }
        }
        HelixVerb::MergeConsecutiveSelections => {
            if let Err(reason) = member_ranges(snapshot, &state) {
                return HelixVerbOutcome::NothingToDo(reason);
            }
            // Adjacency is byte geometry, so the surface decides it and the
            // set rule decides what to do with the answer.
            if !state.merge_consecutive(|previous, member| {
                members_are_adjacent(snapshot, previous, member)
            }) {
                return SetVerbOutcome::NothingToDo("no two selections are adjacent".to_string())
                    .into();
            }
        }
        HelixVerb::RotateSelectionContents { member_delta } => {
            return match rotate_selection_contents(snapshot, &state, member_delta) {
                Ok(edit) => set_edit_outcome(edit),
                Err(reason) => HelixVerbOutcome::NothingToDo(reason),
            };
        }
        HelixVerb::AlignSelections => {
            return match align_selections(snapshot, &state) {
                Ok(edit) => set_edit_outcome(edit),
                Err(reason) => HelixVerbOutcome::NothingToDo(reason),
            };
        }
        HelixVerb::SplitSelection { ref pattern } => {
            return match split_selection(snapshot, &mut state, pattern) {
                Ok(SetVerbOutcome::Handled) => {
                    // A regex verb resolves no motion, so the find slot is
                    // `None` and this cannot clear a record the dispatcher
                    // meant to keep.
                    write_back(
                        &state,
                        presentation,
                        scrolled_to,
                        probe_desired_column,
                        find_record,
                        paragraph_record,
                        match_record,
                    );
                    HelixVerbOutcome::Handled
                }
                Ok(SetVerbOutcome::NothingToDo(reason)) => HelixVerbOutcome::NothingToDo(reason),
                Err(reason) => HelixVerbOutcome::NothingToDo(reason),
            };
        }
        HelixVerb::KeepSelections { ref pattern } => {
            return match filter_selections(snapshot, &mut state, pattern, true) {
                Ok(SetVerbOutcome::Handled) => {
                    // A regex verb resolves no motion, so the find slot is
                    // `None` and this cannot clear a record the dispatcher
                    // meant to keep.
                    write_back(
                        &state,
                        presentation,
                        scrolled_to,
                        probe_desired_column,
                        find_record,
                        paragraph_record,
                        match_record,
                    );
                    HelixVerbOutcome::Handled
                }
                Ok(SetVerbOutcome::NothingToDo(reason)) => HelixVerbOutcome::NothingToDo(reason),
                Err(reason) => HelixVerbOutcome::NothingToDo(reason),
            };
        }
        HelixVerb::RemoveSelections { ref pattern } => {
            return match filter_selections(snapshot, &mut state, pattern, false) {
                Ok(SetVerbOutcome::Handled) => {
                    // A regex verb resolves no motion, so the find slot is
                    // `None` and this cannot clear a record the dispatcher
                    // meant to keep.
                    write_back(
                        &state,
                        presentation,
                        scrolled_to,
                        probe_desired_column,
                        find_record,
                        paragraph_record,
                        match_record,
                    );
                    HelixVerbOutcome::Handled
                }
                Ok(SetVerbOutcome::NothingToDo(reason)) => HelixVerbOutcome::NothingToDo(reason),
                Err(reason) => HelixVerbOutcome::NothingToDo(reason),
            };
        }
        // ── The `m` grammar ──────────────────────────────────────────────
        //
        // A text object is a **selection**, so it goes through the plain verb
        // path: it sets the set and edits no byte, and it pushes no undo
        // entry. The three surround verbs are the opposite — they edit, so
        // they return their transaction to the caller and the caller submits
        // it through the shared funnel.
        HelixVerb::SelectObject { kind, around } => {
            let cursor = presentation.cursor_byte;
            let range = match resolve_object(snapshot, kind, around, cursor) {
                Ok(range) => range,
                Err(reason) => return HelixVerbOutcome::NothingToDo(reason),
            };
            let Some(member) = member_for_range(snapshot, range) else {
                return HelixVerbOutcome::NothingToDo("the object is empty".to_string());
            };
            // The cursor moves to the object: the reader asked for a thing,
            // and leaving the cursor where it was would make the selection
            // appear somewhere they did not ask about.
            state.set_members(editor_point(snapshot, member.anchor.byte), vec![member], 0);
        }
        HelixVerb::SurroundAdd { ch } => {
            return match surround_add(snapshot, &state, ch) {
                Ok(outcome) => outcome,
                Err(reason) => HelixVerbOutcome::NothingToDo(reason),
            };
        }
        HelixVerb::SurroundDelete { ch } => {
            return match surround_delete(snapshot, &state, ch) {
                Ok(outcome) => outcome,
                Err(reason) => HelixVerbOutcome::NothingToDo(reason),
            };
        }
        HelixVerb::SurroundReplace { from, to } => {
            return match surround_replace(snapshot, &state, from, to) {
                Ok(outcome) => outcome,
                Err(reason) => HelixVerbOutcome::NothingToDo(reason),
            };
        }
    }
    write_back(
        &state,
        presentation,
        scrolled_to,
        probe_desired_column,
        find_record,
        paragraph_record,
        match_record,
    );
    outcome
}

impl From<SetVerbOutcome> for HelixVerbOutcome {
    /// A set verb that changed nothing reports why, and one that changed
    /// something is ordinary `Handled` — the shared tail already writes it
    /// back.
    fn from(outcome: SetVerbOutcome) -> Self {
        match outcome {
            SetVerbOutcome::Handled => Self::Handled,
            SetVerbOutcome::NothingToDo(reason) => Self::NothingToDo(reason),
        }
    }
}

/// Write a restored set back onto the presentation.
///
/// The one place M007's set reaches the stored fields. Factored out of
/// [`apply_helix_verb`] because the three regex verbs need to write back and
/// *then* return, while every other verb falls through to the tail — and two
/// copies of "copy four fields out of the state machine" is two places for
/// the cursor and the primary to drift apart.
fn write_back(
    state: &SelectionState<EditorPoint>,
    presentation: &mut TuiDocumentPresentation,
    scrolled_to: usize,
    desired_column: Option<usize>,
    find_record: Option<(Option<FindMotion>, FindNote)>,
    paragraph_record: Option<ParagraphNote>,
    match_record: Option<MatchNote>,
) {
    presentation.cursor_byte = state.cursor().byte;
    presentation.selections = state.selections().to_vec();
    presentation.primary_index = state.primary_index().unwrap_or_default();
    presentation.viewport_line = scrolled_to;
    // Unconditional, unlike the find record below: `apply_motion` always
    // rewrites the desired column — `None` for every motion that is not a step
    // along a column — so a column from the previous motion must not survive a
    // `w` or a `G` just because those write no find record.
    presentation.desired_column = desired_column;
    if let Some((last_find, find_note)) = find_record {
        presentation.last_find = last_find;
        presentation.find_note = find_note;
    }
    if let Some(paragraph_note) = paragraph_record {
        presentation.paragraph_note = paragraph_note;
    }
    if let Some(match_note) = match_record {
        presentation.match_note = match_note;
    }
}

/// Whether two members' ranges touch, for `Alt-_`.
///
/// A closure over a snapshot rather than a method on the state machine, because
/// "adjacent" is byte geometry — a character selection ending at byte 4 and
/// one beginning at byte 5 are not adjacent, but line 3 and line 4 are, and
/// only the surface knows how wide a character is.
fn members_are_adjacent(
    snapshot: &DocumentSnapshot,
    previous: &Selection<EditorPoint>,
    member: &Selection<EditorPoint>,
) -> bool {
    let (Some(previous_range), Some(member_range)) = (
        member_range(snapshot, previous),
        member_range(snapshot, member),
    ) else {
        return false;
    };
    // Touching counts as adjacent: two selections that share a byte are one
    // region, and leaving them split would make `Alt-_` produce a set that
    // still highlights overlapping text.
    member_range.start <= previous_range.end
}

/// Nominal viewport height used to resolve a motion driven by a Helix verb.
///
/// The widget resolves the exact window at render time; a motion only needs
/// *a* height to turn a page into a line count, and the viewport follows the
/// cursor on the next frame.
const NOMINAL_HELIX_PAGES: usize = 20;

/// Byte offset of the first content byte on the line after `line`.
///
/// Clamped to the end of the buffer, so `x` on the last line grows into the
/// terminator rather than past the document.
fn next_line_content_start(snapshot: &DocumentSnapshot, line: usize) -> usize {
    let last = snapshot.len_lines().saturating_sub(1);
    line_start(snapshot, (line + 1).min(last))
        .unwrap_or_else(|| line_content_end(snapshot, last).unwrap_or_else(|| snapshot.len_bytes()))
}

/// What a Helix verb asked the caller to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelixVerbOutcome {
    /// Pure navigation or selection; presentation state was updated.
    Handled,
    /// The cursor moved to this byte offset.
    Moved(usize),
    /// Delete these ranges, then clear the selection.
    ///
    /// A **set**, already merged and in document order: one transaction
    /// covers the whole list, and the caller must not split it.
    DeleteRanges(Vec<std::ops::Range<usize>>),
    /// Copy these ranges to the clipboard, then clear the selection.
    YankRanges(Vec<std::ops::Range<usize>>),
    /// Apply this edit, already built as one transaction.
    ///
    /// The mutating set verbs (`&`, `Alt-(`, `Alt-)`) produce their edit here
    /// and the App layer submits it through the same [`crate::tui::editor`]
    /// funnel every other edit uses, so undo, the read-only gate, and the
    /// selection-clearing rule are inherited rather than re-implemented.
    /// Apply this edit, already built as one transaction.
    ///
    /// The mutating set verbs (`&`, `Alt-(`, `Alt-)`) produce their edit here
    /// and the App layer submits it through the same funnel every other edit
    /// uses, so undo, the read-only gate, and the selection-clearing rule are
    /// inherited rather than re-implemented.
    ///
    /// The second field is an optional note to show *after* the edit lands —
    /// `&` uses it to disclose that a member was too wide to measure exactly,
    /// so its padding is approximate. Carried beside the edit rather than on
    /// the presentation so it is impossible to leave it behind.
    Edit(EditorEdit, Option<String>),
    /// Apply this edit, **and** hold the selection it produced.
    ///
    /// The surround verbs need this. `m s` inserts the delimiters and the
    /// surrounded text must stay selected, so the reader can go on to delete
    /// or replace it as one thing; `m d` and `m r` leave the contents
    /// selected.
    ///
    /// A separate variant rather than a field on [`Self::Edit`] because the
    /// two outcomes are decided in different places: the edit is built by the
    /// verb, and the selection is built from offsets that only make sense
    /// **after** the edit lands. `submit_edit` clears every selection on
    /// success — a selection is a set of byte offsets into text that no
    /// longer exists — so a verb that *means* to leave a selection behind
    /// has to say which one, and the caller has to apply it after the
    /// submission.
    ///
    /// The members are in **post-edit** coordinates.
    EditAndSelect(EditorEdit, Option<String>, Vec<Selection<EditorPoint>>),
    /// The verb had nothing to act on, carrying the reader-facing reason.
    ///
    /// Distinct from `Handled` because "you have nothing selected" and "`C`
    /// could not find a next line for this cursor" are different answers, and
    /// a reader who pressed `C` needs to be told which one they got. Consuming
    /// both silently is how a verb that does nothing reads as a broken key.
    NothingToDo(String),
}

// ── Set verbs ────────────────────────────────────────────────────────────────
//
// Everything above this line decides *what a verb means*; everything below
// resolves it against bytes. Two constraints shape all of it:
//
//   - **Bounded work.** Nothing here may read the whole buffer per keystroke.
//     Width measurement inherits the 32 KiB per-line budget; the verbs that
//     genuinely need member text — rotating contents, matching a regex —
//     declare their own ceiling and refuse rather than allocate without one.
//   - **One transaction.** Every mutating verb builds exactly one, and refuses
//     above `MAX_DOCUMENT_EDITS` instead of truncating. A partial application
//     would leave a set whose members point at text that is already gone.

/// Ceiling on the bytes one set verb will read across every member.
///
/// Sized to the documented frame budget (`MAX_FRAME_SOURCE_BYTES`) so a verb
/// cannot cost more per keystroke than drawing a frame does. Reached only by
/// a member spanning a whole file — `%` then `Alt-)`, say — and reported when
/// it is.
const MAX_SET_VERB_READ_BYTES: usize = MAX_FRAME_SOURCE_BYTES;

/// Ceiling on a compiled regex program, in bytes.
///
/// `regex` is a backtracking engine: a pattern that compiles into an enormous
/// program is the documented way to turn one keystroke into a hang. The limit
/// is applied at **compile** time so the pattern is refused before it ever
/// runs, which is the only point where it can be.
const MAX_REGEX_PROGRAM_BYTES: usize = 1 << 20;

/// What a pure set verb resolved to.
///
/// Separate from [`HelixVerbOutcome`] because most of these verbs never build
/// an edit; folding them into the outer enum would mean writing
/// `Handled` nine more times and losing the reason string on the three that
/// need one.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SetVerbOutcome {
    /// The set changed.
    Handled,
    /// Nothing to act on, with the reader-facing reason.
    NothingToDo(String),
}

/// Per-member byte ranges, in document order, **unmerged**.
///
/// Deliberately not [`selection_ranges`]: the verbs here need one range per
/// member so they can write one result back per member, and merging would
/// destroy that correspondence. Refuses an overlapping set rather than
/// silently picking a winner — two members that overlap cannot both own the
/// bytes between them, so any operation that writes per member would have to
/// invent which one.
fn member_ranges(
    snapshot: &DocumentSnapshot,
    state: &SelectionState<EditorPoint>,
) -> Result<Vec<std::ops::Range<usize>>, String> {
    let ranges = state
        .selections()
        .iter()
        .filter_map(|member| member_range(snapshot, member))
        .collect::<Vec<_>>();
    let mut sorted = ranges.clone();
    sorted.sort_by_key(|range| range.start);
    for pair in sorted.windows(2) {
        if pair[1].start < pair[0].end {
            return Err("two selections overlap — merge them with `Alt-minus` first".to_string());
        }
    }
    Ok(ranges)
}

/// Map a [`RangeSetEdit`] onto the outcome the caller dispatches on.
///
/// `Empty` and `TooMany` are deliberately *both* refusals with a message here:
/// the buffer layer does not know whether "nothing to do" and "too much to
/// do" are worth distinguishing to a reader, but the App layer does, and it
/// reads the reason rather than re-deriving it.
fn set_edit_outcome(edit: RangeSetEdit) -> HelixVerbOutcome {
    match edit {
        RangeSetEdit::Edit(edit, note) => HelixVerbOutcome::Edit(edit, note),
        RangeSetEdit::Empty => HelixVerbOutcome::NothingToDo("nothing selected".to_string()),
        RangeSetEdit::TooMany(count) => HelixVerbOutcome::NothingToDo(format!(
            "{count} ranges exceed one transaction's limit of {MAX_DOCUMENT_EDITS}"
        )),
    }
}

/// `%` — select the entire file as one primary member.
///
/// `line_start(0)..buffer_content_end`, which is the whole buffer. Note that
/// `buffer_content_end` reports the **end of the last line**, and a document
/// ending in a newline has an empty line after it — so the range covers the
/// terminator too. That is the correct reading of "select all": deleting it
/// empties the file, which is what a reader who pressed `%` then `d` is asking
/// for. Stopping short of the terminator would instead leave a bare `\n` and
/// make the buffer *look* emptied while still having a byte in it.
///
/// A member that resolves to an empty range — an empty document — is still
/// *held*. It is the honest description of "everything is selected", and
/// `d` on it correctly does nothing.
fn select_all(snapshot: &DocumentSnapshot, state: &mut SelectionState<EditorPoint>) {
    let first = line_start(snapshot, 0).unwrap_or(0);
    let last = buffer_content_end(snapshot);
    let mut member = Selection::new(editor_point(snapshot, first), SelectionMode::Char);
    member.head = editor_point(snapshot, last);
    // A one-member set has its primary at index 0 and there is nothing to
    // preserve, so the cursor stays exactly where the reader left it.
    state.set_members(state.cursor(), vec![member], 0);
}

/// `C` / `Alt-C` — copy every member onto the next / previous line.
///
/// Each added cursor is a **new member**, so pressing `C` repeatedly grows the
/// set the way a reader expects: one press gives two cursors, two give four.
/// It is also why the ceiling is a member ceiling and not a line ceiling.
///
/// A member with no line in that direction contributes nothing rather than
/// being clamped back onto its own line: a cursor that did not move is not the
/// "cursor below" the reader asked for, and silently adding it would make
/// `C` on the last line report success while leaving the set unchanged.
fn copy_selection_on_line(
    snapshot: &DocumentSnapshot,
    state: &mut SelectionState<EditorPoint>,
    line_delta: isize,
) -> SetVerbOutcome {
    let members: Vec<Selection<EditorPoint>> = state.selections().to_vec();
    // Bounded by the last line the **cursor** can reach, not by
    // [`last_line_of`]. `len_lines()` counts a trailing line that exists only
    // because the buffer ends in a newline, and no byte offset resolves into
    // it — `editor_point` clamps back onto the last real line. Bound by that
    // phantom line and `C` on the final row would resolve to the cursor's own
    // byte, add a "cursor below" that is a cursor above, and report success
    // while nothing moved. Every motion is addressed by byte, so the bound is
    // too.
    let last_reachable = cursor_line(snapshot, snapshot.len_bytes().saturating_sub(1));
    let mut added = Vec::with_capacity(members.len());
    for member in &members {
        let Some(shifted) = (member.head.line as isize).checked_add(line_delta) else {
            continue;
        };
        if shifted < 0 || shifted > last_reachable as isize {
            continue;
        }
        let target = point_on_line_like(snapshot, member.head.byte, shifted as usize);
        // Belt and braces: a target that resolved to the member's own byte is
        // not a cursor below it, whatever the reason the clamping happened.
        if target.byte == member.head.byte {
            continue;
        }
        added.push(Selection::new(target, SelectionMode::Char));
    }
    if added.is_empty() {
        return SetVerbOutcome::NothingToDo(if line_delta > 0 {
            "no cursor has a line below it".to_string()
        } else {
            "no cursor has a line above it".to_string()
        });
    }
    let total = members.len() + added.len();
    if total > MAX_DOCUMENT_EDITS {
        return SetVerbOutcome::NothingToDo(format!(
            "adding {total} cursors would exceed the limit of {MAX_DOCUMENT_EDITS}"
        ));
    }
    let mut next = members;
    next.extend(added);
    // The primary stays put: adding cursors is about coverage, not about
    // moving the reader's attention to one of the new ones.
    state.set_members(
        state.cursor(),
        next,
        state.primary_index().unwrap_or_default(),
    );
    SetVerbOutcome::Handled
}

/// The point on `target_line` at the same byte column as `from_byte`.
///
/// Column, not byte: a shorter target line clamps to its content end rather
/// than running off into the next one, which is what makes `C` onto a one
/// character line land at that character.
fn point_on_line_like(
    snapshot: &DocumentSnapshot,
    from_byte: usize,
    target_line: usize,
) -> EditorPoint {
    let from_line = cursor_line(snapshot, from_byte);
    let from_start = line_start(snapshot, from_line).unwrap_or(0);
    let column = from_byte.saturating_sub(from_start);
    let target_start = line_start(snapshot, target_line).unwrap_or(0);
    let target_end = line_content_end(snapshot, target_line).unwrap_or(target_start);
    editor_point(
        snapshot,
        target_start.saturating_add(column).min(target_end),
    )
}

/// `Alt-(` / `Alt-)` — rotate the members' *contents* around the set.
///
/// Member `i` receives what member `i + delta` held. Because every member is
/// a **replace** edit rather than a move, the whole rotation is one
/// transaction over the original coordinates: nothing is rewritten twice and
/// no intermediate text is observable.
///
/// Refuses an overlapping set outright — see [`member_ranges`] — and a
/// member set too large to read within the verb's byte budget. Either would
/// otherwise mean writing text into a member that partly owns someone else's
/// bytes.
fn rotate_selection_contents(
    snapshot: &DocumentSnapshot,
    state: &SelectionState<EditorPoint>,
    member_delta: isize,
) -> Result<RangeSetEdit, String> {
    let ranges = member_ranges(snapshot, state)?;
    if ranges.len() < 2 {
        return Ok(RangeSetEdit::Empty);
    }
    if ranges.len() > MAX_DOCUMENT_EDITS {
        return Ok(RangeSetEdit::TooMany(ranges.len()));
    }
    let mut total = 0usize;
    let mut texts = Vec::with_capacity(ranges.len());
    for range in &ranges {
        total = total.saturating_add(range.end.saturating_sub(range.start));
        if total > MAX_SET_VERB_READ_BYTES {
            return Err(format!(
                "rotating would read {total} bytes, past the {MAX_SET_VERB_READ_BYTES}-byte budget"
            ));
        }
        match snapshot.read_bytes(range.start, range.end) {
            Ok(text) => texts.push(text),
            Err(_) => return Err("a selection could not be read".to_string()),
        }
    }
    let count = texts.len() as isize;
    let edits: Vec<TextEdit> = ranges
        .iter()
        .enumerate()
        .map(|(index, range)| {
            let from = (index as isize + member_delta).rem_euclid(count) as usize;
            TextEdit::new(range.clone(), texts[from].clone())
        })
        .collect();
    // A rotation of a set whose texts are all identical produces a transaction
    // that changes nothing; refusing it keeps `u` free of an entry that would
    // undo nothing, which is the M004 rule.
    if edits.iter().zip(&ranges).all(|(edit, range)| {
        snapshot
            .read_bytes(range.start, range.end)
            .is_ok_and(|t| t == edit.insert)
    }) {
        return Ok(RangeSetEdit::Empty);
    }
    let cursor_after = ranges
        .get(state.primary_index().unwrap_or_default())
        .map(|range| range.start)
        .unwrap_or_else(|| snapshot.len_bytes());
    Ok(
        match build_edit(snapshot, TextTransaction::new(edits), cursor_after, false) {
            Some(edit) => RangeSetEdit::Edit(edit, None),
            None => RangeSetEdit::Empty,
        },
    )
}

/// `&` — pad every member out to the widest member's **display** width.
///
/// Display width, not bytes, is the whole content of the verb: a tab is one
/// column and a CJK ideograph is two, so byte-aligning them would produce
/// output that lines up in the source and not on the screen.
///
/// Measured through [`bounded_display_width`] so a member wider than the
/// 32 KiB line budget costs the budget and not the line — and when that
/// happens the truncation is **disclosed** rather than papered over, because
/// a member padded to a width that was measured over a prefix of it is
/// genuinely misaligned and the reader deserves to know that.
///
/// Padding is appended after each member, so equal-width members end in the
/// same column. That is the reading that makes the verb useful for the case it
/// exists for — lining up a trailing `|`, a comment marker, or a comma
/// across several edited lines.
fn align_selections(
    snapshot: &DocumentSnapshot,
    state: &SelectionState<EditorPoint>,
) -> Result<RangeSetEdit, String> {
    let ranges = member_ranges(snapshot, state)?;
    if ranges.is_empty() {
        return Ok(RangeSetEdit::Empty);
    }
    let mut measured = Vec::with_capacity(ranges.len());
    let mut truncated = false;
    for range in &ranges {
        let (width, cut) = bounded_display_width(snapshot, range.clone());
        truncated |= cut;
        measured.push((range, width));
    }
    let target = measured.iter().map(|(_, width)| *width).max().unwrap_or(0);
    let edits: Vec<TextEdit> = measured
        .iter()
        .filter_map(|(range, width)| {
            let pad = target.saturating_sub(*width);
            (pad > 0).then(|| TextEdit::new(range.end..range.end, " ".repeat(pad)))
        })
        .collect();
    if edits.is_empty() {
        return Ok(RangeSetEdit::Empty);
    }
    if edits.len() > MAX_DOCUMENT_EDITS {
        return Ok(RangeSetEdit::TooMany(edits.len()));
    }
    let note = truncated.then(|| {
        format!(
            "a selection was wider than the {MAX_RENDER_LINE_BYTES}-byte measure budget; its padding is approximate"
        )
    });
    Ok(
        match build_edit(
            snapshot,
            TextTransaction::new(edits),
            snapshot.len_bytes(),
            false,
        ) {
            Some(edit) => RangeSetEdit::Edit(edit, note),
            None => RangeSetEdit::Empty,
        },
    )
}

/// `S` — split every member into one member per regex match.
fn split_selection(
    snapshot: &DocumentSnapshot,
    state: &mut SelectionState<EditorPoint>,
    pattern: &str,
) -> Result<SetVerbOutcome, String> {
    let regex = compile_selection_regex(pattern)?;
    let mut split = Vec::new();
    let mut budget = 0usize;
    for member in state.selections() {
        let Some(range) = member_range(snapshot, member) else {
            continue;
        };
        let window = bounded_regex_window(snapshot, range.clone(), &mut budget);
        let Ok(text) = snapshot.read_bytes(window.start, window.end) else {
            continue;
        };
        for found in regex.find_iter(&text) {
            let Some(head) = last_char_start(snapshot, window.start + found.end()) else {
                continue;
            };
            split.push(Selection::new(
                editor_point(snapshot, window.start + found.start()),
                SelectionMode::Char,
            ));
            let index = split.len() - 1;
            split[index].head = editor_point(snapshot, head);
        }
    }
    if split.is_empty() {
        return Ok(SetVerbOutcome::NothingToDo(format!(
            "`{pattern}` matched none of the {} selected regions",
            state.selections().len()
        )));
    }
    if split.len() > MAX_DOCUMENT_EDITS {
        return Ok(SetVerbOutcome::NothingToDo(format!(
            "`{pattern}` matched {n} times, past the {MAX_DOCUMENT_EDITS}-selection limit",
            n = split.len()
        )));
    }
    // A match is a point pair, so the primary index is meaningless across a
    // split; keeping the cursor's own member as primary would be more work
    // than it is worth when the members have just been rebuilt from text.
    state.set_members(state.cursor(), split, 0);
    Ok(SetVerbOutcome::Handled)
}

/// `K` / `Alt-K` — keep or remove the members whose text matches.
fn filter_selections(
    snapshot: &DocumentSnapshot,
    state: &mut SelectionState<EditorPoint>,
    pattern: &str,
    keep: bool,
) -> Result<SetVerbOutcome, String> {
    let regex = compile_selection_regex(pattern)?;
    let mut budget = 0usize;
    let survivors: Vec<Selection<EditorPoint>> = state
        .selections()
        .iter()
        .filter(|member| {
            let Some(range) = member_range(snapshot, member) else {
                return false;
            };
            let window = bounded_regex_window(snapshot, range, &mut budget);
            snapshot
                .read_bytes(window.start, window.end)
                .is_ok_and(|text| regex.is_match(&text) == keep)
        })
        .copied()
        .collect();
    if survivors.len() == state.selections().len() {
        return Ok(SetVerbOutcome::NothingToDo(format!(
            "`{pattern}` already {} every selection",
            if keep { "matched" } else { "missed" }
        )));
    }
    // An empty survivor set is a **result**, not a refusal. `K` that matches
    // nothing must leave nothing selected — that is what "keep what matches"
    // means when nothing does, and Helix behaves the same way. Refusing it
    // would leave the reader holding exactly the selections they asked to
    // discard, which is the failure mode §8 is about in its other direction.
    //
    // (This is the one place where the "report a no-op" rule does not apply:
    // nothing matching is not a no-op for `K`, it is the operation succeeding.)
    // A survivor inherits the primary so the reader keeps a distinguished
    // cursor; with nothing left the set is cleared.
    state.set_members(
        state.cursor(),
        survivors,
        state.primary_index().unwrap_or_default(),
    );
    Ok(SetVerbOutcome::Handled)
}

/// Compile `pattern`, refusing a program too large to run.
///
/// [`MAX_REGEX_PROGRAM_BYTES`] is a compile-time refusal on purpose: the
/// expensive thing about a pathological pattern is running it, and a pattern
/// that will not compile cannot run.
fn compile_selection_regex(pattern: &str) -> Result<regex::Regex, String> {
    if pattern.is_empty() {
        return Err("no pattern given".to_string());
    }
    regex::RegexBuilder::new(pattern)
        .size_limit(MAX_REGEX_PROGRAM_BYTES)
        .dfa_size_limit(MAX_REGEX_PROGRAM_BYTES)
        .build()
        .map_err(|error| format!("`{pattern}` is not a valid regex: {error}"))
}

/// The slice of `range` a regex verb is allowed to match against.
///
/// Bounded twice, because a member can be arbitrarily large: per member by the
/// 32 KiB line budget, and across the whole set by `budget`, which is
/// [`MAX_SET_VERB_READ_BYTES`]. Running out of budget ends the window rather
/// than the verb — the caller still gets the matches found so far, and the
/// truncation is visible because a match count that stops part-way is
/// visibly smaller than a full scan.
fn bounded_regex_window(
    snapshot: &DocumentSnapshot,
    range: std::ops::Range<usize>,
    budget: &mut usize,
) -> std::ops::Range<usize> {
    let end = bounded_prefix_end(snapshot, range.start, range.end);
    *budget = budget.saturating_add(end.saturating_sub(range.start));
    range.start..end
}

/// Byte offset of the last character that ends at `end`.
///
/// A character selection's range runs to `char_end(head)`, so a member built
/// to cover `start..end` has to name the **last character's** start as its
/// head — pointing it at `end` would cover one character too many.
fn last_char_start(snapshot: &DocumentSnapshot, end: usize) -> Option<usize> {
    let end = clamp_offset(snapshot, end);
    if end == 0 {
        return None;
    }
    let mut candidate = end - 1;
    while candidate > 0 && !snapshot.is_byte_boundary(candidate) {
        candidate -= 1;
    }
    Some(candidate)
}

/// Line reached by a half-page motion, from the current viewport height.
pub fn half_page_target(viewport_line: usize, visible_lines: usize, down: bool) -> usize {
    let step = (visible_lines / 2).max(1);
    if down {
        viewport_line.saturating_add(step)
    } else {
        viewport_line.saturating_sub(step)
    }
}

/// Place the cursor on `line` at the given byte column, clamped to that
/// line's content end.
pub fn offset_on_line(snapshot: &DocumentSnapshot, line: usize, column: usize) -> usize {
    let last = snapshot.len_lines().saturating_sub(1);
    let line = line.min(last);
    let start = line_start(snapshot, line).unwrap_or(0);
    let end = line_content_end(snapshot, line).unwrap_or(start);
    clamp_offset(snapshot, (start + column).min(end))
}

// ── Transaction construction ─────────────────────────────────────────────────

/// Compute the inverse of `transaction` against the pre-transaction text.
///
/// Mirrors the offset arithmetic `DocumentBuffer::apply` uses so undo and
/// redo are exact local inverses without the frontend ever retaining a copy
/// of the document.
pub fn invert(
    snapshot: &DocumentSnapshot,
    transaction: &TextTransaction,
) -> Option<TextTransaction> {
    let mut inverse = Vec::with_capacity(transaction.edits.len());
    let mut shift: isize = 0;
    for edit in &transaction.edits {
        let removed = snapshot.read_bytes(edit.range.start, edit.range.end).ok()?;
        let post_start =
            usize::try_from(isize::try_from(edit.range.start).ok()?.checked_add(shift)?).ok()?;
        let post_end = post_start.checked_add(edit.insert.len())?;
        inverse.push(TextEdit::new(post_start..post_end, removed));
        let inserted = isize::try_from(edit.insert.len()).ok()?;
        let removed_len = isize::try_from(edit.range.end - edit.range.start).ok()?;
        shift = shift.checked_add(inserted - removed_len)?;
    }
    Some(TextTransaction::new(inverse))
}

fn build_edit(
    snapshot: &DocumentSnapshot,
    transaction: TextTransaction,
    cursor_after: usize,
    stay_insert: bool,
) -> Option<EditorEdit> {
    if transaction.edits.is_empty() {
        return None;
    }
    let inverse = invert(snapshot, &transaction)?;
    let bytes = transaction
        .edits
        .iter()
        .map(|edit| edit.insert.len())
        .sum::<usize>()
        + inverse
            .edits
            .iter()
            .map(|edit| edit.insert.len())
            .sum::<usize>();
    Some(EditorEdit {
        transaction,
        inverse,
        bytes,
        // Deliberately not clamped against `snapshot`: that is the
        // *pre*-edit text, and an insertion legitimately places the
        // cursor beyond it. The caller clamps against the post-edit
        // snapshot once the controller has accepted the transaction.
        cursor_after,
        stay_insert,
    })
}

/// Insert `text` at `offset`, leaving the cursor after the inserted bytes.
pub fn insert_text(snapshot: &DocumentSnapshot, offset: usize, text: &str) -> Option<EditorEdit> {
    if text.is_empty() {
        return None;
    }
    let offset = clamp_offset(snapshot, offset);
    let transaction = TextTransaction::new(vec![TextEdit::new(offset..offset, text.to_string())]);
    build_edit(snapshot, transaction, offset + text.len(), true)
}

/// What a verb that acts on a **set** of ranges produced.
///
/// Separate from `Option<EditorEdit>` because the two failures are not the
/// same and the caller has to report them differently: "nothing to do" is
/// silence, while "too many selections for one transaction" must be a visible
/// refusal. Collapsing both into `None` is how a reader ends up pressing `d`
/// on two hundred cursors and watching some of them vanish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RangeSetEdit {
    /// One transaction carrying every range.
    ///
    /// The note rides with the edit because it describes the edit: `&` builds
    /// this only when a member was too wide to measure exactly, and that fact
    /// has to reach the reader. Carrying it beside the edit rather than on the
    /// presentation is what makes "leave a notice behind" impossible.
    Edit(EditorEdit, Option<String>),
    /// The set resolved to no non-empty range; nothing to apply.
    Empty,
    /// The set has more distinct ranges than one transaction may carry.
    ///
    /// Carries the count so the notice can name it.
    TooMany(usize),
}

impl RangeSetEdit {
    /// The transaction, when there is one.
    pub fn edit(self) -> Option<EditorEdit> {
        match self {
            Self::Edit(edit, _) => Some(edit),
            Self::Empty | Self::TooMany(_) => None,
        }
    }
}

/// Delete `range`, leaving the cursor at its start.
pub fn delete_range(
    snapshot: &DocumentSnapshot,
    range: std::ops::Range<usize>,
) -> Option<EditorEdit> {
    delete_ranges(snapshot, vec![range]).edit()
}

/// Delete every range in one transaction, leaving the cursor at the start of
/// the first.
///
/// One transaction, not N. The editor's controller has no multi-call concept
/// here, and a partially applied multi-cursor delete would leave the reader
/// with a set whose members point at text that is already gone — a state
/// neither `u` nor `redo` can describe.
///
/// Ranges are clamped and merged before submission. Merging is a requirement
/// rather than a tidy-up: the transaction validator rejects overlapping edits
/// outright, and `invert` only produces an exact inverse for edits that
/// ascend and do not overlap.
pub fn delete_ranges(
    snapshot: &DocumentSnapshot,
    ranges: Vec<std::ops::Range<usize>>,
) -> RangeSetEdit {
    let ranges = merge_ranges(
        ranges
            .into_iter()
            .map(|range| {
                let start = clamp_offset(snapshot, range.start);
                let end = clamp_offset(snapshot, range.end);
                start..end
            })
            .collect(),
    );
    if ranges.is_empty() {
        return RangeSetEdit::Empty;
    }
    if ranges.len() > MAX_DOCUMENT_EDITS {
        return RangeSetEdit::TooMany(ranges.len());
    }
    let transaction = TextTransaction::new(
        ranges
            .iter()
            .map(|range| TextEdit::new(range.clone(), String::new()))
            .collect(),
    );
    match build_edit(snapshot, transaction, ranges[0].start, false) {
        Some(edit) => RangeSetEdit::Edit(edit, None),
        None => RangeSetEdit::Empty,
    }
}

/// The byte just past the character that starts at `offset`.
///
/// [`next_char_boundary`] answers "where does the *next* character begin", so
/// it returns `None` for a document that is a single character. That is right
/// at the end of a document and wrong at the start of one: on `"日"`,
/// `next_char_boundary(0)` is `None` even though there is a real character
/// under the cursor. Falling back to the document length is what tells those
/// two cases apart, and it is the same distinction M005 hit when a word
/// scanner could not advance at the end of a multi-byte character.
fn char_end(snapshot: &DocumentSnapshot, offset: usize) -> usize {
    let offset = clamp_offset(snapshot, offset);
    next_char_boundary(snapshot, offset).unwrap_or(snapshot.len_bytes())
}

/// Every byte range a selection-consuming verb acts on.
///
/// Helix's `r`, `R`, and `~` all name their target as *the selection*, and a
/// bare cursor in this editor is not a selection — it is an offset. So the
/// rule is one shared answer rather than three: every held selection when
/// there is one, otherwise the single character under the cursor.
///
/// With the cursor at the end of the document there is no character left to
/// act on, so the range is empty and every verb built on this is a no-op.
/// That is the same answer `DeleteChar` gives at the end of the buffer, and
/// for the same reason: there is nothing to change.
fn verb_targets(
    snapshot: &DocumentSnapshot,
    presentation: &TuiDocumentPresentation,
) -> Vec<std::ops::Range<usize>> {
    let ranges = selection_ranges(presentation, snapshot);
    if !ranges.is_empty() {
        return ranges;
    }
    let cursor = clamp_offset(snapshot, presentation.cursor_byte);
    // `std::iter::once` rather than `vec![..]`: clippy's
    // `single_range_in_vec_init` fires on the literal and both of its
    // suggested rewrites mean something else for a range set.
    std::iter::once(cursor..char_end(snapshot, cursor)).collect()
}

/// Where a paste lands, per target.
///
/// "After" and "before" are measured against **what is under the cursor**:
/// with nothing selected that is one character, so `p` inserts past it rather
/// than splitting it — `p` with the cursor on `a` in `abc` yields `aXbc`. At
/// the end of the buffer there is no character, so "after" is the end of the
/// document, which is where a paste has to go to insert anything at all.
///
/// The anchors come back ascending and disjoint because the ranges were
/// merged, so two members can never target the same byte and double-insert.
fn paste_anchors(
    snapshot: &DocumentSnapshot,
    presentation: &TuiDocumentPresentation,
    after: bool,
) -> Vec<usize> {
    let ranges = selection_ranges(presentation, snapshot);
    if !ranges.is_empty() {
        return ranges
            .iter()
            .map(|range| if after { range.end } else { range.start })
            .collect();
    }
    let cursor = clamp_offset(snapshot, presentation.cursor_byte);
    vec![if after {
        char_end(snapshot, cursor)
    } else {
        cursor
    }]
}

/// Helix `p` — paste `text` after every target, or after the character
/// under the cursor.
///
/// The cursor lands on the **last byte of the last pasted run**, which is what
/// makes `p p` append twice rather than skip over the character that followed
/// the first paste.
pub fn paste_after(
    snapshot: &DocumentSnapshot,
    presentation: &TuiDocumentPresentation,
    text: &str,
) -> Option<EditorEdit> {
    paste(snapshot, presentation, text, true)
}

/// Helix `P` — paste `text` before every target, or before the character
/// under the cursor.
pub fn paste_before(
    snapshot: &DocumentSnapshot,
    presentation: &TuiDocumentPresentation,
    text: &str,
) -> Option<EditorEdit> {
    paste(snapshot, presentation, text, false)
}

fn paste(
    snapshot: &DocumentSnapshot,
    presentation: &TuiDocumentPresentation,
    text: &str,
    after: bool,
) -> Option<EditorEdit> {
    if text.is_empty() {
        return None;
    }
    let anchors = paste_anchors(snapshot, presentation, after);
    if anchors.len() > MAX_DOCUMENT_EDITS {
        return None;
    }
    let transaction = TextTransaction::new(
        anchors
            .iter()
            .map(|at| TextEdit::new(*at..*at, text.to_string()))
            .collect(),
    );
    // On the last byte written, like a replacement: in Normal mode the cursor
    // sits *on* a character, so `at + len` would put it on whatever followed
    // the paste, and `p p` would step over that character on the way out.
    // With several targets the cursor belongs to the last one — the only one a
    // single `cursor_byte` can honestly describe.
    //
    // `stay_insert: false` — paste is a Normal-mode verb. `insert_text`'s
    // `true` is right for typing and wrong here: an accepted paste must not
    // hand the keyboard to the document.
    let last = *anchors.last()?;
    build_edit(
        snapshot,
        transaction,
        last + text.len().saturating_sub(1),
        false,
    )
}

/// Helix `r {ch}` — replace the selection, or the character under the
/// cursor, with `ch`.
pub fn replace_char(
    snapshot: &DocumentSnapshot,
    presentation: &TuiDocumentPresentation,
    ch: char,
) -> Option<EditorEdit> {
    replace_targets(
        snapshot,
        &verb_targets(snapshot, presentation),
        &ch.to_string(),
    )
    .edit()
}

/// Helix `R` — replace the selection, or the character under the cursor,
/// with `text`.
///
/// Helix binds this to `replace_with_yanked`; the clipboard *is* this
/// editor's yanked text, because there is no local register. The target is
/// the same range `r` uses, so the two compose: `R` then `r a` replaces a
/// selection and then replaces the result again.
pub fn replace_with_text(
    snapshot: &DocumentSnapshot,
    presentation: &TuiDocumentPresentation,
    text: &str,
) -> Option<EditorEdit> {
    replace_targets(snapshot, &verb_targets(snapshot, presentation), text).edit()
}

/// Replace every target with `text`, in one transaction, leaving the cursor
/// on the last byte written.
///
/// `None`-equivalent for an empty replacement or an empty target set — a verb
/// that would change nothing is not an edit, and the caller needs to tell
/// "did nothing" apart from "refused".
fn replace_targets(
    snapshot: &DocumentSnapshot,
    targets: &[std::ops::Range<usize>],
    text: &str,
) -> RangeSetEdit {
    if text.is_empty() {
        return RangeSetEdit::Empty;
    }
    let ranges = merge_ranges(
        targets
            .iter()
            .map(|range| {
                let start = clamp_offset(snapshot, range.start);
                let end = clamp_offset(snapshot, range.end);
                start..end
            })
            .collect(),
    );
    if ranges.is_empty() {
        return RangeSetEdit::Empty;
    }
    if ranges.len() > MAX_DOCUMENT_EDITS {
        return RangeSetEdit::TooMany(ranges.len());
    }
    let last_start = ranges[ranges.len() - 1].start;
    let transaction = TextTransaction::new(
        ranges
            .iter()
            .map(|range| TextEdit::new(range.clone(), text.to_string()))
            .collect(),
    );
    match build_edit(
        snapshot,
        transaction,
        last_start + text.len().saturating_sub(1),
        false,
    ) {
        Some(edit) => RangeSetEdit::Edit(edit, None),
        None => RangeSetEdit::Empty,
    }
}

/// Helix `~` — switch the case of the selection, or of the character under
/// the cursor.
pub fn switch_case(
    snapshot: &DocumentSnapshot,
    presentation: &TuiDocumentPresentation,
) -> Option<EditorEdit> {
    case_targets(
        snapshot,
        &verb_targets(snapshot, presentation),
        CaseMode::Flip,
    )
    .edit()
}

/// Helix `` ` `` — set the selected text, or the character under the cursor,
/// to lower case.
pub fn switch_to_lower(
    snapshot: &DocumentSnapshot,
    presentation: &TuiDocumentPresentation,
) -> Option<EditorEdit> {
    case_targets(
        snapshot,
        &verb_targets(snapshot, presentation),
        CaseMode::Lower,
    )
    .edit()
}

/// Helix `` Alt-` `` — set the selected text, or the character under the
/// cursor, to upper case.
pub fn switch_to_upper(
    snapshot: &DocumentSnapshot,
    presentation: &TuiDocumentPresentation,
) -> Option<EditorEdit> {
    case_targets(
        snapshot,
        &verb_targets(snapshot, presentation),
        CaseMode::Upper,
    )
    .edit()
}

/// What a case verb does with each character it finds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaseMode {
    /// `~` — flip: lowercase to uppercase, uppercase to lowercase, leave
    /// caseless characters alone. Two presses return the original text,
    /// which is what makes `~` a toggle rather than a direction.
    Flip,
    /// `` ` `` — set: to lowercase.
    Lower,
    /// `` Alt-` `` — set: to uppercase.
    Upper,
}

/// Apply one case verb to every target, in one transaction.
///
/// Per character, not per-range: `~` on `abC` yields `Abc`, which is what
/// "flip the case" means.
///
/// The mapping is Unicode-aware and **expands**: `ß` uppercases to `SS`, and
/// `İ` lowercases to `i` plus a combining dot. `Lower` and `Upper` use the
/// whole expansion, because they name the whole thing. `Flip` cannot use one
/// and therefore leaves those characters alone: a flip asks for *the other
/// case of this character*, and `ß` has no single one — half of `SS` is not
/// a case. Half-converting it would also mean `~ ~` never returns to the
/// original text, so `~` would stop being a toggle. Reporting "nothing to
/// flip" is the answer that stays true.
///
/// Either way the inserted text may be longer than the range it replaces,
/// which is safe because `EditorEdit` carries an arbitrary transaction/inverse
/// pair and the caller's cursor clamp runs against the **post**-edit
/// snapshot.
///
/// An identity mapping — text that is already lower case under `` ` ``, or
/// already upper case under `Alt-` `, or unmappable under any of them — is
/// dropped from the transaction rather than submitted. Submitting one would
/// push an undo entry that undoes nothing, and make `u` appear to do nothing
/// on the first press. Dropping it *per target* rather than abandoning the
/// whole verb is what lets `~` flip the members it can and leave the member
/// that is already flipped alone; with one target the two readings coincide,
/// which is why this is not a behaviour change for any existing verb.
fn case_targets(
    snapshot: &DocumentSnapshot,
    targets: &[std::ops::Range<usize>],
    mode: CaseMode,
) -> RangeSetEdit {
    let mut edits: Vec<TextEdit> = Vec::with_capacity(targets.len());
    // Cursor lands on the last byte of the last edit that actually wrote, so
    // a verb whose final member was a no-op still leaves the cursor where the
    // text it changed ended.
    let mut cursor_after = None;
    for range in targets {
        let start = clamp_offset(snapshot, range.start);
        let end = clamp_offset(snapshot, range.end);
        if start >= end {
            continue;
        }
        // Both ends are clamped above, so this read cannot be refused for
        // straddling a character — the same hazard that made `read_bytes` a
        // forbidden way to peek at one byte inside a multi-byte character.
        let Some(original) = snapshot.read_bytes(start, end).ok() else {
            continue;
        };
        let Some(mapped) = map_case(&original, mode) else {
            continue;
        };
        cursor_after = Some(start + mapped.len().saturating_sub(1));
        edits.push(TextEdit::new(start..end, mapped));
    }
    if edits.is_empty() {
        return RangeSetEdit::Empty;
    }
    if edits.len() > MAX_DOCUMENT_EDITS {
        return RangeSetEdit::TooMany(edits.len());
    }
    match build_edit(
        snapshot,
        TextTransaction::new(edits),
        cursor_after.unwrap_or_default(),
        false,
    ) {
        Some(edit) => RangeSetEdit::Edit(edit, None),
        None => RangeSetEdit::Empty,
    }
}

/// Map one target's text under `mode`, or `None` when it would not change.
fn map_case(original: &str, mode: CaseMode) -> Option<String> {
    let mut mapped = String::with_capacity(original.len());
    for ch in original.chars() {
        match mode {
            CaseMode::Flip => {
                let other: String = if ch.is_lowercase() {
                    ch.to_uppercase().collect()
                } else if ch.is_uppercase() {
                    ch.to_lowercase().collect()
                } else {
                    String::new()
                };
                if other.chars().count() == 1 {
                    mapped.push_str(&other);
                } else {
                    mapped.push(ch);
                }
            }
            CaseMode::Lower => {
                if ch.is_lowercase() {
                    mapped.push(ch);
                } else {
                    mapped.extend(ch.to_lowercase());
                }
            }
            CaseMode::Upper => {
                if ch.is_uppercase() {
                    mapped.push(ch);
                } else {
                    mapped.extend(ch.to_uppercase());
                }
            }
        }
    }
    (mapped != original).then_some(mapped)
}

/// Resolve one M003 applicable completion edit against the current snapshot.
///
/// Validates the DTO's UTF-16 range against `snapshot` and builds the single
/// forward/inverse pair through the shared [`build_edit`], so an accepted
/// completion is exactly one controller edit with one undo unit — the same
/// shape as a typed edit. Returns `None` for out-of-snapshot ranges,
/// reversed ranges, oversized replacement text, and no-op edits: each is a
/// refusal the caller reports rather than applying something the server did
/// not mean. Pure deletions (range plus empty text) are meaningful and kept.
pub fn resolve_completion_edit(
    snapshot: &DocumentSnapshot,
    edit: &codegg_protocol::lsp::LspCompletionEditDto,
    stay_insert: bool,
) -> Option<EditorEdit> {
    if edit.new_text.len() > codegg_protocol::lsp::MAX_LSP_COMPLETION_EDIT_BYTES {
        return None;
    }
    let start_line = usize::try_from(edit.start_line).ok()?;
    let end_line = usize::try_from(edit.end_line).ok()?;
    let start_row = line_start(snapshot, start_line)?;
    let start_row_end = line_content_end(snapshot, start_line)?;
    let end_row = line_start(snapshot, end_line)?;
    let end_row_end = line_content_end(snapshot, end_line)?;
    let start_text = snapshot.read_bytes(start_row, start_row_end).ok()?;
    let end_text = snapshot.read_bytes(end_row, end_row_end).ok()?;
    let start_col =
        crate::tui::editor_lsp::utf16_column_to_byte(&start_text, edit.start_character)?;
    let end_col = crate::tui::editor_lsp::utf16_column_to_byte(&end_text, edit.end_character)?;
    let start = start_row + start_col;
    let end = end_row + end_col;
    if end < start {
        return None;
    }
    if start == end && edit.new_text.is_empty() {
        return None;
    }
    let transaction = TextTransaction::new(vec![TextEdit::new(start..end, edit.new_text.clone())]);
    build_edit(
        snapshot,
        transaction,
        start + edit.new_text.len(),
        stay_insert,
    )
}

// ── Resolution against a presentation record ────────────────────────────────

/// Apply a motion command to the presentation. Never touches text.
///
/// Deliberate movement that arrived through the Helix verbs goes through
/// [`apply_helix_verb`], which owns the selection rules. This is the
/// *raw* motion: it always drops a held selection, so an Insert-mode arrow or
/// an LSP jump leaves nothing highlighted rather than dragging an old anchor
/// onto wherever the cursor landed.
///
/// A find motion is the one command resolved here rather than in
/// [`apply_motion_inner`], because it is the only motion that has an
/// *outcome* to disclose: the bounded scan can stop short, and that has to
/// reach the status row whether or not the cursor moved. It is recorded
/// here, next to the resolution that produced it, rather than in the
/// dispatch layer that pressed the key — otherwise the note and the move
/// would be two facts about the same keystroke that can disagree.
///
/// A paragraph motion joins it for the same reason and under the same rule:
/// `]p` at the last paragraph must be able to say so. Resolving it here also
/// means the scan runs **once** — the note and the offset cannot come from two
/// passes that saw different documents.
pub fn apply_motion(
    presentation: &mut TuiDocumentPresentation,
    snapshot: &DocumentSnapshot,
    command: EditorCommand,
    visible_lines: usize,
) -> usize {
    let from = clamp_offset(snapshot, presentation.cursor_byte);
    // Read before `apply_motion_inner`, which reads it too, to resolve a
    // vertical move: the column it aimed at is what must survive, not the one
    // the target line clamped to.
    let aimed_at = presentation.desired_column;
    let find = command
        .find_motion()
        .map(|motion| (motion, motion.outcome(snapshot, from)));
    let paragraph = command
        .paragraph_motion()
        .map(|motion| (motion, motion.outcome(snapshot, from)));
    // The bracket match is the third motion with an **outcome** to disclose,
    // and it joins the other two for the same reason: `m m` on a bracket with
    // no mate must be able to say so, and the scan must run once so the note
    // and the offset cannot come from two passes that saw different text.
    let bracket = match command {
        EditorCommand::MatchBracket => Some(bracket_mate(snapshot, from)),
        _ => None,
    };
    let resolved = match (find, paragraph) {
        (Some((_, FindOutcome::Found(offset))), _) => offset,
        // A find that found nothing leaves the cursor exactly where it was,
        // and `apply_motion_inner`'s catch-all would do the same — but only
        // by accident, and it would leave the disclosure unwritten.
        (Some((_, FindOutcome::Absent | FindOutcome::Bounded)), _) => from,
        // Same for `]p`/`[p`: "there is no next paragraph" is a no-op that
        // must still be explained, and the catch-all would explain nothing.
        (_, Some((_, ParagraphOutcome::Landed(offset)))) => offset,
        (_, Some((_, ParagraphOutcome::NoParagraph | ParagraphOutcome::Bounded))) => from,
        (None, None) => match bracket {
            Some(BracketOutcome::Landed(offset)) => offset,
            // A bracket that did not match leaves the cursor exactly where it
            // was, and the note says which of the three reasons it was.
            Some(_) => from,
            None => apply_motion_inner(presentation, snapshot, command, visible_lines),
        },
    };
    presentation.cursor_byte = resolved;
    presentation.viewport_line =
        viewport_after_motion(presentation, snapshot, command, resolved, visible_lines);
    presentation.desired_column = desired_column_after(command, snapshot, from, resolved, aimed_at);
    if let Some((motion, outcome)) = find {
        // Recorded whether or not it landed: Helix's repeat slot is set by
        // running the command, so `f z` with no `z` ahead still leaves `Alt-.`
        // armed to try `f z` again from wherever the cursor is.
        presentation.last_find = Some(motion);
        presentation.find_note = FindNote::from_outcome(outcome, motion.target);
    }
    if let Some((motion, outcome)) = paragraph {
        presentation.paragraph_note = motion.note(outcome).unwrap_or_default();
    } else {
        // A motion that is not a paragraph motion has nothing to say about
        // paragraphs, and the note from the *previous* motion must not
        // outlive it: `]p` at the last paragraph and then `j` would leave
        // "no next paragraph" on screen beside a cursor that had since moved.
        presentation.paragraph_note = ParagraphNote::None;
    }
    // The same rule for the bracket note, and the same reason: the note
    // describes exactly the keystroke that just happened.
    presentation.match_note = match bracket {
        Some(BracketOutcome::Landed(_)) => MatchNote::None,
        Some(BracketOutcome::NoBracket) => MatchNote::NoBracket,
        Some(BracketOutcome::Unbalanced) => MatchNote::Unbalanced,
        Some(BracketOutcome::Bounded) => MatchNote::ScanBounded,
        None => MatchNote::None,
    };
    resolved
}

/// The desired column left behind by a motion, or `None` to forget the run.
///
/// A **column** is a thing you can aim at vertically, so the motions that move
/// along a column keep one. Everything else clears it. That is the vi-family
/// rule and it is the safe direction: `gg`, `G`, `w`, `e`, `$`, `gs`, a find, a
/// page, a search jump — none of those say "I am part of a vertical run", and a
/// remembered column across them would make the next `j` aim at a column
/// chosen for a line the cursor no longer sits on. A cleared column costs one
/// `j` to re-establish; a wrongly remembered one silently sends the cursor to
/// an unrelated place.
///
/// The asymmetry between horizontal and vertical is the whole feature. A
/// horizontal move **re-seeds** the column from where it landed, because that
/// is the column the reader has chosen. A vertical move **keeps** the column it
/// was aiming for rather than re-seeding from where it landed: landing on a
/// short line clamps the cursor to that line's end, and re-seeding there would
/// mean `j` past a short line quietly rewrote the target — so `k` back would
/// return to the short line's end instead of to where the run started. That is
/// the bug this field exists to prevent, and it is why `move_vertical` takes
/// the column as an argument at all.
fn desired_column_after(
    command: EditorCommand,
    snapshot: &DocumentSnapshot,
    from: usize,
    resolved: usize,
    aimed_at: Option<usize>,
) -> Option<usize> {
    let column_of = |offset: usize| {
        offset.saturating_sub(line_start(snapshot, cursor_line(snapshot, offset)).unwrap_or(0))
    };
    match command {
        EditorCommand::MoveLeft | EditorCommand::MoveRight => Some(column_of(resolved)),
        EditorCommand::MoveUp | EditorCommand::MoveDown => {
            Some(aimed_at.unwrap_or_else(|| column_of(from)))
        }
        _ => None,
    }
}

/// The geometry half of [`apply_motion`]: resolve the offset a motion lands
/// on, without writing anything. Split out so the Helix driver can resolve a
/// motion against a throwaway presentation and then commit the result exactly
/// once through the shared selection state machine.
fn apply_motion_inner(
    presentation: &TuiDocumentPresentation,
    snapshot: &DocumentSnapshot,
    command: EditorCommand,
    visible_lines: usize,
) -> usize {
    let cursor = clamp_offset(snapshot, presentation.cursor_byte);
    let next = match command {
        EditorCommand::MoveLeft => prev_char_boundary(snapshot, cursor),
        EditorCommand::MoveRight => next_char_boundary(snapshot, cursor).unwrap_or(cursor),
        EditorCommand::MoveUp => move_vertical(snapshot, cursor, -1, presentation.desired_column),
        EditorCommand::MoveDown => move_vertical(snapshot, cursor, 1, presentation.desired_column),
        EditorCommand::LineStart => {
            line_start(snapshot, cursor_line(snapshot, cursor)).unwrap_or(0)
        }
        EditorCommand::LineEnd => {
            let line = cursor_line(snapshot, cursor);
            line_content_end(snapshot, line).unwrap_or(cursor)
        }
        EditorCommand::FirstNonWhitespace => {
            first_non_whitespace(snapshot, cursor_line(snapshot, cursor)).unwrap_or(cursor)
        }
        EditorCommand::BufferStart => 0,
        EditorCommand::BufferEnd => buffer_content_end(snapshot),
        EditorCommand::WordForward => word_forward(snapshot, cursor),
        EditorCommand::WordBackward => word_backward(snapshot, cursor),
        EditorCommand::WordEnd => word_end(snapshot, cursor),
        EditorCommand::LongWordForward => long_word_forward(snapshot, cursor),
        EditorCommand::LongWordBackward => long_word_backward(snapshot, cursor),
        EditorCommand::LongWordEnd => long_word_end(snapshot, cursor),
        EditorCommand::HalfPageDown => {
            // A page that would land past the end continues to the end
            // instead of stopping short. Half a window from the last page
            // is a very ordinary place to be — and a motion that quietly
            // refuses there reads as a dropped keypress rather than as the
            // end of the document.
            let target = half_page_target(presentation.viewport_line, visible_lines, true);
            if target > last_line_of(snapshot) {
                buffer_content_end(snapshot)
            } else {
                let column = cursor.saturating_sub(
                    line_start(snapshot, cursor_line(snapshot, cursor)).unwrap_or(0),
                );
                offset_on_line(snapshot, target, column)
            }
        }
        EditorCommand::HalfPageUp => {
            let target = half_page_target(presentation.viewport_line, visible_lines, false);
            let column = cursor
                .saturating_sub(line_start(snapshot, cursor_line(snapshot, cursor)).unwrap_or(0));
            offset_on_line(snapshot, target, column)
        }
        // A command this surface does not own (`DeleteChar`, `DeleteWord`, …)
        // is an *editing* intent, not a motion. Returning the cursor
        // unchanged keeps it out of `resolve_edit`, which has its own arm for
        // it.
        _ => return clamp_offset(snapshot, cursor),
    };
    clamp_offset(snapshot, next)
}

/// Where `viewport_line` ends up after a motion.
///
/// Two different questions, deliberately split:
///
/// - A **page** (`Ctrl+d`/`Ctrl+u`) advances the view by half a window each
///   press, exactly as Helix does, and the cursor lands relative to the *new*
///   viewport. Merely keeping the cursor visible is not enough here: after the
///   first `Ctrl+d` the cursor is still inside the window, so the view would
///   not move and a second press would resolve against the same anchor and
///   visibly do nothing.
/// - Every **other** motion scrolls the minimum amount needed to reveal the
///   cursor, so `j` does not reframe the document under the reader.
///
/// The widget still resolves the exact window at render time; this keeps the
/// motion arithmetic independent of render cadence, which is what made
/// paging behave differently depending on whether a frame happened to land
/// between two presses.
fn viewport_after_motion(
    presentation: &TuiDocumentPresentation,
    snapshot: &DocumentSnapshot,
    command: EditorCommand,
    cursor: usize,
    visible_lines: usize,
) -> usize {
    let last_line = snapshot.len_lines().saturating_sub(1);
    let window = visible_lines.max(1);
    let viewport = match command {
        EditorCommand::HalfPageDown => presentation.viewport_line + (window / 2).max(1),
        EditorCommand::HalfPageUp => presentation
            .viewport_line
            .saturating_sub((window / 2).max(1)),
        _ => presentation.viewport_line,
    }
    .min(last_line);
    reveal_cursor(
        viewport,
        cursor_line(snapshot, clamp_offset(snapshot, cursor)),
        window,
        last_line,
    )
}

/// Scroll the minimum amount needed to bring `target_line` into a window of
/// `window` rows anchored at `viewport`.
///
/// The one reveal rule, shared by motions and by `/` search, so a jump to a
/// match lands the same way a jump to the end of the buffer does.
pub fn reveal_cursor(
    viewport: usize,
    target_line: usize,
    window: usize,
    last_line: usize,
) -> usize {
    if target_line < viewport {
        target_line
    } else if target_line >= viewport + window {
        (target_line + 1 - window).min(last_line)
    } else {
        viewport
    }
}

/// Park the cursor on `offset` and scroll the minimum amount to reveal it.
///
/// Used by anything that moves the cursor *to a place* rather than by a
/// relative step: `/` search, `n`/`N`, and an LSP jump. It shares the reveal
/// rule with the Helix motions so the two cannot disagree about where the
/// view lands, and it drops a held selection rather than dragging the old
/// anchor onto wherever the cursor ended up.
pub fn move_cursor_to(
    presentation: &mut TuiDocumentPresentation,
    snapshot: &DocumentSnapshot,
    offset: usize,
    visible_lines: usize,
) -> usize {
    let cursor = clamp_offset(snapshot, offset);
    presentation.cursor_byte = cursor;
    presentation.clear_selections();
    // A jump is a discontinuity, not a step along a column: whatever run the
    // reader was in was relative to where they just left.
    presentation.desired_column = None;
    presentation.viewport_line = reveal_cursor(
        presentation.viewport_line,
        cursor_line(snapshot, cursor),
        visible_lines.max(1),
        last_line_of(snapshot),
    );
    cursor
}

/// Resolve an editing command into a local transaction plus the cursor and
/// mode the editor should adopt if the controller accepts it.
///
/// Pure: reads the snapshot, mutates nothing. Returns `None` when the
/// command is a no-op at the current position, or when the request needs a
/// pending prefix the caller has not supplied.
///
/// **Scope: document-only verbs.** A command whose entire input is the
/// snapshot plus the cursor belongs here. `r`, `R`, `~`, `p`, and `P` do
/// not: they take their target from the held *selection* and their payload
/// from outside the document (the system clipboard, for three of the five),
/// and they consume the selection they acted on. Expressing that here would
/// mean either a second argument this function does not have or an
/// `EditorCommand` variant carrying a `String` — a data-carrying verb enum
/// the motion machine and the `Alt-.` recorder would then have to know about
/// as well. They go through the command layer's `editor_editing_verb`
/// instead, which shares this funnel's gate, this snapshot, and this
/// `EditorEdit`, and differs only in clearing the selection it consumed.
/// The newline `o` opens below `line` with, cursor placed on it in Insert.
///
/// Split the existing line terminator: the new line inherits it and the
/// current line gets a fresh one. With no terminator to split (last line of
/// the buffer), append a full blank line.
fn open_line_below(snapshot: &DocumentSnapshot, line: usize) -> Option<EditorEdit> {
    let end = line_content_end(snapshot, line).unwrap_or_default();
    let insert_at = next_char_boundary(snapshot, end).unwrap_or(end);
    let has_terminator = insert_at > end;
    let text = if has_terminator { "\n" } else { "\n\n" };
    build_edit(
        snapshot,
        TextTransaction::new(vec![TextEdit::new(insert_at..insert_at, text.to_string())]),
        insert_at + 1,
        true,
    )
}

/// The newline `O` opens above `line` with, cursor placed on it in Insert.
fn open_line_above(snapshot: &DocumentSnapshot, line: usize) -> Option<EditorEdit> {
    let start = line_start(snapshot, line).unwrap_or(0);
    build_edit(
        snapshot,
        TextTransaction::new(vec![TextEdit::new(start..start, "\n".to_string())]),
        start,
        true,
    )
}

/// Helix `]Space`: a newline below the cursor's line, and the cursor stays.
///
/// Inserting at the **start of the next line** is what Helix's
/// `add_newline_impl` does (`let line = end + 1; let pos = line_to_char(line)`)
/// and it is not the same as [`open_line_below`], which splits the terminator
/// instead. Both produce a blank line below, but this one does not touch the
/// cursor's own line and does not enter Insert.
///
/// Staying put is the point, not an oversight: `]Space` then `]p` splits the
/// paragraph the cursor is in, and the user keeps reading from where they
/// were. `o` would have thrown the keyboard at them first.
fn add_newline_below(
    snapshot: &DocumentSnapshot,
    line: usize,
    cursor: usize,
) -> Option<EditorEdit> {
    let last = snapshot.len_lines().saturating_sub(1);
    // Helix's `end + 1` on the last line is `len_lines()`, which is one past
    // this editor's last line and resolves to the end of the buffer — the
    // correct place to append the newline Helix would append there.
    let target = (line + 1).min(last);
    let at = if line >= last {
        snapshot.len_bytes()
    } else {
        line_start(snapshot, target).unwrap_or_else(|| snapshot.len_bytes())
    };
    build_edit(
        snapshot,
        TextTransaction::new(vec![TextEdit::new(at..at, "\n".to_string())]),
        cursor,
        false,
    )
}

/// Helix `[Space`: a newline above the cursor's line, and the cursor stays.
///
/// `build_edit`'s cursor is an offset in the buffer **after** the edit, so
/// staying put means `cursor + 1`: the newline lands at or before the cursor,
/// pushing everything the cursor was pointing at one byte along. Every byte up
/// to the insertion point is unchanged, so `+ 1` is the whole correction.
fn add_newline_above(
    snapshot: &DocumentSnapshot,
    line: usize,
    cursor: usize,
) -> Option<EditorEdit> {
    let at = line_start(snapshot, line).unwrap_or(0);
    build_edit(
        snapshot,
        TextTransaction::new(vec![TextEdit::new(at..at, "\n".to_string())]),
        cursor + 1,
        false,
    )
}

pub fn resolve_edit(
    snapshot: &DocumentSnapshot,
    presentation: &TuiDocumentPresentation,
    command: EditorCommand,
) -> Option<EditorEdit> {
    let cursor = clamp_offset(snapshot, presentation.cursor_byte);
    let line = cursor_line(snapshot, cursor);
    match command {
        EditorCommand::InsertBefore => None, // handled by mode transition
        EditorCommand::InsertAfter => None,
        EditorCommand::InsertLineStart => None,
        EditorCommand::InsertLineEnd => None,
        EditorCommand::OpenLineBelow => open_line_below(snapshot, line),
        EditorCommand::OpenLineAbove => open_line_above(snapshot, line),
        // Helix's `add_newline_*` is the same insertion as `open_*` with a
        // different consequence, so both verbs share one implementation rather
        // than two arms that could drift: `stay_insert` is the only thing that
        // separates them, and that belongs to the caller that changes mode.
        EditorCommand::AddNewlineBelow => add_newline_below(snapshot, line, cursor),
        EditorCommand::AddNewlineAbove => add_newline_above(snapshot, line, cursor),
        EditorCommand::DeleteChar => {
            let end = next_char_boundary(snapshot, cursor).unwrap_or(cursor);
            if end > cursor {
                delete_range(snapshot, cursor..end)
            } else {
                None
            }
        }
        EditorCommand::DeleteToLineEnd => {
            let end = line_content_end(snapshot, line).unwrap_or(cursor);
            if end > cursor {
                delete_range(snapshot, cursor..end)
            } else {
                None
            }
        }
        EditorCommand::DeleteLine => {
            // vi `dd` semantics: remove the line and its terminator. The
            // last line has no terminator, so the *preceding* terminator is
            // removed instead, which leaves no trailing blank line.
            let last = snapshot.len_lines().saturating_sub(1);
            let (start, end) = if line < last {
                (
                    line_start(snapshot, line).unwrap_or(0),
                    line_start(snapshot, line + 1).unwrap_or(snapshot.len_bytes()),
                )
            } else if line > 0 {
                (
                    line_content_end(snapshot, line - 1).unwrap_or(0),
                    snapshot.len_bytes(),
                )
            } else {
                (0, snapshot.len_bytes())
            };
            if end > start {
                delete_range(snapshot, start..end)
            } else {
                None
            }
        }
        EditorCommand::DeleteWord => {
            let start = word_backward(
                snapshot,
                next_char_boundary(snapshot, cursor).unwrap_or(cursor),
            );
            if start < cursor {
                delete_range(snapshot, start..cursor)
            } else {
                None
            }
        }
        _ => None,
    }
}

// ── Insert-mode deletion builders ───────────────────────────────────────────

/// Delete from the cursor back to the start of its line.
///
/// Helix's `kill_to_line_start`, the insert-mode `Ctrl-u`.
///
/// The range is the line's start up to the cursor, so a cursor already at
/// the line start deletes **nothing**: there is no text between the line
/// start and the cursor to remove. Helix joins the line with the previous
/// one in that case, which is a different edit under a different name, so it
/// is not smuggled in here.
pub fn kill_to_line_start(snapshot: &DocumentSnapshot, cursor: usize) -> Option<EditorEdit> {
    let cursor = clamp_offset(snapshot, cursor);
    let line = cursor_line(snapshot, cursor);
    let start = line_start(snapshot, line)?;
    if start < cursor {
        delete_range(snapshot, start..cursor)
    } else {
        None
    }
}

/// Delete the word before the cursor, **stopping at the line boundary**.
///
/// Helix's `delete_word_backward`, the insert-mode `Ctrl-w` and
/// `Alt-Backspace`.
///
/// **Line-clamped, deliberately.** [`word_backward`] is buffer-wide, and a
/// buffer-wide backward word scan at the start of a line deletes the
/// previous line's last word. That is the right answer for the `b` motion
/// and the wrong one here: in insert mode the reader is thinking about the
/// line they are typing, so a `Ctrl-w` in column zero deletes nothing
/// rather than reaching into the line above.
///
/// The scan reuses [`is_word_byte`], so it finds the same word the
/// `w`/`b`/`e` motions find, and it steps by [`next_char_boundary`], so a
/// multi-byte character is never split.
pub fn delete_word_backward(snapshot: &DocumentSnapshot, cursor: usize) -> Option<EditorEdit> {
    let cursor = clamp_offset(snapshot, cursor);
    let line = cursor_line(snapshot, cursor);
    let floor = line_start(snapshot, line)?;
    if cursor <= floor {
        return None;
    }
    // Two passes: the separator run between the cursor and the word, then
    // the word itself. The deleted range is therefore the word *and* the
    // whitespace that separated it from the cursor, which is the range a
    // shell's `Ctrl-w` removes.
    let mut start = cursor;
    while start > floor && !byte_ending_at(snapshot, start).is_some_and(is_word_byte) {
        start = prev_char_boundary(snapshot, start);
    }
    while start > floor && byte_ending_at(snapshot, start).is_some_and(is_word_byte) {
        start = prev_char_boundary(snapshot, start);
    }
    if start < cursor {
        delete_range(snapshot, start..cursor)
    } else {
        None
    }
}

/// Delete the word after the cursor, **stopping at the line boundary**.
///
/// Helix's `delete_word_forward`, the insert-mode `Alt-d` and `Alt-Delete`.
///
/// Line-clamped for the same reason as [`delete_word_backward`]: the next
/// word is the next word *on this line*, and a buffer-wide scan would
/// delete the first word of the following line.
///
/// From inside a word the scan stops at that word's end rather than at the
/// next one's start, so `Alt-d` in the middle of `hello` removes `llo` —
/// the forward half of the word the cursor is in, which is what a forward
/// word delete does everywhere.
pub fn delete_word_forward(snapshot: &DocumentSnapshot, cursor: usize) -> Option<EditorEdit> {
    let cursor = clamp_offset(snapshot, cursor);
    let line = cursor_line(snapshot, cursor);
    let ceiling = line_content_end(snapshot, line)?;
    if cursor >= ceiling {
        return None;
    }
    // The separator run first, then the word: the deleted range is the word
    // and whatever whitespace preceded it, so `Alt-d` at the end of a word
    // removes the next word whole rather than stopping at its first byte.
    let mut end = cursor;
    while end < ceiling && !leading_byte(snapshot, end).is_some_and(is_word_byte) {
        end = next_char_boundary(snapshot, end).unwrap_or(ceiling);
    }
    while end < ceiling && leading_byte(snapshot, end).is_some_and(is_word_byte) {
        end = next_char_boundary(snapshot, end).unwrap_or(ceiling);
    }
    if end > cursor {
        delete_range(snapshot, cursor..end)
    } else {
        None
    }
}

/// The leading byte of the character that ends at `offset`.
///
/// `None` when `offset` is the start of the text, where there is no
/// character before it to describe.
fn byte_ending_at(snapshot: &DocumentSnapshot, offset: usize) -> Option<u8> {
    if offset == 0 {
        return None;
    }
    leading_byte(snapshot, prev_char_boundary(snapshot, offset))
}

// ── Undo history ────────────────────────────────────────────────────────────

/// Record an accepted edit in the undo history and clear the redo history.
///
/// Convenience wrapper over [`push_undo`] that reads the pre-edit
/// position from the presentation itself. The command layer uses
/// [`push_undo`] directly, because it has already moved the cursor by the
/// time the controller accepts the transaction.
pub fn record_undo(presentation: &mut TuiDocumentPresentation, edit: &EditorEdit) {
    let selections = presentation.selections.clone();
    push_undo(
        presentation,
        edit.transaction.clone(),
        edit.inverse.clone(),
        edit.bytes,
        presentation.cursor_byte,
        selections,
    );
}

/// Push one accepted edit onto the history and truncate anything after it.
///
/// `cursor_before` is the position observed *before*
/// the forward transaction, so undo restores where the user actually was.
/// `selections_before` is the same idea for the selection set: undo restores
/// the members the reader had, not an empty buffer with the text put back.
///
/// A new edit after an undo **truncates the tail**: everything after the
/// cursor is dropped, because that text is no longer reachable. This is
/// today's `push_undo` clearing the redo stack, expressed as a cursor.
pub fn push_undo(
    presentation: &mut TuiDocumentPresentation,
    forward: TextTransaction,
    inverse: TextTransaction,
    bytes: usize,
    cursor_before: usize,
    selections_before: Vec<crate::tui::helix_selection::Selection<EditorPoint>>,
) {
    truncate_history_tail(presentation);
    presentation.history.push(EditorUndoEntry {
        forward,
        inverse,
        cursor_before,
        selections_before,
        bytes,
    });
    presentation.history_bytes = presentation.history_bytes.saturating_add(bytes);
    presentation.history_cursor = presentation.history.len();
    trim_history(presentation);
}

/// Drop everything after the history cursor, and the bytes it held.
///
/// The cursor is a count of applied entries, so truncating the tail leaves
/// it where it is: the applied entries are the ones before it, and they are
/// untouched.
pub fn truncate_history_tail(presentation: &mut TuiDocumentPresentation) {
    let cursor = presentation.history_cursor.min(presentation.history.len());
    if cursor >= presentation.history.len() {
        return;
    }
    let dropped: usize = presentation.history[cursor..]
        .iter()
        .map(|entry| entry.bytes)
        .sum();
    presentation.history_bytes = presentation.history_bytes.saturating_sub(dropped);
    presentation.history.truncate(cursor);
}

/// The entry one step **back** from the cursor, if there is one.
///
/// This is what `u` undoes: the entry the current text was produced by.
pub fn undo_entry(presentation: &TuiDocumentPresentation) -> Option<&EditorUndoEntry> {
    let index = presentation.history_cursor.checked_sub(1)?;
    presentation.history.get(index)
}

/// The entry one step **forward** from the cursor, if there is one.
///
/// This is what `U` redoes: the entry the text can still be moved to.
pub fn redo_entry(presentation: &TuiDocumentPresentation) -> Option<&EditorUndoEntry> {
    presentation.history.get(presentation.history_cursor)
}

/// Move the history cursor **back** one entry.
///
/// Returns `false` at the beginning of the history, where there is nothing
/// earlier to move to.
pub fn move_history_cursor_back(presentation: &mut TuiDocumentPresentation) -> bool {
    if presentation.history_cursor == 0 {
        return false;
    }
    presentation.history_cursor -= 1;
    true
}

/// Move the history cursor **forward** one entry.
///
/// Returns `false` at the tip of the history, where there is nothing later
/// to move to.
pub fn move_history_cursor_forward(presentation: &mut TuiDocumentPresentation) -> bool {
    if presentation.history_cursor >= presentation.history.len() {
        return false;
    }
    presentation.history_cursor += 1;
    true
}

/// Fold one accepted edit into the open insert session, if it fits.
///
/// Returns `true` when the edit joined the session, in which case the
/// caller must **not** push an undo entry for it: the session commits the
/// whole run as one entry later.
///
/// Returns `false` when the edit does not fit, in which case the caller
/// must commit the session (if one is open) and push the edit on its own.
///
/// The redo tail is cleared when the session **opens**, and only then. A
/// reader who undoes and then types must not have the redo tail
/// resurrected by the next keystroke of the new burst, and clearing it on
/// every keystroke of the burst would clear a redo the reader had earned
/// since the burst began.
pub fn absorb_insert_edit(
    presentation: &mut TuiDocumentPresentation,
    transaction: &TextTransaction,
    bytes: usize,
    cursor_before: usize,
    selections_before: Vec<crate::tui::helix_selection::Selection<EditorPoint>>,
) -> bool {
    // Only a single, empty-range insertion can join a run.
    if transaction.edits.len() != 1 {
        return false;
    }
    let edit = &transaction.edits[0];
    if edit.range.start != edit.range.end {
        return false;
    }
    let Some(session) = presentation.insert_session.as_mut() else {
        presentation.insert_session = Some(InsertSession {
            base_offset: edit.range.start,
            edits: vec![transaction.clone()],
            inserted_len: edit.insert.len(),
            bytes,
            cursor_before,
            selections_before,
        });
        // The redo tail is dropped when the session **opens**, and only then.
        // A reader who undoes and then types must not have the tail
        // resurrected by the next keystroke of the new burst, and dropping it
        // on every keystroke of the burst would drop a redo the reader had
        // earned since the burst began.
        truncate_history_tail(presentation);
        return true;
    };
    // The run continues only at the end of the run so far. An insertion
    // anywhere else is a different edit, and folding it in would make the
    // merged entry describe a text the reader never had.
    let expected = session.base_offset.saturating_add(session.inserted_len);
    if edit.range.start != expected {
        return false;
    }
    if session.bytes.saturating_add(bytes) > MAX_EDITOR_UNDO_BYTES {
        return false;
    }
    session.edits.push(transaction.clone());
    session.inserted_len = session.inserted_len.saturating_add(edit.insert.len());
    session.bytes = session.bytes.saturating_add(bytes);
    true
}

/// Commit the open insert session as **one** undo entry.
///
/// The merged entry is a single insertion of everything the run typed, at
/// the offset the run began, with an inverse that deletes exactly those
/// bytes. That is exact because a run is one contiguous insertion by
/// construction — see [`InsertSession`].
///
/// Committing is what "leaving insert mode" means for the history. It is
/// also what a reconciliation must **not** do: [`clear_history`] discards
/// the session instead, because a reload must not resurrect the pre-reload
/// text as an undo step.
pub fn commit_insert_session(presentation: &mut TuiDocumentPresentation) {
    let Some(session) = presentation.insert_session.take() else {
        return;
    };
    if session.edits.is_empty() || session.inserted_len == 0 {
        return;
    }
    // The merged payload is built here, once, and dropped with the session:
    // the session itself never holds a copy of the text.
    let inserted: String = session
        .edits
        .iter()
        .flat_map(|transaction| transaction.edits.iter())
        .map(|edit| edit.insert.clone())
        .collect();
    let total = inserted.len();
    let forward = TextTransaction::new(vec![TextEdit::new(
        session.base_offset..session.base_offset,
        inserted,
    )]);
    let inverse = TextTransaction::new(vec![TextEdit::new(
        session.base_offset..session.base_offset + total,
        String::new(),
    )]);
    push_undo(
        presentation,
        forward,
        inverse,
        session.bytes,
        session.cursor_before,
        session.selections_before,
    );
}

/// Drop the open insert session without committing it.
///
/// The session's edits are already in the text, so this does not undo
/// them: it only declines to record them as an undo step. A
/// reconciliation calls it because the text those edits described is gone.
pub fn discard_insert_session(presentation: &mut TuiDocumentPresentation) {
    presentation.insert_session = None;
}

/// Evict the oldest entries until the history is inside **one** cap.
///
/// **One cap, not one per stack.** The two-stack model enforced the depth
/// and byte bounds twice — once per stack — so the worst case was twice the
/// bound `architecture/tui.md` documents. A single history restores the
/// documented reading and *tightens* the bound.
fn trim_history(presentation: &mut TuiDocumentPresentation) {
    while presentation.history.len() > MAX_EDITOR_UNDO_DEPTH
        || presentation.history_bytes > MAX_EDITOR_UNDO_BYTES
    {
        match presentation.history.first() {
            Some(oldest) => {
                presentation.history_bytes = presentation.history_bytes.saturating_sub(oldest.bytes)
            }
            None => {
                presentation.history_bytes = 0;
                break;
            }
        }
        presentation.history.remove(0);
        // The cursor counts the applied entries, which are the ones at the
        // front. Dropping the oldest shifts every index after it, so the
        // count shifts with them.
        if presentation.history_cursor > 0 {
            presentation.history_cursor -= 1;
        }
    }
}

/// Drop all undo and redo history.
///
/// Called on every reconciliation transition (`resync`, `reconnect`,
/// `reload_from_disk`, `close`, and opening a different document). A
/// divergent draft is retained by the controller in a recovery state; the
/// editor surfaces that state and never replays or discards the draft on the
/// user's behalf, so it must not keep inverse transactions that no longer
/// describe the local text.
///
/// Disarms the pending prefix too, which is the same fact in a different
/// register: the gesture the user was halfway through was aimed at text that
/// has just been replaced underneath it. The baseline only disarmed
/// `pending_command` here and left `yank_prefix_pending` armed across a
/// reload, so a `space` typed before a resync yanked in the reloaded
/// buffer.
///
/// The repeatable find and its disclosure go for the same reason and in the
/// same breath: after a reconciliation the character `Alt-.` would look for,
/// and the answer to whether it is there at all, describe text that no
/// longer exists.
///
/// The desired column goes with them for the same reason and is the one that
/// would bite silently: a column measured against the *old* text is not an
/// error the user can see, it just aims the next `j` at a column of a line
/// that is no longer there.
pub fn clear_history(presentation: &mut TuiDocumentPresentation) {
    presentation.history.clear();
    presentation.history_bytes = 0;
    presentation.history_cursor = 0;
    // An open insert session is discarded, not committed. Its edits are
    // already in the text, but the text they described is the pre-reload
    // text: committing the session would record a step that restores a
    // document the reader is no longer looking at.
    discard_insert_session(presentation);
    presentation.disarm_prefix();
    presentation.forget_find();
    // The unimpaired slot and its disclosure are transient in exactly the way
    // `last_find` is: a reconciliation can invalidate both — the paragraph the
    // cursor was pointed at is gone, and the note now describes a keystroke
    // from a document that no longer exists. Cleared here rather than inside
    // `forget_find` so that method keeps answering only to the find it names.
    presentation.last_unimpaired = None;
    presentation.paragraph_note = crate::tui::editor::ParagraphNote::None;
    presentation.desired_column = None;
}

// ── Viewport ─────────────────────────────────────────────────────────────────

/// Gutter width for a document, in columns.
///
/// Sized from the document's line count so it does not jitter as the cursor
/// moves, and stable across documents of the same length.
pub fn gutter_width(line_count: usize) -> u16 {
    let digits = line_count.max(1).to_string().len();
    (digits as u16).saturating_add(1)
}

/// Result of resolving a viewport against the current presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorViewport {
    /// First visible line, zero-based.
    pub first_line: usize,
    /// Horizontal scroll in display columns.
    pub column: usize,
    /// Gutter width in columns.
    pub gutter: u16,
    /// Columns available for text after the gutter and scrollbar.
    pub text_columns: u16,
    /// Number of visible lines.
    pub visible_lines: u16,
}

/// Resolve the viewport so the cursor is on screen, keeping the line-to-row
/// mapping one-to-one (hard wrap) and clamping the horizontal scroll to the
/// longest visible line.
pub fn resolve_viewport(
    presentation: &mut TuiDocumentPresentation,
    snapshot: &DocumentSnapshot,
    area_width: u16,
    area_height: u16,
) -> EditorViewport {
    let gutter = gutter_width(snapshot.len_lines());
    let text_columns = area_width
        .saturating_sub(gutter)
        .saturating_sub(1)
        .max(MIN_EDITOR_TEXT_COLUMNS);
    let visible_lines = area_height.max(1);
    let cursor_line = cursor_line(snapshot, presentation.cursor_byte);
    let last_line = snapshot.len_lines().saturating_sub(1);
    let max_first = last_line.saturating_sub(visible_lines.saturating_sub(1) as usize);

    // Vertical: move the window minimally to keep the cursor line on screen.
    let mut first = presentation.viewport_line.min(max_first);
    if cursor_line < first {
        first = cursor_line;
    } else if cursor_line >= first + visible_lines as usize {
        first = cursor_line + 1 - visible_lines as usize;
    }
    first = first.min(max_first);

    // Horizontal: keep the cursor's display column on screen, then clamp to
    // the longest visible line so the view never scrolls past content.
    // C002: the cursor column is the bounded prefix width when the cursor
    // sits past the render window of a huge line; the byte offset is
    // untouched, only the scroll target saturates.
    let cursor_start = line_start(snapshot, cursor_line).unwrap_or(0);
    let cursor_column = bounded_display_width(snapshot, cursor_start..presentation.cursor_byte).0;
    let mut column = presentation.viewport_column;
    if cursor_column < column {
        column = cursor_column;
    } else if cursor_column >= column + text_columns as usize {
        column = cursor_column + 1 - text_columns as usize;
    }
    let max_column = longest_visible_width(snapshot, first, visible_lines as usize)
        .saturating_sub(text_columns as usize);
    column = column.min(max_column);

    presentation.viewport_line = first;
    presentation.viewport_column = column;
    EditorViewport {
        first_line: first,
        column,
        gutter,
        text_columns,
        visible_lines,
    }
}

/// Display width of a byte range, in terminal columns.
///
/// The caller must pass a bounded range on the render path; use
/// [`bounded_display_width`] when the range can come from a pathological
/// line. Exact for ordinary short ranges.
pub fn display_width(snapshot: &DocumentSnapshot, range: std::ops::Range<usize>) -> usize {
    snapshot
        .read_bytes(range.start, range.end)
        .map(|text| {
            use unicode_width::UnicodeWidthStr;
            UnicodeWidthStr::width(text.as_str())
        })
        .unwrap_or(0)
}

/// Display width of `range` capped to the per-line render budget.
///
/// Returns the width of the examined prefix and whether the range was cut.
/// The render path uses this (via [`bounded_display_column`] and
/// [`longest_visible_width`]) so a cursor at the far end of a multi-megabyte
/// line costs 32 KiB, not the whole line.
pub fn bounded_display_width(
    snapshot: &DocumentSnapshot,
    range: std::ops::Range<usize>,
) -> (usize, bool) {
    let end = bounded_prefix_end(snapshot, range.start, range.end);
    let truncated = end < range.end;
    (display_width(snapshot, range.start..end), truncated)
}

/// Longest display width among the visible lines, in terminal columns.
///
/// C002: each line contributes at most its 32 KiB prefix, and measurement
/// stops once the frame total reaches 256 KiB. Lines past the frame budget
/// contribute nothing further; use [`frame_truncated_lines`] for the honest
/// truncation signal shown in the UI.
pub fn longest_visible_width(
    snapshot: &DocumentSnapshot,
    first_line: usize,
    visible_lines: usize,
) -> usize {
    let mut widest = 0usize;
    let mut examined = 0usize;
    for line in first_line..(first_line + visible_lines) {
        if line >= snapshot.len_lines() {
            break;
        }
        if examined >= MAX_FRAME_SOURCE_BYTES {
            break;
        }
        let Some(start) = line_start(snapshot, line) else {
            break;
        };
        let end = line_content_end(snapshot, line).unwrap_or(start);
        let window_end = bounded_prefix_end(snapshot, start, end);
        examined = examined.saturating_add(window_end.saturating_sub(start));
        widest = widest.max(display_width(snapshot, start..window_end));
    }
    widest
}

/// Number of visible lines exceeding the per-line render budget.
///
/// Cheap length check, no content read. The banner shows this so a truncated
/// frame is never presented as complete.
pub fn frame_truncated_lines(
    snapshot: &DocumentSnapshot,
    first_line: usize,
    visible_lines: usize,
) -> usize {
    (first_line..first_line.saturating_add(visible_lines))
        .take_while(|line| *line < snapshot.len_lines())
        .filter(|line| line_is_truncated(snapshot, *line))
        .count()
}

/// Deterministic per-frame input cost for the visible window.
///
/// Counts source bytes the bounded render path would examine (prefix windows
/// only), the largest single `read_bytes` window, how many visible lines are
/// truncated, and how many column-seeking scans run (one per visible line).
/// Tests assert budgets on this rather than on wall-clock time, which shared
/// CI cannot calibrate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameCost {
    pub total_bytes: usize,
    pub max_line_bytes: usize,
    pub truncated_lines: usize,
    pub column_scans: usize,
}

/// Measure [`FrameCost`] for `visible_lines` starting at `first_line`.
///
/// Mirrors the `render_text` loop exactly: row sizes come from the cheap
/// length check, rows past the 256 KiB frame budget cost zero bytes and
/// count as truncated, and one column-seeking scan runs per rendered row.
pub fn measure_frame_cost(
    snapshot: &DocumentSnapshot,
    first_line: usize,
    visible_lines: usize,
) -> FrameCost {
    let mut total_bytes = 0usize;
    let mut max_line_bytes = 0usize;
    let mut truncated_lines = 0usize;
    let mut column_scans = 0usize;
    let mut remaining = MAX_FRAME_SOURCE_BYTES;
    for line in first_line..first_line.saturating_add(visible_lines) {
        if line >= snapshot.len_lines() {
            break;
        }
        let row_bytes = line_byte_len(snapshot, line)
            .unwrap_or(0)
            .min(MAX_RENDER_LINE_BYTES);
        max_line_bytes = max_line_bytes.max(row_bytes);
        let per_line_truncated = line_is_truncated(snapshot, line);
        if row_bytes > remaining {
            // Frame-capped: the renderer shows a marker without reading.
            truncated_lines += 1;
            continue;
        }
        remaining = remaining.saturating_sub(row_bytes);
        total_bytes = total_bytes.saturating_add(row_bytes);
        column_scans += 1;
        if per_line_truncated {
            truncated_lines += 1;
        }
    }
    FrameCost {
        total_bytes,
        max_line_bytes,
        truncated_lines,
        column_scans,
    }
}

/// Display column of `offset` within its own line.
///
/// C002: saturates at the prefix width when `offset` lies past the 32 KiB
/// render window. The byte offset itself stays exact; only the displayed
/// column is bounded. Pair with [`display_column_truncated`] for the UI
/// signal.
pub fn display_column(snapshot: &DocumentSnapshot, offset: usize) -> usize {
    let offset = clamp_offset(snapshot, offset);
    let start = line_start(snapshot, cursor_line(snapshot, offset)).unwrap_or(0);
    bounded_display_width(snapshot, start..offset).0
}

/// Whether [`display_column`] saturated for `offset` (cursor past the render
/// window of a huge line). The banner shows this instead of a bare column.
pub fn display_column_truncated(snapshot: &DocumentSnapshot, offset: usize) -> bool {
    let offset = clamp_offset(snapshot, offset);
    let start = line_start(snapshot, cursor_line(snapshot, offset)).unwrap_or(0);
    bounded_display_width(snapshot, start..offset).1
}

/// Byte offset for a display column on `line`, clamped to the line's content
/// end so a click or motion can never land past the terminator.
///
/// C002: the scan examines at most 32 KiB. A column past the prefix end
/// resolves to the prefix end — horizontal motion on a huge line is limited
/// to its rendered prefix, which the UI marks with [`TRUNCATED_LINE_SUFFIX`].
/// Normal lines are unaffected.
pub fn offset_at_column(snapshot: &DocumentSnapshot, line: usize, column: usize) -> Option<usize> {
    let start = line_start(snapshot, line)?;
    let end = line_content_end(snapshot, line)?;
    let window_end = bounded_prefix_end(snapshot, start, end);
    let mut offset = start;
    let mut consumed = 0usize;
    while offset < window_end {
        // `next_char_boundary` is `None` at the end of the document, which
        // is a legitimate position here, not a failure.
        let next = next_char_boundary(snapshot, offset)
            .unwrap_or(window_end)
            .min(window_end);
        if next == offset {
            break;
        }
        let width = display_width(snapshot, offset..next);
        if consumed + width > column {
            break;
        }
        consumed += width;
        offset = next;
    }
    Some(clamp_offset(snapshot, offset.min(window_end)))
}

/// Resolve a `BytePosition` into a clamped byte offset.
pub fn offset_for_position(snapshot: &DocumentSnapshot, position: BytePosition) -> usize {
    snapshot
        .position_to_byte(position)
        .ok()
        .map(|offset| clamp_offset(snapshot, offset))
        .unwrap_or_else(|| clamp_offset(snapshot, snapshot.len_bytes()))
}

// ── M009: match mode and text objects ───────────────────────────────────────

/// A bounded slice of the document, centred on a byte offset.
///
/// Every pair scan and every text-object resolver in this milestone needs the
/// same thing: a window of the document it is allowed to look at, whose
/// offsets are **window-relative** and have to be mapped back to document
/// offsets before they are used for anything.
///
/// The window is **centred** on the cursor rather than starting at it,
/// because a bracket's mate is as often behind the cursor as in front of it.
/// The budget is [`MAX_SET_VERB_READ_BYTES`], so one keystroke costs no more
/// than one screenful of the document — the same bound every other set verb
/// obeys, and the bound the roadmap's exit condition names.
///
/// The window is a **bounded** slice, never a copy of the document: it is
/// built per keystroke, dropped at the end of it, and cannot exceed the
/// budget above. The field is named `chunk` rather than `text` so the
/// text-authority guard's name-based rule can see at a glance that this is a
/// window and not a retained document.
struct ScanWindow {
    /// The window's bytes.
    chunk: String,
    /// The document byte offset the window starts at.
    base: usize,
}

impl ScanWindow {
    /// The window of at most `budget` bytes centred on `cursor`.
    ///
    /// `None` when the window could not be read — a range the snapshot
    /// refuses, which is a refusal and never a panic.
    fn around(snapshot: &DocumentSnapshot, cursor: usize, budget: usize) -> Option<Self> {
        let cursor = clamp_offset(snapshot, cursor);
        // Both ends are snapped **down** onto a character boundary: a window
        // that started mid-character could not be read at all, because
        // `read_bytes` needs a boundary at both ends.
        let start = clamp_offset(snapshot, cursor.saturating_sub(budget / 2));
        let end = clamp_offset(
            snapshot,
            (start.saturating_add(budget)).min(snapshot.len_bytes()),
        )
        .max(cursor);
        let chunk = snapshot.read_bytes(start, end).ok()?;
        Some(Self { chunk, base: start })
    }

    /// The window-relative offset of a document offset inside this window.
    fn relative(&self, offset: usize) -> usize {
        offset.saturating_sub(self.base)
    }

    /// The document offset of a window-relative offset.
    fn absolute(&self, offset: usize) -> usize {
        self.base + offset
    }
}

/// The character starting at `offset`, when there is one.
///
/// `None` at the end of the document, and at any offset that is not a
/// character boundary.
///
/// [`char_end`], not [`next_char_boundary`]: the latter is `None` for the
/// **last** character of a document as well as for the end of one, and a
/// bracket at the end of a line is a bracket. `char_end` is the helper that
/// already draws that distinction.
fn char_at(snapshot: &DocumentSnapshot, offset: usize) -> Option<char> {
    let offset = clamp_offset(snapshot, offset);
    let end = char_end(snapshot, offset);
    if end <= offset {
        return None;
    }
    snapshot.read_bytes(offset, end).ok()?.chars().next()
}

/// Where `m m` lands, or why it does not.
///
/// Three refusals and one landing, because the reader's next move differs in
/// each case: "there is no bracket here" says *move the cursor*, "the bracket
/// here has no mate" says *the code is unbalanced*, and "the scan hit its
/// budget" says *the file is too big to answer*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BracketOutcome {
    /// The mate of the pair under the cursor is at this document offset.
    Landed(usize),
    /// No delimiter is under the cursor.
    NoBracket,
    /// A delimiter is under the cursor, but no mate was found for it.
    Unbalanced,
    /// The scan hit its byte budget before it could answer.
    Bounded,
}

/// The mate of the pair under the cursor, or why there is none.
///
/// A **pair-table** match, not a syntax-tree match: Helix marks `m m` as
/// `TS`, and CodeGG's is a deliberate local divergence — see the milestone
/// plan, `docs/tui.md` and `architecture/tui.md`.
fn bracket_mate(snapshot: &DocumentSnapshot, cursor: usize) -> BracketOutcome {
    let cursor = clamp_offset(snapshot, cursor);
    let Some(ch) = char_at(snapshot, cursor) else {
        return BracketOutcome::NoBracket;
    };
    let Some(pair) = editor_match::pair_for(ch) else {
        return BracketOutcome::NoBracket;
    };
    let Some(window) = ScanWindow::around(snapshot, cursor, MAX_SET_VERB_READ_BYTES) else {
        return BracketOutcome::Bounded;
    };
    let anchor = window.relative(cursor);
    // A quote is both an opening and a closing delimiter, and its mate is
    // the **next** unescaped quote — the same reading every editor gives a
    // string delimiter. A bracketing pair is matched from the side the cursor
    // is on.
    let outcome = if pair.is_quote() || ch == pair.open {
        editor_match::pair_from_open(&window.chunk, anchor, pair, window.chunk.len())
    } else {
        editor_match::pair_from_close(&window.chunk, anchor, pair, window.chunk.len())
    };
    match outcome {
        editor_match::MatchOutcome::Found { open, close } => {
            let mate = if ch == pair.close && !pair.is_quote() {
                open
            } else {
                close
            };
            BracketOutcome::Landed(window.absolute(mate))
        }
        editor_match::MatchOutcome::Unbalanced => BracketOutcome::Unbalanced,
        editor_match::MatchOutcome::BudgetExhausted => BracketOutcome::Bounded,
    }
}

/// The byte range one text object covers, or why it covers nothing.
///
/// `around` selects the object **with** its boundary — Helix's `a` — and
/// `inside` selects it **without** it — Helix's `i`.
///
/// The cursor is the only anchor: a text object is a thing *under* the
/// cursor, and there is no selection-shaped variant of it.
fn resolve_object(
    snapshot: &DocumentSnapshot,
    kind: ObjectKind,
    around: bool,
    cursor: usize,
) -> Result<std::ops::Range<usize>, String> {
    let cursor = clamp_offset(snapshot, cursor);
    match kind {
        ObjectKind::Word => {
            let range = word_object(snapshot, cursor)?;
            Ok(if around {
                extend_over_trailing_space(snapshot, range)
            } else {
                range
            })
        }
        ObjectKind::LongWord => {
            let range = long_word_object(snapshot, cursor)?;
            Ok(if around {
                extend_over_trailing_space(snapshot, range)
            } else {
                range
            })
        }
        ObjectKind::Paragraph => paragraph_object(snapshot, cursor, around),
        ObjectKind::Pair(ch) => pair_object(snapshot, cursor, ch, around),
        ObjectKind::ClosestPair => closest_pair_object(snapshot, cursor, around),
    }
}

/// The word under the cursor.
///
/// The byte classification is [`is_word_byte`]'s, so a CJK run is one word
/// and the object agrees with the `w` motion the reader just used to get
/// there.
fn word_object(
    snapshot: &DocumentSnapshot,
    cursor: usize,
) -> Result<std::ops::Range<usize>, String> {
    let cursor = clamp_offset(snapshot, cursor);
    // The word **at** the cursor. A cursor on a separator names the next
    // word, the same rule Helix's `e` uses to step onto one.
    let start = if is_word_byte(leading_byte(snapshot, cursor).unwrap_or(0)) {
        word_start_at(snapshot, cursor)
    } else {
        word_forward(snapshot, cursor)
    };
    let end = char_end(snapshot, word_end(snapshot, start));
    if end <= start {
        return Err("no word under the cursor".to_string());
    }
    Ok(start..end)
}

/// The start of the word run that contains `cursor`.
fn word_start_at(snapshot: &DocumentSnapshot, cursor: usize) -> usize {
    let mut start = clamp_offset(snapshot, cursor);
    while start > 0 {
        let previous = prev_char_boundary(snapshot, start);
        if !is_word_byte(leading_byte(snapshot, previous).unwrap_or(0)) {
            break;
        }
        start = previous;
    }
    start
}

/// The whitespace-delimited run under the cursor.
fn long_word_object(
    snapshot: &DocumentSnapshot,
    cursor: usize,
) -> Result<std::ops::Range<usize>, String> {
    let cursor = clamp_offset(snapshot, cursor);
    let start = if is_long_word_byte(leading_byte(snapshot, cursor).unwrap_or(0)) {
        long_word_start_at(snapshot, cursor)
    } else {
        long_word_forward(snapshot, cursor)
    };
    let end = char_end(snapshot, long_word_end(snapshot, start));
    if end <= start {
        return Err("no WORD under the cursor".to_string());
    }
    Ok(start..end)
}

/// The start of the whitespace-delimited run that contains `cursor`.
fn long_word_start_at(snapshot: &DocumentSnapshot, cursor: usize) -> usize {
    let mut start = clamp_offset(snapshot, cursor);
    while start > 0 {
        let previous = prev_char_boundary(snapshot, start);
        if !is_long_word_byte(leading_byte(snapshot, previous).unwrap_or(0)) {
            break;
        }
        start = previous;
    }
    start
}

/// Extend `range` over the whitespace that follows it.
///
/// Helix's "around word" is the word **plus the whitespace after it**: the
/// difference between `a w` and `i w` is that trailing boundary, and a reader
/// who selects a word to replace it usually wants the space that separated
/// it from the next one to go with it.
///
/// The run stops at a line terminator: a paragraph break is not the word's
/// trailing whitespace, and swallowing it would make `a w` select across a
/// blank line.
fn extend_over_trailing_space(
    snapshot: &DocumentSnapshot,
    range: std::ops::Range<usize>,
) -> std::ops::Range<usize> {
    let mut end = range.end;
    while end < snapshot.len_bytes() {
        let Some(byte) = leading_byte(snapshot, end) else {
            break;
        };
        if !byte.is_ascii_whitespace() || byte == b'\n' || byte == b'\r' {
            break;
        }
        // `char_end`, not `next_char_boundary`: the latter is `None` for the
        // last character of the document, so a file ending in a space would
        // lose that space from the "around word" range.
        let next = char_end(snapshot, end);
        if next <= end {
            break;
        }
        end = next;
    }
    range.start..end
}

/// The paragraph under the cursor.
///
/// A paragraph is a run of **content** lines; a blank line ends it. The
/// "inside" form is the paragraph's own bytes, and the "around" form adds
/// the blank run that follows it — Helix's `a p`, which takes the separator
/// with the paragraph so that replacing one replaces the break too.
///
/// A cursor on a blank line has no paragraph under it, and says so rather
/// than guessing which of the two neighbours was meant.
fn paragraph_object(
    snapshot: &DocumentSnapshot,
    cursor: usize,
    around: bool,
) -> Result<std::ops::Range<usize>, String> {
    let line = cursor_line(snapshot, cursor);
    if line_is_blank(snapshot, line) {
        return Err("the cursor is on a blank line — no paragraph here".to_string());
    }
    let last_line = last_line_of(snapshot);
    let mut first = line;
    while first > 0 && !line_is_blank(snapshot, first - 1) {
        first -= 1;
    }
    let mut last = line;
    while last < last_line && !line_is_blank(snapshot, last + 1) {
        last += 1;
    }
    let start = line_start(snapshot, first).unwrap_or(0);
    let mut end = line_content_end(snapshot, last).unwrap_or(start);
    if around {
        // The separator: the blank run that follows the paragraph.
        let mut separator = last + 1;
        while separator <= last_line && line_is_blank(snapshot, separator) {
            separator += 1;
        }
        end = line_start(snapshot, separator)
            .unwrap_or_else(|| snapshot.len_bytes())
            .min(snapshot.len_bytes());
    }
    if end <= start {
        return Err("the paragraph is empty".to_string());
    }
    Ok(start..end)
}

/// The pair named by `ch`, and the cursor's place in it.
///
/// `ch` names the **kind** of pair: `(` and `)` name the same one. A cursor
/// sitting on one of the pair's delimiters names that pair; a cursor inside
/// one names the innermost pair of that kind that encloses it.
fn pair_object(
    snapshot: &DocumentSnapshot,
    cursor: usize,
    ch: char,
    around: bool,
) -> Result<std::ops::Range<usize>, String> {
    let Some(pair) = editor_match::pair_for(ch) else {
        return Err(format!("{ch:?} is not a delimiter pair"));
    };
    let Some(window) = ScanWindow::around(snapshot, cursor, MAX_SET_VERB_READ_BYTES) else {
        return Err("the bracket scan could not read a window".to_string());
    };
    let anchor = window.relative(cursor);
    // The character **under the cursor** decides how the pair is found — not
    // the character the reader typed. The typed one names the *kind* of pair;
    // the one under the cursor says whether the cursor is standing on a
    // delimiter of that kind, and a cursor standing on one names that pair
    // while a cursor inside one names the innermost pair that encloses it.
    //
    // A quote is its own opener and closer, so the two readings cannot be
    // told apart by the character alone: a cursor on a quote names the run
    // that starts there, and a cursor inside a run names the run it is in.
    let under = char_at(snapshot, cursor);
    let outcome = match under {
        Some(ch) if ch == pair.open => {
            editor_match::pair_from_open(&window.chunk, anchor, pair, window.chunk.len())
        }
        Some(ch) if ch == pair.close && !pair.is_quote() => {
            editor_match::pair_from_close(&window.chunk, anchor, pair, window.chunk.len())
        }
        _ => editor_match::innermost_pair(&window.chunk, anchor, pair, window.chunk.len()),
    };
    let (open, close) = match outcome {
        editor_match::MatchOutcome::Found { open, close } => (open, close),
        editor_match::MatchOutcome::BudgetExhausted => {
            return Err("the bracket scan hit its byte budget".to_string())
        }
        editor_match::MatchOutcome::Unbalanced => {
            return Err("no matching pair around the cursor".to_string())
        }
    };
    let open = window.absolute(open);
    let close = window.absolute(close);
    let range = if around {
        open..close + pair.close.len_utf8()
    } else {
        open + pair.open.len_utf8()..close
    };
    if range.is_empty() {
        return Err("the pair is empty".to_string());
    }
    Ok(range)
}

/// The innermost pair around the cursor, of any kind.
fn closest_pair_object(
    snapshot: &DocumentSnapshot,
    cursor: usize,
    around: bool,
) -> Result<std::ops::Range<usize>, String> {
    let Some(window) = ScanWindow::around(snapshot, cursor, MAX_SET_VERB_READ_BYTES) else {
        return Err("the bracket scan could not read a window".to_string());
    };
    let anchor = window.relative(cursor);
    let (open, close) = match editor_match::closest_pair(&window.chunk, anchor, window.chunk.len())
    {
        editor_match::MatchOutcome::Found { open, close } => (open, close),
        editor_match::MatchOutcome::BudgetExhausted => {
            return Err("the bracket scan hit its byte budget".to_string())
        }
        editor_match::MatchOutcome::Unbalanced => {
            return Err("no matching pair around the cursor".to_string())
        }
    };
    let open = window.absolute(open);
    let close = window.absolute(close);
    let Some(ch) = char_at(snapshot, open) else {
        return Err("no matching pair around the cursor".to_string());
    };
    let Some(pair) = editor_match::pair_for(ch) else {
        return Err("no matching pair around the cursor".to_string());
    };
    let range = if around {
        open..close + pair.close.len_utf8()
    } else {
        open + pair.open.len_utf8()..close
    };
    if range.is_empty() {
        return Err("the pair is empty".to_string());
    }
    Ok(range)
}

/// The one member that covers `range`.
///
/// A character selection's range runs to `char_end(head)`, so a member built
/// to cover `start..end` must name the **last character's start** as its head
/// — pointing `head` at `end` would cover one character too many.
fn member_for_range(
    snapshot: &DocumentSnapshot,
    range: std::ops::Range<usize>,
) -> Option<Selection<EditorPoint>> {
    let head = last_char_start(snapshot, range.end)?;
    let mut member = Selection::new(editor_point(snapshot, range.start), SelectionMode::Char);
    member.head = editor_point(snapshot, head);
    Some(member)
}

/// The member that covers the content, moved by `shift` in the post-edit text.
///
/// `range` is in **pre-edit** coordinates and the head is the content's last
/// character, so the post-edit head is that character moved by `shift`.
///
/// The shift is applied to a **pre-edit** offset on purpose. A post-edit
/// offset past the old end of the document cannot be read from the pre-edit
/// snapshot at all — `clamp_offset` would pull it back to the old end, and
/// the member would cover one character too few. Every surround removes or
/// adds single-character delimiters, and none of them is a newline, so no
/// line boundary moves and the pre-edit line of the same logical position is
/// the post-edit line.
fn member_shifted(
    snapshot: &DocumentSnapshot,
    range: std::ops::Range<usize>,
    shift: isize,
) -> Option<Selection<EditorPoint>> {
    let head_before = last_char_start(snapshot, range.end)?;
    let anchor = EditorPoint {
        byte: (range.start as isize + shift).max(0) as usize,
        line: cursor_line(snapshot, range.start),
    };
    let head = EditorPoint {
        byte: (head_before as isize + shift).max(0) as usize,
        line: cursor_line(snapshot, head_before),
    };
    let mut member = Selection::new(anchor, SelectionMode::Char);
    member.head = head;
    Some(member)
}

/// The range a surround verb acts on: the held selection, or the character
/// under the cursor when nothing is selected.
///
/// Helix's surround applies to the selection; with nothing selected it
/// applies to the character under the cursor, which is the same gesture with
/// the smallest possible target.
///
/// The **primary** member only. A surround that rewrites several members at
/// once is a different gesture, and Helix's is not offered here.
fn surround_target(
    snapshot: &DocumentSnapshot,
    state: &SelectionState<EditorPoint>,
) -> Option<std::ops::Range<usize>> {
    let primary = state.primary_index().unwrap_or_default();
    if let Some(member) = state.selections().get(primary) {
        if let Some(range) = member_range(snapshot, member) {
            return Some(range);
        }
    }
    let cursor = clamp_offset(snapshot, state.cursor().byte);
    let end = char_end(snapshot, cursor);
    (end > cursor).then_some(cursor..end)
}

/// The delimiters a surround character stands for.
///
/// A character that is one of the table's delimiters brings its **pair** with
/// it: `(` adds `(` and `)`. A character that is not in the table is its own
/// one-character pair, which is what a quote needs and what Helix's surround
/// accepts for any character at all.
fn surround_delimiters(ch: char) -> (char, char) {
    match editor_match::pair_for(ch) {
        Some(pair) => (pair.open, pair.close),
        None => (ch, ch),
    }
}

/// `m s` — surround the held selection with a character's pair.
///
/// The selection **grows** to cover the new delimiters: Helix's `m s`
/// selects the surrounded text, so the reader can go on to delete or replace
/// it as one thing.
fn surround_add(
    snapshot: &DocumentSnapshot,
    state: &SelectionState<EditorPoint>,
    ch: char,
) -> Result<HelixVerbOutcome, String> {
    let Some(range) = surround_target(snapshot, state) else {
        return Err("nothing to surround".to_string());
    };
    let (open, close) = surround_delimiters(ch);
    let open_text = open.to_string();
    let close_text = close.to_string();
    let edits = if range.is_empty() {
        vec![TextEdit::new(
            range.start..range.start,
            format!("{open_text}{close_text}"),
        )]
    } else {
        vec![
            TextEdit::new(range.start..range.start, open_text.clone()),
            TextEdit::new(range.end..range.end, close_text.clone()),
        ]
    };
    let cursor = clamp_offset(snapshot, state.cursor().byte);
    let cursor_after = if cursor >= range.start {
        cursor + open_text.len()
    } else {
        cursor
    };
    let edit = build_edit(snapshot, TextTransaction::new(edits), cursor_after, false)
        .ok_or("the surround produced no edit".to_string())?;
    // The selection, in **post-edit** coordinates: the opening delimiter
    // pushed the content right, and the closing delimiter is the last
    // character of the surrounded text.
    // The selection, in **post-edit** coordinates: the opening delimiter is
    // the first character and the closing one is the last, so the member's
    // head is the closing delimiter's start.
    //
    // Computed rather than read: the post-edit offsets are past the end of
    // the pre-edit text, and the pre-edit snapshot cannot answer for them.
    let anchor = EditorPoint {
        byte: range.start,
        line: cursor_line(snapshot, range.start),
    };
    let head = EditorPoint {
        byte: range.end + open_text.len(),
        line: cursor_line(snapshot, range.end),
    };
    let mut member = Selection::new(anchor, SelectionMode::Char);
    member.head = head;
    Ok(HelixVerbOutcome::EditAndSelect(edit, None, vec![member]))
}

/// `m d` — remove a character's pair around the held selection, keeping the
/// contents.
///
/// The **innermost** enclosing pair is the one removed: those are the
/// delimiters immediately around the selected text, and removing them
/// unwraps it in place. The contents stay selected.
fn surround_delete(
    snapshot: &DocumentSnapshot,
    state: &SelectionState<EditorPoint>,
    ch: char,
) -> Result<HelixVerbOutcome, String> {
    let Some(range) = surround_target(snapshot, state) else {
        return Err("nothing to unsurround".to_string());
    };
    let Some(pair) = editor_match::pair_for(ch) else {
        return Err(format!("{ch:?} is not a delimiter pair"));
    };
    let Some(window) = ScanWindow::around(snapshot, range.start, MAX_SET_VERB_READ_BYTES) else {
        return Err("the bracket scan could not read a window".to_string());
    };
    let anchor = window.relative(range.start);
    let (open, close) =
        match editor_match::innermost_pair(&window.chunk, anchor, pair, window.chunk.len()) {
            editor_match::MatchOutcome::Found { open, close } => (open, close),
            editor_match::MatchOutcome::BudgetExhausted => {
                return Err("the bracket scan hit its byte budget".to_string())
            }
            editor_match::MatchOutcome::Unbalanced => {
                return Err("no matching pair around the selection".to_string())
            }
        };
    let open = window.absolute(open);
    let close = window.absolute(close);
    // The delimiters have to actually surround the target.
    if open >= range.start || close < range.end {
        return Err("no matching pair around the selection".to_string());
    }
    let edits = vec![
        TextEdit::new(open..open + pair.open.len_utf8(), String::new()),
        TextEdit::new(close..close + pair.close.len_utf8(), String::new()),
    ];
    let cursor = clamp_offset(snapshot, state.cursor().byte);
    // The cursor moves with the text it was on. The opening delimiter's
    // bytes are gone, so anything **after** it shifts left; a cursor before
    // it, or on it, does not move. The closing delimiter is after the
    // contents, and the same rule applies to it.
    let mut cursor_after = cursor;
    if cursor >= open + pair.open.len_utf8() {
        cursor_after = cursor_after.saturating_sub(pair.open.len_utf8());
    }
    if cursor >= close + pair.close.len_utf8() {
        cursor_after = cursor_after.saturating_sub(pair.close.len_utf8());
    }
    let edit = build_edit(snapshot, TextTransaction::new(edits), cursor_after, false)
        .ok_or("the unsurround produced no edit".to_string())?;
    // The contents, in post-edit coordinates: only the opening delimiter's
    // bytes are gone, and they were before the contents.
    let member = member_shifted(snapshot, range.clone(), -(pair.open.len_utf8() as isize));
    Ok(HelixVerbOutcome::EditAndSelect(
        edit,
        None,
        member.into_iter().collect(),
    ))
}

/// `m r` — replace a character's pair around the held selection with another
/// character's pair.
///
/// The **outermost** enclosing pair of `from` is the one replaced, so the
/// replaced text keeps its inner pairs intact: `(a(b))` with `m r ([` becomes
/// `[a(b)]`, not `[a(b])`.
fn surround_replace(
    snapshot: &DocumentSnapshot,
    state: &SelectionState<EditorPoint>,
    from: char,
    to: char,
) -> Result<HelixVerbOutcome, String> {
    let Some(range) = surround_target(snapshot, state) else {
        return Err("nothing to resurround".to_string());
    };
    let Some(pair) = editor_match::pair_for(from) else {
        return Err(format!("{from:?} is not a delimiter pair"));
    };
    let (open, close) = surround_delimiters(to);
    let Some(window) = ScanWindow::around(snapshot, range.start, MAX_SET_VERB_READ_BYTES) else {
        return Err("the bracket scan could not read a window".to_string());
    };
    let anchor = window.relative(range.start);
    let (found_open, found_close) =
        match editor_match::enclosing_pair(&window.chunk, anchor, pair, window.chunk.len()) {
            editor_match::MatchOutcome::Found { open, close } => (open, close),
            editor_match::MatchOutcome::BudgetExhausted => {
                return Err("the bracket scan hit its byte budget".to_string())
            }
            editor_match::MatchOutcome::Unbalanced => {
                return Err("no matching pair around the selection".to_string())
            }
        };
    let found_open = window.absolute(found_open);
    let found_close = window.absolute(found_close);
    if found_open >= range.start || found_close < range.end {
        return Err("no matching pair around the selection".to_string());
    }
    let edits = vec![
        TextEdit::new(
            found_open..found_open + pair.open.len_utf8(),
            open.to_string(),
        ),
        TextEdit::new(
            found_close..found_close + pair.close.len_utf8(),
            close.to_string(),
        ),
    ];
    // The cursor moves with the text it was on: the opening delimiter's
    // replacement can be a different width, and the closing one is after the
    // cursor.
    let cursor = clamp_offset(snapshot, state.cursor().byte);
    let mut cursor_after = cursor as isize;
    if cursor >= found_close {
        cursor_after += close.len_utf8() as isize - pair.close.len_utf8() as isize;
    }
    if cursor >= found_open {
        cursor_after += open.len_utf8() as isize - pair.open.len_utf8() as isize;
    }
    let edit = build_edit(
        snapshot,
        TextTransaction::new(edits),
        cursor_after.max(0) as usize,
        false,
    )
    .ok_or("the resurround produced no edit".to_string())?;
    // The contents, in post-edit coordinates.
    let shift = open.len_utf8() as isize - pair.open.len_utf8() as isize;
    let member = member_shifted(snapshot, range.clone(), shift);
    Ok(HelixVerbOutcome::EditAndSelect(
        edit,
        None,
        member.into_iter().collect(),
    ))
}

/// A one-member range set.
///
/// Test-only, and shared with the command-layer tests: both assert on range
/// sets, and both would otherwise write `vec![a..b]` — which clippy's
/// `single_range_in_vec_init` rejects, offering two rewrites that each mean
/// something else entirely for a `Vec<Range<usize>>`.
#[cfg(test)]
pub(crate) fn one_range(range: std::ops::Range<usize>) -> Vec<std::ops::Range<usize>> {
    std::iter::once(range).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::helix_selection::{Selection, SelectionMode};
    use codegg_document::{DocumentBuffer, DocumentLimits};

    fn snapshot_of(text: &str) -> DocumentSnapshot {
        DocumentBuffer::new(text).snapshot()
    }

    fn presentation_at(cursor: usize) -> TuiDocumentPresentation {
        TuiDocumentPresentation {
            cursor_byte: cursor,
            ..TuiDocumentPresentation::default()
        }
    }

    /// Park the cursor on `offset`, as the `/` search handler does between
    /// two `step_search` calls.
    fn editor_move(
        presentation: &mut TuiDocumentPresentation,
        snapshot: &DocumentSnapshot,
        offset: usize,
    ) {
        move_cursor_to(presentation, snapshot, offset, 10);
    }

    // ── M011: insert-mode deletion builders ─────────────────────────────

    /// The byte range a builder would delete, or `None` when it deletes
    /// nothing.
    fn deleted_range(edit: Option<EditorEdit>) -> Option<std::ops::Range<usize>> {
        let edit = edit?;
        let first = edit.transaction.edits.first()?;
        Some(first.range.start..first.range.end)
    }

    /// The text a builder would delete.
    fn deleted_text(snapshot: &DocumentSnapshot, edit: Option<EditorEdit>) -> Option<String> {
        let range = deleted_range(edit)?;
        snapshot.read_bytes(range.start, range.end).ok()
    }

    #[test]
    fn kill_to_line_start_deletes_the_line_head_and_nothing_more() {
        let snapshot = snapshot_of("hello world\nsecond\n");
        assert_eq!(deleted_range(kill_to_line_start(&snapshot, 6)), Some(0..6));
        assert_eq!(
            deleted_range(kill_to_line_start(&snapshot, 0)),
            None,
            "a cursor already at the line start has nothing to kill"
        );
    }

    #[test]
    fn delete_word_backward_stops_at_the_line_start() {
        let snapshot = snapshot_of("abc\ndef\n");
        assert_eq!(
            deleted_range(delete_word_backward(&snapshot, 7)),
            Some(4..7),
            "the word before the cursor"
        );
        assert_eq!(
            deleted_range(delete_word_backward(&snapshot, 4)),
            None,
            "at the line start the scan must not reach into the line above"
        );
    }

    #[test]
    fn delete_word_forward_stops_at_the_line_end() {
        let snapshot = snapshot_of("abc\ndef\n");
        assert_eq!(
            deleted_range(delete_word_forward(&snapshot, 0)),
            Some(0..3),
            "the word after the cursor"
        );
        assert_eq!(
            deleted_range(delete_word_forward(&snapshot, 2)),
            Some(2..3),
            "from inside a word, the rest of that word"
        );
        assert_eq!(
            deleted_range(delete_word_forward(&snapshot, 3)),
            None,
            "at the line end there is no next word on this line"
        );
    }

    #[test]
    fn the_word_deletions_never_split_a_multi_byte_character() {
        // "é" is two bytes, and the scan steps by character boundary.
        let snapshot = snapshot_of("café x\n");
        assert_eq!(
            deleted_text(&snapshot, delete_word_backward(&snapshot, 6)).as_deref(),
            Some("café "),
            "the accented character is one word unit, and the space with it"
        );
        assert_eq!(
            deleted_text(&snapshot, delete_word_forward(&snapshot, 3)).as_deref(),
            Some("é"),
            "and the forward scan stops on its last byte, not its first"
        );
    }

    // ── M011: the insert session ─────────────────────────────────────────

    #[test]
    fn a_deletion_ends_the_typing_run() {
        let mut presentation = presentation_at(0);
        let insertion = TextTransaction::new(vec![TextEdit::new(0..0, "a")]);
        assert!(
            absorb_insert_edit(&mut presentation, &insertion, 1, 0, Vec::new()),
            "the first insertion opens the run"
        );
        let deletion = TextTransaction::new(vec![TextEdit::new(0..1, "")]);
        assert!(
            !absorb_insert_edit(&mut presentation, &deletion, 1, 0, Vec::new()),
            "a deletion is not a continuation of an insertion run"
        );
        commit_insert_session(&mut presentation);
        assert_eq!(presentation.history.len(), 1, "the run is one step");
    }

    #[test]
    fn an_insertion_away_from_the_end_of_the_run_ends_it() {
        let mut presentation = presentation_at(0);
        let insertion = TextTransaction::new(vec![TextEdit::new(0..0, "a")]);
        assert!(absorb_insert_edit(
            &mut presentation,
            &insertion,
            1,
            0,
            Vec::new()
        ));
        let elsewhere = TextTransaction::new(vec![TextEdit::new(0..0, "b")]);
        assert!(
            !absorb_insert_edit(&mut presentation, &elsewhere, 1, 0, Vec::new()),
            "an insertion anywhere but the end of the run is a different edit"
        );
    }

    #[test]
    fn a_run_is_committed_as_one_insertion_with_one_inverse() {
        let mut presentation = presentation_at(0);
        let first = TextTransaction::new(vec![TextEdit::new(0..0, "a")]);
        assert!(absorb_insert_edit(
            &mut presentation,
            &first,
            1,
            0,
            Vec::new()
        ));
        let second = TextTransaction::new(vec![TextEdit::new(1..1, "b")]);
        assert!(
            absorb_insert_edit(&mut presentation, &second, 1, 0, Vec::new()),
            "the run continues at the end of what it has typed"
        );
        commit_insert_session(&mut presentation);

        assert_eq!(presentation.history.len(), 1, "one run, one step");
        let entry = undo_entry(&presentation).expect("one entry");
        let forward = entry.forward.edits.first().expect("one edit");
        assert_eq!(forward.range.start..forward.range.end, 0..0);
        assert_eq!(forward.insert, "ab", "the whole run, as one insertion");
        let inverse = entry.inverse.edits.first().expect("one edit");
        assert_eq!(inverse.range.start..inverse.range.end, 0..2);
        assert!(inverse.insert.is_empty(), "and an inverse that deletes it");
    }

    #[test]
    fn the_first_edit_of_a_run_clears_the_redo_tail() {
        let snapshot = snapshot_of("hello");
        let mut presentation = presentation_at(0);
        // An entry, undone, so that there is a tail to invalidate. Without
        // one the assertion below would hold for a history that never had
        // anything in it.
        record_undo(
            &mut presentation,
            &insert_text(&snapshot, 0, "abc").unwrap(),
        );
        assert!(move_history_cursor_back(&mut presentation));
        assert!(
            redo_entry(&presentation).is_some(),
            "there is a tail to clear"
        );

        let insertion = TextTransaction::new(vec![TextEdit::new(0..0, "a")]);
        assert!(absorb_insert_edit(
            &mut presentation,
            &insertion,
            1,
            0,
            Vec::new()
        ));
        assert!(redo_entry(&presentation).is_none(), "the redo tail is gone");
        assert_eq!(
            presentation.history_cursor, 0,
            "and the cursor is back at the beginning"
        );
    }

    // ── `Ctrl+d` continues to the end ───────────────────────────────────

    #[test]
    fn half_page_down_continues_to_the_last_line() {
        // 30 lines, a 10-row window. From viewport 20 a half page would land
        // on 25 — still inside — so this first press pages normally.
        let text = (0..30).map(|i| format!("line{i}\n")).collect::<String>();
        let snapshot = snapshot_of(&text);
        let mut presentation = presentation_at(0);
        presentation.viewport_line = 20;
        apply_motion(
            &mut presentation,
            &snapshot,
            EditorCommand::HalfPageDown,
            10,
        );
        assert_eq!(cursor_line(&snapshot, presentation.cursor_byte), 25);

        // From 25 the next half page would land on 30, which is past the last
        // line. It must continue to the end instead of refusing there.
        presentation.viewport_line = 25;
        apply_motion(
            &mut presentation,
            &snapshot,
            EditorCommand::HalfPageDown,
            10,
        );
        assert_eq!(
            cursor_line(&snapshot, presentation.cursor_byte),
            last_line_of(&snapshot),
            "a half page that would overshoot must continue to the last line"
        );
    }

    #[test]
    fn half_page_down_already_at_the_end_stays_at_the_end() {
        let text = (0..12).map(|i| format!("line{i}\n")).collect::<String>();
        let snapshot = snapshot_of(&text);
        let mut presentation = presentation_at(buffer_content_end(&snapshot));
        presentation.viewport_line = last_line_of(&snapshot);
        apply_motion(
            &mut presentation,
            &snapshot,
            EditorCommand::HalfPageDown,
            10,
        );
        assert_eq!(
            cursor_line(&snapshot, presentation.cursor_byte),
            last_line_of(&snapshot)
        );
    }

    // ── `/` search ─────────────────────────────────────────────────────

    #[test]
    fn find_matches_returns_every_offset_in_order() {
        let snapshot = snapshot_of("let host_status = 1;\nfn host_status() {}\n");
        let outcome = find_matches(&snapshot, "host_status");
        assert_eq!(outcome.matches.len(), 2);
        assert!(outcome.matches[0] < outcome.matches[1]);
        assert!(!outcome.truncated && !outcome.bounded);
        // Every offset is a cursor position, so every one must be a char
        // boundary — a non-boundary offset would panic the renderer.
        for offset in outcome.matches {
            assert!(snapshot.is_byte_boundary(offset));
        }
    }

    #[test]
    fn an_empty_needle_matches_nothing() {
        let snapshot = snapshot_of("alpha\n");
        let outcome = find_matches(&snapshot, "");
        assert!(outcome.matches.is_empty());
        assert!(!outcome.truncated && !outcome.bounded);
    }

    #[test]
    fn a_search_that_finds_nothing_leaves_the_cursor_alone() {
        let snapshot = snapshot_of("alpha\nbeta\n");
        let mut presentation = presentation_at(7);
        let outcome = find_matches(&snapshot, "zzz");
        let landed = presentation.adopt_search(outcome, presentation.cursor_byte);
        assert_eq!(landed, None, "a failed search must not move the cursor");
        assert_eq!(presentation.cursor_byte, 7);
    }

    #[test]
    fn search_lands_on_the_match_the_cursor_is_already_on() {
        let snapshot = snapshot_of("aaa bbb aaa\n");
        let mut presentation = presentation_at(0);
        let outcome = find_matches(&snapshot, "aaa");
        // The cursor sits on the first `aaa`. A live search must leave it
        // there: a strict `>` would hop to the second match on the very first
        // keystroke, so refining a needle you are looking at would run away
        // from you.
        assert_eq!(
            presentation.adopt_search(outcome, presentation.cursor_byte),
            Some(0)
        );
    }

    #[test]
    fn stepping_walks_matches_and_wraps_at_both_ends() {
        let snapshot = snapshot_of("aaa bbb aaa\n");
        let mut presentation = presentation_at(0);
        let outcome = find_matches(&snapshot, "aaa");
        presentation.adopt_search(outcome, 0);
        let landed = |presentation: &mut TuiDocumentPresentation, forward: bool| {
            let offset = presentation.step_search(forward).expect("two matches");
            editor_move(presentation, &snapshot, offset);
            offset
        };
        // `n` must move *off* the match the cursor is on, not re-report it.
        assert_eq!(landed(&mut presentation, true), 8);
        // And from the last match it wraps rather than stalling.
        assert_eq!(landed(&mut presentation, true), 0);
        // `N` walks the other way and wraps at the top.
        assert_eq!(landed(&mut presentation, false), 8);
    }

    #[test]
    fn search_stepping_with_no_matches_reports_nothing_to_do() {
        let mut presentation = presentation_at(0);
        presentation.cancel_search();
        assert_eq!(presentation.step_search(true), None);
        assert_eq!(presentation.step_search(false), None);
        assert!(!presentation.has_search());
    }

    #[test]
    fn the_search_line_is_bounded_at_the_keystroke() {
        let mut presentation = presentation_at(0);
        presentation.open_search_line("");
        for _ in 0..(MAX_EDITOR_SEARCH_LINE + 50) {
            presentation.push_search_char('a');
        }
        assert_eq!(
            presentation.search_line.chars().count(),
            MAX_EDITOR_SEARCH_LINE,
            "the needle is a query, not a buffer; it must not grow without bound"
        );
        // And a refused keystroke leaves the line exactly as it was.
        presentation.push_search_char('b');
        assert_eq!(
            presentation.search_line.chars().count(),
            MAX_EDITOR_SEARCH_LINE
        );
        assert!(presentation.search_line.ends_with('a'));
    }

    #[test]
    fn accepting_a_search_keeps_the_highlight_and_cancelling_drops_it() {
        let snapshot = snapshot_of("aaa bbb aaa\n");
        let mut presentation = presentation_at(0);
        presentation.open_search_line("aaa");
        let outcome = find_matches(&snapshot, "aaa");
        presentation.adopt_search(outcome, 0);
        presentation.close_search_line();
        assert!(!presentation.search_active, "Enter closes the line");
        assert!(presentation.has_search(), "and keeps the matches for n/N");

        presentation.open_search_line("aaa");
        presentation.cancel_search();
        assert!(!presentation.search_active);
        assert!(!presentation.has_search(), "Esc disowns the highlight too");
    }

    #[test]
    fn an_empty_search_confirms_to_nothing() {
        let mut presentation = presentation_at(0);
        presentation.open_search_line("");
        presentation.close_search_line();
        // Enter on an empty needle is a search for nothing, so there is
        // nothing to leave painted.
        assert!(!presentation.has_search());
    }

    #[test]
    fn word_at_seeds_the_search_line_from_the_cursor() {
        let snapshot = snapshot_of("let host_status = 1;\n");
        assert_eq!(
            word_at(&snapshot, 6),
            Some("host_status".to_string()),
            "`/` on a word should start from that word"
        );
        // On whitespace there is no word, so the line opens empty rather than
        // searching for a space.
        assert_eq!(word_at(&snapshot, 3), None);
    }

    #[test]
    fn word_at_handles_multibyte_text_without_slicing_a_character() {
        let snapshot = snapshot_of("let café = 1;\n");
        // Byte 8 is the start of `é`; the seed must be the whole word.
        assert_eq!(word_at(&snapshot, 8), Some("café".to_string()));
        // A mid-character offset is refused rather than sliced — callers
        // clamp to boundaries, so reaching one means something is wrong
        // upstream and it must not be papered over with a panic.
        assert_eq!(word_at(&snapshot, 9), None);
    }

    #[test]
    fn move_cursor_to_reveals_the_target_and_drops_a_held_selection() {
        let text = (0..40).map(|i| format!("line{i}\n")).collect::<String>();
        let snapshot = snapshot_of(&text);
        let mut presentation = presentation_at(0);
        hold(
            &mut presentation,
            vec![Selection {
                anchor: EditorPoint { byte: 0, line: 0 },
                head: EditorPoint { byte: 3, line: 0 },
                mode: SelectionMode::Char,
                growing: false,
            }],
        );
        let end = line_start(&snapshot, 30).expect("line 30 starts somewhere");
        move_cursor_to(&mut presentation, &snapshot, end, 10);
        assert_eq!(presentation.cursor_byte, end);
        assert_eq!(cursor_line(&snapshot, presentation.cursor_byte), 30);
        assert!(
            presentation.selections.is_empty(),
            "a jump must not drag a stale selection anchor along with it"
        );
        // Revealed, so the target is inside the window the viewport shows.
        assert!(30 >= presentation.viewport_line && 30 < presentation.viewport_line + 10);
    }

    // ── Completion edit resolution ────────────────────────────────────

    fn completion_edit(
        start: (u32, u32),
        end: (u32, u32),
        new_text: &str,
    ) -> codegg_protocol::lsp::LspCompletionEditDto {
        codegg_protocol::lsp::LspCompletionEditDto {
            start_line: start.0,
            start_character: start.1,
            end_line: end.0,
            end_character: end.1,
            new_text: new_text.to_string(),
        }
    }

    #[test]
    fn a_completion_replace_resolves_to_one_forward_and_inverse() {
        // Line 0 is 12 bytes, so "    prin" is bytes 12..20 and the word
        // itself is 16..20: the assertion is on bytes, not UTF-16 units.
        let snapshot = snapshot_of("fn main() {\n    prin\n}\n");
        let edit = completion_edit((1, 4), (1, 8), "println!");
        let resolved = resolve_completion_edit(&snapshot, &edit, true).expect("resolves");
        assert_eq!(resolved.transaction.edits.len(), 1);
        assert_eq!(
            resolved.transaction.edits[0].range.start, 16,
            "byte range, not UTF-16"
        );
        assert_eq!(resolved.transaction.edits[0].range.end, 20);
        assert_eq!(resolved.transaction.edits[0].insert, "println!");
        assert_eq!(resolved.inverse.edits.len(), 1);
        assert_eq!(resolved.inverse.edits[0].insert, "prin");
        assert_eq!(resolved.cursor_after, 16 + "println!".len());
        assert!(resolved.stay_insert);
    }

    #[test]
    fn a_completion_insert_at_cursor_resolves_to_a_zero_width_replace() {
        let snapshot = snapshot_of("fn main() {\n    prin\n}\n");
        let edit = completion_edit((1, 8), (1, 8), "tln!");
        let resolved = resolve_completion_edit(&snapshot, &edit, false).expect("resolves");
        assert_eq!(resolved.transaction.edits[0].range.start, 20);
        assert_eq!(resolved.transaction.edits[0].range.end, 20);
        assert!(!resolved.stay_insert);
    }

    #[test]
    fn completion_ranges_validate_utf16_exactly() {
        // "a😀b": the emoji is 2 UTF-16 units; column 2 splits the pair.
        let snapshot = snapshot_of("a😀b\n");
        let split = completion_edit((0, 0), (0, 2), "x");
        assert!(
            resolve_completion_edit(&snapshot, &split, false).is_none(),
            "a split surrogate must refuse"
        );
        let whole = completion_edit((0, 0), (0, 3), "x");
        assert!(
            resolve_completion_edit(&snapshot, &whole, false).is_some(),
            "exact surrogate span resolves"
        );
        // "a😀b" is 4 UTF-16 units, so 4 is the valid line end and 5 is
        // past it.
        let past_end = completion_edit((0, 0), (0, 5), "x");
        assert!(
            resolve_completion_edit(&snapshot, &past_end, false).is_none(),
            "past-the-end refuses"
        );
        let reversed = completion_edit((0, 3), (0, 0), "x");
        assert!(
            resolve_completion_edit(&snapshot, &reversed, false).is_none(),
            "reversed ranges refuse"
        );
    }

    #[test]
    fn completion_refuses_absent_lines_noop_edits_and_oversized_text() {
        let snapshot = snapshot_of("ab\n");
        let absent = completion_edit((9, 0), (9, 1), "x");
        assert!(resolve_completion_edit(&snapshot, &absent, false).is_none());
        let noop = completion_edit((0, 1), (0, 1), "");
        assert!(
            resolve_completion_edit(&snapshot, &noop, false).is_none(),
            "a no-op is not an edit"
        );
        let deletion = completion_edit((0, 0), (0, 1), "");
        let resolved = resolve_completion_edit(&snapshot, &deletion, false).expect("deletes");
        assert_eq!(resolved.transaction.edits[0].insert, "");
        assert_eq!(resolved.inverse.edits[0].insert, "a");
        let big = "x".repeat(codegg_protocol::lsp::MAX_LSP_COMPLETION_EDIT_BYTES + 1);
        let oversized = completion_edit((0, 0), (0, 1), &big);
        assert!(
            resolve_completion_edit(&snapshot, &oversized, false).is_none(),
            "oversized edits refuse even when the DTO claims otherwise"
        );
    }

    // ── Offset clamping ───────────────────────────────────────────────

    #[test]
    fn clamp_lands_on_a_char_boundary_inside_a_multibyte_char() {
        let snapshot = snapshot_of("aé€b");
        // é is 2 bytes, € is 3 bytes; offset 2 splits é.
        assert_eq!(clamp_offset(&snapshot, 2), 1);
        assert_eq!(clamp_offset(&snapshot, 4), 3);
        assert_eq!(clamp_offset(&snapshot, 6), 6);
    }

    #[test]
    fn clamp_on_empty_document_is_zero() {
        let snapshot = snapshot_of("");
        assert_eq!(snapshot.len_bytes(), 0);
        assert_eq!(clamp_offset(&snapshot, 0), 0);
        assert_eq!(clamp_offset(&snapshot, 99), 0);
        assert_eq!(buffer_content_end(&snapshot), 0);
        assert_eq!(cursor_line(&snapshot, 0), 0);
    }

    #[test]
    fn cursor_past_end_clamps_to_end() {
        let snapshot = snapshot_of("hello");
        assert_eq!(clamp_offset(&snapshot, 5), 5);
        assert_eq!(clamp_offset(&snapshot, 4096), 5);
    }

    #[test]
    fn a_char_selection_reads_back_as_an_ordered_inclusive_range() {
        let snapshot = snapshot_of("abcdef");
        let mut presentation = presentation_at(2);
        // A backwards anchor is the point: `v` then `h` leaves the head left
        // of the anchor, and the resolved range must still read left-to-right.
        let (anchor, head) = (4usize, 2usize);
        hold(
            &mut presentation,
            vec![Selection {
                anchor: editor_point(&snapshot, anchor),
                head: editor_point(&snapshot, head),
                mode: SelectionMode::Char,
                growing: false,
            }],
        );
        // Helix selections are inclusive at both ends, so `v` at `d` then `h`
        // back to `c` covers `c`, `d`, and `e`: bytes 2..5.
        assert_eq!(selection_range(&presentation, &snapshot), Some(2..5));
    }

    #[test]
    fn a_line_selection_spans_whole_lines_including_their_terminators() {
        let snapshot = snapshot_of("one\ntwo\nthree\n");
        let mut presentation = presentation_at(4);
        // Byte 7 is the newline that *ends* line 1; line 2 starts at byte 8.
        // Anchoring on the terminator is the whole hazard here — resolve it
        // as line 1 and the "span two lines" assertion would pass on a
        // one-line selection.
        assert_eq!(editor_point(&snapshot, 7).line, 1);
        assert_eq!(editor_point(&snapshot, 8).line, 2);
        hold(
            &mut presentation,
            vec![Selection {
                anchor: editor_point(&snapshot, 4),
                head: editor_point(&snapshot, 8),
                mode: SelectionMode::Line,
                growing: true,
            }],
        );
        // Lines 1..=2, i.e. "two\nthree\n". "one\ntwo\nthree\n" is 14
        // bytes and line_start(3) is the end of the buffer, so the span runs
        // to 14 — deleting it must not leave the trailing blank line that a
        // content-only span would.
        assert_eq!(selection_range(&presentation, &snapshot), Some(4..14));
    }

    #[test]
    fn selecting_a_line_then_pressing_x_grows_it_by_exactly_one_line() {
        let snapshot = snapshot_of("one\ntwo\nthree\nfour\n");
        let mut presentation = presentation_at(1);
        apply_helix_verb(&snapshot, &mut presentation, HelixVerb::SelectLine);
        assert_eq!(primary(&presentation).unwrap().line_range(), (0, 0));

        apply_helix_verb(&snapshot, &mut presentation, HelixVerb::SelectLine);
        assert_eq!(primary(&presentation).unwrap().line_range(), (0, 1));

        // Any deliberate movement disarms growth, so the next `x` re-anchors
        // instead of swallowing everything walked past.
        apply_helix_verb(
            &snapshot,
            &mut presentation,
            HelixVerb::Moved(EditorCommand::MoveDown),
        );
        assert_eq!(primary(&presentation).unwrap().line_range(), (0, 1));
        // The cursor is on line 1 now, so the next `x` re-anchors there
        // rather than swallowing everything walked past.
        apply_helix_verb(&snapshot, &mut presentation, HelixVerb::SelectLine);
        assert_eq!(primary(&presentation).unwrap().line_range(), (1, 1));
    }

    #[test]
    fn movement_extends_a_char_selection_but_not_a_line_selection() {
        let snapshot = snapshot_of("one\ntwo\nthree\n");
        let mut presentation = presentation_at(1);

        apply_helix_verb(&snapshot, &mut presentation, HelixVerb::SelectChar);
        apply_helix_verb(
            &snapshot,
            &mut presentation,
            HelixVerb::Moved(EditorCommand::MoveRight),
        );
        let (first, last) = primary(&presentation).unwrap().bounds();
        assert_eq!((first.byte, last.byte), (1, 2));

        // Same movement, but the selection was anchored by `x`.
        let mut line_selection = presentation_at(1);
        apply_helix_verb(&snapshot, &mut line_selection, HelixVerb::SelectLine);
        apply_helix_verb(
            &snapshot,
            &mut line_selection,
            HelixVerb::Moved(EditorCommand::MoveDown),
        );
        assert_eq!(
            primary(&line_selection).unwrap().line_range(),
            (0, 0),
            "a bare movement must not add lines to a line selection"
        );
    }

    #[test]
    fn deleting_with_nothing_selected_is_a_claimed_no_op() {
        let snapshot = snapshot_of("one\ntwo\n");
        let mut presentation = presentation_at(1);
        assert_eq!(
            apply_helix_verb(&snapshot, &mut presentation, HelixVerb::DeleteSelection),
            HelixVerbOutcome::Handled
        );
        assert_eq!(presentation.cursor_byte, 1);
    }

    #[test]
    fn deleting_a_line_selection_returns_the_whole_line_range() {
        let snapshot = snapshot_of("one\ntwo\nthree\n");
        let mut presentation = presentation_at(4);
        apply_helix_verb(&snapshot, &mut presentation, HelixVerb::SelectLine);
        assert_eq!(
            apply_helix_verb(&snapshot, &mut presentation, HelixVerb::DeleteSelection),
            HelixVerbOutcome::DeleteRanges(one_range(4..8))
        );
        // The set is left for the caller to consume: that is what lets the undo
        // entry record the members the reader actually had.
        assert_eq!(presentation.selections.len(), 1);
    }

    // ── The selection set ───────────────────────────────────────────────

    /// A member spanning `start..=end` in character mode.
    fn spanning(snapshot: &DocumentSnapshot, start: usize, end: usize) -> Selection<EditorPoint> {
        Selection {
            anchor: editor_point(snapshot, start),
            head: editor_point(snapshot, end),
            mode: SelectionMode::Char,
            growing: false,
        }
    }

    #[test]
    fn selection_ranges_answers_for_every_member_in_document_order() {
        let snapshot = snapshot_of("aa bb cc dd ee");
        let mut presentation = presentation_at(0);
        // Handed over out of order on purpose: the set normalizes on write.
        hold(
            &mut presentation,
            vec![
                spanning(&snapshot, 9, 12),
                spanning(&snapshot, 0, 1),
                spanning(&snapshot, 3, 4),
            ],
        );
        assert_eq!(
            selection_ranges(&presentation, &snapshot),
            vec![0..2, 3..5, 9..13]
        );
    }

    #[test]
    fn overlapping_members_merge_instead_of_producing_an_invalid_transaction() {
        let snapshot = snapshot_of("abcdefghij");
        let mut presentation = presentation_at(0);
        hold(
            &mut presentation,
            vec![
                spanning(&snapshot, 0, 5),
                spanning(&snapshot, 3, 8),
                // Touching rather than overlapping: `a..b` + `b..c` deletes
                // the same bytes as `a..c`, and must collapse too.
                spanning(&snapshot, 8, 10),
            ],
        );
        let ranges = selection_ranges(&presentation, &snapshot);
        assert_eq!(ranges, vec![0..10]);
        // And the merged set is a legal transaction: the controller validator
        // rejects overlapping edits outright.
        let edit = delete_ranges(&snapshot, ranges)
            .edit()
            .expect("one transaction");
        assert_eq!(edit.transaction.edits.len(), 1);
    }

    #[test]
    fn a_delete_over_n_members_is_one_transaction_covering_all_of_them() {
        let snapshot = snapshot_of("aa bb cc dd");
        let ranges = vec![0..2, 3..5, 6..8, 9..11];
        let RangeSetEdit::Edit(edit, _) = delete_ranges(&snapshot, ranges) else {
            panic!("a non-empty set must produce a transaction");
        };
        assert_eq!(
            edit.transaction.edits.len(),
            4,
            "one transaction carrying every range — never one edit per range, \
             which is what would let a rejection land half of them"
        );
        assert_eq!(edit.cursor_after, 0, "the cursor lands on the first range");
        // The inverse is what makes one transaction undoable as one step, and
        // it is only exact because the edits ascend and never overlap.
        assert_eq!(edit.inverse.edits.len(), 4);
    }

    #[test]
    fn the_transaction_inverse_restores_the_text_it_removed() {
        let snapshot = snapshot_of("aa bb cc dd");
        let RangeSetEdit::Edit(edit, _) = delete_ranges(&snapshot, vec![0..2, 3..5, 6..8, 9..11])
        else {
            panic!("a non-empty set must produce a transaction");
        };
        // "aa bb cc dd" minus "aa","bb","cc","dd" leaves the three spaces.
        let after = apply("aa bb cc dd", &edit.transaction);
        assert_eq!(after, "   ");
        let restored = apply(&after, &edit.inverse);
        assert_eq!(restored, "aa bb cc dd", "undo must restore every member");
    }

    /// Apply `transaction` to `text` and read the result back.
    ///
    /// Goes through the real buffer rather than re-implementing the splice, so
    /// "the transaction is well formed" and "the transaction does the right
    /// thing" are the same assertion — the buffer's validator is what rejects
    /// an overlapping edit set.
    fn apply(text: &str, transaction: &TextTransaction) -> String {
        let mut buffer = DocumentBuffer::new(text);
        buffer
            .apply(transaction, DocumentLimits::default())
            .expect("the transaction must be valid for this text");
        let snapshot = buffer.snapshot();
        snapshot
            .read_bytes(0, snapshot.len_bytes())
            .expect("whole buffer")
    }

    #[test]
    fn more_selections_than_one_transaction_may_carry_is_refused_not_truncated() {
        // Distinct, disjoint ranges over a document wide enough to hold them.
        // Building them over a short buffer would test nothing: every range
        // would clamp to the same span and merge into one long before the cap
        // was consulted.
        // Every other byte, so the ranges are disjoint **and** non-touching:
        // touching ranges merge into one region by design, which would hide
        // them from the cap entirely. The document has to be wide enough for
        // the last range to survive clamping — a range that clamps to empty
        // drops out before the cap is consulted, so an under-sized document
        // would test nothing.
        let width = (MAX_DOCUMENT_EDITS + 2) * 2;
        let snapshot = snapshot_of(&"x".repeat(width));
        let over = (0..MAX_DOCUMENT_EDITS + 1)
            .map(|i| i * 2..i * 2 + 1)
            .collect::<Vec<_>>();
        assert_eq!(
            delete_ranges(&snapshot, over),
            RangeSetEdit::TooMany(MAX_DOCUMENT_EDITS + 1)
        );
        // Exactly at the limit still goes through: the cap bounds the
        // transaction, it is not an off-by-one trap.
        let at = (0..MAX_DOCUMENT_EDITS)
            .map(|i| i * 2..i * 2 + 1)
            .collect::<Vec<_>>();
        assert!(matches!(
            delete_ranges(&snapshot, at),
            RangeSetEdit::Edit(..)
        ));
    }

    #[test]
    fn an_empty_set_is_reported_as_empty_rather_than_as_a_refusal() {
        let snapshot = snapshot_of("abc");
        assert_eq!(delete_ranges(&snapshot, vec![]), RangeSetEdit::Empty);
        assert_eq!(
            delete_ranges(&snapshot, one_range(2..2)),
            RangeSetEdit::Empty,
            "an empty range has nothing to delete and is not a cap problem"
        );
    }

    #[test]
    fn replace_and_case_verbs_target_every_member_in_one_transaction() {
        let snapshot = snapshot_of("ab cd ef");
        let mut presentation = presentation_at(0);
        hold(
            &mut presentation,
            vec![
                spanning(&snapshot, 0, 0),
                spanning(&snapshot, 3, 3),
                spanning(&snapshot, 6, 6),
            ],
        );
        let edit = replace_char(&snapshot, &presentation, 'X').expect("replacement");
        assert_eq!(edit.transaction.edits.len(), 3);
        assert_eq!(apply("ab cd ef", &edit.transaction), "Xb Xd Xf");

        let edit = switch_case(&snapshot, &presentation).expect("case flip");
        assert_eq!(edit.transaction.edits.len(), 3);
        assert_eq!(apply("ab cd ef", &edit.transaction), "Ab Cd Ef");
    }

    #[test]
    fn a_case_verb_drops_a_member_that_is_already_flipped_rather_than_abandoning_the_set() {
        // `1` has no case, so it is a genuine no-op member while `A` is not.
        // Both members of "Ab Cd" would flip, which would test nothing.
        let snapshot = snapshot_of("A1 b");
        let mut presentation = presentation_at(0);
        hold(
            &mut presentation,
            vec![spanning(&snapshot, 0, 0), spanning(&snapshot, 2, 2)],
        );
        let edit = switch_case(&snapshot, &presentation).expect("one member changes");
        assert_eq!(
            edit.transaction.edits.len(),
            1,
            "refusing the whole set because one member was already flipped \
             would make `~` depend on the order the cursors were created in"
        );
        assert_eq!(apply("A1 b", &edit.transaction), "a1 b");
    }

    #[test]
    fn paste_lands_after_every_member_without_two_anchors_colliding() {
        let snapshot = snapshot_of("aa bb cc");
        let mut presentation = presentation_at(0);
        hold(
            &mut presentation,
            vec![
                spanning(&snapshot, 0, 0),
                spanning(&snapshot, 3, 3),
                spanning(&snapshot, 6, 6),
            ],
        );
        let edit = paste_after(&snapshot, &presentation, "-").expect("paste");
        assert_eq!(edit.transaction.edits.len(), 3);
        assert_eq!(apply("aa bb cc", &edit.transaction), "a-a b-b c-c");
    }

    #[test]
    fn selection_range_answers_for_the_primary_when_a_set_is_held() {
        let snapshot = snapshot_of("aa bb cc");
        let mut presentation = presentation_at(0);
        hold(
            &mut presentation,
            vec![spanning(&snapshot, 0, 1), spanning(&snapshot, 6, 7)],
        );
        // The pre-set accessor did not change meaning: it answers for the
        // distinguished member, which is what every existing caller meant.
        assert_eq!(selection_range(&presentation, &snapshot), Some(0..2));
        presentation.primary_index = 1;
        assert_eq!(selection_range(&presentation, &snapshot), Some(6..8));
    }

    #[test]
    fn line_and_byte_conversion_round_trips() {
        let snapshot = snapshot_of("one\ntwo\nthree");
        for offset in 0..=snapshot.len_bytes() {
            let position = snapshot.byte_to_position(offset).unwrap();
            assert_eq!(snapshot.position_to_byte(position).unwrap(), offset);
        }
        assert_eq!(cursor_line(&snapshot, 5), 1);
        assert_eq!(line_start(&snapshot, 1), Some(4));
        assert_eq!(line_content_end(&snapshot, 1), Some(7));
        assert_eq!(buffer_content_end(&snapshot), 13);
    }

    #[test]
    fn char_boundary_walk_crosses_multibyte_text() {
        let snapshot = snapshot_of("aé€b");
        assert_eq!(next_char_boundary(&snapshot, 0), Some(1));
        assert_eq!(next_char_boundary(&snapshot, 1), Some(3));
        assert_eq!(next_char_boundary(&snapshot, 3), Some(6));
        assert_eq!(next_char_boundary(&snapshot, 6), None);
        assert_eq!(prev_char_boundary(&snapshot, 6), 3);
        assert_eq!(prev_char_boundary(&snapshot, 3), 1);
        assert_eq!(prev_char_boundary(&snapshot, 1), 0);
    }

    // ── Motion ────────────────────────────────────────────────────────

    #[test]
    fn horizontal_motion_stops_at_line_content_not_terminator() {
        let snapshot = snapshot_of("ab\ncd");
        // Offset 3 is 'c', the first byte of line 1.
        let mut presentation = presentation_at(3);
        apply_motion(&mut presentation, &snapshot, EditorCommand::LineEnd, 10);
        assert_eq!(presentation.cursor_byte, 5);
        apply_motion(&mut presentation, &snapshot, EditorCommand::LineStart, 10);
        assert_eq!(presentation.cursor_byte, 3);
        apply_motion(&mut presentation, &snapshot, EditorCommand::BufferEnd, 10);
        assert_eq!(presentation.cursor_byte, 5);
        apply_motion(&mut presentation, &snapshot, EditorCommand::BufferStart, 10);
        assert_eq!(presentation.cursor_byte, 0);
    }

    #[test]
    fn vertical_motion_keeps_the_column_across_a_shorter_line_and_resets_after_a_jump() {
        let snapshot = snapshot_of("longest line here\nab\nanother long line");
        let mut presentation = presentation_at(10);
        apply_motion(&mut presentation, &snapshot, EditorCommand::MoveDown, 10);
        // Target line "ab" is shorter, so the cursor clamps to its content end.
        assert_eq!(presentation.cursor_byte, 20);
        apply_motion(&mut presentation, &snapshot, EditorCommand::MoveUp, 10);
        // Column 10 comes back — **not** the short line's clamped column 2.
        // This is the one place M005 changed behaviour the baseline had on
        // purpose: a vertical run remembers the column it started in, so a
        // reader scrolling a ragged file does not lose their place every time
        // the file narrows. Helix keeps a desired column for exactly this.
        assert_eq!(presentation.cursor_byte, 10);
        assert_eq!(presentation.desired_column, Some(10));

        // And the run is not sticky: a placement that is not a step along a
        // column ends it, so the next `k` re-seeds from where the cursor is.
        apply_motion(&mut presentation, &snapshot, EditorCommand::BufferEnd, 10);
        assert_eq!(presentation.desired_column, None);
        apply_motion(&mut presentation, &snapshot, EditorCommand::MoveUp, 10);
        assert_eq!(presentation.cursor_byte, 20, "clamped to `ab` again");
        assert_eq!(
            presentation.desired_column,
            Some(17),
            "re-seeded from the long line's own column, not from the old run"
        );
    }

    #[test]
    fn word_motion_finds_word_boundaries() {
        let snapshot = snapshot_of("alpha beta  gamma");
        assert_eq!(word_forward(&snapshot, 0), 6);
        assert_eq!(
            word_forward(&snapshot, 6),
            12,
            "skips both separator spaces"
        );
        assert_eq!(word_backward(&snapshot, 12), 6);
        assert_eq!(word_backward(&snapshot, 16), 12, "lands on the word start");
    }

    // ── Long-word motions (Helix `W` / `B` / `E`) ──────────────────────

    /// A one-byte read is `Err` inside **and at the start of** a multi-byte
    /// character, because `read_bytes` wants a character boundary at both
    /// ends. Every word scanner read that as "not a word byte" and walked
    /// straight past every non-ASCII character: on `日本語 text`, `w` from
    /// byte 0 landed on the **end of the document** instead of on `text`, and
    /// `e` did the same.
    ///
    /// Invisible in the suite because ASCII single-byte characters are the
    /// only kind that reads back, so the whole surface was untested on CJK.
    /// This is the defect class M003 fixed in `find_char`; the word scanners
    /// were never given the fix.
    #[test]
    fn word_motions_walk_over_multi_byte_characters_instead_of_past_them() {
        // 日=0..3 本=3..6 語=6..9 ' '=9 t=10 e=11 x=12 t=13
        let snapshot = snapshot_of("日本語 text");

        assert_eq!(
            word_forward(&snapshot, 0),
            10,
            "`w` from 日 lands on `text`, not past it"
        );
        assert_eq!(
            word_backward(&snapshot, 10),
            0,
            "`b` to the start of 日本語"
        );
        assert_eq!(
            word_end(&snapshot, 0),
            6,
            "`e` from 日 lands on 語, not at the end of the document"
        );
        assert_eq!(word_end(&snapshot, 10), 13, "`e` on `text` stops at `t`");

        // And the same for a two-byte character.
        let accented = snapshot_of("éx yó");
        assert_eq!(word_forward(&accented, 0), 4, "`w` from é lands on `yó`");
        assert_eq!(word_end(&accented, 0), 2, "`e` from é stops after `éx`");
        assert_eq!(word_end(&accented, 4), 5, "`e` on `yó` lands on ó");
    }

    /// Every scanner must land on a character boundary and must **terminate**.
    /// The `*_end` pair used to step with `next_char_boundary(..).unwrap_or(cursor)`,
    /// which cannot advance when the last character of a document is
    /// multi-byte — an unbounded spin, found by sampling the running test
    /// binary's stack rather than by reasoning about it.
    #[test]
    fn word_and_long_word_scanners_terminate_and_land_on_a_character_boundary() {
        // Each of these ends in a multi-byte character, which is the spin case.
        for text in ["éx yó zêw 中文 末", "日本語", "abc 末", " ", ""] {
            let snapshot = snapshot_of(text);
            for offset in 0..=snapshot.len_bytes() {
                if !snapshot.is_byte_boundary(offset) {
                    continue;
                }
                for landed in [
                    word_forward(&snapshot, offset),
                    word_backward(&snapshot, offset),
                    word_end(&snapshot, offset),
                    long_word_forward(&snapshot, offset),
                    long_word_backward(&snapshot, offset),
                    long_word_end(&snapshot, offset),
                ] {
                    assert!(
                        snapshot.is_byte_boundary(landed),
                        "{text:?}: landed on {landed}, not a boundary, from {offset}"
                    );
                    assert!(
                        landed <= snapshot.len_bytes(),
                        "{text:?}: landed at {landed}, past the document"
                    );
                }
            }
        }
    }

    /// The whole reason `W` exists: punctuation is part of a WORD for it and
    /// is not part of a word for `w`, so the two must land somewhere different
    /// on ordinary source.
    #[test]
    fn long_word_motions_differ_from_word_motions_on_punctuation() {
        // f=0 o=1 o=2 .=3 b=4 a=5 r=6 ' '=7 b=8 a=9 z=10
        let snapshot = snapshot_of("foo.bar baz");

        assert_eq!(long_word_forward(&snapshot, 0), 8, "`foo.bar` is one WORD");
        assert_eq!(
            word_forward(&snapshot, 0),
            4,
            "`w` stops inside the WORD, at `bar`"
        );

        assert_eq!(long_word_backward(&snapshot, 8), 0, "one WORD back");
        assert_eq!(
            word_backward(&snapshot, 8),
            4,
            "`b` stops inside the WORD, at `bar`"
        );

        assert_eq!(long_word_end(&snapshot, 0), 6, "the last char of `foo.bar`");
        assert_eq!(word_end(&snapshot, 0), 2, "`e` stops at the end of `foo`");
    }

    /// `is_word_byte` treats every byte `>= 0x80` as word content, which is
    /// what lets a scan classify a multi-byte character without decoding it.
    /// For a *WORD* that rule is wrong: it would make every non-ASCII byte and
    /// every ASCII punctuation mark part of the same word as its neighbours.
    #[test]
    fn long_word_motions_keep_non_ascii_together_but_join_it_across_punctuation() {
        // 日本語 is bytes 0..9, a space at 9, `text` at 10..14.
        let snapshot = snapshot_of("日本語 text");
        assert_eq!(
            long_word_forward(&snapshot, 0),
            10,
            "the CJK run is one WORD"
        );
        assert_eq!(
            word_forward(&snapshot, 0),
            10,
            "so `w` agrees here — but by a different rule"
        );

        // Punctuation is the difference. 日=0..3 本=3..6 `,`=6 語=7..10
        // ` `=10 `x`=11.
        let mixed = snapshot_of("日本,語 x");
        assert_eq!(
            long_word_forward(&mixed, 0),
            11,
            "`,` does not break a WORD"
        );
        assert_eq!(
            word_forward(&mixed, 0),
            7,
            "`w` breaks at the comma, landing on 語"
        );
    }

    /// Starting a long-word motion from the *interior* of a multi-byte
    /// character must not strand the scan on a continuation byte.
    #[test]
    fn long_word_motions_start_from_inside_a_multi_byte_character() {
        let snapshot = snapshot_of("éx yó");
        // Offset 1 is the interior of `é`. The scan clamps back onto the
        // boundary before classifying, so it still resolves to the next WORD.
        assert_eq!(long_word_forward(&snapshot, 1), 4, "past `éx `, to `yó`");
        assert_eq!(long_word_backward(&snapshot, 4), 0);
        assert!(snapshot.is_byte_boundary(long_word_forward(&snapshot, 1)));
    }

    // ── First non-whitespace (Helix `gs`) ──────────────────────────────

    #[test]
    fn first_non_whitespace_resolves_the_cases_gs_has_to_handle() {
        // Indented line.
        let snapshot = snapshot_of("    let x = 1;");
        assert_eq!(first_non_whitespace(&snapshot, 0), Some(4));

        // A blank line resolves to its own start, so `gs` round-trips with `0`
        // rather than appearing stuck. `a\n\nb`: a=0, \n=1, blank=2, \n=2, b=3.
        let blank = snapshot_of("a\n\nb");
        assert_eq!(first_non_whitespace(&blank, 0), Some(0), "flush line");
        assert_eq!(first_non_whitespace(&blank, 1), Some(2), "the blank line");
        assert_eq!(
            first_non_whitespace(&blank, 2),
            Some(3),
            "the line after it"
        );

        // Whitespace-only line resolves to where `$` already sits.
        let padded = snapshot_of("a\n   \nb");
        assert_eq!(first_non_whitespace(&padded, 1), Some(5));
        assert_eq!(
            line_content_end(&padded, 1),
            Some(5),
            "which is the line content end, i.e. `$`"
        );

        // No indentation at all.
        let flush = snapshot_of("top");
        assert_eq!(first_non_whitespace(&flush, 0), Some(0));

        // A multi-byte leading character is found whole, not split.
        let wide = snapshot_of("  日本語 x");
        assert_eq!(first_non_whitespace(&wide, 0), Some(2));
        assert!(wide.is_byte_boundary(first_non_whitespace(&wide, 0).unwrap()));
    }

    /// Bounded like `line_content_end`: a multi-megabyte indented line must
    /// not cost a scan of the whole line to answer. Without the bound this
    /// would return 4 MiB; with it, it stops at the rendered horizon — and
    /// that line is already drawn with `TRUNCATED_LINE_SUFFIX`, so the answer
    /// is disclosed rather than silently wrong.
    #[test]
    fn first_non_whitespace_does_not_read_a_huge_indented_line() {
        let bytes = 4 * 1024 * 1024;
        let snapshot = DocumentBuffer::new(format!("{}{}", " ".repeat(bytes - 1), "x")).snapshot();
        let landed = first_non_whitespace(&snapshot, 0).expect("line exists");
        assert_eq!(
            landed, MAX_RENDER_LINE_BYTES,
            "stops at the render horizon instead of scanning to the first glyph"
        );
        assert!(line_is_truncated(&snapshot, 0));
        // A short indentation is answered exactly, unbounded scan or not.
        let short = DocumentBuffer::new("      x").snapshot();
        assert_eq!(first_non_whitespace(&short, 0), Some(6));
    }

    /// `j` down onto a short line and back up returns to the original column.
    /// Without a remembered column the round trip lands on the short line's
    /// end and the reader loses their place.
    #[test]
    fn vertical_motion_keeps_the_column_across_a_short_line() {
        let snapshot = snapshot_of("aaaaaaaaaa\nbb\ncccccccccc");
        let mut presentation = presentation_at(9);

        apply_motion(&mut presentation, &snapshot, EditorCommand::MoveDown, 10);
        assert_eq!(
            presentation.cursor_byte, 13,
            "clamped to the short line's end"
        );
        assert_eq!(
            presentation.desired_column,
            Some(9),
            "the column survived the clamp — that is the feature"
        );

        apply_motion(&mut presentation, &snapshot, EditorCommand::MoveDown, 10);
        assert_eq!(
            presentation.cursor_byte, 23,
            "column restored on the way down"
        );

        apply_motion(&mut presentation, &snapshot, EditorCommand::MoveUp, 10);
        apply_motion(&mut presentation, &snapshot, EditorCommand::MoveUp, 10);
        assert_eq!(presentation.cursor_byte, 9, "and back where it started");
    }

    /// A remembered column is a column, so horizontal moves re-seed it; a
    /// motion that is not a step along a column clears it, exactly as
    /// vi-family editors do. A column carried across `G` would aim the next
    /// `j` at a column chosen for a line the cursor no longer sits on.
    #[test]
    fn desired_column_is_seeded_by_horizontal_motion_and_cleared_by_a_jump() {
        let snapshot = snapshot_of("aaaaaaaaaa\nbbbbbbbbbb\ncc");
        let mut presentation = presentation_at(9);

        apply_motion(&mut presentation, &snapshot, EditorCommand::MoveRight, 10);
        assert_eq!(presentation.desired_column, Some(10));

        // `j` keeps it.
        apply_motion(&mut presentation, &snapshot, EditorCommand::MoveDown, 10);
        assert_eq!(presentation.desired_column, Some(10));

        for command in [
            EditorCommand::BufferStart,
            EditorCommand::BufferEnd,
            EditorCommand::LineStart,
            EditorCommand::WordForward,
            EditorCommand::LongWordForward,
            EditorCommand::FirstNonWhitespace,
            EditorCommand::HalfPageDown,
        ] {
            let mut presentation = presentation_at(9);
            apply_motion(&mut presentation, &snapshot, EditorCommand::MoveRight, 10);
            assert_eq!(presentation.desired_column, Some(10), "seeded");
            apply_motion(&mut presentation, &snapshot, command, 10);
            assert_eq!(
                presentation.desired_column, None,
                "{command:?} must end the vertical run"
            );
        }
    }

    /// A column recorded against older text can never place the cursor outside
    /// a line or inside a multi-byte character, even when the text is
    /// re-encoded under it.
    #[test]
    fn a_stale_desired_column_cannot_place_the_cursor_outside_the_line() {
        let snapshot = snapshot_of("aaa\n日本語");
        let mut presentation = presentation_at(0);
        presentation.desired_column = Some(9999);
        apply_motion(&mut presentation, &snapshot, EditorCommand::MoveDown, 10);
        assert_eq!(
            presentation.cursor_byte, 13,
            "clamped to the second line's content end, not to 9999"
        );
        assert!(snapshot.is_byte_boundary(presentation.cursor_byte));
    }

    /// The Helix verb path resolves motions on a throwaway probe and writes the
    /// real presentation once. The desired column has to survive that hop, or
    /// every `j` would silently restart the run.
    #[test]
    fn the_helix_verb_path_carries_the_desired_column_back_from_its_probe() {
        let snapshot = snapshot_of("aaaaaaaaaa\nbb\ncccccccccc");
        let mut presentation = presentation_at(9);
        apply_helix_verb(
            &snapshot,
            &mut presentation,
            HelixVerb::Moved(EditorCommand::MoveDown),
        );
        assert_eq!(presentation.cursor_byte, 13);
        assert_eq!(
            presentation.desired_column,
            Some(9),
            "the column crossed the probe"
        );

        apply_helix_verb(
            &snapshot,
            &mut presentation,
            HelixVerb::Moved(EditorCommand::MoveUp),
        );
        apply_helix_verb(
            &snapshot,
            &mut presentation,
            HelixVerb::Moved(EditorCommand::MoveUp),
        );
        assert_eq!(presentation.cursor_byte, 9);
    }

    /// `v` and `x` resolve no motion at all, so they must leave the column
    /// exactly as they found it rather than clearing it as a side effect.
    #[test]
    fn a_selection_verb_leaves_the_desired_column_alone() {
        let snapshot = snapshot_of("aaaaaaaaaa\nbb\ncccccccccc");
        let mut presentation = presentation_at(9);
        apply_motion(&mut presentation, &snapshot, EditorCommand::MoveRight, 10);
        let before = presentation.desired_column;

        apply_helix_verb(&snapshot, &mut presentation, HelixVerb::SelectChar);
        apply_helix_verb(&snapshot, &mut presentation, HelixVerb::SelectLine);
        assert_eq!(presentation.desired_column, before);
    }

    // ── Viewport ──────────────────────────────────────────────────────

    #[test]
    fn gutter_width_is_stable_across_cursor_movement() {
        let snapshot = snapshot_of(&"line\n".repeat(9));
        let first = gutter_width(snapshot.len_lines());
        let mut presentation = presentation_at(0);
        let viewport = resolve_viewport(&mut presentation, &snapshot, 40, 10);
        assert_eq!(viewport.gutter, first);
        presentation.cursor_byte = snapshot.len_bytes();
        let moved = resolve_viewport(&mut presentation, &snapshot, 40, 10);
        assert_eq!(
            moved.gutter, first,
            "gutter must not jitter with the cursor"
        );
    }

    #[test]
    fn gutter_width_grows_only_with_line_count_digits() {
        assert_eq!(gutter_width(1), 2);
        assert_eq!(gutter_width(9), 2);
        assert_eq!(gutter_width(10), 3);
        assert_eq!(gutter_width(100), 4);
    }

    #[test]
    fn vertical_scroll_follows_the_cursor_and_clamps_at_the_last_page() {
        let snapshot = snapshot_of(&"line\n".repeat(30));
        let mut presentation = presentation_at(0);
        let viewport = resolve_viewport(&mut presentation, &snapshot, 40, 5);
        assert_eq!(viewport.first_line, 0);
        presentation.cursor_byte = snapshot.len_bytes();
        let viewport = resolve_viewport(&mut presentation, &snapshot, 40, 5);
        // The trailing newline yields a 31st (empty) line, so the last
        // page starts at 31 - 5.
        assert_eq!(viewport.first_line, 31 - 5);
        presentation.cursor_byte = 0;
        let viewport = resolve_viewport(&mut presentation, &snapshot, 40, 5);
        assert_eq!(viewport.first_line, 0, "scroll clamps at the top edge");
    }

    #[test]
    fn horizontal_scroll_tracks_the_cursor_and_clamps_at_the_right_edge() {
        let long = "x".repeat(400);
        let snapshot = snapshot_of(&long);
        let mut presentation = presentation_at(0);
        let viewport = resolve_viewport(&mut presentation, &snapshot, 40, 5);
        assert_eq!(viewport.column, 0);
        presentation.cursor_byte = 400;
        let viewport = resolve_viewport(&mut presentation, &snapshot, 40, 5);
        assert!(viewport.column > 0, "cursor far right must scroll right");
        let max = longest_visible_width(&snapshot, viewport.first_line, 5)
            - viewport.text_columns as usize;
        assert_eq!(viewport.column, max, "scroll clamps at the buffer edge");
    }

    #[test]
    fn narrow_area_still_yields_a_usable_text_column() {
        let snapshot = snapshot_of("hello world");
        let mut presentation = presentation_at(0);
        let viewport = resolve_viewport(&mut presentation, &snapshot, 1, 1);
        assert_eq!(viewport.text_columns, 1);
        assert_eq!(viewport.visible_lines, 1);
    }

    // ── Edits and inverse transactions ────────────────────────────────

    #[test]
    fn inverse_of_an_insertion_restores_the_original_text() {
        let snapshot = snapshot_of("hello");
        let transaction = TextTransaction::new(vec![TextEdit::new(5..5, " world")]);
        let inverse = invert(&snapshot, &transaction).unwrap();
        assert_eq!(inverse.edits, vec![TextEdit::new(5..11, "")]);

        let mut buffer = DocumentBuffer::new("hello");
        let applied = buffer
            .apply(&transaction, DocumentLimits::default())
            .unwrap();
        assert_eq!(applied.snapshot.to_string(), "hello world");
        let restored = buffer.apply(&inverse, DocumentLimits::default()).unwrap();
        assert_eq!(restored.snapshot.to_string(), "hello");
    }

    #[test]
    fn inverse_of_a_deletion_restores_the_removed_text() {
        let snapshot = snapshot_of("hello world");
        let transaction = TextTransaction::new(vec![TextEdit::new(5..11, "")]);
        let inverse = invert(&snapshot, &transaction).unwrap();
        assert_eq!(inverse.edits, vec![TextEdit::new(5..5, " world")]);
    }

    #[test]
    fn inverse_matches_the_controllers_own_inverse_for_multi_edit_transactions() {
        let snapshot = snapshot_of("0123456789");
        let transaction =
            TextTransaction::new(vec![TextEdit::new(0..2, "AA"), TextEdit::new(5..7, "BB")]);
        let mut buffer = DocumentBuffer::new("0123456789");
        let controller_inverse = buffer
            .apply(&transaction, DocumentLimits::default())
            .unwrap()
            .inverse;
        assert_eq!(
            invert(&snapshot, &transaction).unwrap(),
            controller_inverse,
            "frontend inverse must match DocumentBuffer::apply exactly"
        );
    }

    #[test]
    fn resolve_edit_produces_the_expected_delete_line_range() {
        let snapshot = snapshot_of("one\ntwo\nthree");
        let presentation = presentation_at(4);
        let edit = resolve_edit(&snapshot, &presentation, EditorCommand::DeleteLine).unwrap();
        assert_eq!(edit.transaction.edits, vec![TextEdit::new(4..8, "")]);
        let mut buffer = DocumentBuffer::new("one\ntwo\nthree");
        buffer
            .apply(&edit.transaction, DocumentLimits::default())
            .unwrap();
        assert_eq!(buffer.snapshot().to_string(), "one\nthree");
    }

    #[test]
    fn delete_line_on_the_last_line_removes_only_that_line() {
        let snapshot = snapshot_of("one\ntwo");
        let presentation = presentation_at(4);
        let edit = resolve_edit(&snapshot, &presentation, EditorCommand::DeleteLine).unwrap();
        assert_eq!(edit.transaction.edits, vec![TextEdit::new(4..7, "")]);
    }

    #[test]
    fn delete_char_at_the_end_of_the_buffer_is_a_noop() {
        let snapshot = snapshot_of("one\ntwo");
        let presentation = presentation_at(7);
        assert!(resolve_edit(&snapshot, &presentation, EditorCommand::DeleteChar).is_none());
    }

    #[test]
    fn delete_char_on_a_newline_joins_the_next_line() {
        let snapshot = snapshot_of("one\ntwo");
        let presentation = presentation_at(3);
        let edit = resolve_edit(&snapshot, &presentation, EditorCommand::DeleteChar).unwrap();
        assert_eq!(edit.transaction.edits, vec![TextEdit::new(3..4, "")]);
        let mut buffer = DocumentBuffer::new("one\ntwo");
        buffer
            .apply(&edit.transaction, DocumentLimits::default())
            .unwrap();
        assert_eq!(buffer.snapshot().to_string(), "onetwo");
    }

    #[test]
    fn open_line_below_splits_the_existing_terminator() {
        let snapshot = snapshot_of("one\ntwo");
        let presentation = presentation_at(0);
        let edit = resolve_edit(&snapshot, &presentation, EditorCommand::OpenLineBelow).unwrap();
        let mut buffer = DocumentBuffer::new("one\ntwo");
        buffer
            .apply(&edit.transaction, DocumentLimits::default())
            .unwrap();
        assert_eq!(buffer.snapshot().to_string(), "one\n\ntwo");
        assert!(edit.stay_insert);
    }

    #[test]
    fn open_line_above_the_first_line_preserves_a_leading_blank() {
        let snapshot = snapshot_of("one\ntwo");
        let presentation = presentation_at(0);
        let edit = resolve_edit(&snapshot, &presentation, EditorCommand::OpenLineAbove).unwrap();
        let mut buffer = DocumentBuffer::new("one\ntwo");
        buffer
            .apply(&edit.transaction, DocumentLimits::default())
            .unwrap();
        assert_eq!(buffer.snapshot().to_string(), "\none\ntwo");
    }

    // ── Undo history ──────────────────────────────────────────────────

    #[test]
    fn undo_history_pushes_caps_and_records_bytes() {
        let snapshot = snapshot_of("hello world");
        let mut presentation = presentation_at(0);
        let edit = insert_text(&snapshot, 0, "abc").unwrap();
        record_undo(&mut presentation, &edit);
        assert_eq!(presentation.history.len(), 1);
        let bytes = presentation.history_bytes;
        assert!(bytes > 0);
        assert_eq!(
            presentation.history_cursor, 1,
            "a new entry is applied, so the cursor is at the tip"
        );

        // Moving the cursor back does not remove the entry. The history is
        // one list and the cursor names a position in it, so an entry that is
        // no longer applied is still there to be applied again.
        assert!(move_history_cursor_back(&mut presentation));
        assert_eq!(presentation.history.len(), 1);
        assert_eq!(presentation.history_cursor, 0);
        assert_eq!(
            presentation.history_bytes, bytes,
            "the bytes are retained too"
        );
        assert!(
            redo_entry(&presentation).is_some(),
            "the undone entry is redoable"
        );
    }

    #[test]
    fn undo_history_drops_the_oldest_entry_past_the_depth_cap() {
        let mut presentation = presentation_at(0);
        for _ in 0..(MAX_EDITOR_UNDO_DEPTH + 10) {
            let edit = EditorEdit {
                transaction: TextTransaction::new(vec![TextEdit::new(0..0, "x")]),
                inverse: TextTransaction::new(vec![TextEdit::new(0..1, "")]),
                bytes: 1,
                cursor_after: 0,
                stay_insert: false,
            };
            let _ = &edit;
            record_undo(&mut presentation, &edit);
        }
        assert_eq!(presentation.history.len(), MAX_EDITOR_UNDO_DEPTH);
    }

    #[test]
    fn undo_history_drops_the_oldest_entry_past_the_byte_cap() {
        let mut presentation = presentation_at(0);
        let big = MAX_EDITOR_UNDO_BYTES / 4;
        for _ in 0..8 {
            let edit = EditorEdit {
                transaction: TextTransaction::new(vec![TextEdit::new(0..0, "x")]),
                inverse: TextTransaction::new(vec![TextEdit::new(0..1, "")]),
                bytes: big,
                cursor_after: 0,
                stay_insert: false,
            };
            record_undo(&mut presentation, &edit);
        }
        assert!(presentation.history_bytes <= MAX_EDITOR_UNDO_BYTES);
        assert!(presentation.history.len() < 8);
    }

    /// The one behaviour the cursor model must not get wrong.
    ///
    /// A refused apply leaves the cursor exactly where it was. The old
    /// two-stack model expressed the same guarantee as "the entry goes back
    /// on the stack"; with a cursor there is no transfer to undo, so the
    /// guarantee is that the cursor does not move. The apply side is
    /// `traverse_history`, and its refusal test is
    /// `a_refused_undo_leaves_the_history_cursor_where_it_was`.
    #[test]
    fn the_entry_stays_in_the_history_when_the_cursor_moves_back() {
        let snapshot = snapshot_of("hello");
        let mut presentation = presentation_at(0);
        let edit = insert_text(&snapshot, 0, "abc").unwrap();
        record_undo(&mut presentation, &edit);
        let bytes = presentation.history_bytes;
        assert!(move_history_cursor_back(&mut presentation));
        assert_eq!(presentation.history.len(), 1);
        assert_eq!(presentation.history_bytes, bytes);
        assert_eq!(
            presentation.history_cursor, 0,
            "and the cursor did not move"
        );
    }

    #[test]
    fn clear_history_drops_undo_redo_and_the_pending_prefix() {
        let snapshot = snapshot_of("hello");
        let mut presentation = presentation_at(0);
        record_undo(
            &mut presentation,
            &insert_text(&snapshot, 0, "abc").unwrap(),
        );
        presentation.pending_prefix = EditorPrefix::Goto;
        // A reconciliation replaces the text, so a recorded find aimed at the
        // old bytes — and the disclosure about whether the target is there —
        // are both statements about text that no longer exists.
        presentation.last_find = Some(FindMotion {
            kind: FindKind::Next,
            target: 'h',
        });
        presentation.find_note = FindNote::Absent('h');
        clear_history(&mut presentation);
        assert!(presentation.history.is_empty());
        assert_eq!(presentation.history_bytes, 0);
        assert_eq!(
            presentation.history_cursor, 0,
            "a cleared history has no position in it"
        );
        assert_eq!(presentation.pending_prefix, EditorPrefix::Idle);
        assert_eq!(
            presentation.last_find, None,
            "`Alt-.` must not repeat a motion resolved against the replaced text"
        );
        assert_eq!(presentation.find_note, FindNote::None);
    }

    // ── Character-finding motions ─────────────────────────────────────────

    /// The defect this milestone exists to not ship. Helix states the
    /// difference from vi in one sentence — *"`f`, `F`, `t` and `T` are not
    /// confined to the current line"* — because the current line is what an
    /// implementation reaches for by habit.
    ///
    /// Every target here sits on a *different* line from the cursor, so a
    /// scan bounded by `line_content_end` would report `Absent` and leave the
    /// cursor where it was, which is exactly the same observable result as
    /// the motion working. Asserting only "the cursor moved" would pass on
    /// both implementations.
    #[test]
    fn a_find_motion_spans_line_boundaries() {
        // `alpha\nbeta\ngamma\ndelta\n` — `g` is at byte 11, in the third
        // line; `b` is at byte 6, in the second. Both targets are the *only*
        // occurrence of their character anywhere in the document, so a
        // line-confined scan has nothing to find and reports "absent".
        let text = "alpha\nbeta\ngamma\ndelta\n";
        let snapshot = snapshot_of(text);

        let mut presentation = presentation_at(0);
        apply_motion(
            &mut presentation,
            &snapshot,
            EditorCommand::FindNext('g'),
            10,
        );
        assert_eq!(
            presentation.cursor_byte, 11,
            "`f g` must cross the two line breaks between `alpha` and `gamma`"
        );

        let end = text.len();
        let mut presentation = presentation_at(end);
        apply_motion(
            &mut presentation,
            &snapshot,
            EditorCommand::FindPrevious('b'),
            10,
        );
        assert_eq!(
            presentation.cursor_byte, 6,
            "`F b` must cross three line breaks back to the `b` in `beta`"
        );
    }

    /// `t`/`T` land **on** the target character, as does `f`/`F`.
    ///
    /// The plan rejects the other reading five times, and it is the reading an
    /// implementer arrives at from vi and kakoune, where a till-motion stops
    /// *before* the character. That mistake is invisible in a suite that only
    /// asserts "the cursor moved somewhere near the target", which is why the
    /// expected offset here is the target's own byte, spelled out.
    #[test]
    fn till_motions_land_on_the_target_character() {
        let text = "alpha\nbeta\ngamma\n";
        let snapshot = snapshot_of(text);

        // `alpha`: a(0) l(1) p(2) h(3) a(4) \n(5)
        let cases = [
            (EditorCommand::FindNext('l'), 1),
            (EditorCommand::FindTillNext('l'), 1),
            (EditorCommand::FindNext('a'), 4),
            (EditorCommand::FindTillNext('a'), 4),
        ];
        for (command, expected) in cases {
            let mut presentation = presentation_at(0);
            apply_motion(&mut presentation, &snapshot, command, 10);
            assert_eq!(
                presentation.cursor_byte, expected,
                "{command:?} must land on the target, not before it"
            );
        }

        // `gamma` starts at byte 11: g(11) a(12) m(13) m(14) a(15).
        let cases = [
            (EditorCommand::FindPrevious('g'), 11),
            (EditorCommand::FindTillPrevious('g'), 11),
            (EditorCommand::FindPrevious('m'), 14),
            (EditorCommand::FindTillPrevious('m'), 14),
        ];
        for (command, expected) in cases {
            let mut presentation = presentation_at(16);
            apply_motion(&mut presentation, &snapshot, command, 10);
            assert_eq!(
                presentation.cursor_byte, expected,
                "{command:?} must land on the target, not past it"
            );
        }
    }

    /// The scan starts strictly past the cursor in the direction of travel.
    ///
    /// Without this, `f x` with the cursor already on `x` is a no-op that
    /// looks exactly like a failed find, and `F x` cannot find the `x` the
    /// cursor is sitting on — which is the one `x` the user can see.
    #[test]
    fn a_find_motion_scans_strictly_past_the_cursor() {
        let snapshot = snapshot_of("xax");
        let mut presentation = presentation_at(0);
        apply_motion(
            &mut presentation,
            &snapshot,
            EditorCommand::FindNext('x'),
            10,
        );
        assert_eq!(
            presentation.cursor_byte, 2,
            "`f x` on an `x` must find the next one, not stand still"
        );

        let mut presentation = presentation_at(2);
        apply_motion(
            &mut presentation,
            &snapshot,
            EditorCommand::FindPrevious('x'),
            10,
        );
        assert_eq!(
            presentation.cursor_byte, 0,
            "`F x` on an `x` must find the previous one"
        );
    }

    /// A multi-byte target matches whole characters, never a byte of a
    /// neighbour — and the scan advances across multi-byte characters.
    ///
    /// The load-bearing case is the *interior* byte. The second byte of `é` is
    /// `0xA9`, and `read_bytes` refuses any range that does not end on a
    /// character boundary — so a scan that reads one byte at a time cannot
    /// read byte 1 of `"aé…"` at all, and walks straight past the `é`. That
    /// implementation passed on ASCII and failed silently on every document
    /// containing a non-ASCII character, which is why the two-byte and
    /// four-byte targets are both spelled out here.
    #[test]
    fn a_multi_byte_target_matches_whole_characters_only() {
        // `a`(1) `é`(2) `€`(3) `a`(1) — 7 bytes, 4 characters.
        let snapshot = snapshot_of("aé€a");
        let mut presentation = presentation_at(0);
        apply_motion(
            &mut presentation,
            &snapshot,
            EditorCommand::FindNext('é'),
            10,
        );
        assert_eq!(presentation.cursor_byte, 1);

        let mut presentation = presentation_at(0);
        apply_motion(
            &mut presentation,
            &snapshot,
            EditorCommand::FindNext('€'),
            10,
        );
        assert_eq!(presentation.cursor_byte, 3);

        let mut presentation = presentation_at(6);
        apply_motion(
            &mut presentation,
            &snapshot,
            EditorCommand::FindPrevious('é'),
            10,
        );
        assert_eq!(
            presentation.cursor_byte, 1,
            "backward multi-byte targets resolve to their own byte offset"
        );

        // A four-byte target: the scan must step over one whole character to
        // reach the byte after it, and must not mistake the emoji's own
        // continuation bytes for a match.
        let snapshot = snapshot_of("z😀z");
        let mut presentation = presentation_at(0);
        apply_motion(
            &mut presentation,
            &snapshot,
            EditorCommand::FindNext('😀'),
            10,
        );
        assert_eq!(presentation.cursor_byte, 1);
        apply_motion(
            &mut presentation,
            &snapshot,
            EditorCommand::FindNext('z'),
            10,
        );
        assert_eq!(
            presentation.cursor_byte, 5,
            "the scan must advance by whole characters across a 4-byte one"
        );
    }

    /// A target past the bound is disclosed, not reported as absent.
    ///
    /// Both mean "the cursor did not move", and they call for opposite
    /// answers from the reader — type something else, or reach for `/`. A
    /// scan that stopped at the bound and said "not found" would be a search
    /// that lies by omission, which is the `search_partial` defect one layer
    /// down.
    #[test]
    fn a_find_beyond_the_scan_bound_is_disclosed_rather_than_called_absent() {
        // A target well past the window, and none within it at all.
        let filler = "a".repeat(MAX_EDITOR_FIND_BYTES + 1024);
        let text = format!("{filler}z");
        let snapshot = snapshot_of(&text);

        assert_eq!(
            find_char(&snapshot, 0, 'z', true),
            FindOutcome::Bounded,
            "a target past the window must be disclosed, not called absent"
        );
        assert_eq!(
            find_char(&snapshot, 0, 'q', true),
            FindOutcome::Bounded,
            "an absent target is also only known to be absent within the window"
        );

        // Scanning backwards from the end of a document *larger* than the
        // window still only covers the window, so that direction is bounded
        // too — it cannot claim "absent" any more than the forward scan can.
        assert_eq!(
            find_char(&snapshot, text.len(), 'q', false),
            FindOutcome::Bounded,
            "a backward scan past the window is bounded as well"
        );
        assert_eq!(
            find_char(&snapshot, text.len(), 'z', false),
            FindOutcome::Found(MAX_EDITOR_FIND_BYTES + 1024),
            "the target just past the forward bound is reachable backwards"
        );

        // A document inside the window is different: the whole of it was
        // scanned, so "absent" is a fact about the document rather than
        // about the scan.
        let small = snapshot_of("abc");
        assert_eq!(
            find_char(&small, 3, 'q', false),
            FindOutcome::Absent,
            "a backward scan inside the window may say absent for real"
        );
        assert_eq!(
            find_char(&small, 0, 'q', true),
            FindOutcome::Absent,
            "and so may a forward one"
        );

        // And the disclosure says the bound, so the two are distinguishable
        // in the status row and not only in a test.
        assert_eq!(
            FindNote::from_outcome(FindOutcome::Absent, 'z'),
            FindNote::Absent('z')
        );
        assert_eq!(
            FindNote::from_outcome(FindOutcome::Bounded, 'z'),
            FindNote::Bounded('z')
        );
        assert_eq!(FindNote::Absent('z').label(), "no 'z' in that direction");
        assert!(
            FindNote::Bounded('z').label().contains("2 MiB"),
            "the bounded note must state the bound: {}",
            FindNote::Bounded('z').label()
        );
        assert_ne!(
            FindNote::Absent('z').label(),
            FindNote::Bounded('z').label(),
            "the two not-found cases must be distinguishable on screen"
        );
        assert_eq!(
            FindNote::from_outcome(FindOutcome::Found(4), 'z'),
            FindNote::None
        );
        assert_eq!(FindNote::None.label(), "");
    }

    /// A find motion records itself for `Alt-.` and discloses how it went,
    /// including when it went nowhere.
    ///
    /// Recording only on success would be the intuitive choice and the wrong
    /// one: Helix sets the repeat slot by *running* the command, so `f z` with
    /// no `z` ahead still leaves `Alt-.` armed to try `f z` again from
    /// wherever the cursor is.
    #[test]
    fn a_find_motion_records_itself_for_repeat_and_discloses_its_outcome() {
        let snapshot = snapshot_of("abc\nxyz\n");
        let mut presentation = presentation_at(0);

        apply_motion(
            &mut presentation,
            &snapshot,
            EditorCommand::FindNext('x'),
            10,
        );
        assert_eq!(
            presentation.last_find,
            Some(FindMotion {
                kind: FindKind::Next,
                target: 'x'
            })
        );
        assert_eq!(
            presentation.find_note,
            FindNote::None,
            "a find that landed has nothing to disclose"
        );

        // Now one that finds nothing: recorded, and disclosed.
        apply_motion(
            &mut presentation,
            &snapshot,
            EditorCommand::FindNext('q'),
            10,
        );
        assert_eq!(
            presentation.last_find,
            Some(FindMotion {
                kind: FindKind::Next,
                target: 'q'
            }),
            "a failed find is still the last find"
        );
        assert_eq!(presentation.find_note, FindNote::Absent('q'));

        // Only a find motion is ever recorded, and only a find motion maps
        // back to the command that produced it.
        for command in [
            EditorCommand::MoveRight,
            EditorCommand::WordForward,
            EditorCommand::BufferStart,
        ] {
            assert_eq!(command.find_motion(), None, "{command:?}");
            assert!(!command.is_find_motion(), "{command:?}");
        }
        for (kind, target, expected) in [
            (FindKind::Next, 'a', EditorCommand::FindNext('a')),
            (FindKind::Previous, 'b', EditorCommand::FindPrevious('b')),
            (FindKind::TillNext, 'c', EditorCommand::FindTillNext('c')),
            (
                FindKind::TillPrevious,
                'd',
                EditorCommand::FindTillPrevious('d'),
            ),
        ] {
            let motion = FindMotion { kind, target };
            assert_eq!(motion.command(), expected);
            assert_eq!(expected.find_motion(), Some(motion), "{expected:?}");
        }
    }

    /// Every prefix declares its leaves, its unmatched-key policy, its
    /// indicator text, and its find kind.
    ///
    /// `gg` and `space y` are the reason the unmatched-key column is a table
    /// and not a single boolean: the baseline disagreed about the unmatched
    /// key, and merging them while picking one answer would have changed a
    /// gesture.
    ///
    /// The leaf column is a **set** where it used to be a single `Option<char>`:
    /// M005 gave goto mode seven leaves, and a one-glyph accessor would have
    /// meant a special case per prefix. M013 grew four of those seven rows and
    /// two bracket rows into LSP gestures, which is the point of this table: a
    /// new leaf is an edit to a row here, not a new branch in the dispatcher.
    /// Every other column is unchanged, and
    /// the facts M002 asserted about `gg` and `space y` — `Goto` accepts `g`,
    /// `Space` accepts `y`, and the four find prefixes accept no leaf at all —
    /// are still asserted here, against the new shape. The four find prefixes
    /// accept nothing on purpose: their next key is a target *character*, not a
    /// leaf, so there is no glyph that could complete them.
    #[test]
    fn each_prefix_declares_its_leaf_and_its_unmatched_key_policy() {
        // Named because the tuple is otherwise unreadable inline, and
        // because the *set* of leaves is the thing being asserted.
        type Case = (
            EditorPrefix,
            Vec<char>,
            bool,
            &'static str,
            Option<FindKind>,
        );
        let cases: [Case; 10] = [
            (EditorPrefix::Idle, vec![], false, "", None),
            (
                EditorPrefix::Goto,
                vec!['g', 's', 'e', 'h', 'l', 'j', 'k', 'd', 'i', 'r', 'y'],
                true,
                "g",
                None,
            ),
            (
                EditorPrefix::Space,
                vec!['y', 'k', 's', 'S', 'd', 'h', 'f', 'F'],
                false,
                "space",
                None,
            ),
            // The two unimpaired rows stopped being empty in M006, which is the change
            // this table exists to notice. Claiming a key is supposed to require
            // editing the row here.
            (
                EditorPrefix::OpenBracket,
                vec![' ', 'p', 'd'],
                false,
                "[",
                None,
            ),
            (
                EditorPrefix::CloseBracket,
                vec![' ', 'p', 'd'],
                false,
                "]",
                None,
            ),
            (
                EditorPrefix::FindNext,
                vec![],
                false,
                "f",
                Some(FindKind::Next),
            ),
            (
                EditorPrefix::FindPrevious,
                vec![],
                false,
                "F",
                Some(FindKind::Previous),
            ),
            (
                EditorPrefix::FindTillNext,
                vec![],
                false,
                "t",
                Some(FindKind::TillNext),
            ),
            (
                EditorPrefix::FindTillPrevious,
                vec![],
                false,
                "T",
                Some(FindKind::TillPrevious),
            ),
            // The `m` grammar's first level. It **retries** an unmatched key
            // rather than swallowing it: `m z` is not a gesture, and a gesture
            // that ate the key would leave the reader with a keyboard that
            // silently stopped answering.
            (
                EditorPrefix::Match,
                vec!['m', 's', 'r', 'd', 'a', 'i'],
                false,
                "m",
                None,
            ),
        ];
        for (prefix, leaves, swallows, display, find_kind) in cases {
            assert_eq!(
                prefix
                    .leaves()
                    .iter()
                    .map(|leaf| leaf.glyph)
                    .collect::<Vec<_>>(),
                leaves,
                "{prefix:?} leaves"
            );
            assert_eq!(
                prefix.swallows_unmatched(),
                swallows,
                "{prefix:?} unmatched-key policy"
            );
            assert_eq!(prefix.display(), display, "{prefix:?} indicator");
            assert_eq!(prefix.find_kind(), find_kind, "{prefix:?} find kind");
            assert_eq!(
                prefix.is_find(),
                find_kind.is_some(),
                "{prefix:?} takes a target character"
            );
            assert_eq!(
                prefix.is_pending(),
                prefix != EditorPrefix::Idle,
                "{prefix:?}"
            );
        }
    }

    /// The one table mapping the four find keys onto the kinds they arm.
    ///
    /// Asserted against the dispatch layer's arms rather than trusted to them:
    /// a mistyped table here would arm `FindPrevious` for `T`, and the result
    /// would still be a pending prefix that resolves — just the wrong motion.
    #[test]
    fn the_four_find_keys_arm_the_four_find_kinds() {
        let cases = [
            ('f', EditorPrefix::FindNext, FindKind::Next, true),
            ('F', EditorPrefix::FindPrevious, FindKind::Previous, false),
            ('t', EditorPrefix::FindTillNext, FindKind::TillNext, true),
            (
                'T',
                EditorPrefix::FindTillPrevious,
                FindKind::TillPrevious,
                false,
            ),
        ];
        for (glyph, prefix, kind, forward) in cases {
            let armed = EditorPrefix::armed_by(glyph)
                .unwrap_or_else(|| panic!("{glyph:?} must arm a prefix"));
            assert_eq!(armed, prefix, "{glyph:?} arms");
            assert_eq!(armed.find_kind(), Some(kind), "{glyph:?} kind");
            assert_eq!(kind.forward(), forward, "{glyph:?} direction");
        }
        // Nothing else arms a find. Without this, the next milestone's leaves
        // could quietly join the table.
        for glyph in ['g', ' ', '[', ']', 'y', 'd', 'w', '\n', '.'] {
            assert_eq!(
                EditorPrefix::armed_by(glyph),
                None,
                "{glyph:?} must not arm a find prefix"
            );
        }
    }

    /// The indicator's width is bounded by construction: the pending prefix is a
    /// typed value rather than accumulated text, so the bound is asserted
    /// against every label instead of against a length a caller might pass.
    #[test]
    fn every_pending_prefix_display_fits_the_indicator_bound() {
        for prefix in [
            EditorPrefix::Idle,
            EditorPrefix::Goto,
            EditorPrefix::Space,
            EditorPrefix::OpenBracket,
            EditorPrefix::CloseBracket,
            EditorPrefix::FindNext,
            EditorPrefix::FindPrevious,
            EditorPrefix::FindTillNext,
            EditorPrefix::FindTillPrevious,
            // `r` was missing from this list, which made the test's own claim
            // — "the bound is asserted against every label" — false. It fits.
            EditorPrefix::ReplaceChar,
            // The `m` grammar's two levels. `"m r ("` is the longest label in
            // the set, and it is exactly the bound.
            EditorPrefix::Match,
            EditorPrefix::MatchArg(MatchArg::SurroundAdd),
            EditorPrefix::MatchArg(MatchArg::SurroundDelete),
            EditorPrefix::MatchArg(MatchArg::SurroundReplaceFrom),
            EditorPrefix::MatchArg(MatchArg::SurroundReplaceTo('(')),
            EditorPrefix::MatchArg(MatchArg::AroundObject),
            EditorPrefix::MatchArg(MatchArg::InnerObject),
        ] {
            assert!(
                prefix.display().len() <= MAX_EDITOR_PENDING_PREFIX,
                "{prefix:?} renders {:?}, over the {MAX_EDITOR_PENDING_PREFIX} byte bound",
                prefix.display()
            );
        }
        assert_eq!(
            EditorPrefix::Space.display().len(),
            MAX_EDITOR_PENDING_PREFIX,
            "the bound is the longest label, not a guess"
        );
        assert_eq!(
            EditorPrefix::MatchArg(MatchArg::SurroundReplaceTo('('))
                .display()
                .len(),
            MAX_EDITOR_PENDING_PREFIX,
            "the `m r (` half of the replace is exactly the bound too"
        );
        assert_eq!(
            MAX_EDITOR_FIND_BYTES, MAX_EDITOR_SEARCH_BYTES,
            "a find motion inherits the search window rather than inventing a second bound"
        );
    }

    /// The `m` grammar's second level: every argument is a **capture**, and the
    /// two halves of `m r` must not render alike.
    ///
    /// The rows carry a payload, so they cannot go in the table above — which
    /// is itself the point: the second level is a different *state*, not
    /// another row of the first.
    #[test]
    fn every_match_argument_is_a_capture_and_the_two_halves_of_m_r_differ() {
        let args = [
            (MatchArg::SurroundAdd, "m s", false),
            (MatchArg::SurroundDelete, "m d", false),
            (MatchArg::SurroundReplaceFrom, "m r", false),
            (MatchArg::SurroundReplaceTo('('), "m r (", false),
            (MatchArg::AroundObject, "m a", true),
            (MatchArg::InnerObject, "m i", true),
        ];
        for (arg, display, restricted) in args {
            let prefix = EditorPrefix::MatchArg(arg);
            assert!(prefix.is_capture(), "{arg:?} takes a character as data");
            assert!(
                prefix.leaves().is_empty(),
                "{arg:?} has no leaves: its next key is data, not a command"
            );
            assert_eq!(prefix.display(), display, "{arg:?} indicator");
            assert_eq!(arg.is_restricted(), restricted, "{arg:?} vocabulary");
            assert_eq!(prefix.match_arg(), Some(arg), "{arg:?} classifier");
        }
        assert_ne!(
            EditorPrefix::MatchArg(MatchArg::SurroundReplaceFrom).display(),
            EditorPrefix::MatchArg(MatchArg::SurroundReplaceTo('(')).display(),
            "the two halves of `m r` must not render alike: a reader who cannot \
             tell them apart cannot predict what the next key does"
        );
    }

    /// The grammar is one prefix deep: arming while armed replaces, because a
    /// stacked grammar is how a gesture ends up needing a third key.
    #[test]
    fn arming_replaces_rather_than_stacking_and_disarm_returns_to_idle() {
        let mut presentation = presentation_at(0);
        assert_eq!(presentation.pending_prefix, EditorPrefix::Idle);
        assert!(!presentation.pending_prefix.is_pending());

        presentation.arm_prefix(EditorPrefix::Goto);
        assert_eq!(presentation.pending_prefix, EditorPrefix::Goto);
        presentation.arm_prefix(EditorPrefix::Space);
        assert_eq!(
            presentation.pending_prefix,
            EditorPrefix::Space,
            "the second prefix replaced the first rather than stacking on it"
        );

        presentation.disarm_prefix();
        assert_eq!(presentation.pending_prefix, EditorPrefix::Idle);
        assert_eq!(presentation.pending_prefix.display(), "");
    }

    #[test]
    fn reset_for_open_clears_history_and_viewport() {
        let mut presentation = presentation_at(42);
        presentation.viewport_line = 9;
        presentation.viewport_column = 7;
        presentation.mode = EditorMode::Insert;
        presentation.reset_for_open();
        assert_eq!(presentation.cursor_byte, 0);
        assert_eq!(presentation.viewport_line, 0);
        assert_eq!(presentation.viewport_column, 0);
        assert_eq!(presentation.mode, EditorMode::Normal);
    }

    // ── Display helpers ───────────────────────────────────────────────

    #[test]
    fn display_column_counts_terminal_columns_not_bytes() {
        let snapshot = snapshot_of("aé€b");
        assert_eq!(display_column(&snapshot, 0), 0);
        assert_eq!(display_column(&snapshot, 1), 1);
        assert_eq!(display_column(&snapshot, 3), 2);
        assert_eq!(display_column(&snapshot, 6), 3);
    }

    #[test]
    fn offset_at_column_never_passes_the_line_content_end() {
        let snapshot = snapshot_of("ab\ncdef");
        assert_eq!(offset_at_column(&snapshot, 0, 0), Some(0));
        assert_eq!(offset_at_column(&snapshot, 0, 2), Some(2));
        assert_eq!(offset_at_column(&snapshot, 0, 99), Some(2));
        assert_eq!(offset_at_column(&snapshot, 1, 2), Some(5));
        assert_eq!(offset_at_column(&snapshot, 1, 4), Some(7));
    }

    // ── C002 bounded render budgets ─────────────────────────────────────

    fn huge_single_line(megabytes: usize) -> DocumentSnapshot {
        DocumentBuffer::new("x".repeat(megabytes * 1024 * 1024)).snapshot()
    }

    #[test]
    fn line_content_end_does_not_read_a_huge_line() {
        let bytes = 4 * 1024 * 1024;
        let snapshot = huge_single_line(4);
        let end = line_content_end(&snapshot, 0).expect("line exists");
        assert_eq!(end, bytes, "a one-line document has no terminator");
        assert_eq!(line_byte_len(&snapshot, 0), Some(bytes));
        assert!(line_is_truncated(&snapshot, 0));
        // The cost proof lives in `frame_cost_*` below; this asserts the
        // answer is exact without materializing the line.
    }

    #[test]
    fn frame_cost_stays_within_budget_for_a_4mib_single_line() {
        let snapshot = huge_single_line(4);
        let cost = measure_frame_cost(&snapshot, 0, 24);
        assert!(
            cost.total_bytes <= MAX_FRAME_SOURCE_BYTES,
            "frame must cost at most 256 KiB, got {}",
            cost.total_bytes
        );
        assert!(
            cost.max_line_bytes <= MAX_RENDER_LINE_BYTES,
            "one line must cost at most 32 KiB, got {}",
            cost.max_line_bytes
        );
        assert_eq!(cost.total_bytes, MAX_RENDER_LINE_BYTES);
        assert_eq!(cost.truncated_lines, 1);
        // The rope reports a trailing empty line past the final newline (or
        // end of text); it costs zero bytes and one scan, like the renderer.
        assert_eq!(cost.column_scans, 2);
    }

    #[test]
    fn frame_cost_stays_within_budget_for_many_huge_lines() {
        let text = (0..30)
            .map(|_| "y".repeat(1024 * 1024))
            .collect::<Vec<_>>()
            .join("\n");
        let snapshot = snapshot_of(&text);
        let cost = measure_frame_cost(&snapshot, 0, 30);
        assert!(
            cost.total_bytes <= MAX_FRAME_SOURCE_BYTES,
            "frame total must stay within 256 KiB, got {}",
            cost.total_bytes
        );
        assert!(cost.max_line_bytes <= MAX_RENDER_LINE_BYTES);
        // 8 full 32 KiB windows fit in 256 KiB; the remaining 22 rows are
        // frame-capped markers, still counted as truncated.
        assert_eq!(cost.truncated_lines, 30);
        assert_eq!(cost.total_bytes, MAX_FRAME_SOURCE_BYTES);
    }

    #[test]
    fn frame_cost_is_exact_for_ordinary_lines() {
        let snapshot = snapshot_of("ab\ncdef\n");
        let cost = measure_frame_cost(&snapshot, 0, 10);
        // Row sizes include the terminator (3 + 5); the budget is
        // deliberately conservative by one byte per line.
        assert_eq!(cost.total_bytes, 3 + 5);
        assert_eq!(cost.max_line_bytes, 5);
        assert_eq!(cost.truncated_lines, 0);
        // Two content lines plus the rope's trailing empty line, which costs
        // zero bytes — the same rows the renderer walks.
        assert_eq!(cost.column_scans, 3);
    }

    #[test]
    fn cursor_at_the_far_right_of_a_huge_line_saturates_honestly() {
        let snapshot = huge_single_line(1);
        let end = snapshot.len_bytes();
        let column = display_column(&snapshot, end);
        assert!(
            column <= MAX_RENDER_LINE_BYTES,
            "column display must saturate at the window, got {column}"
        );
        assert!(
            display_column_truncated(&snapshot, end),
            "the banner signal must fire for a cursor past the window"
        );
        assert!(!display_column_truncated(&snapshot, 0));
        assert_eq!(display_column(&snapshot, 0), 0);
    }

    #[test]
    fn motion_past_a_huge_line_prefix_clamps_to_the_prefix() {
        let snapshot = huge_single_line(1);
        let start = line_start(&snapshot, 0).expect("line exists");
        let landed = offset_at_column(&snapshot, 0, usize::MAX).expect("clamps");
        let window_end = bounded_prefix_end(&snapshot, start, snapshot.len_bytes());
        assert_eq!(landed, window_end);
        assert!(landed <= start + MAX_RENDER_LINE_BYTES);
        assert!(landed > start, "the whole prefix stays reachable");
        // Ordinary lines keep exact motion.
        let small = snapshot_of("ab\ncdef");
        assert_eq!(offset_at_column(&small, 0, 99), Some(2));
    }

    #[test]
    fn bounded_prefix_matches_exact_for_short_lines_tabs_and_unicode() {
        // Tabs (variable advance), wide CJK (width 2), and a combining mark
        // (width 0) must measure exactly when inside the budget.
        let snapshot = snapshot_of("a\t\te\u{301}€\t汉\nend");
        let line = 0;
        assert!(!line_is_truncated(&snapshot, line));
        let start = line_start(&snapshot, line).expect("line exists");
        let end = line_content_end(&snapshot, line).expect("end exists");
        assert_eq!(bounded_prefix_end(&snapshot, start, end), end);
        let (bounded, truncated) = bounded_display_width(&snapshot, start..end);
        assert!(!truncated);
        // Whatever the width walk computes for tabs, wide, and combining
        // characters, the bounded path must agree exactly inside the budget.
        assert_eq!(bounded, display_width(&snapshot, start..end));
    }

    #[test]
    fn bounded_prefix_end_never_splits_a_multibyte_char() {
        // Fill the window edge with a 3-byte char so the 32 KiB cut lands
        // inside one.
        let mut text = "a".repeat(MAX_RENDER_LINE_BYTES - 2);
        text.push('€');
        text.push_str(&"b".repeat(1024));
        let snapshot = snapshot_of(&text);
        assert!(line_is_truncated(&snapshot, 0));
        let start = line_start(&snapshot, 0).expect("line exists");
        let end = line_content_end(&snapshot, 0).expect("end exists");
        let window_end = bounded_prefix_end(&snapshot, start, end);
        assert!(window_end <= start + MAX_RENDER_LINE_BYTES);
        assert!(snapshot.is_byte_boundary(window_end));
        assert!(
            snapshot.read_bytes(start, window_end).is_ok(),
            "the window must be directly readable"
        );
    }

    #[test]
    fn command_spellings_parse_to_their_canonical_names() {
        assert_eq!(
            parse_editor_command("w"),
            Some((EditorCommandName::Write, None))
        );
        assert_eq!(
            parse_editor_command("q"),
            Some((EditorCommandName::Quit, None))
        );
        assert_eq!(
            parse_editor_command("wq"),
            Some((EditorCommandName::WriteQuit, None))
        );
        // `:x` is the vim spelling of the same intent, and must not become
        // a second command with its own identity.
        assert_eq!(
            parse_editor_command("x"),
            Some((EditorCommandName::WriteQuit, None))
        );
        assert_eq!(
            parse_editor_command("q!"),
            Some((EditorCommandName::ForceQuit, None))
        );
        assert_eq!(
            parse_editor_command("  w  "),
            Some((EditorCommandName::Write, None))
        );
    }

    #[test]
    fn a_stray_argument_refuses_the_whole_command() {
        // `:wq now` must not silently run a `:wq`. Half of what was asked
        // for is worse than refusing it.
        assert_eq!(parse_editor_command("wq now"), None);
        assert_eq!(parse_editor_command("q extra"), None);
        assert_eq!(parse_editor_command(""), None);
        assert_eq!(parse_editor_command("zz"), None);
    }

    #[test]
    fn only_open_takes_an_argument() {
        assert_eq!(
            parse_editor_command("open src/main.rs"),
            Some((EditorCommandName::Open, Some("src/main.rs".to_string())))
        );
        assert!(EditorCommandName::Open.takes_argument());
        assert!(!EditorCommandName::Write.takes_argument());
    }

    #[test]
    fn complete_parses_without_an_argument() {
        assert_eq!(
            parse_editor_command("complete"),
            Some((EditorCommandName::Complete, None))
        );
        assert!(!EditorCommandName::Complete.takes_argument());
        assert_eq!(parse_editor_command("complete now"), None);
        assert_eq!(EditorCommandName::Complete.spec(), "complete");
        assert!(EditorCommandName::ALL.contains(&EditorCommandName::Complete));
    }

    #[test]
    fn an_empty_query_offers_the_whole_registry_in_declared_order() {
        let matches = complete_editor_command("");
        assert_eq!(matches.len(), EditorCommandName::ALL.len());
        // Declared order, not alphabetical: the empty palette is the
        // discovery menu and reads as one.
        assert_eq!(matches[0].command, EditorCommandName::ALL[0]);
        assert_eq!(matches[1].command, EditorCommandName::ALL[1]);
    }

    #[test]
    fn fuzzy_completion_narrows_and_ranks_a_prefix_first() {
        let matches = complete_editor_command("w");
        let names: Vec<&str> = matches.iter().map(|m| m.command.spec()).collect();
        assert!(names.contains(&"w"), "{names:?}");
        assert!(names.contains(&"wq"), "{names:?}");
        // `w` is an exact prefix and `new` only a subsequence match, so `w`
        // must rank first even though `fuzzy_score("w", "new")` is no worse.
        assert_eq!(names[0], "w", "{names:?}");
        // Within the prefix tier the shortest wins.
        let wq_at = names.iter().position(|name| *name == "wq").unwrap();
        assert!(wq_at > 0, "{names:?}");
    }

    #[test]
    fn fuzzy_completion_excludes_commands_that_do_not_match() {
        // `fuzzy_score` returns 0 for both an empty query and a total
        // miss, so a non-matching command must be filtered by the prefix
        // test rather than the score alone.
        let names: Vec<&str> = complete_editor_command("qz")
            .iter()
            .map(|m| m.command.spec())
            .collect();
        assert!(names.is_empty(), "{names:?}");
    }

    #[test]
    fn the_completion_list_stays_bounded() {
        let matches = complete_editor_command("a");
        assert!(
            matches.len() <= MAX_EDITOR_COMMAND_MATCHES,
            "{}",
            matches.len()
        );
    }

    #[test]
    fn the_command_line_is_bounded_so_it_cannot_grow_into_a_buffer() {
        let mut presentation = TuiDocumentPresentation::default();
        presentation.open_command_line("");
        for _ in 0..(MAX_EDITOR_COMMAND_LINE + 10) {
            presentation.push_command_char('x');
        }
        assert_eq!(
            presentation.command_line.chars().count(),
            MAX_EDITOR_COMMAND_LINE
        );
        // A refused keystroke must not have disturbed the line.
        assert!(!presentation.push_command_char('y'));
        assert_eq!(
            presentation.command_line.chars().count(),
            MAX_EDITOR_COMMAND_LINE
        );
    }

    #[test]
    fn closing_the_command_line_discards_what_was_typed() {
        let mut presentation = TuiDocumentPresentation::default();
        presentation.open_command_line("");
        presentation.push_command_char('w');
        assert!(presentation.command_active);
        presentation.close_command_line();
        assert!(!presentation.command_active);
        assert!(presentation.command_line.is_empty());
    }

    #[test]
    fn palette_selection_stays_inside_the_current_rows() {
        let mut presentation = TuiDocumentPresentation::default();
        presentation.open_command_line("");
        let count = presentation.command_matches().len();
        presentation.move_command_selection(-5);
        assert_eq!(presentation.command_selection, 0);
        for _ in 0..(count + 5) {
            presentation.move_command_selection(1);
        }
        assert_eq!(presentation.command_selection, count - 1);
    }

    #[test]
    fn an_unknown_command_yields_nothing_to_run() {
        // `parse_editor_command` returning `None` is what turns a typo into
        // a notice rather than a silent no-op.
        assert_eq!(parse_editor_command("write"), None);
        assert_eq!(parse_editor_command("q!q"), None);
    }

    // ── M004 change verbs ───────────────────────────────────────────────

    /// Apply `edit` to a fresh buffer and return the resulting text.
    fn apply_to(text: &str, edit: Option<EditorEdit>) -> String {
        let Some(edit) = edit else {
            return text.to_string();
        };
        let mut buffer = DocumentBuffer::new(text);
        buffer
            .apply(&edit.transaction, DocumentLimits::default())
            .expect("applies");
        let snapshot = buffer.snapshot();
        snapshot
            .read_bytes(0, snapshot.len_bytes())
            .expect("whole document")
    }

    /// Where `submit_edit` would leave the cursor: `cursor_after`, clamped
    /// against the **post**-edit snapshot.
    ///
    /// Mirroring the clamp is the point. A transform that expands its
    /// replacement past the old range — `ß` to `SS` — is only safe because
    /// of it, and asserting the raw `cursor_after` would prove nothing about
    /// where the user actually lands.
    fn cursor_of(text: &str, edit: &EditorEdit) -> usize {
        let mut buffer = DocumentBuffer::new(text);
        buffer
            .apply(&edit.transaction, DocumentLimits::default())
            .expect("applies");
        let after = buffer.snapshot();
        clamp_offset(&after, edit.cursor_after)
    }

    /// Hold `members` as the presentation's selection set, with member 0
    /// primary.
    ///
    /// Stands in for the `C`/`Alt-C` gestures that M008 will bind: until then
    /// no key builds a multi-member set, and a rule that is only reachable
    /// from a future key cannot be tested today.
    fn hold(presentation: &mut TuiDocumentPresentation, members: Vec<Selection<EditorPoint>>) {
        presentation.selections = members;
        presentation.primary_index = 0;
    }

    /// The primary member, through the same scope `selection_range` uses.
    fn primary(presentation: &TuiDocumentPresentation) -> Option<Selection<EditorPoint>> {
        presentation
            .selections
            .get(presentation.primary_index)
            .copied()
    }

    /// A held character selection spanning `start..=end`.
    fn selecting(start: usize, end: usize) -> Selection<EditorPoint> {
        Selection {
            anchor: EditorPoint {
                byte: start,
                line: 0,
            },
            head: EditorPoint { byte: end, line: 0 },
            mode: SelectionMode::Char,
            growing: false,
        }
    }

    // ── M008 set verbs: geometry and content ───────────────────────────

    /// A member on `line`, spanning `start..=end` in byte columns of that line.
    fn member_on(
        snapshot: &DocumentSnapshot,
        line: usize,
        start: usize,
        end: usize,
    ) -> Selection<EditorPoint> {
        let base = line_start(snapshot, line).unwrap_or(0);
        Selection {
            anchor: editor_point(snapshot, base + start),
            head: editor_point(snapshot, base + end),
            mode: SelectionMode::Char,
            growing: false,
        }
    }

    /// The byte after every held member, in document order.
    fn member_ends(
        presentation: &TuiDocumentPresentation,
        snapshot: &DocumentSnapshot,
    ) -> Vec<usize> {
        presentation
            .selections
            .iter()
            .filter_map(|member| member_range(snapshot, member))
            .map(|range| range.end)
            .collect()
    }

    fn refusal(outcome: HelixVerbOutcome) -> String {
        match outcome {
            HelixVerbOutcome::NothingToDo(reason) => reason,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    // `%`

    #[test]
    fn select_all_covers_the_whole_buffer_terminator_included() {
        let snapshot = snapshot_of("one\ntwo\nthree\n");
        let mut presentation = presentation_at(0);
        assert_eq!(
            apply_helix_verb(&snapshot, &mut presentation, HelixVerb::SelectAll),
            HelixVerbOutcome::Handled
        );
        assert_eq!(presentation.selections.len(), 1);
        assert_eq!(presentation.primary_index, 0);
        assert_eq!(
            selection_ranges(&presentation, &snapshot),
            vec![0..14],
            "every byte including the trailing newline — stopping short would \
             leave a bare `\\n` and make `%` then `d` look like it emptied the \
             file while a byte remained in it"
        );
    }

    #[test]
    fn select_all_on_an_unterminated_document_covers_the_last_character() {
        // `buffer_content_end` is the end of the buffer here, so the member's
        // range must not stop one character short of it.
        let snapshot = snapshot_of("abc");
        let mut presentation = presentation_at(0);
        apply_helix_verb(&snapshot, &mut presentation, HelixVerb::SelectAll);
        assert_eq!(selection_ranges(&presentation, &snapshot), vec![0..3]);
    }

    #[test]
    fn select_all_on_an_empty_document_is_held_but_resolves_to_nothing() {
        let snapshot = snapshot_of("");
        let mut presentation = presentation_at(0);
        assert_eq!(
            apply_helix_verb(&snapshot, &mut presentation, HelixVerb::SelectAll),
            HelixVerbOutcome::Handled
        );
        assert_eq!(
            presentation.selections.len(),
            1,
            "everything is selected — including when there is nothing"
        );
        assert!(
            selection_ranges(&presentation, &snapshot).is_empty(),
            "and deleting it correctly deletes nothing"
        );
    }

    // `C` / `Alt-C`

    #[test]
    fn copy_onto_the_next_line_adds_a_member_rather_than_moving_one() {
        let snapshot = snapshot_of("ab\ncd\nef\n");
        let mut presentation = presentation_at(0);
        apply_helix_verb(&snapshot, &mut presentation, HelixVerb::SelectChar);
        assert_eq!(
            apply_helix_verb(
                &snapshot,
                &mut presentation,
                HelixVerb::CopySelectionOnLine { line_delta: 1 }
            ),
            HelixVerbOutcome::Handled
        );
        assert_eq!(presentation.selections.len(), 2);
        assert_eq!(
            member_ends(&presentation, &snapshot),
            vec![1, 4],
            "the new cursor is at column 0 of line 1"
        );
        // The original member is still there: `C` adds coverage.
        assert_eq!(presentation.cursor_byte, 0, "the reader does not move");
    }

    #[test]
    fn copy_onto_the_next_line_reports_when_there_is_no_next_line() {
        // Unterminated on purpose. `len_lines()` counts a trailing line that
        // exists only because a buffer ends in a newline, and no byte offset
        // resolves into it — so bounding the walk by `last_line_of` would let
        // `C` resolve "the next line" to the cursor's own byte and report a
        // cursor below that had not moved. `C` is bounded by the last line the
        // cursor can actually reach instead.
        let snapshot = snapshot_of("a\nb\nc");
        let last_start = snapshot.len_bytes() - 1;
        let mut presentation = presentation_at(last_start);
        apply_helix_verb(&snapshot, &mut presentation, HelixVerb::SelectChar);
        let outcome = apply_helix_verb(
            &snapshot,
            &mut presentation,
            HelixVerb::CopySelectionOnLine { line_delta: 1 },
        );
        assert!(refusal(outcome).contains("below"));
        assert_eq!(
            presentation.selections.len(),
            1,
            "a cursor that did not move is not the cursor below"
        );
    }

    #[test]
    fn copy_onto_the_previous_line_refuses_at_the_first_line() {
        let snapshot = snapshot_of("one\ntwo\n");
        let mut presentation = presentation_at(0);
        apply_helix_verb(&snapshot, &mut presentation, HelixVerb::SelectChar);
        let outcome = apply_helix_verb(
            &snapshot,
            &mut presentation,
            HelixVerb::CopySelectionOnLine { line_delta: -1 },
        );
        assert!(refusal(outcome).contains("above"));
    }

    #[test]
    fn copy_onto_the_next_line_keeps_the_set_sorted_and_the_primary_intact() {
        let snapshot = snapshot_of("a\nb\nc\nd\n");
        let mut presentation = presentation_at(0);
        // Two members on different lines, the second one primary.
        hold(
            &mut presentation,
            vec![member_on(&snapshot, 0, 0, 0), member_on(&snapshot, 2, 0, 0)],
        );
        presentation.primary_index = 1;
        apply_helix_verb(
            &snapshot,
            &mut presentation,
            HelixVerb::CopySelectionOnLine { line_delta: 1 },
        );
        assert_eq!(member_ends(&presentation, &snapshot), vec![1, 3, 5, 7]);
        assert_eq!(
            presentation.primary_index, 2,
            "the primary follows its member to line 2, which is now index 2 — \
             adding cursors must not move the reader's attention to a new one"
        );
    }

    #[test]
    fn copy_onto_the_next_line_clamps_to_a_shorter_target_line() {
        let snapshot = snapshot_of("abcd\ne\n");
        let mut presentation = presentation_at(3);
        apply_helix_verb(&snapshot, &mut presentation, HelixVerb::SelectChar);
        apply_helix_verb(
            &snapshot,
            &mut presentation,
            HelixVerb::CopySelectionOnLine { line_delta: 1 },
        );
        assert_eq!(
            member_ends(&presentation, &snapshot),
            vec![4, 7],
            "column 3 does not exist on a one-character line, so the cursor \
             lands on that character rather than running into the next line"
        );
    }

    #[test]
    fn copy_onto_the_next_line_refuses_past_the_selection_ceiling() {
        // 256 single-character members; one more press would need 512.
        let width = MAX_DOCUMENT_EDITS;
        let text = "x\n".repeat(width);
        let snapshot = snapshot_of(&text);
        let mut presentation = presentation_at(0);
        hold(
            &mut presentation,
            (0..width)
                .map(|index| member_on(&snapshot, index * 2, 0, 0))
                .collect(),
        );
        let outcome = apply_helix_verb(
            &snapshot,
            &mut presentation,
            HelixVerb::CopySelectionOnLine { line_delta: 1 },
        );
        assert!(refusal(outcome).contains(&MAX_DOCUMENT_EDITS.to_string()));
        assert_eq!(
            presentation.selections.len(),
            width,
            "refused, not truncated: no subset of the cursors vanished"
        );
    }

    // Focus rotation versus content rotation

    #[test]
    fn rotating_the_primary_moves_no_bytes_and_rotating_contents_moves_no_cursor() {
        let snapshot = snapshot_of("aa\nbb\ncc\n");
        let mut focus = presentation_at(4);
        hold(
            &mut focus,
            vec![
                member_on(&snapshot, 0, 0, 1),
                member_on(&snapshot, 1, 0, 1),
                member_on(&snapshot, 2, 0, 1),
            ],
        );
        apply_helix_verb(
            &snapshot,
            &mut focus,
            HelixVerb::RotateSelections { member_delta: 1 },
        );
        assert_eq!(focus.primary_index, 1);
        assert_eq!(
            focus.cursor_byte, 4,
            "the cursor is the primary's head, which is column 1 of line 1"
        );

        let mut contents = focus.clone();
        let HelixVerbOutcome::Edit(edit, note) = apply_helix_verb(
            &snapshot,
            &mut contents,
            HelixVerb::RotateSelectionContents { member_delta: 1 },
        ) else {
            panic!("rotating contents must produce an edit");
        };
        assert!(note.is_none());
        assert_eq!(apply("aa\nbb\ncc\n", &edit.transaction), "bb\ncc\naa\n");
    }

    #[test]
    fn rotating_contents_of_one_member_is_a_no_op_rather_than_an_empty_edit() {
        let snapshot = snapshot_of("aa\nbb\n");
        let mut presentation = presentation_at(0);
        hold(&mut presentation, vec![member_on(&snapshot, 0, 0, 1)]);
        assert_eq!(
            apply_helix_verb(
                &snapshot,
                &mut presentation,
                HelixVerb::RotateSelectionContents { member_delta: 1 }
            ),
            HelixVerbOutcome::NothingToDo("nothing selected".to_string())
        );
    }

    #[test]
    fn rotating_identical_contents_produces_no_edit() {
        let snapshot = snapshot_of("aa\naa\n");
        let mut presentation = presentation_at(0);
        hold(
            &mut presentation,
            vec![member_on(&snapshot, 0, 0, 1), member_on(&snapshot, 1, 0, 1)],
        );
        // A rotation whose members all hold the same text moves nothing, and
        // an edit that undoes nothing must not reach the undo history.
        assert_eq!(
            apply_helix_verb(
                &snapshot,
                &mut presentation,
                HelixVerb::RotateSelectionContents { member_delta: 1 }
            ),
            HelixVerbOutcome::NothingToDo("nothing selected".to_string())
        );
    }

    #[test]
    fn rotating_contents_refuses_an_overlapping_set() {
        let snapshot = snapshot_of("abcdefghij");
        let mut presentation = presentation_at(0);
        hold(
            &mut presentation,
            vec![spanning(&snapshot, 0, 5), spanning(&snapshot, 3, 8)],
        );
        let outcome = apply_helix_verb(
            &snapshot,
            &mut presentation,
            HelixVerb::RotateSelectionContents { member_delta: 1 },
        );
        assert!(refusal(outcome).contains("overlap"));
    }

    #[test]
    fn rotating_contents_is_one_transaction_over_the_original_coordinates() {
        let snapshot = snapshot_of("aa\nbb\ncc\n");
        let mut presentation = presentation_at(0);
        hold(
            &mut presentation,
            vec![
                member_on(&snapshot, 0, 0, 1),
                member_on(&snapshot, 1, 0, 1),
                member_on(&snapshot, 2, 0, 1),
            ],
        );
        let HelixVerbOutcome::Edit(edit, _) = apply_helix_verb(
            &snapshot,
            &mut presentation,
            HelixVerb::RotateSelectionContents { member_delta: -1 },
        ) else {
            panic!("expected one transaction");
        };
        assert_eq!(edit.transaction.edits.len(), 3);
        assert_eq!(
            apply("aa\nbb\ncc\n", &edit.transaction),
            "cc\naa\nbb\n",
            "each member receives its predecessor's text"
        );
    }

    // `&`

    #[test]
    fn align_pads_every_member_to_the_widest_one() {
        let snapshot = snapshot_of("ab\ncdef\n");
        let mut presentation = presentation_at(0);
        hold(
            &mut presentation,
            vec![member_on(&snapshot, 0, 0, 1), member_on(&snapshot, 1, 0, 3)],
        );
        let HelixVerbOutcome::Edit(edit, note) =
            apply_helix_verb(&snapshot, &mut presentation, HelixVerb::AlignSelections)
        else {
            panic!("aligning must produce an edit");
        };
        assert!(note.is_none());
        assert_eq!(
            edit.transaction.edits.len(),
            1,
            "the widest member already has the target width"
        );
        assert_eq!(apply("ab\ncdef\n", &edit.transaction), "ab  \ncdef\n");
    }

    #[test]
    fn align_measures_display_columns_not_bytes() {
        // A CJK ideograph is three bytes and two columns. Padding to the byte
        // width would leave the shorter member short on screen.
        let snapshot = snapshot_of("漢字\nab\n");
        let mut presentation = presentation_at(0);
        hold(
            &mut presentation,
            vec![member_on(&snapshot, 0, 0, 5), member_on(&snapshot, 1, 0, 1)],
        );
        let HelixVerbOutcome::Edit(edit, _) =
            apply_helix_verb(&snapshot, &mut presentation, HelixVerb::AlignSelections)
        else {
            panic!("expected an edit");
        };
        assert_eq!(
            apply("漢字\nab\n", &edit.transaction),
            "漢字\nab  \n",
            "two ideographs are four columns, so `ab` takes two spaces"
        );
    }

    #[test]
    fn align_of_one_widest_member_needs_no_padding_and_reports_a_no_op() {
        let snapshot = snapshot_of("abc\nxyz\n");
        let mut presentation = presentation_at(0);
        hold(
            &mut presentation,
            vec![member_on(&snapshot, 0, 0, 2), member_on(&snapshot, 1, 0, 2)],
        );
        assert!(matches!(
            apply_helix_verb(&snapshot, &mut presentation, HelixVerb::AlignSelections),
            HelixVerbOutcome::NothingToDo(_)
        ));
    }

    #[test]
    fn align_discloses_that_an_over_budget_member_was_measured_in_part() {
        let width = MAX_RENDER_LINE_BYTES + 64;
        let text = format!("{}\nshort\n", "x".repeat(width));
        let snapshot = snapshot_of(&text);
        let mut presentation = presentation_at(0);
        hold(
            &mut presentation,
            vec![
                member_on(&snapshot, 0, 0, width - 1),
                member_on(&snapshot, 1, 0, 4),
            ],
        );
        let HelixVerbOutcome::Edit(_, note) =
            apply_helix_verb(&snapshot, &mut presentation, HelixVerb::AlignSelections)
        else {
            panic!("expected an edit");
        };
        let note = note.expect("a member wider than the budget must be disclosed");
        assert!(
            note.contains(&MAX_RENDER_LINE_BYTES.to_string()),
            "the notice names the budget: {note}"
        );
    }

    // Regex verbs

    #[test]
    fn split_makes_one_member_per_match() {
        let snapshot = snapshot_of("ab ab ab\n");
        let mut presentation = presentation_at(0);
        hold(&mut presentation, vec![member_on(&snapshot, 0, 0, 7)]);
        let outcome = apply_helix_verb(
            &snapshot,
            &mut presentation,
            HelixVerb::SplitSelection {
                pattern: "ab".to_string(),
            },
        );
        assert_eq!(outcome, HelixVerbOutcome::Handled);
        assert_eq!(
            member_ends(&presentation, &snapshot),
            vec![2, 5, 8],
            "three two-character matches, each covering exactly its own bytes"
        );
    }

    #[test]
    fn split_whose_pattern_matches_nothing_leaves_the_set_alone() {
        let snapshot = snapshot_of("alpha\nbeta\n");
        let mut presentation = presentation_at(0);
        hold(
            &mut presentation,
            vec![member_on(&snapshot, 0, 0, 4), member_on(&snapshot, 1, 0, 3)],
        );
        let outcome = apply_helix_verb(
            &snapshot,
            &mut presentation,
            HelixVerb::SplitSelection {
                pattern: "zzz".to_string(),
            },
        );
        assert!(refusal(outcome).contains("zzz"));
        assert_eq!(presentation.selections.len(), 2, "the set is unchanged");
    }

    #[test]
    fn a_pattern_that_will_not_compile_is_reported_and_the_buffer_is_untouched() {
        let snapshot = snapshot_of("alpha\n");
        let mut presentation = presentation_at(0);
        hold(&mut presentation, vec![member_on(&snapshot, 0, 0, 4)]);
        let outcome = apply_helix_verb(
            &snapshot,
            &mut presentation,
            HelixVerb::SplitSelection {
                pattern: "a(".to_string(),
            },
        );
        assert!(refusal(outcome).contains("not a valid regex"));
        assert_eq!(presentation.selections.len(), 1);
    }

    #[test]
    fn split_lands_on_character_boundaries_inside_a_multi_byte_member() {
        // The match ends on byte 3, inside the second ideograph. A head taken
        // from `end` rather than from the last character would cover one
        // character too many and split the UTF-8 sequence.
        let snapshot = snapshot_of("a漢字b\n");
        let mut presentation = presentation_at(0);
        hold(&mut presentation, vec![member_on(&snapshot, 0, 0, 7)]);
        apply_helix_verb(
            &snapshot,
            &mut presentation,
            HelixVerb::SplitSelection {
                pattern: "字".to_string(),
            },
        );
        assert_eq!(
            selection_ranges(&presentation, &snapshot),
            vec![4..7],
            "the match is three bytes wide and its member covers exactly those \
             three — a head taken from `end` would have covered one character \
             more and split the UTF-8 sequence"
        );
    }

    #[test]
    fn a_last_char_start_names_the_character_that_ends_at_the_range() {
        // The invariant every split member relies on.
        let snapshot = snapshot_of("a漢字b");
        // Only meaningful on a character boundary: a match ends there by
        // definition, and naming the character that ends at an offset *inside*
        // one is a question with no honest answer.
        for end in 1..=snapshot.len_bytes() {
            let start = last_char_start(&snapshot, end).expect("a last character");
            assert!(snapshot.is_byte_boundary(start));
            if snapshot.is_byte_boundary(end) {
                assert_eq!(char_end(&snapshot, start), end, "byte {end}");
            }
        }
        assert_eq!(last_char_start(&snapshot, 0), None);
    }

    #[test]
    fn keep_and_remove_select_by_whether_a_members_text_matches() {
        let snapshot = snapshot_of("apple\nbanana\n");
        let mut presentation = presentation_at(0);
        hold(
            &mut presentation,
            vec![member_on(&snapshot, 0, 0, 4), member_on(&snapshot, 1, 0, 5)],
        );
        apply_helix_verb(
            &snapshot,
            &mut presentation,
            HelixVerb::KeepSelections {
                pattern: "an".to_string(),
            },
        );
        assert_eq!(member_ends(&presentation, &snapshot), vec![12]);

        let mut again = presentation_at(0);
        hold(
            &mut again,
            vec![member_on(&snapshot, 0, 0, 4), member_on(&snapshot, 1, 0, 5)],
        );
        apply_helix_verb(
            &snapshot,
            &mut again,
            HelixVerb::RemoveSelections {
                pattern: "an".to_string(),
            },
        );
        assert_eq!(member_ends(&again, &snapshot), vec![5]);
    }

    #[test]
    fn keeping_a_set_where_everything_matches_is_a_no_op() {
        let snapshot = snapshot_of("a\nb\n");
        let mut presentation = presentation_at(0);
        hold(
            &mut presentation,
            vec![member_on(&snapshot, 0, 0, 0), member_on(&snapshot, 1, 0, 0)],
        );
        let outcome = apply_helix_verb(
            &snapshot,
            &mut presentation,
            HelixVerb::KeepSelections {
                pattern: ".".to_string(),
            },
        );
        assert!(refusal(outcome).contains("already"));
        assert_eq!(presentation.selections.len(), 2);
    }

    #[test]
    fn keeping_a_set_where_nothing_matches_empties_it_and_says_so() {
        let snapshot = snapshot_of("a\nb\n");
        let mut presentation = presentation_at(0);
        hold(
            &mut presentation,
            vec![member_on(&snapshot, 0, 0, 0), member_on(&snapshot, 1, 0, 0)],
        );
        assert_eq!(
            apply_helix_verb(
                &snapshot,
                &mut presentation,
                HelixVerb::KeepSelections {
                    pattern: "zzz".to_string(),
                },
            ),
            HelixVerbOutcome::Handled,
            "keeping nothing matched is the operation succeeding"
        );
        assert!(
            presentation.selections.is_empty(),
            "`K` must leave the reader holding exactly the selections they \
             asked to discard"
        );
    }

    #[test]
    fn a_regex_verb_sees_only_its_members_not_the_whole_buffer() {
        // The pattern matches line 2, which no member covers. A verb that
        // scanned the document would match; a verb that matches a member's own
        // text cannot.
        let snapshot = snapshot_of("aaa\nbbb\n");
        let mut presentation = presentation_at(0);
        hold(&mut presentation, vec![member_on(&snapshot, 0, 0, 2)]);
        let outcome = apply_helix_verb(
            &snapshot,
            &mut presentation,
            HelixVerb::SplitSelection {
                pattern: "bbb".to_string(),
            },
        );
        assert!(refusal(outcome).contains("matched none"));
    }

    #[test]
    fn the_regex_window_is_bounded_per_member() {
        // Wider than the per-member budget: only the prefix is scanned.
        let width = MAX_RENDER_LINE_BYTES + 128;
        let mut text = "ab".repeat(width / 2);
        text.push('\n');
        let snapshot = snapshot_of(&text);
        let mut presentation = presentation_at(0);
        hold(
            &mut presentation,
            vec![member_on(&snapshot, 0, 0, width - 1)],
        );
        apply_helix_verb(
            &snapshot,
            &mut presentation,
            HelixVerb::SplitSelection {
                pattern: "ab".to_string(),
            },
        );
        let ends = member_ends(&presentation, &snapshot);
        assert!(
            ends.len() < width / 2,
            "the scan stopped at the budget, it did not read the whole line: {}",
            ends.len()
        );
    }

    // Merge

    #[test]
    fn merge_all_bridges_the_gap_and_merge_consecutive_does_not() {
        let snapshot = snapshot_of("aa bb cc\n");
        let mut both = presentation_at(0);
        hold(
            &mut both,
            vec![member_on(&snapshot, 0, 0, 1), member_on(&snapshot, 0, 3, 4)],
        );
        apply_helix_verb(&snapshot, &mut both, HelixVerb::MergeSelections);
        assert_eq!(
            selection_ranges(&both, &snapshot),
            vec![0..5],
            "`Alt-minus` bridges the gap"
        );

        let mut touching = presentation_at(0);
        hold(
            &mut touching,
            vec![member_on(&snapshot, 0, 0, 1), member_on(&snapshot, 0, 2, 3)],
        );
        apply_helix_verb(
            &snapshot,
            &mut touching,
            HelixVerb::MergeConsecutiveSelections,
        );
        assert_eq!(
            selection_ranges(&touching, &snapshot),
            vec![0..4],
            "`Alt-_` merges them because they are adjacent"
        );
    }

    #[test]
    fn merge_consecutive_leaves_a_real_gap_alone() {
        let snapshot = snapshot_of("aa bb cc\n");
        let mut presentation = presentation_at(0);
        hold(
            &mut presentation,
            vec![member_on(&snapshot, 0, 0, 1), member_on(&snapshot, 0, 3, 4)],
        );
        let outcome = apply_helix_verb(
            &snapshot,
            &mut presentation,
            HelixVerb::MergeConsecutiveSelections,
        );
        assert!(refusal(outcome).contains("adjacent"));
        assert_eq!(presentation.selections.len(), 2, "the set is untouched");
    }

    #[test]
    fn merge_consecutive_bridges_two_whole_lines_because_lines_touch() {
        let snapshot = snapshot_of("one\ntwo\nthree\n");
        let mut presentation = presentation_at(0);
        let mut first = member_on(&snapshot, 0, 0, 2);
        first.mode = SelectionMode::Line;
        let mut second = member_on(&snapshot, 1, 0, 2);
        second.mode = SelectionMode::Line;
        hold(&mut presentation, vec![first, second]);
        apply_helix_verb(
            &snapshot,
            &mut presentation,
            HelixVerb::MergeConsecutiveSelections,
        );
        assert_eq!(
            selection_ranges(&presentation, &snapshot),
            vec![0..8],
            "line 0 ends exactly where line 1 starts"
        );
    }

    #[test]
    fn the_set_algebra_refuses_every_verb_that_needs_a_selection_when_none_is_held() {
        let snapshot = snapshot_of("alpha\nbeta\n");
        let verbs = [
            HelixVerb::CollapseSelection,
            HelixVerb::FlipSelections,
            HelixVerb::EnsureSelectionsForward,
            HelixVerb::KeepPrimarySelection,
            HelixVerb::RemovePrimarySelection,
            HelixVerb::RotateSelections { member_delta: 1 },
            HelixVerb::MergeSelections,
            HelixVerb::MergeConsecutiveSelections,
            HelixVerb::AlignSelections,
            HelixVerb::RotateSelectionContents { member_delta: 1 },
        ];
        for verb in verbs {
            let mut presentation = presentation_at(0);
            assert!(
                matches!(
                    apply_helix_verb(&snapshot, &mut presentation, verb.clone()),
                    HelixVerbOutcome::NothingToDo(_)
                ),
                "{verb:?} must report rather than act on nothing"
            );
        }
    }

    // `EditorCommandName` — the regex verbs' command surface

    #[test]
    fn the_regex_commands_parse_with_and_without_their_argument() {
        assert_eq!(
            parse_editor_command("split-regex a+b"),
            Some((EditorCommandName::SplitRegex, Some("a+b".to_string())))
        );
        assert_eq!(
            parse_editor_command("keep-regex"),
            Some((EditorCommandName::KeepRegex, None))
        );
        assert_eq!(
            parse_editor_command("remove-regex x"),
            Some((EditorCommandName::RemoveRegex, Some("x".to_string())))
        );
        assert!(
            EditorCommandName::SplitRegex.takes_argument(),
            "a pattern has to be somewhere to be typed"
        );
        assert_eq!(EditorCommandName::SplitRegex.spec(), "split-regex");
    }

    #[test]
    fn paste_after_lands_past_the_character_under_the_cursor() {
        // "After" means after the character the cursor is on, which is the
        // same target `r` and `~` act on — all three read "what is under the
        // cursor" as one unit, and `p` past that unit rather than inside it.
        let presentation = presentation_at(0);
        let edit = paste_after(&snapshot_of("abc"), &presentation, "X");
        assert_eq!(apply_to("abc", edit), "aXbc");
    }

    #[test]
    fn a_single_multi_byte_character_is_still_a_character_to_act_on() {
        // `next_char_boundary(0)` is `None` here, because the document is one
        // character long. Reading that as "no character under the cursor"
        // would make every verb a silent no-op in a one-character file.
        let text = "\u{65e5}";
        let snapshot = snapshot_of(text);
        let presentation = presentation_at(0);
        let edit = replace_char(&snapshot, &presentation, 'x').expect("a character");
        assert_eq!(apply_to(text, Some(edit)), "x");
        // And "after" that character is the end of the document.
        let paste = paste_after(&snapshot, &presentation, "!").expect("a paste");
        assert_eq!(apply_to(text, Some(paste)), "\u{65e5}!");
        // `~` still reports nothing here, and for the right reason: `日` has
        // no case, so the *range* is non-empty and the *mapping* is empty.
        assert!(switch_case(&snapshot, &presentation).is_none());
    }

    #[test]
    fn paste_before_lands_on_the_cursor() {
        let presentation = presentation_at(0);
        let edit = paste_before(&snapshot_of("abc"), &presentation, "X");
        assert_eq!(apply_to("abc", edit), "Xabc");
    }

    #[test]
    fn paste_leaves_the_cursor_on_the_last_pasted_byte_so_pp_appends_twice() {
        // Cursor on 'b' in "abc": "after" is offset 2, the paste ends at 4, and the
        // cursor lands on the last pasted byte. A second `p` from there has
        // to append rather than step over the 'c' that followed it.
        let first = paste_after(&snapshot_of("abc"), &presentation_at(1), "XY").expect("edit");
        assert_eq!(apply_to("abc", Some(first.clone())), "abXYc");
        assert_eq!(cursor_of("abc", &first), 3);

        let after_first = apply_to("abc", Some(first));
        let second =
            paste_after(&snapshot_of(&after_first), &presentation_at(3), "Z").expect("edit");
        assert_eq!(apply_to(&after_first, Some(second)), "abXYZc");
    }

    #[test]
    fn paste_at_the_end_of_the_document_lands_at_the_end() {
        // There is no character under the cursor to move past, so "after"
        // has to mean the end of the buffer — otherwise `p` on the last
        // character could never insert anything.
        let text = "abc";
        let presentation = presentation_at(3);
        let edit = paste_after(&snapshot_of(text), &presentation, "X");
        assert_eq!(apply_to(text, edit), "abcX");
    }

    #[test]
    fn an_empty_clipboard_is_not_an_edit() {
        let presentation = presentation_at(0);
        assert!(paste_after(&snapshot_of("abc"), &presentation, "").is_none());
        assert!(paste_before(&snapshot_of("abc"), &presentation, "").is_none());
    }

    #[test]
    fn replace_char_swaps_one_character_and_leaves_the_line_intact() {
        let text = "abc";
        let presentation = presentation_at(1);
        let edit = replace_char(&snapshot_of(text), &presentation, 'X').expect("edit");
        assert_eq!(apply_to(text, Some(edit)), "aXc");
    }

    #[test]
    fn replace_char_replaces_a_whole_character_not_one_byte_of_it() {
        // The hazard M003 found: `read_bytes` refuses a range that is not
        // boundary-aligned on *both* ends, so a transform built from
        // `read_bytes(offset, offset + 1)` cannot even read `日`. Replacing
        // it must consume all three bytes and put back exactly three.
        let text = "a日b";
        let presentation = presentation_at(1);
        let edit = replace_char(&snapshot_of(text), &presentation, '本').expect("edit");
        assert_eq!(apply_to(text, Some(edit)), "a本b");
    }

    #[test]
    fn replace_char_at_the_end_of_the_buffer_is_not_an_edit() {
        let text = "abc";
        let presentation = presentation_at(3);
        assert!(replace_char(&snapshot_of(text), &presentation, 'X').is_none());
    }

    #[test]
    fn replacing_a_selection_replaces_the_whole_selection() {
        let snapshot = snapshot_of("alpha beta");
        let mut presentation = presentation_at(0);
        hold(&mut presentation, vec![selecting(0, 4)]);
        let edit = replace_char(&snapshot, &presentation, 'X').expect("edit");
        assert_eq!(apply_to("alpha beta", Some(edit)), "X beta");
    }

    #[test]
    fn replace_with_text_swaps_the_character_under_the_cursor() {
        let text = "abc";
        let presentation = presentation_at(0);
        let edit = replace_with_text(&snapshot_of(text), &presentation, "XY").expect("edit");
        assert_eq!(apply_to(text, Some(edit)), "XYbc");
    }

    #[test]
    fn an_empty_replacement_text_is_not_an_edit() {
        let presentation = presentation_at(0);
        assert!(replace_with_text(&snapshot_of("abc"), &presentation, "").is_none());
    }

    #[test]
    fn switch_case_flips_the_character_under_the_cursor() {
        let text = "aBc";
        let presentation = presentation_at(0);
        let edit = switch_case(&snapshot_of(text), &presentation).expect("edit");
        assert_eq!(apply_to(text, Some(edit)), "ABc");
    }

    #[test]
    fn switch_case_leaves_the_cursor_on_the_character_it_flipped() {
        // `~ ~` has to return to where it started, which only holds if the
        // cursor stays on the toggled character rather than drifting past it.
        let text = "abc";
        let edit = switch_case(&snapshot_of(text), &presentation_at(0)).expect("edit");
        assert_eq!(cursor_of(text, &edit), 0);
    }

    #[test]
    fn switch_case_flips_every_character_of_a_selection() {
        let snapshot = snapshot_of("alpha Beta");
        let mut presentation = presentation_at(0);
        hold(&mut presentation, vec![selecting(0, 10)]);
        let edit = switch_case(&snapshot, &presentation).expect("edit");
        assert_eq!(apply_to("alpha Beta", Some(edit)), "ALPHA bETA");
    }

    #[test]
    fn the_case_setters_expand_where_the_mapped_form_is_longer() {
        // `ß` uppercases to `SS`: one character in, two out. The replacement
        // is longer than the range it replaces, which is safe only because the
        // edit carries arbitrary transactions and the caller clamps the
        // cursor against the *post*-edit text.
        let text = "aßb";
        let edit = switch_to_upper(&snapshot_of(text), &presentation_at(1)).expect("edit");
        assert_eq!(apply_to(text, Some(edit.clone())), "aSSb");
        assert_eq!(
            cursor_of(text, &edit),
            2,
            "the last byte of the mapped form"
        );
    }

    #[test]
    fn switch_case_on_text_with_no_cased_character_is_not_an_edit() {
        // Submitting it would push an undo entry for nothing and make `u`
        // appear to do nothing on the first press.
        for text in ["123", "--", "", "日本語"] {
            assert!(
                switch_case(&snapshot_of(text), &presentation_at(0)).is_none(),
                "{text:?} has nothing to flip"
            );
        }
    }

    #[test]
    fn switch_case_leaves_a_character_that_has_no_single_other_case() {
        // `İ` lowercases to `i` plus U+0307. A flip asks for the other case of
        // *this* character, and there is no single one — half of that mapping
        // is not a case, and taking it would mean `~ ~` never came back.
        let text = "İ";
        assert!(
            switch_case(&snapshot_of(text), &presentation_at(0)).is_none(),
            "an expanding mapping has no flip"
        );
        // The setters are the verbs that name a whole case, so they use it.
        assert_eq!(
            apply_to(
                text,
                switch_to_lower(&snapshot_of(text), &presentation_at(0))
            ),
            "i̇"
        );
        assert_eq!(
            apply_to("abC", switch_case(&snapshot_of("abC"), &presentation_at(2))),
            "abc"
        );
    }

    #[test]
    fn the_case_setters_set_rather_than_flip() {
        // `~` is a toggle; `` ` `` and `` Alt-` `` are not. The difference is
        // observable on the second press, which is the only way to tell them
        // apart.
        let snapshot = snapshot_of("aBc");
        // On `B`, not `a`: lower-casing an already-lower-case character is the
        // no-op this test also asserts below.
        let presentation = presentation_at(1);
        let lower = switch_to_lower(&snapshot, &presentation).expect("edit");
        assert_eq!(apply_to("aBc", Some(lower.clone())), "abc");
        let mid = apply_to("aBc", Some(lower));
        assert!(
            switch_to_lower(&snapshot_of(&mid), &presentation_at(1)).is_none(),
            "lower-casing already-lower-case text is not an edit"
        );

        let upper = switch_to_upper(&snapshot, &presentation_at(0)).expect("edit");
        assert_eq!(apply_to("aBc", Some(upper.clone())), "ABc");
        let mid = apply_to("aBc", Some(upper));
        assert!(
            switch_to_upper(&snapshot_of(&mid), &presentation_at(1)).is_none(),
            "upper-casing already-upper-case text is not an edit"
        );
    }

    #[test]
    fn the_case_setters_cover_a_whole_selection() {
        let snapshot = snapshot_of("alpha Beta");
        let mut presentation = presentation_at(0);
        hold(&mut presentation, vec![selecting(0, 10)]);
        assert_eq!(
            apply_to("alpha Beta", switch_to_upper(&snapshot, &presentation)),
            "ALPHA BETA"
        );
    }

    #[test]
    fn the_case_setters_use_the_whole_expansion_where_flip_cannot() {
        // `` Alt-` `` on `ß` is `SS`. `~` on the same character takes only the
        // first of the expansion, because a flip that grew would never
        // return the original text and would stop being a toggle.
        let text = "aßb";
        let upper = switch_to_upper(&snapshot_of(text), &presentation_at(1)).expect("edit");
        assert_eq!(apply_to(text, Some(upper)), "aSSb");

        assert!(
            switch_case(&snapshot_of(text), &presentation_at(1)).is_none(),
            "a flip cannot use an expansion it could never undo"
        );
    }

    #[test]
    fn every_capture_prefix_takes_a_character_rather_than_a_leaf() {
        // `r` has no single-key spelling, and its next key is data. If any
        // capture ever grew a leaf row, `EditorPrefix::leaves` would be the
        // wrong place to resolve it and the target would be looked for as a
        // glyph. Swept over the whole class rather than `ReplaceChar` alone,
        // because the rule that matters is the one the classifier states.
        for prefix in [
            EditorPrefix::FindNext,
            EditorPrefix::FindPrevious,
            EditorPrefix::FindTillNext,
            EditorPrefix::FindTillPrevious,
            EditorPrefix::ReplaceChar,
        ] {
            assert!(prefix.is_capture(), "{prefix:?} must classify as a capture");
            assert!(
                prefix.leaves().is_empty(),
                "{prefix:?} takes a target character, so it has no leaves"
            );
            assert!(
                !prefix.swallows_unmatched(),
                "{prefix:?} must hand back a key it cannot use"
            );
        }
        // And the discriminators are disjoint, so `is_capture` means one thing.
        assert!(EditorPrefix::ReplaceChar.is_capture());
        assert!(!EditorPrefix::FindNext.is_replace_char());
        assert!(!EditorPrefix::Goto.is_capture());
        assert!(!EditorPrefix::Space.is_capture());
        assert_eq!(EditorPrefix::ReplaceChar.display(), "r");
    }

    // ── M006 unimpaired navigation, at the model level ─────────────────────

    /// Both bracket grammars resolve by glyph, which is what `leaves()` claims.
    #[test]
    fn the_unimpaired_prefixes_have_leaves_and_are_not_captures() {
        assert!(!EditorPrefix::OpenBracket.is_capture());
        assert!(!EditorPrefix::CloseBracket.is_capture());
        // The mechanism and the claim together: these are the arrays `leaves`
        // hands the dispatch layer, so a table that went empty again would make
        // `p` and `Space` unreachable rather than merely unlisted. Compared by
        // content rather than by pointer — a `const` slice may be materialised
        // at each use site, so `as_ptr` equality is not a fact about the code.
        assert_eq!(
            EditorPrefix::OpenBracket
                .leaves()
                .iter()
                .map(|leaf| leaf.glyph)
                .collect::<Vec<_>>(),
            OPEN_BRACKET_LEAVES
                .iter()
                .map(|leaf| leaf.glyph)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            EditorPrefix::CloseBracket
                .leaves()
                .iter()
                .map(|leaf| leaf.glyph)
                .collect::<Vec<_>>(),
            CLOSE_BRACKET_LEAVES
                .iter()
                .map(|leaf| leaf.glyph)
                .collect::<Vec<_>>()
        );
        for glyph in [' ', 'p'] {
            assert!(
                OPEN_BRACKET_LEAVES.iter().any(|l| l.glyph == glyph),
                "`[{glyph}` must be bound"
            );
            assert!(
                CLOSE_BRACKET_LEAVES.iter().any(|l| l.glyph == glyph),
                "`]{glyph}` must be bound"
            );
        }
    }

    /// A leaf must name what kind of thing it does. `]Space` editing through
    /// the motion path would put a newline into `editor_motion`, which is the
    /// one function in the dispatch that promises never to touch text.
    #[test]
    fn the_newline_leaves_are_edits_and_the_paragraph_leaves_are_motions() {
        fn find(leaves: &[PrefixLeaf], glyph: char) -> PrefixAction {
            leaves
                .iter()
                .find(|l| l.glyph == glyph)
                .expect("bound")
                .action
        }
        assert!(matches!(
            find(CLOSE_BRACKET_LEAVES, ' '),
            PrefixAction::Edit(EditorCommand::AddNewlineBelow)
        ));
        assert!(matches!(
            find(OPEN_BRACKET_LEAVES, ' '),
            PrefixAction::Edit(EditorCommand::AddNewlineAbove)
        ));
        assert!(matches!(
            find(CLOSE_BRACKET_LEAVES, 'p'),
            PrefixAction::Motion(EditorCommand::NextParagraph)
        ));
        assert!(matches!(
            find(OPEN_BRACKET_LEAVES, 'p'),
            PrefixAction::Motion(EditorCommand::PrevParagraph)
        ));
    }

    /// Blank lines are ASCII whitespace only, terminator excluded.
    #[test]
    fn a_line_is_blank_exactly_when_its_content_is_whitespace() {
        let snapshot = snapshot_of("a\n\n   \n\t\nb\n");
        assert!(line_is_blank(&snapshot, 1), "an empty line");
        assert!(line_is_blank(&snapshot, 2), "spaces");
        assert!(line_is_blank(&snapshot, 3), "a tab");
        assert!(!line_is_blank(&snapshot, 0), "content");
        assert!(!line_is_blank(&snapshot, 4), "content");
        // The terminator is not content, so a line of "\n" is one blank line
        // rather than a line containing a newline.
        assert!(line_is_blank(&snapshot, 1));
    }

    /// A non-ASCII byte is content. A byte-at-a-time scan cannot decode it,
    /// and a non-breaking space is not an ASCII space.
    #[test]
    fn a_non_ascii_byte_is_not_blankness() {
        let snapshot = snapshot_of("a\n\u{a0}\nb\n");
        assert!(!line_is_blank(&snapshot, 1));
    }

    /// The document shapes the plan names, each answered explicitly.
    ///
    /// Every start offset of every fixture is walked in both directions, so
    /// this covers blank-first, blank-last, all-blank and non-ASCII
    /// documents at the same strength as the ordinary one — a rule that only
    /// holds for "paragraph, blank, paragraph" is not a paragraph rule.
    #[test]
    fn every_paragraph_shape_either_lands_or_reports() {
        let fixtures: [(&str, &str); 8] = [
            ("single paragraph", "one\ntwo\n"),
            ("blank first line", "\none\n\ntwo\n"),
            ("blank last line", "one\n\ntwo\n\n"),
            ("entirely blank", "\n\n\n"),
            ("empty", ""),
            ("three blanks as one separator", "a\n\n\n\nb\n"),
            (
                "paragraph opening with a multi-byte char",
                "\u{65e5}\u{672c}\n\nend\n",
            ),
            ("whitespace-only trailing line", "a\n\n   \t \n"),
        ];
        for (label, text) in fixtures {
            let snapshot = snapshot_of(text);
            for start in 0..=text.len() {
                if !text.is_char_boundary(start) {
                    continue;
                }
                for forward in [true, false] {
                    let outcome = ParagraphMotion { forward }.outcome(&snapshot, start);
                    match outcome {
                        ParagraphOutcome::Landed(offset) => {
                            assert!(snapshot.is_byte_boundary(offset), "{label} at {start}");
                            assert_ne!(offset, start, "{label} at {start}: claimed no move");
                            // A landing is on the line after a blank separator.
                            let line = cursor_line(&snapshot, offset);
                            assert!(
                                line == 0 || line_is_blank(&snapshot, line - 1),
                                "{label} at {start} landed on line {line}, which no blank precedes"
                            );
                        }
                        ParagraphOutcome::NoParagraph => {}
                        ParagraphOutcome::Bounded => {
                            panic!("{label} at {start} exhausted the budget")
                        }
                    }
                }
            }
        }
    }

    /// The `gs` precedent for a genuinely multi-megabyte line, at the same 4
    /// MiB the frame-cost tests use.
    ///
    /// `MAX_PARAGRAPH_SCAN_BYTES` is 256 KiB, so a 4 MiB indented line cannot
    /// be read in full by any scan. It must still be *classified* — as content,
    /// never as a separator — and the walk must stay inside its budget.
    #[test]
    fn a_four_mib_indented_line_is_content_and_the_walk_stays_bounded() {
        let huge = " ".repeat(4 * 1024 * 1024);
        // A blank line before `tail`, so `tail` is a *second* paragraph rather
        // than the tail of the oversized one — adjacent lines are one
        // paragraph, and a fixture without the separator would have been
        // testing that fact instead of this one.
        let text = format!("head\n\n{huge}\n\ntail\n");
        let snapshot = snapshot_of(&text);
        assert!(
            !line_is_blank(&snapshot, 2),
            "a 4 MiB indented line is not blank"
        );
        match next_paragraph_start(&snapshot, 0) {
            ParagraphOutcome::Landed(offset) => assert_eq!(
                cursor_line(&snapshot, offset),
                2,
                "landed on the oversized line"
            ),
            other => panic!("expected a landing, got {other:?}"),
        }
        // From inside the oversized line, the next paragraph is `tail` — the walk
        // costs one 32 KiB window to classify the line it leaves, not four
        // megabytes. Exhausting the whole-walk budget needs a document shaped
        // to make the scanner walk and keep walking, which is
        // `an_exhausted_budget_is_a_third_answer_not_a_refusal`'s job.
        assert_eq!(
            next_paragraph_start(&snapshot, 6 + huge.len()),
            ParagraphOutcome::Landed(line_start(&snapshot, 4).expect("`tail`")),
            "the walk reaches `tail` without reading all of the oversized line"
        );
    }

    /// Offsets are asserted against literals, not against the scanner.
    #[test]
    fn the_paragraph_scanners_resolve_the_documented_offsets() {
        let snapshot = snapshot_of("alpha\n\n  beta\n\ngamma\n");
        //                 0123456 7 8901234 5 6 7890123 4
        assert_eq!(
            next_paragraph_start(&snapshot, 0),
            ParagraphOutcome::Landed(9)
        );
        assert_eq!(
            next_paragraph_start(&snapshot, 9),
            ParagraphOutcome::Landed(15)
        );
        assert_eq!(
            next_paragraph_start(&snapshot, 15),
            ParagraphOutcome::NoParagraph
        );
        assert_eq!(
            prev_paragraph_start(&snapshot, 15),
            ParagraphOutcome::Landed(9)
        );
        assert_eq!(
            prev_paragraph_start(&snapshot, 9),
            ParagraphOutcome::Landed(0)
        );
        assert_eq!(
            prev_paragraph_start(&snapshot, 0),
            ParagraphOutcome::NoParagraph
        );
    }

    /// Every start offset is a legal place to stand, and from every one of
    /// them each direction either moves or reports — never stands still.
    #[test]
    fn a_paragraph_motion_never_succeeds_without_moving() {
        let text = "alpha\n\n  beta\n\ngamma\n";
        let snapshot = snapshot_of(text);
        for start in 0..=text.len() {
            if !text.is_char_boundary(start) {
                continue;
            }
            for forward in [true, false] {
                let outcome = ParagraphMotion { forward }.outcome(&snapshot, start);
                match outcome {
                    ParagraphOutcome::Landed(offset) => {
                        assert_ne!(offset, start, "claimed to land at {start} from {start}")
                    }
                    other => assert!(
                        ParagraphMotion { forward }.note(other).is_some(),
                        "{other:?} must carry a disclosure"
                    ),
                }
            }
        }
    }

    /// Running out of budget is reported as *unknown*, never as "no paragraph".
    #[test]
    fn an_exhausted_budget_is_a_third_answer_not_a_refusal() {
        let wide = "x".repeat(4096);
        let text = format!("{wide}\n").repeat(128);
        assert!(text.len() > MAX_PARAGRAPH_SCAN_BYTES);
        let snapshot = snapshot_of(&text);
        assert_eq!(
            next_paragraph_start(&snapshot, 0),
            ParagraphOutcome::Bounded
        );
        let bounded = ParagraphMotion { forward: true }.outcome(&snapshot, 0);
        assert_eq!(
            ParagraphMotion { forward: true }.note(bounded).unwrap(),
            ParagraphNote::ScanBounded
        );
        let label = ParagraphNote::ScanBounded.label().expect("words");
        assert!(
            label.contains("not known"),
            "an unknown must not read like a refusal: {label:?}"
        );
        // Every other outcome must be distinguishable from it.
        assert_ne!(
            ParagraphNote::NoNextParagraph.label().unwrap(),
            ParagraphNote::ScanBounded.label().unwrap()
        );
    }

    /// A huge whitespace-only line cannot be *proved* blank within the read
    /// budget, and calling it blank would invent a paragraph boundary.
    #[test]
    fn an_unprovably_blank_line_counts_as_content() {
        let huge = " ".repeat(MAX_RENDER_LINE_BYTES * 2);
        let text = format!("head\n\n{huge}\n");
        let snapshot = snapshot_of(&text);
        assert!(!line_is_blank(&snapshot, 2));
        let landed = next_paragraph_start(&snapshot, 0);
        let offset = match landed {
            ParagraphOutcome::Landed(offset) => offset,
            other => panic!("expected a landing, got {other:?}"),
        };
        // Landing 32 KiB into an all-whitespace line is the same rule `gs` uses
        // and is fine here: what matters is *which line* it is on.
        assert_eq!(
            cursor_line(&snapshot, offset),
            2,
            "the oversized line, not `head`"
        );
    }

    /// The paragraph classifier and the find classifier must be disjoint, and
    /// between them exhaustive of the repeatable motions — otherwise a command
    /// could be repeatable by one rule and forgotten by the other.
    #[test]
    fn the_paragraph_classifier_agrees_with_the_find_classifier() {
        for command in [EditorCommand::NextParagraph, EditorCommand::PrevParagraph] {
            assert!(command.is_paragraph_motion(), "{command:?}");
            assert!(!command.is_find_motion(), "{command:?} is not a find");
            assert_eq!(
                command.paragraph_motion().unwrap().command(),
                command,
                "{command:?} must round-trip through its classifier"
            );
        }
        // An edit is not a motion, so it must be repeatable by neither
        // classifier — which is what keeps `]Space` then `Alt-.` from
        // inserting a second newline.
        for command in [
            EditorCommand::AddNewlineBelow,
            EditorCommand::AddNewlineAbove,
            EditorCommand::OpenLineBelow,
            EditorCommand::OpenLineAbove,
        ] {
            assert!(!command.is_paragraph_motion(), "{command:?}");
            assert!(!command.is_find_motion(), "{command:?}");
            assert_eq!(command.paragraph_motion(), None, "{command:?}");
            assert_eq!(command.find_motion(), None, "{command:?}");
        }
    }

    /// `o`/`O` and `]Space`/`[Space` insert the same text into the same place
    /// and differ only in `stay_insert` — so a regression that swapped them
    /// would show here as a changed mode, not a changed document.
    #[test]
    fn open_and_add_newline_agree_on_text_and_differ_on_mode() {
        let snapshot = snapshot_of("one\ntwo\n");
        let presentation = presentation_at(0);
        let open = resolve_edit(&snapshot, &presentation, EditorCommand::OpenLineBelow)
            .expect("o resolves");
        let add = resolve_edit(&snapshot, &presentation, EditorCommand::AddNewlineBelow)
            .expect("]Space resolves");
        // The forward transaction is the insertion itself, so equality here is
        // "same edit to the document", which is the half the two verbs share.
        assert_eq!(open.transaction, add.transaction, "same insertion");
        // The two things that are *not* shared: `o` takes the keyboard and
        // leaves the cursor on the line it opened.
        assert!(open.stay_insert, "`o` enters Insert");
        assert!(!add.stay_insert, "`]Space` does not");
        assert_eq!(open.cursor_after, 5, "on the line `o` opened");
        assert_eq!(add.cursor_after, 0, "wherever the cursor already was");
    }
}
