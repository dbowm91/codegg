use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Record {
    pub package: String,
    pub build: String,
    pub revision: String,
    pub dependencies: Vec<String>,
}

fn checksum(text: &str) -> String {
    let value = text.bytes().fold(0x811c9dc5u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x01000193)
    });
    format!("{value:08x}")
}

fn identity(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
    })
}

pub fn encode(record: &Record, schema: u8) -> String {
    let deps = if record.dependencies.is_empty() { "-".to_string() }
        else { record.dependencies.join(",") };
    let prefix = format!("v{schema}|{}|{}|{}|{}", record.package, record.build,
                         record.revision, deps);
    format!("{prefix}|{}", checksum(&prefix))
}

pub fn parse(line: &str) -> Result<Record, String> {
    let fields: Vec<_> = line.trim_end_matches('\r').split('|').collect();
    if fields.len() != 6 || !matches!(fields[0], "v1" | "v2") {
        return Err("unsupported schema or malformed field count".into());
    }
    if checksum(&fields[..5].join("|")) != fields[5] {
        return Err("checksum mismatch".into());
    }
    if !identity(fields[1]) || !identity(fields[2]) || fields[3].is_empty() {
        return Err("invalid package, build, or revision identity".into());
    }
    let mut dependencies: Vec<String> = if fields[4] == "-" { Vec::new() }
        else { fields[4].split(',').map(str::to_owned).collect() };
    if dependencies.iter().any(|item| !identity(item)) {
        return Err("invalid dependency identity".into());
    }
    let unique: BTreeSet<_> = dependencies.iter().cloned().collect();
    dependencies = unique.into_iter().collect();
    Ok(Record { package: fields[1].into(), build: fields[2].into(),
                revision: fields[3].into(), dependencies })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_normalize_and_checksum_is_checked() {
        let row = Record { package: "app".into(), build: "b1".into(),
            revision: "abc123".into(), dependencies: vec!["core".into()] };
        let v1 = encode(&row, 1);
        let v2 = encode(&row, 2);
        assert_eq!(parse(&v1).unwrap(), parse(&v2).unwrap());
        assert!(parse(&v1.replace("abc123", "bad999")).is_err());
    }

    #[test]
    fn duplicate_dependencies_are_canonicalized() {
        let row = Record { package: "app".into(), build: "b1".into(),
            revision: "r1".into(), dependencies: vec!["core".into(), "core".into()] };
        assert_eq!(parse(&encode(&row, 2)).unwrap().dependencies, vec!["core"]);
    }
}
