use ledger_core::Record;
use ledger_index::canonicalize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

fn encode(row: &Record) -> String {
    let body = format!("{}|{}|{}|{}|{}", row.timestamp, row.id, row.kind, row.amount, row.source);
    let checksum = body.bytes().fold(0u32, |sum, byte| sum.wrapping_add(u32::from(byte)));
    format!("v1|{body}|{checksum:08x}")
}

fn decode(line: &str) -> Result<Record, String> {
    let parts: Vec<_> = line.split('|').collect();
    if parts.len() != 7 || parts[0] != "v1" { return Err("invalid journal version or fields".into()); }
    let body = parts[1..6].join("|");
    let expected = body.bytes().fold(0u32, |sum, byte| sum.wrapping_add(u32::from(byte)));
    if u32::from_str_radix(parts[6], 16).ok() != Some(expected) { return Err("checksum mismatch".into()); }
    let (rows, errors) = ledger_core::parse(&body);
    if !errors.is_empty() || rows.len() != 1 { return Err("invalid journal record".into()); }
    Ok(rows[0].clone())
}

pub fn append(path: &Path, rows: &[Record]) -> Result<(), String> {
    if let Some(parent) = path.parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
    let mut file = OpenOptions::new().create(true).append(true).open(path).map_err(|e| e.to_string())?;
    for row in rows { writeln!(file, "{}", encode(row)).map_err(|e| e.to_string())?; }
    file.sync_all().map_err(|e| e.to_string())
}

pub fn replay(path: &Path) -> Result<Vec<Record>, String> {
    let text = match fs::read_to_string(path) { Ok(value) => value, Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()), Err(error) => return Err(error.to_string()) };
    let lines: Vec<_> = text.lines().collect();
    let mut rows = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        match decode(line) {
            Ok(row) => rows.push(row),
            Err(_) => break,
        }
    }
    canonicalize(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replay_ignores_only_a_torn_tail_and_is_idempotent() {
        let path = std::env::temp_dir().join(format!("ledger-{}.log", std::process::id()));
        let row = ledger_core::parse("1|a|open|2|src").0.remove(0);
        append(&path, &[row]).unwrap();
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "torn").unwrap();
        let once = replay(&path).unwrap();
        let twice = replay(&path).unwrap();
        assert_eq!(once, twice);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn rejects_corrupt_interior_records() {
        let path = std::env::temp_dir().join(format!("ledger-corrupt-{}.log", std::process::id()));
        let first = ledger_core::parse("1|a|open|2|src").0.remove(0);
        let second = ledger_core::parse("2|b|close|3|src").0.remove(0);
        append(&path, &[first, second]).unwrap();
        let original = fs::read_to_string(&path).unwrap();
        let mut lines: Vec<_> = original.lines().map(str::to_string).collect();
        lines[0].push('x');
        fs::write(&path, lines.join("\n") + "\n").unwrap();
        assert!(replay(&path).unwrap_err().contains("journal line 1"));
        let _ = fs::remove_file(path);
    }
}
