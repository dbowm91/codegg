//! The TUI editor's pending-effect mechanism.
//!
//! Owned by the M010 implementation plan
//! (`plans/implementation/tui-editor-helix-parity/010-space-mode-pickers-and-clipboard-family.md`),
//! work package W1: this module generalises the M013 LSP-typed pending effect
//! into an editor-level one, so a file picker and a clipboard verb are first
//! class pending effects rather than LSP facts.
//!
//! ## The mechanism, unchanged
//!
//! The two halves of a leaf live at different authority levels. The prefix
//! resolver holds `&mut EditorState` and can decide *what* was asked for;
//! only the App layer holds a core client, a task registry and toasts, so it
//! owns *how* the request runs. The resolver parks the effect on
//! [`EditorState::pending_effect`] and `handle_editor_key` drains it on the
//! very next statement.
//!
//! One hop, not a queued channel: a leaf can never leave a request armed, and
//! a document change discards it. That contract is M013's and is preserved
//! exactly. What changed is the *type* of the thing being parked: it was an
//! LSP fact, and it is now an editor fact.

use codegg_protocol::lsp::LspReadOperation;

use crate::tui::document_session::TuiDocumentPresentation;
use crate::tui::editor::{self, EditorEdit};
use crate::tui::editor_lsp::EditorDiagnosticStep;

/// One verb whose payload is a block of text — the clipboard's, or a
/// register's.
///
/// The clipboard is not editor state, so a clipboard-backed paste is reported
/// to the App layer and performed there, in the same division of labour as
/// `yank_editor_range`: the buffer resolves the *target*, the App layer owns
/// the *bytes*.
///
/// M007 split the **source** from the verb without adding a fourth variant.
/// These three describe *where the text lands* — after, before, or in place of
/// the target — and that is all they ever described. Whether the bytes come
/// from the system clipboard (`space p`) or from a register (`p`, or `"ap`) is
/// the App layer's other axis, so a register paste is the same builder with
/// different text rather than a fourth destination. A register is not a place
/// in the document, and adding one here would have made this enum answer two
/// questions.
///
/// Public rather than `pub(crate)` because [`EditorPendingEffect`] is public —
/// it is named by the public `PrefixAction::Effect` arm — and a `pub` type
/// may not hold a `pub(crate)` one without a `private_interfaces` warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardVerb {
    /// `p` — paste after the selection, or after the character under the
    /// cursor.
    PasteAfter,
    /// `P` — paste before the selection, or before the character under the
    /// cursor.
    PasteBefore,
    /// `R` — replace the selection, or the character under the cursor, with
    /// the clipboard's contents.
    Replace,
}

impl ClipboardVerb {
    /// Build this verb's edit against `snapshot` with the clipboard's `text`.
    pub fn build(
        self,
        snapshot: &codegg_document::DocumentSnapshot,
        presentation: &TuiDocumentPresentation,
        text: &str,
    ) -> Option<EditorEdit> {
        match self {
            Self::PasteAfter => editor::paste_after(snapshot, presentation, text),
            Self::PasteBefore => editor::paste_before(snapshot, presentation, text),
            Self::Replace => editor::replace_with_text(snapshot, presentation, text),
        }
    }
}

/// What one completed prefix gesture asks the App layer to do.
///
/// Split out from [`PrefixAction`](crate::tui::editor::PrefixAction) because
/// the two halves of a leaf live at different authority levels. The prefix
/// resolver holds `&mut EditorState` and can decide *what* was asked for; only
/// the App layer holds a core client, a task registry and toasts, so it owns
/// *how* the request runs. The leaf names the effect and the App drains it.
///
/// The type is **editor-level**, not LSP-level. M013's version could only say
/// "ask a language server", which is why a file picker and a clipboard verb
/// had nowhere to live: they are not LSP facts, and `EditorLspState` is
/// reset wholesale on a scope change, which is the wrong fate for an effect
/// that is not about the language server at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorPendingEffect {
    /// Issue one read. Workspace symbols are resolved by the App from the
    /// token under the cursor, exactly as F7 does — a read with no query is
    /// not a request the daemon can answer.
    Read(LspReadOperation),
    /// Refresh and open the diagnostics panel.
    OpenDiagnostics,
    /// Step to the next or previous diagnostic and move the cursor there.
    ///
    /// Not an `LspReadOperation`: nothing is asked of a language server. The
    /// answer is already in hand, in `EditorLspState::diagnostics`. It lives
    /// in this enum because it shares the leaf's plumbing — a prefix gesture
    /// naming an effect the App layer must perform.
    DiagnosticStep(EditorDiagnosticStep),
    /// Run one clipboard verb against the system clipboard.
    ///
    /// The `space` family, and only that family: `space p`/`P`/`R` mean the
    /// clipboard whatever `"` selected. The bare `p`/`P`/`R` chords read a
    /// register instead, which is why they are a `BufferKey` rather than an
    /// effect — see `ClipboardVerb`'s own doc.
    Clipboard(ClipboardVerb),
    /// Yank **only** the primary selection to the system clipboard.
    YankPrimary,
    /// Open the workspace file picker.
    OpenFilePicker,
    /// Open the editor's `:` command palette.
    OpenCommandPalette,
    /// Open the **jumplist picker** — the editor's own history of visited
    /// positions, newest first.
    ///
    /// Not a read: the list is on the presentation already
    /// ([`Jumplist`](crate::tui::document_session::Jumplist)) and nothing is
    /// asked of the daemon. A new variant rather than a reuse of
    /// [`OpenFilePicker`](Self::OpenFilePicker) or the LSP picker: the file
    /// browser lists the workspace and the LSP panel lists a server's answer,
    /// and neither can carry this editor's own byte offsets.
    OpenJumplist,
    /// Toggle the editor's **keymap box** — its own catalogue of its keys,
    /// in the bottom-right corner of the editor.
    OpenKeymap,
    /// Open the **rename prompt** — the `:` line, pre-filled with the rename
    /// command, waiting for the new name.
    ///
    /// Not a read: the read follows the prompt, and the prompt needs a name
    /// the reader has not typed yet. The leaf names the *step*, and the
    /// request is issued when the `:` line is submitted.
    RenamePrompt,
    /// A Helix leaf this build deliberately leaves unbound, with the reason a
    /// user needs.
    ///
    /// Present so the key says *why* rather than doing nothing. An unbound
    /// letter is the M002 failure mode: the user cannot tell a missing
    /// feature from a key that never arrived. The message says which, and the
    /// reason is the same text the docs carry.
    Unavailable(&'static str),
}
