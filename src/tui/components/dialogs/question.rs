use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use serde::{Deserialize, Deserializer};
use std::sync::Arc;

use crate::tui::app::TuiMsg;
use crate::tui::components::component::{Component, DialogType};
use crate::tui::components::hint_line::HintLine;
use crate::tui::components::section::Section;
use crate::tui::theme::Theme;

#[derive(Debug, Clone, Deserialize)]
pub struct QuestionSpec {
    pub question: String,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_options")]
    pub options: Option<Vec<String>>,
    #[serde(default)]
    pub initial: Option<String>,
}

fn deserialize_options<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Debug, Deserialize)]
    #[serde(untagged)]
    enum OptionCompat {
        Text(String),
        Object {
            label: Option<String>,
            value: Option<String>,
            description: Option<String>,
        },
    }

    let raw = Option::<Vec<OptionCompat>>::deserialize(deserializer)?;
    Ok(raw.map(|items| {
        items
            .into_iter()
            .map(|item| match item {
                OptionCompat::Text(text) => text,
                OptionCompat::Object {
                    label,
                    value,
                    description,
                } => label.or(value).or(description).unwrap_or_default(),
            })
            .filter(|s| !s.is_empty())
            .collect()
    }))
}

#[derive(Clone)]
pub struct QuestionDialog {
    pub questions: Vec<QuestionSpec>,
    pub answers: Vec<String>,
    pub selected_question: usize,
    pub current_input: String,
    pub cursor_pos: usize,
}

/// The content rows of the title and hint boxes, and the body's floor.
const TITLE_ROWS: u16 = 1;
const HINT_ROWS: u16 = 1;
const MIN_BODY_ROWS: u16 = 3;

impl QuestionDialog {
    fn prev_char_boundary(s: &str, idx: usize) -> usize {
        let i = idx.min(s.len());
        s[..i]
            .char_indices()
            .next_back()
            .map(|(pos, _)| pos)
            .unwrap_or(0)
    }

    fn next_char_boundary(s: &str, idx: usize) -> usize {
        let i = idx.min(s.len());
        if i >= s.len() {
            s.len()
        } else {
            s[i..]
                .char_indices()
                .nth(1)
                .map(|(off, _)| i + off)
                .unwrap_or(s.len())
        }
    }

    pub fn new(questions: Vec<QuestionSpec>) -> Self {
        let answers: Vec<String> = questions
            .iter()
            .map(|q| q.initial.clone().unwrap_or_default())
            .collect();
        let current_input = answers.first().cloned().unwrap_or_default();
        let cursor_pos = current_input.len();
        Self {
            questions,
            answers,
            selected_question: 0,
            current_input,
            cursor_pos,
        }
    }

    pub fn questions_json(&self) -> String {
        serde_json::to_string(
            &self
                .questions
                .iter()
                .map(|q| {
                    let mut map = serde_json::Map::new();
                    map.insert(
                        "question".to_string(),
                        serde_json::Value::String(q.question.clone()),
                    );
                    if let Some(ref opts) = q.options {
                        map.insert(
                            "options".to_string(),
                            serde_json::Value::Array(
                                opts.iter()
                                    .map(|o| serde_json::Value::String(o.clone()))
                                    .collect(),
                            ),
                        );
                    }
                    if let Some(ref init) = q.initial {
                        map.insert(
                            "initial".to_string(),
                            serde_json::Value::String(init.clone()),
                        );
                    }
                    serde_json::Value::Object(map)
                })
                .collect::<Vec<_>>(),
        )
        .unwrap_or_default()
    }

    pub fn answers_json(&self) -> String {
        serde_json::to_string(
            &self
                .questions
                .iter()
                .zip(self.answers.iter())
                .map(|(q, a)| {
                    let mut map = serde_json::Map::new();
                    map.insert(
                        "question".to_string(),
                        serde_json::Value::String(q.question.clone()),
                    );
                    map.insert("answer".to_string(), serde_json::Value::String(a.clone()));
                    serde_json::Value::Object(map)
                })
                .collect::<Vec<_>>(),
        )
        .unwrap_or_default()
    }

    pub fn select_up(&mut self) {
        if self.selected_question > 0 {
            self.selected_question -= 1;
            self.current_input = self.answers[self.selected_question].clone();
            self.cursor_pos = self.current_input.len();
        }
    }

