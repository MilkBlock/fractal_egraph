use crate::cache::{hash_bytes, read_json};
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub struct Case {
    pub name: String,
    pub bytes: Vec<u8>,
    pub spec: Value,
}
pub fn input(root: &Path, c: &Value) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for part in c["parts"].as_array().context("input parts")? {
        let part = PathBuf::from(part.as_str().context("part path")?);
        ensure!(
            !part.is_absolute()
                && !part
                    .components()
                    .any(|x| x == std::path::Component::ParentDir),
            "part escapes suite"
        );
        bytes.extend(fs::read(root.join(part))?);
    }
    ensure!(
        Some(hash_bytes(&bytes).as_str()) == c["sha256"].as_str(),
        "input checksum mismatch"
    );
    Ok(bytes)
}
pub fn load(path: &Path, selected: &[String], all: bool) -> Result<Vec<Case>> {
    let suite = read_json(path)?;
    ensure!(
        suite["schema"] == "eggfuse-profile-suite/v1",
        "unsupported suite"
    );
    let root = path.parent().context("suite directory")?;
    let cases = suite["cases"].as_array().context("suite cases")?;
    for name in selected {
        ensure!(
            cases.iter().any(|c| c["name"].as_str() == Some(name)),
            "unknown case {name}"
        );
    }
    let mut out = Vec::new();
    for c in cases {
        let name = c["name"].as_str().context("case name")?;
        ensure!(
            !name.is_empty() && name.chars().all(|x| x.is_ascii_alphanumeric() || x == '-'),
            "invalid case name"
        );
        if !(all
            || selected.iter().any(|x| x == name)
            || (selected.is_empty() && c["default"] == true))
        {
            continue;
        }
        ensure!(
            c["pass_number"] == 3,
            "suite requires actual Pass 3 exports"
        );
        let bytes = input(root, c).with_context(|| format!("source {name}"))?;
        out.push(Case {
            name: name.into(),
            bytes,
            spec: c.clone(),
        });
    }
    ensure!(!out.is_empty(), "empty suite selection");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_has_three_full_passes_and_raytrace_is_explicit() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../experiments/profile_suite/suite.json");
        let small = load(&root, &[], false)?;
        assert_eq!(small.len(), 3);
        assert!(small.iter().all(|x| x.name != "raytrace"));
        let ray = load(&root, &["raytrace".into()], false)?;
        assert_eq!(ray.len(), 1);
        assert!(ray[0].bytes.len() > small[0].bytes.len());
        assert_eq!(load(&root, &[], true)?.len(), 4);
        assert!(load(&root, &["typo".into()], false).is_err());
        Ok(())
    }
}
