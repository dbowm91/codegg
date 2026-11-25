use codegg_protocol::core::{EditorInlineOutcome, ReasoningOverride};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::composer_text::ComposerText;

pub const MAX_EDITOR_INLINE_PAYLOAD_BYTES: usize = 8 * 1024;

pub const MAX_EDITOR_INLINE_PAYLOAD_LINES: usize = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlinePromptMode {
    Normal,
    Insert,
}

impl InlinePromptMode {
    pub fn badge(self) -> &'static str {
        match self {
            Self::Normal => "NOR",
            Self::Insert => "INS",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InlinePromptChoices {
    pub model: Option<String>,
    pub reasoning: Option<ReasoningOverride>,
}

#[derive(Debug, Clone)]
pub struct InlinePromptField {
    instruction: ComposerText,
    mode: InlinePromptMode,
    choices: InlinePromptChoices,
    scroll: usize,
    prefix: Option<char>,
}

impl Default for InlinePromptField {
    fn default() -> Self {
        Self {
            instruction: ComposerText::new(),
            mode: InlinePromptMode::Insert,
            choices: InlinePromptChoices::default(),
            scroll: 0,
            prefix: None,
        }
    }
}

impl InlinePromptField {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mode(&self) -> InlinePromptMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: InlinePromptMode) {
        self.mode = mode;
    }

    pub fn instruction(&self) -> &str {
        self.instruction.text()
    }

    pub fn is_empty(&self) -> bool {
        self.instruction.is_empty()
    }

    pub fn caret(&self) -> usize {
        self.instruction.caret()
    }

    pub fn caret_line(&self) -> usize {
        self.instruction.caret_line()
    }

    pub fn line_count(&self) -> usize {
        self.instruction.line_count()
    }

    pub fn lines(&self) -> Vec<&str> {
        self.instruction.text().lines().collect()
    }

    pub fn choices(&self) -> &InlinePromptChoices {
        &self.choices
    }

    pub fn scroll(&self) -> usize {
        self.scroll
    }

    pub fn set_scroll(&mut self, scroll: usize) {
        self.scroll = scroll;
    }

    pub fn model_label(&self) -> &str {
        self.choices.model.as_deref().unwrap_or("inherit")
    }

    pub fn effort_label(&self) -> String {
        match &self.choices.reasoning {
            None => "inherit".to_string(),
            Some(ReasoningOverride::Effort { level }) => level.clone(),
            Some(ReasoningOverride::Budget { tokens }) => format!("{tokens} tok"),
            Some(ReasoningOverride::Off) => "off".to_string(),
        }
    }

    pub fn cycle_model(&mut self, options: &[String]) {
        self.choices.model = next_choice(options, self.choices.model.clone());
    }

    pub fn cycle_reasoning(&mut self, options: &[String]) {
        let current = match &self.choices.reasoning {
            Some(ReasoningOverride::Effort { level }) => Some(level.clone()),
            _ => None,
        };
        let next = next_choice(options, current);
        self.choices.reasoning = next.map(|level| ReasoningOverride::Effort { level });
    }

    pub fn clear(&mut self) {
        self.instruction.clear();
        self.scroll = 0;
        self.prefix = None;
    }

    pub fn set_instruction(&mut self, instruction: String) {
        self.instruction.set_text(instruction);
        self.scroll = 0;
    }
}

fn next_choice(options: &[String], current: Option<String>) -> Option<String> {
    if options.is_empty() {
        return None;
    }
    let position = match &current {
        None => return Some(options[0].clone()),
        Some(current) => options
            .iter()
            .position(|option| option == current)
            .map(|index| index + 1)
            .unwrap_or(0),
    };
    if position >= options.len() {
        None
    } else {
        Some(options[position].clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineAssistRegion {
    pub generated_text: String,
    pub anchor: u64,
    pub document_id: String,
    pub document_revision: u64,
    pub request_id: String,
    pub model: String,
    pub effort: Option<String>,
    pub instruction: String,
}

impl InlineAssistRegion {
    pub fn height(&self) -> usize {
        self.generated_text.lines().count().max(1)
    }

    pub fn within_budget(&self) -> bool {
        self.generated_text.len() <= MAX_EDITOR_INLINE_PAYLOAD_BYTES
            && self.height() <= MAX_EDITOR_INLINE_PAYLOAD_LINES
    }

    pub fn applies_to(&self, document_id: &str, daemon_revision: u64) -> bool {
        self.within_budget()
            && self.document_id == document_id
            && self.document_revision == daemon_revision
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineAssistRequest {
    pub request_id: String,
    pub document_id: String,
    pub document_revision: u64,
    pub generation: u64,
    pub anchor: u64,
    pub instruction: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlineAnswerOutcome {
    Accepted,
    Superseded,
    StaleGeneration,
    StaleRevision,
    OverBudget,
    Refused(String),
}

impl InlineAnswerOutcome {
    pub fn is_accepted(&self) -> bool {
        matches!(self, Self::Accepted)
    }
}

#[derive(Debug, Clone, Default)]
pub struct InlineAssistState {
    field: Option<InlinePromptField>,
    region: Option<InlineAssistRegion>,
    outstanding: Option<InlineAssistRequest>,
}

impl InlineAssistState {
    pub fn field(&self) -> Option<&InlinePromptField> {
        self.field.as_ref()
    }

    pub fn field_mut(&mut self) -> Option<&mut InlinePromptField> {
        self.field.as_mut()
    }

    pub fn is_field_open(&self) -> bool {
        self.field.is_some()
    }

    pub fn open_field(&mut self) -> &mut InlinePromptField {
        let field = InlinePromptField::new();
        self.field = Some(field);
        self.field.as_mut().expect("the field was just installed")
    }

    pub fn dismiss_field(&mut self) -> bool {
        match self.field.as_mut() {
            Some(field) => {
                field.clear();
                self.field = None;
                true
            }
            None => false,
        }
    }

    pub fn outstanding(&self) -> Option<&InlineAssistRequest> {
        self.outstanding.as_ref()
    }

    pub fn is_generating(&self) -> bool {
        self.outstanding.is_some()
    }

    pub fn begin_request(&mut self, request: InlineAssistRequest) {
        self.outstanding = Some(request);
    }

    pub fn cancel_request(&mut self) -> Option<InlineAssistRequest> {
        self.outstanding.take()
    }

    pub fn clear(&mut self) {
        self.field = None;
        self.region = None;
        self.outstanding = None;
    }

    pub fn region(&self) -> Option<&InlineAssistRegion> {
        self.region.as_ref()
    }

    pub fn apply_answer(&mut self, answer: &InlineAnswer, generation: u64) -> InlineAnswerOutcome {
        let Some(request) = self.outstanding.clone() else {
            return InlineAnswerOutcome::Superseded;
        };
        if request.request_id != answer.request_id {
            return InlineAnswerOutcome::Superseded;
        }
        if request.generation != generation {
            self.outstanding = None;
            return InlineAnswerOutcome::StaleGeneration;
        }
        if request.document_id != answer.document_id
            || request.document_revision != answer.document_revision
        {
            self.outstanding = None;
            return InlineAnswerOutcome::StaleRevision;
        }
        let outcome = match &answer.outcome {
            EditorInlineOutcome::Refused { code } => InlineAnswerOutcome::Refused(code.clone()),
            EditorInlineOutcome::Generated {
                model,
                effort,
                text,
            } => {
                let region = InlineAssistRegion {
                    generated_text: text.clone(),
                    anchor: request.anchor,
                    document_id: request.document_id.clone(),
                    document_revision: request.document_revision,
                    request_id: request.request_id.clone(),
                    model: model.clone(),
                    effort: effort.clone(),
                    instruction: request.instruction.clone(),
                };
                if !region.within_budget() {
                    InlineAnswerOutcome::OverBudget
                } else {
                    self.region = Some(region);
                    InlineAnswerOutcome::Accepted
                }
            }
        };
        self.outstanding = None;
        outcome
    }

    pub fn region_for_approval(
        &self,
        document_id: &str,
        current_revision: u64,
    ) -> Result<&InlineAssistRegion, InlineApprovalRefusal> {
        let Some(region) = self.region.as_ref() else {
            return Err(InlineApprovalRefusal::NoRegion);
        };
        if region.document_id != document_id {
            return Err(InlineApprovalRefusal::StaleDocument);
        }
        if region.document_revision != current_revision {
            return Err(InlineApprovalRefusal::StaleRevision {
                expected: region.document_revision,
                actual: current_revision,
            });
        }
        Ok(region)
    }

    pub fn discard_region(&mut self) -> bool {
        self.region.take().is_some()
    }

    pub fn cursor_in_region(&self, cursor: usize) -> bool {
        self.region
            .as_ref()
            .is_some_and(|region| region.anchor as usize == cursor)
    }

    pub fn region_applies_to(&self, document_id: &str, daemon_revision: u64) -> bool {
        self.region
            .as_ref()
            .is_some_and(|region| region.applies_to(document_id, daemon_revision))
    }

    pub fn region_is_reviewable(
        &self,
        document_id: &str,
        daemon_revision: u64,
        cursor: usize,
    ) -> bool {
        self.region.as_ref().is_some_and(|region| {
            region.applies_to(document_id, daemon_revision) && region.anchor as usize == cursor
        })
    }

    pub fn reopen_field_with(&mut self, instruction: String) -> bool {
        let Some(region) = self.region.take() else {
            return false;
        };
        if self
            .outstanding
            .as_ref()
            .is_some_and(|request| request.anchor == region.anchor)
        {
            self.outstanding = None;
        }
        let field = self.open_field();
        field.set_instruction(instruction);
        true
    }

    pub fn invalidate_region(&mut self) -> bool {
        self.discard_region()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineAnswer {
    pub request_id: String,
    pub document_id: String,
    pub document_revision: u64,
    pub outcome: EditorInlineOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlineApprovalRefusal {
    NoRegion,
    StaleDocument,
    StaleRevision { expected: u64, actual: u64 },
}

impl InlineApprovalRefusal {
    pub fn message(&self) -> String {
        match self {
            Self::NoRegion => "there is no generated region to approve".to_string(),
            Self::StaleDocument => {
                "the document changed — the generated region belongs to another document".to_string()
            }
            Self::StaleRevision { expected, actual } => format!(
                "the document changed (revision {expected} to {actual}) — the generated region no longer applies"
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlinePromptOutcome {
    Consumed,
    Unhandled,
    Submit,
    Dismiss,
}

pub fn handle_field_key(field: &mut InlinePromptField, key: KeyEvent) -> InlinePromptOutcome {
    match field.mode {
        InlinePromptMode::Insert => handle_insert_key(field, key),
        InlinePromptMode::Normal => handle_normal_key(field, key),
    }
}

fn handle_insert_key(field: &mut InlinePromptField, key: KeyEvent) -> InlinePromptOutcome {
    match key.code {
        KeyCode::Esc => {
            field.set_mode(InlinePromptMode::Normal);
            InlinePromptOutcome::Consumed
        }
        KeyCode::Enter if key.modifiers == KeyModifiers::SHIFT => {
            field.instruction.insert_newline();
            InlinePromptOutcome::Consumed
        }
        KeyCode::Enter => InlinePromptOutcome::Submit,
        KeyCode::Backspace => {
            field.instruction.backspace();
            InlinePromptOutcome::Consumed
        }
        KeyCode::Delete => {
            field.instruction.delete_forward();
            InlinePromptOutcome::Consumed
        }
        KeyCode::Left => {
            field.instruction.move_left();
            InlinePromptOutcome::Consumed
        }
        KeyCode::Right => {
            field.instruction.move_right();
            InlinePromptOutcome::Consumed
        }
        KeyCode::Up => {
            field.instruction.move_line_up();
            InlinePromptOutcome::Consumed
        }
        KeyCode::Down => {
            field.instruction.move_line_down();
            InlinePromptOutcome::Consumed
        }
        KeyCode::Char(ch)
            if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
        {
            field.instruction.insert(&ch.to_string());
            InlinePromptOutcome::Consumed
        }
        _ => InlinePromptOutcome::Unhandled,
    }
}

fn handle_normal_key(field: &mut InlinePromptField, key: KeyEvent) -> InlinePromptOutcome {
    match key.code {
        KeyCode::Esc => InlinePromptOutcome::Dismiss,
        KeyCode::Enter => InlinePromptOutcome::Submit,
        KeyCode::Left => {
            field.instruction.move_left();
            InlinePromptOutcome::Consumed
        }
        KeyCode::Right => {
            field.instruction.move_right();
            InlinePromptOutcome::Consumed
        }
        KeyCode::Up => {
            field.instruction.move_line_up();
            InlinePromptOutcome::Consumed
        }
        KeyCode::Down => {
            field.instruction.move_line_down();
            InlinePromptOutcome::Consumed
        }
        KeyCode::Char(ch) => handle_normal_char(field, ch, key.modifiers),
        _ => InlinePromptOutcome::Unhandled,
    }
}

fn handle_normal_char(
    field: &mut InlinePromptField,
    ch: char,
    modifiers: KeyModifiers,
) -> InlinePromptOutcome {
    if modifiers != KeyModifiers::NONE && modifiers != KeyModifiers::SHIFT {
        return InlinePromptOutcome::Unhandled;
    }
    if let Some(prefix) = field.prefix.take() {
        return match (prefix, ch) {
            ('g', 'g') => {
                field.instruction.move_caret(0);
                InlinePromptOutcome::Consumed
            }
            _ => InlinePromptOutcome::Consumed,
        };
    }
    match ch {
        'h' => field.instruction.move_left(),
        'l' => field.instruction.move_right(),
        'j' => field.instruction.move_line_down(),
        'k' => field.instruction.move_line_up(),
        'w' => field.instruction.move_word_forward(),
        'b' => field.instruction.move_word_backward(),
        'e' => field.instruction.move_word_end(),
        '0' => field.instruction.move_home(),
        '$' => field.instruction.move_end(),
        'g' => field.prefix = Some('g'),
        'i' => {
            field.set_mode(InlinePromptMode::Insert);
            return InlinePromptOutcome::Consumed;
        }
        'a' => {
            field.instruction.move_right();
            field.set_mode(InlinePromptMode::Insert);
            return InlinePromptOutcome::Consumed;
        }
        'o' => {
            field.instruction.open_line_below();
            field.set_mode(InlinePromptMode::Insert);
            return InlinePromptOutcome::Consumed;
        }
        'v' => field.instruction.toggle_select_chars(),
        'V' => field.instruction.select_line(),
        'd' => {
            if !field.instruction.delete_selection() {
                field.instruction.select_char();
                field.instruction.delete_selection();
            }
        }
        'c' => {
            field.instruction.delete_selection();
            field.set_mode(InlinePromptMode::Insert);
            return InlinePromptOutcome::Consumed;
        }
        'u' => {
            field.instruction.undo();
        }
        'r' => {
            field.instruction.redo();
        }
        _ => return InlinePromptOutcome::Unhandled,
    }
    InlinePromptOutcome::Consumed
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_protocol::core::EditorInlineOutcome;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn typed(field: &mut InlinePromptField, text: &str) {
        for ch in text.chars() {
            let outcome = handle_field_key(field, key(KeyCode::Char(ch)));
            assert_eq!(outcome, InlinePromptOutcome::Consumed);
        }
    }

    fn request(id: &str) -> InlineAssistRequest {
        InlineAssistRequest {
            request_id: id.to_string(),
            document_id: "doc-1".to_string(),
            document_revision: 7,
            generation: 3,
            anchor: 42,
            instruction: "write a function".to_string(),
        }
    }

    fn answer(id: &str, text: &str) -> InlineAnswer {
        InlineAnswer {
            request_id: id.to_string(),
            document_id: "doc-1".to_string(),
            document_revision: 7,
            outcome: EditorInlineOutcome::Generated {
                model: "m".to_string(),
                effort: None,
                text: text.to_string(),
            },
        }
    }

    #[test]
    fn the_field_starts_in_insert_mode_and_takes_typed_text() {
        let mut field = InlinePromptField::new();
        assert_eq!(field.mode(), InlinePromptMode::Insert);
        typed(&mut field, "hi");
        assert_eq!(field.instruction(), "hi");
        assert_eq!(field.caret(), 2);
    }

    #[test]
    fn esc_steps_to_normal_mode_and_a_second_esc_dismisses() {
        let mut field = InlinePromptField::new();
        typed(&mut field, "hi");
        assert_eq!(
            handle_field_key(&mut field, key(KeyCode::Esc)),
            InlinePromptOutcome::Consumed
        );
        assert_eq!(field.mode(), InlinePromptMode::Normal);
        assert_eq!(
            handle_field_key(&mut field, key(KeyCode::Esc)),
            InlinePromptOutcome::Dismiss
        );
    }

    #[test]
    fn enter_submits_and_shift_enter_starts_a_new_line() {
        let mut field = InlinePromptField::new();
        typed(&mut field, "one");
        assert_eq!(
            handle_field_key(
                &mut field,
                KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)
            ),
            InlinePromptOutcome::Consumed
        );
        assert_eq!(field.line_count(), 2);
        assert_eq!(
            handle_field_key(&mut field, key(KeyCode::Enter)),
            InlinePromptOutcome::Submit
        );
    }

    #[test]
    fn the_field_cycles_the_model_and_the_thinking_level() {
        let mut field = InlinePromptField::new();
        let models = ["a".to_string(), "b".to_string()];
        field.cycle_model(&models);
        assert_eq!(field.choices().model.as_deref(), Some("a"));
        field.cycle_model(&models);
        assert_eq!(field.choices().model.as_deref(), Some("b"));
        field.cycle_model(&models);
        assert_eq!(field.choices().model, None);
        let efforts = ["low".to_string(), "high".to_string()];
        field.cycle_reasoning(&efforts);
        assert_eq!(
            field.choices().reasoning,
            Some(ReasoningOverride::Effort {
                level: "low".to_string()
            })
        );
        field.cycle_reasoning(&efforts);
        assert_eq!(
            field.choices().reasoning,
            Some(ReasoningOverride::Effort {
                level: "high".to_string()
            })
        );
    }

    #[test]
    fn a_current_answer_becomes_the_pending_region() {
        let mut state = InlineAssistState::default();
        state.begin_request(request("r1"));
        let outcome = state.apply_answer(&answer("r1", "fn main() {}"), 3);
        assert_eq!(outcome, InlineAnswerOutcome::Accepted);
        let region = state.region().expect("the region is pending");
        assert_eq!(region.generated_text, "fn main() {}");
        assert_eq!(region.anchor, 42);
        assert_eq!(region.document_revision, 7);
    }

    #[test]
    fn a_superseded_answer_is_dropped() {
        let mut state = InlineAssistState::default();
        state.begin_request(request("r1"));
        let outcome = state.apply_answer(&answer("other", "text"), 3);
        assert_eq!(outcome, InlineAnswerOutcome::Superseded);
        assert!(state.region().is_none());
    }

    #[test]
    fn an_answer_for_a_moved_generation_is_dropped() {
        let mut state = InlineAssistState::default();
        state.begin_request(request("r1"));
        let outcome = state.apply_answer(&answer("r1", "text"), 4);
        assert_eq!(outcome, InlineAnswerOutcome::StaleGeneration);
        assert!(state.region().is_none());
    }

    #[test]
    fn an_answer_for_a_moved_revision_is_dropped() {
        let mut state = InlineAssistState::default();
        state.begin_request(request("r1"));
        let mut stale = answer("r1", "text");
        stale.document_revision = 8;
        let outcome = state.apply_answer(&stale, 3);
        assert_eq!(outcome, InlineAnswerOutcome::StaleRevision);
        assert!(state.region().is_none());
    }

    #[test]
    fn a_second_request_supersedes_the_first() {
        let mut state = InlineAssistState::default();
        state.begin_request(request("r1"));
        state.begin_request(request("r2"));
        let outcome = state.apply_answer(&answer("r1", "text"), 3);
        assert_eq!(outcome, InlineAnswerOutcome::Superseded);
        assert!(state.region().is_none());
    }

    #[test]
    fn a_payload_over_budget_is_refused() {
        let mut state = InlineAssistState::default();
        state.begin_request(request("r1"));
        let oversized = "x".repeat(MAX_EDITOR_INLINE_PAYLOAD_BYTES + 1);
        let outcome = state.apply_answer(&answer("r1", &oversized), 3);
        assert_eq!(outcome, InlineAnswerOutcome::OverBudget);
        assert!(state.region().is_none());
    }

    #[test]
    fn a_refusal_is_named_and_produces_no_region() {
        let mut state = InlineAssistState::default();
        state.begin_request(request("r1"));
        let refused = InlineAnswer {
            request_id: "r1".to_string(),
            document_id: "doc-1".to_string(),
            document_revision: 7,
            outcome: EditorInlineOutcome::Refused {
                code: "unknown_model".to_string(),
            },
        };
        let outcome = state.apply_answer(&refused, 3);
        assert_eq!(
            outcome,
            InlineAnswerOutcome::Refused("unknown_model".to_string())
        );
        assert!(state.region().is_none());
    }

    #[test]
    fn an_approval_refuses_a_region_whose_revision_moved() {
        let mut state = InlineAssistState::default();
        state.begin_request(request("r1"));
        state.apply_answer(&answer("r1", "text"), 3);
        let refusal = state.region_for_approval("doc-1", 8).expect_err("stale");
        assert_eq!(
            refusal,
            InlineApprovalRefusal::StaleRevision {
                expected: 7,
                actual: 8
            }
        );
        assert!(
            state.region().is_some(),
            "a refused approval keeps the region"
        );
    }

    #[test]
    fn an_approval_refuses_a_region_that_belongs_to_another_document() {
        let mut state = InlineAssistState::default();
        state.begin_request(request("r1"));
        state.apply_answer(&answer("r1", "text"), 3);
        let refusal = state
            .region_for_approval("doc-2", 7)
            .expect_err("another document");
        assert_eq!(refusal, InlineApprovalRefusal::StaleDocument);
        assert!(
            state.region().is_some(),
            "a refused approval keeps the region"
        );
    }

    #[test]
    fn a_region_that_no_longer_applies_is_not_reviewable() {
        let mut state = InlineAssistState::default();
        state.begin_request(request("r1"));
        state.apply_answer(&answer("r1", "text"), 3);
        assert!(
            state.region_is_reviewable("doc-1", 7, 42),
            "the region's own document and revision make it reviewable at its anchor"
        );
        assert!(
            !state.region_is_reviewable("doc-1", 8, 42),
            "a moved revision retires the region"
        );
        assert!(
            !state.region_is_reviewable("doc-2", 7, 42),
            "another document retires the region"
        );
        assert!(
            !state.region_is_reviewable("doc-1", 7, 43),
            "a cursor off the anchor opens no menu"
        );
    }

    #[test]
    fn an_approval_takes_the_region_and_a_discard_takes_nothing() {
        let mut state = InlineAssistState::default();
        state.begin_request(request("r1"));
        state.apply_answer(&answer("r1", "text"), 3);
        let region = state
            .region_for_approval("doc-1", 7)
            .expect("the region applies")
            .clone();
        assert_eq!(region.generated_text, "text");
        assert!(
            state.region().is_some(),
            "a readable approval does not consume the region"
        );
        assert!(state.discard_region());
        assert!(state.region().is_none());
        state.begin_request(request("r2"));
        state.apply_answer(&answer("r2", "text"), 3);
        assert!(state.discard_region());
        assert!(state.region().is_none());
        assert!(!state.discard_region());
    }

    #[test]
    fn invalidating_a_region_reports_that_there_was_one() {
        let mut state = InlineAssistState::default();
        state.begin_request(request("r1"));
        state.apply_answer(&answer("r1", "text"), 3);
        assert!(state.invalidate_region(), "the region is dropped");
        assert!(state.region().is_none());
        assert!(
            !state.invalidate_region(),
            "a second invalidation has nothing to drop"
        );
    }

    #[test]
    fn dismissing_the_field_clears_its_instruction() {
        let mut state = InlineAssistState::default();
        let field = state.open_field();
        typed(field, "hi");
        assert!(state.dismiss_field());
        assert!(!state.is_field_open());
        let reopened = state.open_field();
        assert_eq!(reopened.instruction(), "");
    }
}
