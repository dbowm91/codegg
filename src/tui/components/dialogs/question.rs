use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use serde::{Deserialize, Deserializer};
use std::sync::Arc;

use crate::tui::app::TuiMsg;
use crate::tui::components::component::{Component, DialogType};
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

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        let chunks = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(area);

        let title = Paragraph::new(Line::from(Span::styled(
            "Agent has questions",
            Style::default().fg(theme.warning),
        )))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Question ")
                .border_style(Style::default().fg(theme.warning)),
        );
        frame.render_widget(title, chunks[0]);

        let mut lines = Vec::new();
        for (i, q) in self.questions.iter().enumerate() {
            let prefix = if i == self.selected_question {
                "> "
            } else {
                "  "
            };
            let style = if i == self.selected_question {
                Style::default().fg(theme.primary).bg(theme.selection)
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

        let questions_para = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Questions (↑↓ navigate  |  type answer  |  ←→ cursor  |  Enter submit) ")
                .border_style(Style::default().fg(theme.border)),
        );
        frame.render_widget(questions_para, chunks[1]);

        let hint = Paragraph::new(Line::from(Span::styled(
            format!(
                "Question {}/{}  |  Backspace/Del edit  |  Enter submit  |  Esc close",
                self.selected_question + 1,
                self.questions.len()
            ),
            Style::default().fg(theme.muted),
        )))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border)),
        );
        frame.render_widget(hint, chunks[2]);
    }
}

impl Component for QuestionDialog {
    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Option<TuiMsg> {
        match key.code {
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k') => {
                self.select_up();
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j') => {
                self.select_down();
            }
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

    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
        let chunks = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(area);

        let title = Paragraph::new(Line::from(Span::styled(
            "Agent has questions",
            Style::default().fg(theme.warning),
        )))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Question ")
                .border_style(Style::default().fg(theme.warning)),
        );
        frame.render_widget(title, chunks[0]);

        let mut lines = Vec::new();
        for (i, q) in self.questions.iter().enumerate() {
            let prefix = if i == self.selected_question {
                "> "
            } else {
                "  "
            };
            let style = if i == self.selected_question {
                Style::default().fg(theme.primary).bg(theme.selection)
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

        let questions_para = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Questions (↑↓ navigate  |  type answer  |  ←→ cursor  |  Enter submit) ")
                .border_style(Style::default().fg(theme.border)),
        );
        frame.render_widget(questions_para, chunks[1]);

        let hint = Paragraph::new(Line::from(Span::styled(
            format!(
                "Question {}/{}  |  Backspace/Del edit  |  Enter submit  |  Esc close",
                self.selected_question + 1,
                self.questions.len()
            ),
            Style::default().fg(theme.muted),
        )))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border)),
        );
        frame.render_widget(hint, chunks[2]);
    }

    fn dialog_type(&self) -> DialogType {
        DialogType::Question
    }
}
