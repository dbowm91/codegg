pub mod cache;
pub mod parser;

pub fn parse_cached(input: &str, cache: &cache::ParseCache) -> anyhow::Result<parser::Document> {
    if let Some(document) = cache.get(input) {
        return Ok(document);
    }
    let document = parser::parse(input)?;
    cache.insert(input, document.clone());
    Ok(document)
}
