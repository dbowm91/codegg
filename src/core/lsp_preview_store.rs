//! The daemon's own registry of the preview candidates it minted. The
//! bound is a count: the oldest entry is dropped when it is reached.

use codegg_protocol::lsp::LspPreviewCandidateDto;

/// How many minted candidates the daemon keeps.
const MAX_ENTRIES: usize = 64;

/// The daemon's registry of the preview candidates it minted.
///
/// A candidate is readable only by the session that minted it, so every
/// entry carries the session that owns it.
#[derive(Default)]
pub struct LspPreviewStore {
    entries: parking_lot::Mutex<Vec<Entry>>,
}

/// One minted candidate, bound to the session that minted it.
struct Entry {
    candidate: LspPreviewCandidateDto,
    session_id: String,
}

impl LspPreviewStore {
    /// Record a candidate the daemon just minted for this session.
    pub fn register(&self, session_id: &str, candidate: LspPreviewCandidateDto) {
        let mut entries = self.entries.lock();
        entries.retain(|entry| entry.candidate.preview_id != candidate.preview_id);
        entries.push(Entry {
            candidate,
            session_id: session_id.to_owned(),
        });
        while entries.len() > MAX_ENTRIES {
            entries.remove(0);
        }
    }

    /// The candidate this session minted under this id.
    pub fn get(&self, session_id: &str, preview_id: &str) -> Option<LspPreviewCandidateDto> {
        self.entries
            .lock()
            .iter()
            .find(|entry| {
                entry.session_id == session_id && entry.candidate.preview_id == preview_id
            })
            .map(|entry| entry.candidate.clone())
    }
}
