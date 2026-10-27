use std::env;
use std::fs;
use std::path::Path;

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();
    let command = args.get(1).ok_or("usage: ledger-cli <ingest|summarize|inspect|export> ...")?;
    match command.as_str() {
        "ingest" => {
            let input = fs::read_to_string(args.get(2).ok_or("missing input")?).map_err(|e| e.to_string())?;
            let journal = Path::new(args.get(3).ok_or("missing journal")?);
            let (rows, errors) = ledger_core::parse(&input);
            let rows = ledger_index::canonicalize(rows)?;
            ledger_store::append(journal, &rows)?;
            println!("ingested={} rejected={}", rows.len(), errors.len());
        }
        "summarize" => {
            let input = fs::read_to_string(args.get(2).ok_or("missing input")?).map_err(|e| e.to_string())?;
            let (rows, errors) = ledger_core::parse(&input);
            let rows = ledger_index::canonicalize(rows)?;
            println!("records={} rejected={}", rows.len(), errors.len());
            for (window, amount, count) in ledger_index::summaries(&rows) { println!("{window}|{amount}|{count}"); }
        }
        "inspect" => {
            let rows = ledger_store::replay(Path::new(args.get(2).ok_or("missing journal")?))?;
            println!("records={}", rows.len());
            for row in rows { println!("{}|{}|{}|{}|{}", row.timestamp, row.id, row.kind, row.amount, row.source); }
        }
        "export" => {
            let input = fs::read_to_string(args.get(2).ok_or("missing input")?).map_err(|e| e.to_string())?;
            let output = Path::new(args.get(3).ok_or("missing output")?);
            let (rows, _) = ledger_core::parse(&input);
            let rows = ledger_index::canonicalize(rows)?;
            let body = rows.iter().map(|r| format!("{}|{}|{}|{}|{}", r.timestamp, r.id, r.kind, r.amount, r.source)).collect::<Vec<_>>().join("\n");
            fs::write(output, body + "\n").map_err(|e| e.to_string())?;
        }
        _ => return Err("unknown command".into()),
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() { eprintln!("{error}"); std::process::exit(2); }
}
