use catalog_format::Record;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Index {
    pub forward: BTreeMap<String, BTreeSet<String>>,
    pub reverse: BTreeMap<String, BTreeSet<String>>,
    pub missing: BTreeSet<(String, String)>,
}

impl Index {
    pub fn build(records: &[Record]) -> Self {
        let known: BTreeSet<_> = records.iter().map(|record| record.package.clone()).collect();
        let mut index = Index::default();
        for record in records {
            let dependencies = index.forward.entry(record.package.clone()).or_default();
            for dependency in &record.dependencies {
                dependencies.insert(dependency.clone());
                index.reverse.entry(dependency.clone()).or_default().insert(record.package.clone());
                if !known.contains(dependency) {
                    index.missing.insert((record.package.clone(), dependency.clone()));
                }
            }
        }
        index
    }

    pub fn update(&mut self, record: &Record) {
        if let Some(old) = self.forward.remove(&record.package) {
            for dependency in old {
                if let Some(users) = self.reverse.get_mut(&dependency) {
                    users.remove(&record.package);
                    if users.is_empty() { self.reverse.remove(&dependency); }
                }
                self.missing.remove(&(record.package.clone(), dependency));
            }
        }
        self.missing.retain(|(_source, missing)| missing != &record.package);
        let dependencies: BTreeSet<_> = record.dependencies.iter().cloned().collect();
        for dependency in &dependencies {
            self.reverse.entry(dependency.clone()).or_default().insert(record.package.clone());
            if !self.forward.contains_key(dependency) {
                self.missing.insert((record.package.clone(), dependency.clone()));
            }
        }
        self.forward.insert(record.package.clone(), dependencies);
    }

    pub fn cycles(&self) -> Vec<Vec<String>> {
        fn visit(node: &str, index: &Index, active: &mut Vec<String>, done: &mut BTreeSet<String>,
                 cycles: &mut BTreeSet<Vec<String>>) {
            if let Some(start) = active.iter().position(|item| item == node) {
                let mut cycle = active[start..].to_vec();
                cycle.sort();
                cycles.insert(cycle);
                return;
            }
            if done.contains(node) { return; }
            active.push(node.to_string());
            if let Some(dependencies) = index.forward.get(node) {
                for dependency in dependencies {
                    if index.forward.contains_key(dependency) {
                        visit(dependency, index, active, done, cycles);
                    }
                }
            }
            active.pop();
            done.insert(node.to_string());
        }
        let mut cycles = BTreeSet::new();
        let mut done = BTreeSet::new();
        for node in self.forward.keys() { visit(node, self, &mut Vec::new(), &mut done, &mut cycles); }
        cycles.into_iter().collect()
    }

    pub fn lineage(&self, package: &str) -> (Vec<String>, bool) {
        fn collect(node: &str, index: &Index, seen: &mut BTreeSet<String>) {
            if let Some(dependencies) = index.forward.get(node) {
                for dependency in dependencies {
                    if seen.insert(dependency.clone()) && index.forward.contains_key(dependency) {
                        collect(dependency, index, seen);
                    }
                }
            }
        }
        let mut lineage = BTreeSet::new();
        collect(package, self, &mut lineage);
        let complete = !lineage.iter().any(|dependency|
            self.missing.iter().any(|(_, missing)| missing == dependency));
        (lineage.into_iter().collect(), complete)
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

    #[test]
    fn adding_a_previously_missing_package_resolves_existing_references() {
        let mut index = Index::build(&[record("app", &["core"])]);
        assert_eq!(index.missing, BTreeSet::from([("app".into(), "core".into())]));
        index.update(&record("core", &[]));
        assert!(index.missing.is_empty());
        assert_eq!(index.reverse["core"], BTreeSet::from(["app".into()]));
    }
}
