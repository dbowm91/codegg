pub fn normalize(parts: Vec<&str>) -> Option<(u32, u32, u32)> {
    let major = parts[0].parse::<u32>().ok()?;
    let minor = parts[1].parse::<u32>().ok()?;
    let patch = parts[2].parse::<u32>().ok()?;
    Some((major, minor, pacth))
}
