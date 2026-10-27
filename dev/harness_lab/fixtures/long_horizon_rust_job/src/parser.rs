pub fn split_version(input: &str) -> Option<Vec<&str>> {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.starts_with('v') {
        return None;
    }
    let parts: Vec<&str> = trimmed.split('.').collect();
    (parts.len() == 3 && parts.iter().all(|part| !part.is_empty())).then_some(parts)
}
