use catalog_format::{parse, Record};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub type Catalog = BTreeMap<(String, String), Record>;

pub fn recover(path: &Path) -> Result<Catalog, String> {
    if !path.exists() { return Ok(Catalog::new()); }
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    let mut rows = Catalog::new();
    let mut chunks = bytes.split(|byte| *byte == b'\n').peekable();
    while let Some(chunk) = chunks.next() {
        if chunk.is_empty() { continue; }
        let is_last = chunks.peek().is_none();
        let text = match std::str::from_utf8(chunk) {
            Ok(text) => text,
            Err(_) if is_last => break,
            Err(error) => return Err(format!("invalid journal encoding: {error}")),
        };
        let record = match parse(text) {
            Ok(record) => record,
            Err(_) if is_last && !bytes.ends_with(b"\n") => break,
            Err(error) => return Err(format!("invalid journal row: {error}")),
        };
        let key = (record.package.clone(), record.build.clone());
        if let Some(previous) = rows.get(&key) {
            if previous != &record { return Err(format!("conflicting identity {}:{}", key.0, key.1)); }
        } else { rows.insert(key, record); }
    }
    Ok(rows)
}

pub fn import(path: &Path, input: &str) -> Result<usize, String> {
    let mut catalog = recover(path)?;
    let before = catalog.len();
    for line in input.lines().filter(|line| !line.trim().is_empty()) {
        let record = parse(line)?;
        let key = (record.package.clone(), record.build.clone());
        if let Some(previous) = catalog.get(&key) {
            if previous != &record { return Err(format!("conflicting identity {}:{}", key.0, key.1)); }
        } else { catalog.insert(key, record); }
    }
    if catalog.len() != before {
        let parent = path.parent().ok_or("catalog path has no parent")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let temporary = path.with_extension("tmp");
        let mut bytes = Vec::new();
        for record in catalog.values() {
            bytes.extend_from_slice(catalog_format::encode(record, 2).as_bytes());
            bytes.push(b'\n');
        }
        fs::write(&temporary, bytes).map_err(|e| e.to_string())?;
        fs::rename(&temporary, path).map_err(|e| e.to_string())?;
    }
    Ok(catalog.len() - before)
}

pub fn records(catalog: &Catalog) -> Vec<Record> { catalog.values().cloned().collect() }

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn path() -> std::path::PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!("catalog-store-{}-{}", std::process::id(),
                                          NEXT.fetch_add(1, Ordering::Relaxed)))
    }
    fn row(package: &str, revision: &str) -> String {
        catalog_format::encode(&Record { package: package.into(), build: "b1".into(),
            revision: revision.into(), dependencies: Vec::new() }, 1)
    }

    #[test]
    fn imports_are_idempotent_and_conflicts_are_atomic() {
        let path = path();
        assert_eq!(import(&path, &(row("app", "r1") + "\n")).unwrap(), 1);
        assert_eq!(import(&path, &(row("app", "r1") + "\n")).unwrap(), 0);
        let before = fs::read(&path).unwrap();
        assert!(import(&path, &(row("app", "r2") + "\n")).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn recovery_ignores_only_a_torn_final_record() {
        let path = path();
        let good = row("app", "r1");
        fs::write(&path, format!("{good}\n{}", &row("core", "r1")[..10])).unwrap();
        assert_eq!(recover(&path).unwrap().len(), 1);
        fs::write(&path, format!("{}\nnot-a-row\n{good}\n", good)).unwrap();
        assert!(recover(&path).is_err());
        let _ = fs::remove_file(path);
    }
}
