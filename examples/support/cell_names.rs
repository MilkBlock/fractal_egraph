//! Current compact names and read-only compatibility with archived benchmarks.
pub fn raw(name: &str) -> [String; 2] {
    [format!("{name}!"), format!("Spine-{name}")]
}
pub fn canonical(name: &str) -> [String; 2] {
    [format!("{name}!!"), format!("Spine-Cano-{name}")]
}
pub fn summary(name: &str) -> bool {
    name.strip_prefix("Summary-")
        .and_then(|n| n.strip_suffix('!'))
        .or_else(|| name.strip_prefix("Spine-Summary-"))
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
}
