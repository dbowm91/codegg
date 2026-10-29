use ledger_core::Record;
use ledger_index::{canonicalize, summaries};

const ROWS_PER_PASS: usize = 500_000;
const PASSES: usize = 24;

#[test]
fn repeated_high_volume_shard_merges_remain_ordered_and_conservative() {
    for pass in 0..PASSES {
        let mut records = Vec::with_capacity(ROWS_PER_PASS + 1_000);
        for index in 0..ROWS_PER_PASS {
            records.push(Record {
                timestamp: ((index * 37 + pass * 11) % 36_000) as i64 - 18_000,
                id: format!("pass-{pass}-event-{index:07}"),
                kind: ["open", "close", "transfer"][index % 3].to_string(),
                amount: (index % 17) as i64 - 8,
                source: "source".to_string(),
            });
        }
        let duplicate_shard = records[..1_000].to_vec();
        records.extend(duplicate_shard);

        let merged = canonicalize(records).expect("identical shard duplicates must deduplicate");
        assert_eq!(merged.len(), ROWS_PER_PASS);
        assert!(merged.windows(2).all(|pair| {
            (pair[0].timestamp.div_euclid(60), &pair[0].id)
                <= (pair[1].timestamp.div_euclid(60), &pair[1].id)
        }));

        let summarized = summaries(&merged);
        assert_eq!(
            summarized.iter().map(|row| row.2).sum::<usize>(),
            ROWS_PER_PASS
        );
    }
}
