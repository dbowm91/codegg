use catalog_format::Record;
use catalog_index::Index;
use std::collections::BTreeMap;

pub fn lineage(records: &[Record], package: &str) -> Result<String, String> {
    let index = Index::build(records);
    if !index.forward.contains_key(package) { return Err(format!("unknown package: {package}")); }
    let (dependencies, complete) = index.lineage(package);
    let status = if complete { "complete" } else { "incomplete" };
    Ok(format!("package={package}\nprovenance={status}\ndependencies={}\n",
               dependencies.join(",")))
}

pub fn diff(before: &[Record], after: &[Record]) -> String {
    fn map(records: &[Record]) -> BTreeMap<(&str, &str), &str> {
        records.iter().map(|record| ((record.package.as_str(), record.build.as_str()),
                                    record.revision.as_str())).collect()
    }
    let left = map(before);
    let right = map(after);
    let keys: std::collections::BTreeSet<_> = left.keys().chain(right.keys()).copied().collect();
    let mut lines = Vec::new();
    for (package, build) in keys {
        match (left.get(&(package, build)), right.get(&(package, build))) {
            (None, Some(revision)) => lines.push(format!("added|{package}|{build}|{revision}")),
            (Some(revision), None) => lines.push(format!("removed|{package}|{build}|{revision}")),
            (Some(old), Some(new)) if old != new =>
                lines.push(format!("changed|{package}|{build}|{old}|{new}")),
            _ => {}
        }
    }
    lines.join("\n") + if lines.is_empty() { "" } else { "\n" }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lineage_and_diff_have_stable_machine_output() {
        let old = vec![Record { package: "core".into(), build: "b1".into(),
            revision: "r1".into(), dependencies: vec![] }];
        let new = vec![Record { revision: "r2".into(), ..old[0].clone() }];
        assert!(lineage(&old, "core").unwrap().contains("provenance=complete"));
        assert_eq!(diff(&old, &new), "changed|core|b1|r1|r2\n");
    }
}
