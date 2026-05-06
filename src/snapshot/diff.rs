use similar::{ChangeTag, TextDiff};

#[derive(Debug, Clone)]
pub struct FileDiff {
    pub path: String,
    pub hunks: Vec<DiffHunk>,
}

#[derive(Debug, Clone)]
pub struct DiffHunk {
    pub old_start: usize,
    pub new_start: usize,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone)]
pub struct DiffLine {
    pub kind: DiffKind,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DiffKind {
    Context,
    Added,
    Removed,
}

pub fn diff_files(old: &str, new: &str, path: &str) -> Vec<FileDiff> {
    let diff = TextDiff::from_lines(old, new);
    let mut hunks = Vec::new();
    let mut current_hunk_lines: Vec<DiffLine> = Vec::new();
    let mut current_old_start: Option<usize> = None;
    let mut current_new_start: Option<usize> = None;

    let max_context = 3;
    let mut context_buffer: Vec<DiffLine> = Vec::new();
    let mut in_change = false;

    for change in diff.iter_all_changes() {
        let kind = match change.tag() {
            ChangeTag::Delete => DiffKind::Removed,
            ChangeTag::Insert => DiffKind::Added,
            ChangeTag::Equal => DiffKind::Context,
        };

        let this_old = change.old_index().unwrap_or(0);
        let this_new = change.new_index().unwrap_or(0);

        if kind != DiffKind::Context {
            if !context_buffer.is_empty() {
                current_hunk_lines.extend(context_buffer.clone());
                context_buffer.clear();
            }

            if current_old_start.is_none() {
                current_old_start = Some(this_old.saturating_sub(context_buffer.len()));
                current_new_start = Some(this_new.saturating_sub(context_buffer.len()));
            }

            current_hunk_lines.push(DiffLine {
                kind,
                content: change.value().trim_end().to_string(),
            });
            in_change = true;
        } else {
            if in_change {
                context_buffer.push(DiffLine {
                    kind: DiffKind::Context,
                    content: change.value().trim_end().to_string(),
                });

                if context_buffer.len() > max_context * 2 {
                    let final_lines: Vec<DiffLine> = context_buffer
                        .drain(..context_buffer.len() - max_context)
                        .collect();
                    current_hunk_lines.extend(final_lines);

                    if !current_hunk_lines.is_empty() {
                        hunks.push(DiffHunk {
                            old_start: current_old_start.unwrap_or(0),
                            new_start: current_new_start.unwrap_or(0),
                            lines: std::mem::take(&mut current_hunk_lines),
                        });
                    }

                    current_old_start = None;
                    current_new_start = None;
                    in_change = false;
                }
            } else {
                if current_hunk_lines.is_empty() {
                    current_old_start = Some(this_old);
                    current_new_start = Some(this_new);
                }
                context_buffer.push(DiffLine {
                    kind: DiffKind::Context,
                    content: change.value().trim_end().to_string(),
                });
            }
        }
    }

    if !context_buffer.is_empty() {
        current_hunk_lines.extend(context_buffer);
    }

    if !current_hunk_lines.is_empty() {
        hunks.push(DiffHunk {
            old_start: current_old_start.unwrap_or(0),
            new_start: current_new_start.unwrap_or(0),
            lines: current_hunk_lines,
        });
    }

    if hunks.is_empty() {
        hunks.push(DiffHunk {
            old_start: 0,
            new_start: 0,
            lines: Vec::new(),
        });
    }

    vec![FileDiff {
        path: path.to_string(),
        hunks,
    }]
}

pub fn format_unified_diff(old: &str, new: &str, old_path: &str, new_path: &str) -> String {
    let diff = TextDiff::from_lines(old, new);
    let mut result = format!("--- {old_path}\n+++ {new_path}\n");

    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            ChangeTag::Delete => "-",
            ChangeTag::Insert => "+",
            ChangeTag::Equal => " ",
        };
        result.push_str(&format!("{sign}{}\n", change.value().trim_end()));
    }

    result
}