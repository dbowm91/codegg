/// Represents the parsed shape of a shell command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellShape {
    /// Empty command (after trimming)
    Empty,
    /// Simple argv with no shell complexity — arguments are properly parsed
    SimpleArgv(Vec<String>),
    /// Complex shell with reasons why it can't be simple argv
    ComplexShell { reasons: Vec<ShellComplexityReason> },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ShellComplexityReason {
    Pipe,
    Semicolon,
    AndOr,
    Background,
    Redirection,
    CommandSubstitution,
    VariableExpansion,
    Heredoc,
    Newline,
    UnbalancedQuotes,
    EnvAssignment,
    Glob,
    Tilde,
}

/// State machine for shell word parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParseState {
    /// Between words, not inside quotes
    BetweenWords,
    /// Inside a word (unquoted)
    InWord,
    /// Inside single quotes
    InSingleQuote,
    /// Inside double quotes
    InDoubleQuote,
    /// Inside double quote, last char was backslash (escape)
    InDoubleQuoteEscaped,
    /// Unquoted backslash escape
    BackslashEscape,
}

/// Parse a shell command string into a `ShellShape`.
///
/// Handles single quotes (no escape inside), double quotes (with `\"` and `\\` escapes),
/// backslash escapes outside quotes, and detects various shell complexity reasons.
///
/// The parser is conservative: if in doubt, it classifies as `ComplexShell`.
pub fn parse_shell_words(command: &str) -> ShellShape {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return ShellShape::Empty;
    }

    let mut reasons = Vec::new();
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut state = ParseState::BetweenWords;
    let mut chars = trimmed.chars().peekable();
    let mut has_env_prefix = false;

    while let Some(ch) = chars.next() {
        match state {
            ParseState::BetweenWords => {
                match ch {
                    ' ' | '\t' => { /* skip whitespace between words */ }
                    '\n' | '\r' => {
                        reasons.push(ShellComplexityReason::Newline);
                    }
                    '\'' => {
                        state = ParseState::InSingleQuote;
                    }
                    '"' => {
                        state = ParseState::InDoubleQuote;
                    }
                    '\\' => {
                        if chars.peek().is_some() {
                            state = ParseState::BackslashEscape;
                            // Peek to see if the next char is a newline (line continuation)
                            if let Some(&next) = chars.peek() {
                                if next == '\n' {
                                    chars.next();
                                    reasons.push(ShellComplexityReason::Newline);
                                    state = ParseState::BetweenWords;
                                }
                            }
                        } else {
                            // Trailing backslash, not inside a word yet
                            // This is an edge case; treat as a single-char arg
                            current.push('\\');
                            state = ParseState::InWord;
                        }
                    }
                    '|' => {
                        if chars.peek() == Some(&'|') {
                            chars.next();
                            reasons.push(ShellComplexityReason::AndOr);
                        } else {
                            reasons.push(ShellComplexityReason::Pipe);
                        }
                    }
                    ';' => {
                        reasons.push(ShellComplexityReason::Semicolon);
                    }
                    '&' => {
                        if chars.peek() == Some(&'&') {
                            chars.next();
                            reasons.push(ShellComplexityReason::AndOr);
                        } else {
                            reasons.push(ShellComplexityReason::Background);
                        }
                    }
                    '$' => {
                        // Could be command substitution $(...) or variable expansion $VAR / ${VAR}
                        if chars.peek() == Some(&'(') {
                            chars.next();
                            reasons.push(ShellComplexityReason::CommandSubstitution);
                        } else if chars.peek() == Some(&'{') {
                            chars.next();
                            // Skip to closing }
                            let mut depth = 1u32;
                            for inner in chars.by_ref() {
                                match inner {
                                    '{' => depth += 1,
                                    '}' => {
                                        depth -= 1;
                                        if depth == 0 {
                                            break;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            reasons.push(ShellComplexityReason::VariableExpansion);
                        } else if chars
                            .peek()
                            .is_some_and(|c| c.is_ascii_alphabetic() || *c == '_')
                        {
                            // $VAR or $1 etc.
                            reasons.push(ShellComplexityReason::VariableExpansion);
                        } else {
                            // Bare $, treat as a regular character in a word
                            current.push(ch);
                            state = ParseState::InWord;
                            continue;
                        }
                    }
                    '`' => {
                        reasons.push(ShellComplexityReason::CommandSubstitution);
                    }
                    '<' => {
                        if chars.peek() == Some(&'<') {
                            chars.next();
                            reasons.push(ShellComplexityReason::Heredoc);
                        } else {
                            reasons.push(ShellComplexityReason::Redirection);
                        }
                    }
                    '>' => {
                        if chars.peek() == Some(&'>') {
                            chars.next();
                        }
                        reasons.push(ShellComplexityReason::Redirection);
                    }
                    '2' => {
                        // Could be 2> or 2>&1
                        if chars.peek() == Some(&'>') {
                            chars.next();
                            if chars.peek() == Some(&'&') {
                                chars.next();
                                if chars.peek() == Some(&'1') {
                                    chars.next();
                                }
                            }
                            reasons.push(ShellComplexityReason::Redirection);
                        } else {
                            current.push(ch);
                            state = ParseState::InWord;
                        }
                    }
                    '*' | '?' => {
                        reasons.push(ShellComplexityReason::Glob);
                        current.push(ch);
                        state = ParseState::InWord;
                    }
                    '[' => {
                        // Could be a glob bracket pattern [a-z]
                        // Check if there's a matching ] nearby
                        let remaining: String = chars.clone().take(20).collect();
                        if remaining.contains(']') {
                            reasons.push(ShellComplexityReason::Glob);
                        }
                        current.push(ch);
                        state = ParseState::InWord;
                    }
                    '~' => {
                        // Tilde expansion at start of word
                        reasons.push(ShellComplexityReason::Tilde);
                        current.push(ch);
                        state = ParseState::InWord;
                    }
                    _ => {
                        current.push(ch);
                        state = ParseState::InWord;
                    }
                }
            }
            ParseState::InWord => {
                match ch {
                    ' ' | '\t' => {
                        words.push(std::mem::take(&mut current));
                        state = ParseState::BetweenWords;
                    }
                    '\n' | '\r' => {
                        words.push(std::mem::take(&mut current));
                        reasons.push(ShellComplexityReason::Newline);
                        state = ParseState::BetweenWords;
                    }
                    '\'' => {
                        state = ParseState::InSingleQuote;
                    }
                    '"' => {
                        state = ParseState::InDoubleQuote;
                    }
                    '\\' => {
                        if chars.peek().is_some() {
                            state = ParseState::BackslashEscape;
                            if let Some(&next) = chars.peek() {
                                if next == '\n' {
                                    chars.next();
                                    // line continuation — word so far stays, then between words
                                    words.push(std::mem::take(&mut current));
                                    reasons.push(ShellComplexityReason::Newline);
                                    state = ParseState::BetweenWords;
                                }
                            }
                        } else {
                            current.push('\\');
                        }
                    }
                    '|' => {
                        words.push(std::mem::take(&mut current));
                        if chars.peek() == Some(&'|') {
                            chars.next();
                            reasons.push(ShellComplexityReason::AndOr);
                        } else {
                            reasons.push(ShellComplexityReason::Pipe);
                        }
                        state = ParseState::BetweenWords;
                    }
                    ';' => {
                        words.push(std::mem::take(&mut current));
                        reasons.push(ShellComplexityReason::Semicolon);
                        state = ParseState::BetweenWords;
                    }
                    '&' => {
                        words.push(std::mem::take(&mut current));
                        if chars.peek() == Some(&'&') {
                            chars.next();
                            reasons.push(ShellComplexityReason::AndOr);
                        } else {
                            reasons.push(ShellComplexityReason::Background);
                        }
                        state = ParseState::BetweenWords;
                    }
                    '$' => {
                        if chars.peek() == Some(&'(') {
                            chars.next();
                            words.push(std::mem::take(&mut current));
                            reasons.push(ShellComplexityReason::CommandSubstitution);
                            state = ParseState::BetweenWords;
                        } else if chars.peek() == Some(&'{') {
                            chars.next();
                            let mut depth = 1u32;
                            for inner in chars.by_ref() {
                                match inner {
                                    '{' => depth += 1,
                                    '}' => {
                                        depth -= 1;
                                        if depth == 0 {
                                            break;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            reasons.push(ShellComplexityReason::VariableExpansion);
                        } else if chars
                            .peek()
                            .is_some_and(|c| c.is_ascii_alphabetic() || *c == '_')
                        {
                            words.push(std::mem::take(&mut current));
                            reasons.push(ShellComplexityReason::VariableExpansion);
                            state = ParseState::BetweenWords;
                        } else {
                            current.push(ch);
                        }
                    }
                    '`' => {
                        words.push(std::mem::take(&mut current));
                        reasons.push(ShellComplexityReason::CommandSubstitution);
                        state = ParseState::BetweenWords;
                    }
                    '<' => {
                        words.push(std::mem::take(&mut current));
                        if chars.peek() == Some(&'<') {
                            chars.next();
                            reasons.push(ShellComplexityReason::Heredoc);
                        } else {
                            reasons.push(ShellComplexityReason::Redirection);
                        }
                        state = ParseState::BetweenWords;
                    }
                    '>' => {
                        words.push(std::mem::take(&mut current));
                        if chars.peek() == Some(&'>') {
                            chars.next();
                        }
                        reasons.push(ShellComplexityReason::Redirection);
                        state = ParseState::BetweenWords;
                    }
                    '2' => {
                        if chars.peek() == Some(&'>') {
                            chars.next();
                            words.push(std::mem::take(&mut current));
                            if chars.peek() == Some(&'&') {
                                chars.next();
                                if chars.peek() == Some(&'1') {
                                    chars.next();
                                }
                            }
                            reasons.push(ShellComplexityReason::Redirection);
                            state = ParseState::BetweenWords;
                        } else {
                            current.push(ch);
                        }
                    }
                    '*' | '?' => {
                        reasons.push(ShellComplexityReason::Glob);
                        current.push(ch);
                    }
                    '[' => {
                        let remaining: String = chars.clone().take(20).collect();
                        if remaining.contains(']') {
                            reasons.push(ShellComplexityReason::Glob);
                        }
                        current.push(ch);
                    }
                    '~' => {
                        // Tilde only expands at start of word; mid-word ~ is literal
                        current.push(ch);
                    }
                    _ => {
                        current.push(ch);
                    }
                }
            }
            ParseState::InSingleQuote => match ch {
                '\'' => {
                    state = ParseState::InWord;
                }
                _ => {
                    current.push(ch);
                }
            },
            ParseState::InDoubleQuote => {
                match ch {
                    '"' => {
                        state = ParseState::InWord;
                    }
                    '\\' => {
                        if let Some(&next) = chars.peek() {
                            if next == '"' || next == '\\' || next == '$' || next == '`' {
                                // Don't consume — let InDoubleQuoteEscaped handle it
                                state = ParseState::InDoubleQuoteEscaped;
                            } else {
                                current.push('\\');
                            }
                        } else {
                            current.push('\\');
                        }
                    }
                    '$' => {
                        if chars.peek() == Some(&'(') {
                            chars.next();
                            reasons.push(ShellComplexityReason::CommandSubstitution);
                        } else if chars.peek() == Some(&'{') {
                            chars.next();
                            let mut depth = 1u32;
                            for inner in chars.by_ref() {
                                match inner {
                                    '{' => depth += 1,
                                    '}' => {
                                        depth -= 1;
                                        if depth == 0 {
                                            break;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            reasons.push(ShellComplexityReason::VariableExpansion);
                        } else if chars
                            .peek()
                            .is_some_and(|c| c.is_ascii_alphabetic() || *c == '_')
                        {
                            reasons.push(ShellComplexityReason::VariableExpansion);
                        } else {
                            current.push(ch);
                        }
                    }
                    '`' => {
                        reasons.push(ShellComplexityReason::CommandSubstitution);
                    }
                    _ => {
                        current.push(ch);
                    }
                }
            }
            ParseState::InDoubleQuoteEscaped => {
                match ch {
                    '"' => current.push('"'),
                    '\\' => current.push('\\'),
                    '$' => current.push('$'),
                    '`' => current.push('`'),
                    _ => {
                        current.push('\\');
                        current.push(ch);
                    }
                }
                state = ParseState::InDoubleQuote;
            }
            ParseState::BackslashEscape => {
                current.push(ch);
                state = ParseState::InWord;
            }
        }
    }

    // Flush any in-progress word
    match state {
        ParseState::InSingleQuote | ParseState::InDoubleQuote => {
            reasons.push(ShellComplexityReason::UnbalancedQuotes);
            if !current.is_empty() {
                words.push(current);
            }
        }
        ParseState::InDoubleQuoteEscaped => {
            // Trailing escape inside double quote — treat as unbalanced
            reasons.push(ShellComplexityReason::UnbalancedQuotes);
            if !current.is_empty() {
                words.push(current);
            }
        }
        ParseState::InWord | ParseState::BackslashEscape => {
            if !current.is_empty() {
                words.push(current);
            }
        }
        ParseState::BetweenWords => {}
    }

    if !reasons.is_empty() {
        reasons.sort();
        reasons.dedup();
        return ShellShape::ComplexShell { reasons };
    }

    // Check for env assignment: VAR=value command...
    if words.len() >= 2 && looks_like_env_assignment(&words[0]) {
        has_env_prefix = true;
    }
    if has_env_prefix {
        reasons.push(ShellComplexityReason::EnvAssignment);
        return ShellShape::ComplexShell { reasons };
    }

    if words.is_empty() {
        ShellShape::Empty
    } else {
        ShellShape::SimpleArgv(words)
    }
}

/// Check if a word looks like a shell environment variable assignment (e.g., `VAR=value`).
fn looks_like_env_assignment(word: &str) -> bool {
    let Some(eq_pos) = word.find('=') else {
        return false;
    };
    if eq_pos == 0 {
        return false;
    }
    let var_name = &word[..eq_pos];
    // VAR names: start with letter or underscore, then alphanumeric or underscore
    let mut chars = var_name.chars();
    if let Some(first) = chars.next() {
        if (first.is_ascii_alphabetic() || first == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_command() {
        assert_eq!(parse_shell_words(""), ShellShape::Empty);
        assert_eq!(parse_shell_words("   "), ShellShape::Empty);
    }

    #[test]
    fn simple_two_word_command() {
        assert_eq!(
            parse_shell_words("git status"),
            ShellShape::SimpleArgv(vec!["git".into(), "status".into()])
        );
    }

    #[test]
    fn single_quoted_argument() {
        assert_eq!(
            parse_shell_words("rg 'fn main' src/"),
            ShellShape::SimpleArgv(vec!["rg".into(), "fn main".into(), "src/".into()])
        );
    }

    #[test]
    fn double_quoted_argument() {
        assert_eq!(
            parse_shell_words("echo \"hello world\""),
            ShellShape::SimpleArgv(vec!["echo".into(), "hello world".into()])
        );
    }

    #[test]
    fn python_inline_single_quotes() {
        assert_eq!(
            parse_shell_words("python -c 'print(1)'"),
            ShellShape::SimpleArgv(vec!["python".into(), "-c".into(), "print(1)".into()])
        );
    }

    #[test]
    fn double_quote_with_escaped_quote() {
        assert_eq!(
            parse_shell_words("echo \"he said \\\"hi\\\"\""),
            ShellShape::SimpleArgv(vec!["echo".into(), "he said \"hi\"".into()])
        );
    }

    #[test]
    fn backslash_escape_outside_quotes() {
        assert_eq!(
            parse_shell_words("echo hello\\ world"),
            ShellShape::SimpleArgv(vec!["echo".into(), "hello world".into()])
        );
    }

    #[test]
    fn pipe_is_complex() {
        let shape = parse_shell_words("cat foo | grep bar");
        match shape {
            ShellShape::ComplexShell { reasons } => {
                assert!(reasons.contains(&ShellComplexityReason::Pipe));
            }
            _ => panic!("expected ComplexShell"),
        }
    }

    #[test]
    fn semicolon_is_complex() {
        let shape = parse_shell_words("echo a; echo b");
        match shape {
            ShellShape::ComplexShell { reasons } => {
                assert!(reasons.contains(&ShellComplexityReason::Semicolon));
            }
            _ => panic!("expected ComplexShell"),
        }
    }

    #[test]
    fn and_operator_is_complex() {
        let shape = parse_shell_words("cargo test && rm -rf .");
        match shape {
            ShellShape::ComplexShell { reasons } => {
                assert!(reasons.contains(&ShellComplexityReason::AndOr));
            }
            _ => panic!("expected ComplexShell"),
        }
    }

    #[test]
    fn or_operator_is_complex() {
        let shape = parse_shell_words("cargo build || echo fail");
        match shape {
            ShellShape::ComplexShell { reasons } => {
                assert!(reasons.contains(&ShellComplexityReason::AndOr));
            }
            _ => panic!("expected ComplexShell"),
        }
    }

    #[test]
    fn background_operator_is_complex() {
        let shape = parse_shell_words("sleep 10 &");
        match shape {
            ShellShape::ComplexShell { reasons } => {
                assert!(reasons.contains(&ShellComplexityReason::Background));
            }
            _ => panic!("expected ComplexShell"),
        }
    }

    #[test]
    fn redirection_is_complex() {
        let shape = parse_shell_words("echo hello > file.txt");
        match shape {
            ShellShape::ComplexShell { reasons } => {
                assert!(reasons.contains(&ShellComplexityReason::Redirection));
            }
            _ => panic!("expected ComplexShell"),
        }
    }

    #[test]
    fn append_redirection_is_complex() {
        let shape = parse_shell_words("echo hello >> file.txt");
        match shape {
            ShellShape::ComplexShell { reasons } => {
                assert!(reasons.contains(&ShellComplexityReason::Redirection));
            }
            _ => panic!("expected ComplexShell"),
        }
    }

    #[test]
    fn stderr_redirect_is_complex() {
        let shape = parse_shell_words("echo hello 2> /dev/null");
        match shape {
            ShellShape::ComplexShell { reasons } => {
                assert!(reasons.contains(&ShellComplexityReason::Redirection));
            }
            _ => panic!("expected ComplexShell"),
        }
    }

    #[test]
    fn stderr_merge_redirect_is_complex() {
        let shape = parse_shell_words("echo hello 2>&1");
        match shape {
            ShellShape::ComplexShell { reasons } => {
                assert!(reasons.contains(&ShellComplexityReason::Redirection));
            }
            _ => panic!("expected ComplexShell"),
        }
    }

    #[test]
    fn heredoc_is_complex() {
        let shape = parse_shell_words("cat << EOF");
        match shape {
            ShellShape::ComplexShell { reasons } => {
                assert!(reasons.contains(&ShellComplexityReason::Heredoc));
            }
            _ => panic!("expected ComplexShell"),
        }
    }

    #[test]
    fn command_substitution_dollar_paren_is_complex() {
        let shape = parse_shell_words("echo $(date)");
        match shape {
            ShellShape::ComplexShell { reasons } => {
                assert!(reasons.contains(&ShellComplexityReason::CommandSubstitution));
            }
            _ => panic!("expected ComplexShell"),
        }
    }

    #[test]
    fn command_substitution_backtick_is_complex() {
        let shape = parse_shell_words("echo `date`");
        match shape {
            ShellShape::ComplexShell { reasons } => {
                assert!(reasons.contains(&ShellComplexityReason::CommandSubstitution));
            }
            _ => panic!("expected ComplexShell"),
        }
    }

    #[test]
    fn variable_expansion_is_complex() {
        let shape = parse_shell_words("echo $HOME");
        match shape {
            ShellShape::ComplexShell { reasons } => {
                assert!(reasons.contains(&ShellComplexityReason::VariableExpansion));
            }
            _ => panic!("expected ComplexShell"),
        }
    }

    #[test]
    fn variable_brace_expansion_is_complex() {
        let shape = parse_shell_words("echo ${HOME}");
        match shape {
            ShellShape::ComplexShell { reasons } => {
                assert!(reasons.contains(&ShellComplexityReason::VariableExpansion));
            }
            _ => panic!("expected ComplexShell"),
        }
    }

    #[test]
    fn glob_star_is_complex() {
        let shape = parse_shell_words("ls *.txt");
        match shape {
            ShellShape::ComplexShell { reasons } => {
                assert!(reasons.contains(&ShellComplexityReason::Glob));
            }
            _ => panic!("expected ComplexShell"),
        }
    }

    #[test]
    fn glob_question_is_complex() {
        let shape = parse_shell_words("ls file?.txt");
        match shape {
            ShellShape::ComplexShell { reasons } => {
                assert!(reasons.contains(&ShellComplexityReason::Glob));
            }
            _ => panic!("expected ComplexShell"),
        }
    }

    #[test]
    fn glob_bracket_is_complex() {
        let shape = parse_shell_words("ls file[0-9].txt");
        match shape {
            ShellShape::ComplexShell { reasons } => {
                assert!(reasons.contains(&ShellComplexityReason::Glob));
            }
            _ => panic!("expected ComplexShell"),
        }
    }

    #[test]
    fn tilde_is_complex() {
        let shape = parse_shell_words("ls ~/Documents");
        match shape {
            ShellShape::ComplexShell { reasons } => {
                assert!(reasons.contains(&ShellComplexityReason::Tilde));
            }
            _ => panic!("expected ComplexShell"),
        }
    }

    #[test]
    fn env_assignment_is_complex() {
        let shape = parse_shell_words("FOO=bar ls");
        match shape {
            ShellShape::ComplexShell { reasons } => {
                assert!(reasons.contains(&ShellComplexityReason::EnvAssignment));
            }
            _ => panic!("expected ComplexShell"),
        }
    }

    #[test]
    fn env_assignment_with_underscore() {
        let shape = parse_shell_words("MY_VAR=1 make test");
        match shape {
            ShellShape::ComplexShell { reasons } => {
                assert!(reasons.contains(&ShellComplexityReason::EnvAssignment));
            }
            _ => panic!("expected ComplexShell"),
        }
    }

    #[test]
    fn newline_is_complex() {
        let shape = parse_shell_words("echo a\necho b");
        match shape {
            ShellShape::ComplexShell { reasons } => {
                assert!(reasons.contains(&ShellComplexityReason::Newline));
            }
            _ => panic!("expected ComplexShell"),
        }
    }

    #[test]
    fn unbalanced_single_quote_is_complex() {
        let shape = parse_shell_words("echo 'hello");
        match shape {
            ShellShape::ComplexShell { reasons } => {
                assert!(reasons.contains(&ShellComplexityReason::UnbalancedQuotes));
            }
            _ => panic!("expected ComplexShell"),
        }
    }

    #[test]
    fn unbalanced_double_quote_is_complex() {
        let shape = parse_shell_words("echo \"hello");
        match shape {
            ShellShape::ComplexShell { reasons } => {
                assert!(reasons.contains(&ShellComplexityReason::UnbalancedQuotes));
            }
            _ => panic!("expected ComplexShell"),
        }
    }

    #[test]
    fn dollar_inside_single_quotes_is_literal() {
        assert_eq!(
            parse_shell_words("echo '$HOME'"),
            ShellShape::SimpleArgv(vec!["echo".into(), "$HOME".into()])
        );
    }

    #[test]
    fn dollar_in_double_quotes_is_expansion() {
        let shape = parse_shell_words("echo \"$HOME\"");
        match shape {
            ShellShape::ComplexShell { reasons } => {
                assert!(reasons.contains(&ShellComplexityReason::VariableExpansion));
            }
            _ => panic!("expected ComplexShell"),
        }
    }

    #[test]
    fn backtick_inside_single_quotes_is_literal() {
        assert_eq!(
            parse_shell_words("echo '`date`'"),
            ShellShape::SimpleArgv(vec!["echo".into(), "`date`".into()])
        );
    }

    #[test]
    fn single_quote_preserves_whitespace() {
        assert_eq!(
            parse_shell_words("echo 'hello   world'"),
            ShellShape::SimpleArgv(vec!["echo".into(), "hello   world".into()])
        );
    }

    #[test]
    fn multiple_operators() {
        let shape = parse_shell_words("echo a | grep b; echo c");
        match shape {
            ShellShape::ComplexShell { reasons } => {
                assert!(reasons.contains(&ShellComplexityReason::Pipe));
                assert!(reasons.contains(&ShellComplexityReason::Semicolon));
            }
            _ => panic!("expected ComplexShell"),
        }
    }

    #[test]
    fn single_word_command() {
        assert_eq!(
            parse_shell_words("ls"),
            ShellShape::SimpleArgv(vec!["ls".into()])
        );
    }

    #[test]
    fn cargo_test_with_args() {
        assert_eq!(
            parse_shell_words("cargo test --lib -p foo"),
            ShellShape::SimpleArgv(vec![
                "cargo".into(),
                "test".into(),
                "--lib".into(),
                "-p".into(),
                "foo".into()
            ])
        );
    }

    #[test]
    fn three_word_command() {
        assert_eq!(
            parse_shell_words("rg pattern src/"),
            ShellShape::SimpleArgv(vec!["rg".into(), "pattern".into(), "src/".into()])
        );
    }
}
