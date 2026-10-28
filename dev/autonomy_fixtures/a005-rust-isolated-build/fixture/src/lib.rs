pub fn clamp_count(value: i32) -> u32 {
    if value < 0 { value as u32 } else { value as u32 }
}