    pub fn select_down(&mut self) {
        if self.selected_question + 1 < self.questions.len() {
            self.selected_question += 1;
            self.current_input = self.answers[self.selected_question].clone();
            self.cursor_pos = self.current_input.len();
        }
    }

    pub fn set_answer(&mut self, ch: char) {
        self.current_input.insert(self.cursor_pos, ch);
        self.cursor_pos += ch.len_utf8();
        self.answers[self.selected_question] = self.current_input.clone();
    }

    pub fn backspace(&mut self) {
        if self.cursor_pos > 0 {
            let prev = Self::prev_char_boundary(&self.current_input, self.cursor_pos);
            self.current_input.drain(prev..self.cursor_pos);
            self.cursor_pos = prev;
            self.answers[self.selected_question] = self.current_input.clone();
        }
    }

    pub fn delete(&mut self) {
        if self.cursor_pos < self.current_input.len() {
            let next = Self::next_char_boundary(&self.current_input, self.cursor_pos);
            self.current_input.drain(self.cursor_pos..next);
            self.answers[self.selected_question] = self.current_input.clone();
        }
    }

    pub fn cursor_left(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos = Self::prev_char_boundary(&self.current_input, self.cursor_pos);
        }
    }

    pub fn cursor_right(&mut self) {
        if self.cursor_pos < self.current_input.len() {
            self.cursor_pos = Self::next_char_boundary(&self.current_input, self.cursor_pos);
        }
    }

    pub fn select_option(&mut self, idx: usize) {
        if let Some(opts) = &self.questions[self.selected_question].options {
            if idx < opts.len() {
                self.answers[self.selected_question] = opts[idx].clone();
                self.current_input = opts[idx].clone();
                self.cursor_pos = self.current_input.len();
            }
        }
    }

    /// The rows the body shows before wrapping: one per question, plus the
    /// selected question's answer line and its options.
    fn body_rows(&self) -> u16 {
        let selected_extra = self
            .questions
            .get(self.selected_question)
            .map(|q| 1 + q.options.as_ref().map_or(0, Vec::len))
            .unwrap_or(0);
        u16::try_from(self.questions.len() + selected_extra).unwrap_or(u16::MAX)
    }

    /// The question rows, in the one spelling the box and its height share.
    fn body_lines(&self, theme: &Theme) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        for (i, q) in self.questions.iter().enumerate() {
            let prefix = if i == self.selected_question {
                "> "
            } else {
                "  "
            };
            // `selection_text` is the token derived to read on `selection`
            // (`primary` is an accent and fails that fill on 14 themes).
            let style = if i == self.selected_question {
                Style::default()
                    .fg(theme.selection_text)
                    .bg(theme.selection)
            } else {
                Style::default().fg(theme.foreground)
            };
            lines.push(Line::from(Span::styled(
                format!("{prefix}{}", q.question),
                style,
            )));
            if i == self.selected_question {
                let answer_style = Style::default().fg(theme.muted);
                let answer_text = if self.answers[i].is_empty() {
                    "(type answer...)".to_string()
                } else {
                    self.answers[i].clone()
                };
                lines.push(Line::from(Span::styled(
                    format!("    Answer: {answer_text}"),
                    answer_style,
                )));
                if let Some(ref opts) = q.options {
                    for (j, opt) in opts.iter().enumerate() {
                        let opt_prefix = if self.answers[i] == *opt { "* " } else { "  " };
                        lines.push(Line::from(Span::styled(
                            format!("      {opt_prefix}[{j}] {opt}"),
                            Style::default().fg(theme.muted),
                        )));
                    }
                }
            }
        }
        lines
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        let chunks = Layout::vertical([
            Constraint::Length(Section::owned_height(TITLE_ROWS)),
            Constraint::Min(Section::owned_height(MIN_BODY_ROWS)),
            Constraint::Length(Section::owned_height(HINT_ROWS)),
        ])
        .split(area);

        let title_inner = Section::new("Question", theme)
            .focused(true)
            .draw(chunks[0], frame.buffer_mut());
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Agent has questions",
                Style::default().fg(theme.warning),
            ))),
            title_inner,
        );

        let lines = self.body_lines(theme);

        let questions_inner = Section::plain(theme)
            .hint(
                &HintLine::from_items(&[
                    ("↑↓", "navigate"),
                    ("type", "answer"),
                    ("←→", "cursor"),
                    ("Enter", "submit"),
                ])
                .render(0),
            )
            .draw(chunks[1], frame.buffer_mut());
        frame.render_widget(Paragraph::new(lines), questions_inner);

        let hint_inner = Section::plain(theme).draw(chunks[2], frame.buffer_mut());
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                {
                    let mut hint = HintLine::new();
                    hint.push_required("Esc", "close")
                        .push("Backspace/Del", "edit")
                        .push("Enter", "submit")
                        .push(
                            &format!(
                                "Question {}/{}",
                                self.selected_question + 1,
                                self.questions.len()
                            ),
                            "",
                        );
                    hint.render(0)
                },
                Style::default().fg(theme.muted),
            ))),
            hint_inner,
        );
    }
}

