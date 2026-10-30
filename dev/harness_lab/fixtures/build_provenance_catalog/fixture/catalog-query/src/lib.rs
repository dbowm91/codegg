use catalog_format::Record;

pub fn lineage(_records: &[Record], package: &str) -> Result<String, String> {
    if package.is_empty() { return Err("unknown package".into()); }
    Ok(format!("package={package}\nprovenance=incomplete\ndependencies=\n"))
}

pub fn diff(_before: &[Record], _after: &[Record]) -> String { String::new() }

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
