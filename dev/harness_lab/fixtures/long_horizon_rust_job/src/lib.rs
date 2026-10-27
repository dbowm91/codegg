mod normalize;
mod parser;

pub fn parse_version(input: &str) -> Option<(u32, u32, u32)> {
    let parts = parser::split_version(input)?;
    normalize::normalize(parts)
}
