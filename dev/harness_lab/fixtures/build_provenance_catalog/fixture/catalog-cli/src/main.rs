use catalog_store::{import, records, recover};
use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

fn run(args: &[String]) -> Result<String, String> {
    let command = args.first().ok_or("expected command")?.as_str();
    match command {
        "import" if args.len() == 3 => {
            let count = import(Path::new(&args[1]), &fs::read_to_string(&args[2])
                .map_err(|error| error.to_string())?)?;
            Ok(format!("imported={count}\n"))
        }
        "verify" if args.len() == 2 => {
            let catalog = recover(Path::new(&args[1]))?;
            let graph = catalog_index::Index::build(&records(&catalog));
            let (cycles, missing) = (graph.cycles(), graph.missing);
            if !cycles.is_empty() { return Err(format!("dependency cycles: {cycles:?}")); }
            if !missing.is_empty() { return Err(format!("missing dependencies: {missing:?}")); }
            Ok(format!("verified={}\n", catalog.len()))
        }
        "lineage" if args.len() == 3 => {
            let catalog = recover(Path::new(&args[1]))?;
            catalog_query::lineage(&records(&catalog), &args[2])
        }
        "diff" if args.len() == 3 => {
            let before = records(&recover(Path::new(&args[1]))?);
            let after = records(&recover(Path::new(&args[2]))?);
            Ok(catalog_query::diff(&before, &after))
        }
        "export" if args.len() == 2 => {
            let catalog = recover(Path::new(&args[1]))?;
            let mut output = String::new();
            for row in records(&catalog) { output.push_str(&catalog_format::encode(&row, 2)); output.push('\n'); }
            Ok(output)
        }
        _ => Err("usage: catalog-cli <import|verify|lineage|diff|export> ...".into()),
    }
}

fn main() -> ExitCode {
    match run(&env::args().skip(1).collect::<Vec<_>>()) {
        Ok(output) => { print!("{output}"); ExitCode::SUCCESS }
        Err(error) => { eprintln!("catalog-cli: {error}"); ExitCode::from(2) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wrong_arity_is_a_diagnostic() {
        assert!(run(&["import".into()]).unwrap_err().contains("usage:"));
    }
    #[test]
    fn catalog_rows_export_as_canonical_v2() {
        let row = catalog_format::Record { package: "app".into(), build: "b1".into(), revision: "r1".into(),
            dependencies: Vec::new() };
        assert!(catalog_format::encode(&row, 2).starts_with("v2|app|b1|r1|-|"));
    }
}
