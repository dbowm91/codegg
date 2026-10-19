use std::io::{self, Read};

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut source_revision = None;
    while let Some(arg) = args.next() {
        if arg == "--source-revision" {
            source_revision = args.next();
        } else {
            return Err(format!("unexpected argument: {arg}"));
        }
    }
    let source_revision = source_revision.ok_or("missing --source-revision <revision>")?;
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .map_err(|error| format!("read stdin: {error}"))?;
    let summary = summary_producer::summarize(&source_revision, &input)
        .map_err(|error| error.to_string())?;
    println!(
        "{}",
        serde_json::to_string(&summary).map_err(|error| format!("serialize summary: {error}"))?
    );
    Ok(())
}
