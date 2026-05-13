pub fn truncate_lines(text: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= max_lines {
        return text.to_string();
    }
    let half = max_lines / 2;
    let mut result = lines[..half].join("\n");
    result.push_str(&format!(
        "\n\n... [{} lines truncated] ...\n\n",
        lines.len() - max_lines
    ));
    result.push_str(&lines[lines.len() - half..].join("\n"));
    result
}

pub fn truncate_bytes(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let safe_end = text
        .char_indices()
        .map(|(i, _)| i)
        .take_while(|&i| i <= max_bytes)
        .last()
        .unwrap_or(0);
    format!("{}... [truncated]", &text[..safe_end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_lines_no_truncation() {
        let text = "line1\nline2\nline3";
        let result = truncate_lines(text, 10);
        assert_eq!(result, text);
    }

    #[test]
    fn test_truncate_lines_truncates() {
        let text = (1..=10)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let result = truncate_lines(&text, 4);
        assert!(result.contains("truncated"));
        assert!(result.starts_with("line1"));
        assert!(result.ends_with("line10"));
    }

    #[test]
    fn test_truncate_lines_keeps_head_and_tail() {
        let text = (1..=10)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let result = truncate_lines(&text, 4);
        assert!(result.contains("line1"));
        assert!(result.contains("line2"));
        assert!(result.contains("line9"));
        assert!(result.contains("line10"));
    }

    #[test]
    fn test_truncate_lines_empty() {
        let result = truncate_lines("", 10);
        assert_eq!(result, "");
    }

    #[test]
    fn test_truncate_lines_single() {
        let result = truncate_lines("single", 10);
        assert_eq!(result, "single");
    }

    #[test]
    fn test_truncate_bytes_no_truncation() {
        let text = "hello world";
        let result = truncate_bytes(text, 20);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_truncate_bytes_truncates() {
        let text = "hello world";
        let result = truncate_bytes(text, 5);
        assert_eq!(result, "hello... [truncated]");
    }

    #[test]
    fn test_truncate_bytes_exact() {
        let text = "hello";
        let result = truncate_bytes(text, 5);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_truncate_bytes_empty() {
        let result = truncate_bytes("", 10);
        assert_eq!(result, "");
    }

    #[test]
    fn test_truncate_bytes_utf8_boundary_safe() {
        let text = "éclair";
        let result = truncate_bytes(text, 1);
        assert_eq!(result, "... [truncated]");
    }

    #[test]
    fn test_truncate_lines_even_max() {
        let text = (1..=6)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let result = truncate_lines(&text, 4);
        assert!(result.contains("line1"));
        assert!(result.contains("line2"));
        assert!(result.contains("line5"));
        assert!(result.contains("line6"));
    }

    #[test]
    fn test_truncate_lines_odd_max() {
        let text = (1..=7)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let result = truncate_lines(&text, 3);
        let lines: Vec<&str> = result.lines().collect();
        assert!(lines.len() >= 3);
    }
}
