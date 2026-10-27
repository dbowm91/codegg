use ledger_core::Record;
use std::collections::BTreeMap;

pub fn canonicalize(records: impl IntoIterator<Item = Record>) -> Result<Vec<Record>, String> {
    let mut by_id = BTreeMap::new();
    for record in records {
        by_id.entry(record.id.clone()).or_insert(record);
    }
    let mut rows: Vec<_> = by_id.into_values().collect();
    rows.sort_by(|a, b| (a.timestamp.div_euclid(60), &a.id).cmp(&(b.timestamp.div_euclid(60), &b.id)));
    Ok(rows)
}

pub fn summaries(records: &[Record]) -> Vec<(i64, i64, usize)> {
    let mut windows: BTreeMap<i64, (i64, usize)> = BTreeMap::new();
    for row in records {
        let entry = windows.entry(row.timestamp.div_euclid(60)).or_default();
        entry.0 += row.amount;
        entry.1 += 1;
    }
    windows.into_iter().map(|(window, (amount, count))| (window, amount, count)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deduplicates_and_orders_independent_of_input() {
        let (rows, errors) = ledger_core::parse("61|b|open|2|src\n1|a|close|3|source\n61|b|open|2|source");
        assert!(errors.is_empty());
        let result = canonicalize(rows).unwrap();
        assert_eq!(result.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["a", "b"]);
    }

    #[test]
    fn rejects_conflicting_ids_instead_of_keeping_the_first() {
        let (rows, errors) = ledger_core::parse("1|same|open|2|src\n1|same|open|3|src");
        assert!(errors.is_empty());
        assert!(canonicalize(rows).unwrap_err().contains("conflicting duplicate id same"));
    }
}