impl Component for QuestionDialog {
    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Option<TuiMsg> {
        match key.code {
            crossterm::event::KeyCode::Up => {
                self.select_up();
            }
            crossterm::event::KeyCode::Down => {
                self.select_down();
            }
            // Unlike a filterable list, this dialog has no selection list:
            // `select_up`/`select_down` only move between questions, which
            // Up/Down already do unambiguously. `j`/`k` therefore stay
            // ordinary characters here so a free-form answer remains fully
            // typeable — including a leading j or k.
            crossterm::event::KeyCode::Char('k') => self.set_answer('k'),
            crossterm::event::KeyCode::Char('j') => self.set_answer('j'),
            crossterm::event::KeyCode::Char(ch) => {
                self.set_answer(ch);
            }
            crossterm::event::KeyCode::Backspace => {
                self.backspace();
            }
            crossterm::event::KeyCode::Delete => {
                self.delete();
            }
            crossterm::event::KeyCode::Left => {
                self.cursor_left();
            }
            crossterm::event::KeyCode::Right => {
                self.cursor_right();
            }
            crossterm::event::KeyCode::Enter => {
                return Some(TuiMsg::SubmitQuestionAnswers {
                    answers_json: self.answers_json(),
                });
            }
            crossterm::event::KeyCode::Esc => {
                self.answers.clear();
                return Some(TuiMsg::CloseDialog);
            }
            _ => {}
        }
        None
    }

    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg> {
        match msg {
            TuiMsg::CloseDialog => Some(TuiMsg::CloseDialog),
            _ => None,
        }
    }

    fn preferred_area(&self, available: Rect) -> Rect {
        let rows = Section::owned_height(TITLE_ROWS)
            + Section::owned_height(self.body_rows().max(MIN_BODY_ROWS))
            + Section::owned_height(HINT_ROWS);
        crate::tui::components::dialog_size::preferred_rows(available, rows)
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        QuestionDialog::render(self, frame, area, theme);
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Question
    }
}

#[cfg(test)]
mod vim_key_tests {
    use super::*;
    use crate::tui::components::component::Component;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn dialog(selected: usize, input: &str) -> QuestionDialog {
        QuestionDialog {
            questions: Vec::new(),
            answers: vec![String::new(), "second".to_string()],
            selected_question: selected,
            current_input: input.to_string(),
            cursor_pos: input.len(),
        }
    }

    #[test]
    fn vim_keys_type_into_a_non_empty_answer() {
        // j/k must reach the answer box once the user has started typing.
        let mut d = dialog(1, "jack");
        d.handle_key(key('j'));
        assert_eq!(
            d.current_input, "jackj",
            "j must be typeable into an answer that already has content"
        );
        assert_eq!(
            d.selected_question, 1,
            "typing must not move between questions"
        );
    }

    #[test]
    fn vim_keys_type_into_an_empty_answer() {
        // A leading j/k must be typeable too. There is no selection list
        // here for the letters to navigate, so they must never be captured.
        let mut d = dialog(0, "");
        d.handle_key(key('j'));
        assert_eq!(
            d.current_input, "j",
            "j must be typeable when starting an answer"
        );
        d.handle_key(key('k'));
        assert_eq!(d.current_input, "jk");
    }

    fn spec(question: &str) -> QuestionSpec {
        QuestionSpec {
            question: question.to_string(),
            options: None,
            initial: None,
        }
    }

    #[test]
    fn arrow_keys_still_move_between_questions() {
        // Up/Down remain the unambiguous way to change question, which is
        // exactly what j/k used to duplicate in this dialog.
        let mut d = QuestionDialog::new(vec![spec("first"), spec("second")]);
        d.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(
            d.selected_question, 1,
            "Down must still move between questions"
        );
    }
}
