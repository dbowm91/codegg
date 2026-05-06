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
    let mut lines = Vec::new();
    let mut old_start = None;
    let mut new_start = None;

    for change in diff.iter_all_changes() {
        let kind = match change.tag() {
            ChangeTag::Delete => DiffKind::Removed,
            ChangeTag::Insert => DiffKind::Added,
            ChangeTag::Equal => DiffKind::Context,
        };

        if old_start.is_none() {
            old_start = change.old_index();
        }
        if new_start.is_none() && change.new_index().is_some() {
            new_start = change.new_index();
        }

        lines.push(DiffLine {
            kind,
            content: change.value().trim_end().to_string(),
        });
    }

    vec![FileDiff {
        path: path.to_string(),
        hunks: vec![DiffHunk {
            old_start: old_start.unwrap_or(0),
            new_start: new_start.unwrap_or(0),
            lines,
        }],
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
