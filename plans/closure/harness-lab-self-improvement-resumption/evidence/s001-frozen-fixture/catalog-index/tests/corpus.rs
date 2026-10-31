use catalog_format::parse;
use catalog_index::Index;
use std::fs;
use std::path::PathBuf;

fn corpus() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../corpus") }

#[test]
fn bounded_shuffled_corpus_has_stable_indexes_across_all_snapshots() {
    let mut rows = Vec::new();
    for snapshot in 0..3 {
        let text = fs::read_to_string(corpus().join(format!("snapshot-{snapshot}.records"))).unwrap();
        let parsed: Vec<_> = text.lines().map(|line| parse(line).unwrap()).collect();
        assert_eq!(parsed.len(), 32);
        rows.extend(parsed);
    }
    assert_eq!(rows.len(), 96);
    let forward = Index::build(&rows);
    assert_eq!(forward.forward.len(), 32);
    assert_eq!(forward.forward["pkg-31"].len(), 2);
    assert!(forward.missing.is_empty());
    let mut reversed = rows.clone();
    reversed.reverse();
    assert_eq!(forward, Index::build(&reversed));
}

#[test]
fn corrupt_checksum_is_rejected_without_partial_corpus_acceptance() {
    let text = fs::read_to_string(corpus().join("invalid-checksum.records")).unwrap();
    assert!(parse(text.trim()).unwrap_err().contains("checksum"));
}

#[test]
fn bounded_dependency_index_scale_is_order_independent() {
    let records: Vec<_> = (0usize..250_000)
        .map(|index| catalog_format::Record {
            package: format!("scale-{index:05}"),
            build: "scale-build".to_string(),
            revision: format!("revision-{index}"),
            dependencies: if index == 0 { Vec::new() }
                else { vec!["scale-00000".to_string()] },
        })
        .collect();
    let forward = Index::build(&records);
    assert_eq!(forward.forward.len(), 250_000);
    assert!(forward.missing.is_empty());
    assert!(forward.cycles().is_empty());
    assert_eq!(forward.reverse["scale-00000"].len(), 249_999);
    let mut reverse_order = records;
    reverse_order.reverse();
    assert_eq!(forward, Index::build(&reverse_order));
}
