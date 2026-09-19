//! Rule programs the analysis loads at runtime.
//!
//! The native build reads them from the repository so they stay editable without a
//! rebuild; the wasm build embeds them so the browser runtime is self-contained.
use crate::pipeline::Result;
use std::path::Path;

fn embedded(path: &str) -> Option<&'static str> {
    let path = path.strip_prefix("./").unwrap_or(path);
    match path {
        "rules/tier2.egg" => Some(include_str!("../rules/tier2.egg")),
        "rules/higher.egg" => Some(include_str!("../rules/higher.egg")),
        "rules/reduce.egg" => Some(include_str!("../rules/reduce.egg")),
        "rules/binding_reduce.egg" => Some(include_str!("../rules/binding_reduce.egg")),
        "rules/arrays.egg" => Some(include_str!("../rules/arrays.egg")),
        "rules/fractal_views.egg" => Some(include_str!("../rules/fractal_views.egg")),
        "rules/recursive_patterns.egg" => Some(include_str!("../rules/recursive_patterns.egg")),
        _ => None,
    }
}

#[cfg(target_arch = "wasm32")]
pub fn rule_text(file: &Path) -> Result<String> {
    embedded(&file.to_string_lossy())
        .map(str::to_owned)
        .ok_or_else(|| {
            format!(
                "rule program {} is not embedded in the wasm build",
                file.display()
            )
            .into()
        })
}

#[cfg(not(target_arch = "wasm32"))]
pub fn rule_text(file: &Path) -> Result<String> {
    Ok(std::fs::read_to_string(file)?)
}
