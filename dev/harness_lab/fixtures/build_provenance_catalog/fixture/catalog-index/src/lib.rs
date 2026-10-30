use catalog_format::Record;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Index {
    pub forward: BTreeMap<String, BTreeSet<String>>,
    pub reverse: BTreeMap<String, BTreeSet<String>>,
    pub missing: BTreeSet<(String, String)>,
}

impl Index {
    pub fn build(_records: &[Record]) -> Self { Index::default() }

    pub fn update(&mut self, _record: &Record) {}

    pub fn cycles(&self) -> Vec<Vec<String>> {
        Vec::new()
    }

    pub fn lineage(&self, _package: &str) -> (Vec<String>, bool) {
        (Vec::new(), false)
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(package: &str, deps: &[&str]) -> Record {
        Record { package: package.into(), build: "b1".into(), revision: "r1".into(),
                 dependencies: deps.iter().map(|value| value.to_string()).collect() }
    }
    #[test]
    fn forward_reverse_missing_and_cycles_are_deterministic() {
        let index = Index::build(&[record("app", &["core", "absent"]),
                                  record("core", &["app"])]);
        assert_eq!(index.reverse["core"], BTreeSet::from(["app".to_string()]));
        assert_eq!(index.missing, BTreeSet::from([("app".into(), "absent".into())]));
        assert_eq!(index.cycles(), vec![vec![String::from("app"), String::from("core")]]);
    }
    #[test]
    fn incremental_update_leaves_other_packages_untouched() {
        let mut index = Index::build(&[record("app", &["core"]), record("tool", &["base"])]);
        let unchanged = index.forward["tool"].clone();
        index.update(&record("app", &["new-core"]));
        assert_eq!(index.forward["tool"], unchanged);
        assert_eq!(index.reverse["new-core"], BTreeSet::from(["app".into()]));
    }
}
