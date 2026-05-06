use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use serde::Deserialize;
use std::sync::Arc;

use crate::tui::app::TuiMsg;
use crate::tui::components::component::{Component, DialogType};
use crate::tui::theme::Theme;

#[derive(Debug, Clone, Deserialize)]
pub struct QuestionSpec {
    pub question: String,
    pub options: Option<Vec<String>>,
    pub initial: Option<String>,
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
    pub fn new(questions: Vec<QuestionSpec>) -> Self {
        let answers = questions
            .iter()
            .map(|q| q.initial.clone().unwrap_or_default())
            .collect();
        Self {
            questions,
            answers,
            selected_question: 0,
            current_input: String::new(),
            cursor_pos: 0,
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
        self.cursor_pos += 1;
        self.answers[self.selected_question] = self.current_input.clone();
    }

    pub fn backspace(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
            self.current_input.remove(self.cursor_pos);
            self.answers[self.selected_question] = self.current_input.clone();
        }
    }

    pub fn delete(&mut self) {
        if self.cursor_pos < self.current_input.len() {
            self.current_input.remove(self.cursor_pos);
            self.answers[self.selected_question] = self.current_input.clone();
        }
    }

    pub fn cursor_left(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
        }
    }

    pub fn cursor_right(&mut self) {
        if self.cursor_pos < self.current_input.len() {
            self.cursor_pos += 1;
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
                .title(" Questions (↑↓ navigate, type to answer, Enter submit) ")
                .border_style(Style::default().fg(theme.border)),
        );
        frame.render_widget(questions_para, chunks[1]);

        let hint = Paragraph::new(Line::from(Span::styled(
            format!(
                "Question {}/{}  |  Enter: submit answers",
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
                .title(" Questions (↑↓ navigate, type to answer, Enter submit) ")
                .border_style(Style::default().fg(theme.border)),
        );
        frame.render_widget(questions_para, chunks[1]);

        let hint = Paragraph::new(Line::from(Span::styled(
            format!(
                "Question {}/{}  |  Enter: submit answers",
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
