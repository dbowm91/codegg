use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Record {
    pub timestamp: i64,
    pub id: String,
    pub kind: String,
    pub amount: i64,
    pub source: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    pub line: usize,
    pub reason: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.reason)
    }
}

pub fn parse(input: &str) -> (Vec<Record>, Vec<ParseError>) {
    let mut records = Vec::new();
    let mut errors = Vec::new();
    for (index, line) in input.lines().enumerate() {
        if line.trim().is_empty() { continue; }
        let parts: Vec<_> = line.split('|').collect();
        let parsed = (|| {
            if parts.len() != 5 { return Err("expected five fields".to_string()); }
            let timestamp = parts[0].parse().map_err(|_| "invalid timestamp".to_string())?;
            if parts[1].is_empty() { return Err("empty id".to_string()); }
            if !matches!(parts[2], "open" | "close" | "transfer") {
                return Err("invalid kind".to_string());
            }
            let amount = parts[3].parse().map_err(|_| "invalid amount".to_string())?;
            let source = match parts[4] { "src" | "source" => "source", value => value };
            if source.is_empty() { return Err("empty source".to_string()); }
            Ok(Record { timestamp, id: parts[1].to_string(), kind: parts[2].to_string(), amount, source: source.to_string() })
        })();
        match parsed {
            Ok(record) => records.push(record),
            Err(reason) => errors.push(ParseError { line: index + 1, reason }),
        }
    }
    (records, errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_rows_keep_line_and_valid_rows() {
        let (rows, errors) = parse("1|a|open|4|src\nbad\n2|b|close|5|source");
        assert_eq!(rows.len(), 2);
        assert_eq!(errors[0].line, 2);
        assert_eq!(rows[0].source, "source");
    }
}
