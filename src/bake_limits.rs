//! Bounds apply to expanded applications. A macro or history window cannot
//! reset a dependency path to depth one.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct BakeLimits {
    pub max_depth: usize,
    pub max_nodes: usize,
    pub max_units: usize,
}
impl Default for BakeLimits {
    fn default() -> Self {
        Self {
            max_depth: 4,
            max_nodes: 8,
            max_units: 64,
        }
    }
}
impl BakeLimits {
    pub fn from_env() -> crate::pipeline::Result<Self> {
        let mut l = Self::default();
        for (name, dst) in [
            ("EGG_LAYOUT_BAKE_MAX_DEPTH", &mut l.max_depth),
            ("EGG_LAYOUT_BAKE_MAX_NODES", &mut l.max_nodes),
            ("EGG_LAYOUT_BAKE_MAX_UNITS", &mut l.max_units),
        ] {
            if let Some(s) = std::env::var_os(name) {
                *dst = s.to_str().ok_or("invalid Bake limit")?.parse()?;
                if *dst == 0 {
                    return Err(format!("{name} must be positive").into());
                }
            }
        }
        Ok(l)
    }
}
