use serde_json::Value;
use sha2::{Digest, Sha256};
pub fn digest(value: &Value) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).expect("JSON value serialization"))
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn diagnostic_times_survive_checkpoint_roundtrip() {
        let value =
            serde_json::json!({"match_seconds":3.3300000000000003e-7,"entry_seconds":2.08e-7});
        let bytes = serde_json::to_vec(&value).unwrap();
        let restored: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(super::digest(&value), super::digest(&restored));
    }
}

/// Stable JSON used by the Recipe and Cell artifact formats. Native frontends
/// may enable serde_json's preserve_order feature through Cargo unification.
pub fn canonical(value: &Value, spaced: bool) -> String {
    let mut sorted = value.clone();
    sorted.sort_all_objects();
    let mut out = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for c in sorted.to_string().chars() {
        if !c.is_ascii() || c == '\u{7f}' {
            use std::fmt::Write;
            for unit in c.encode_utf16(&mut [0; 2]) {
                write!(out, "\\u{unit:04x}").unwrap();
            }
        } else {
            out.push(c);
        }
        if quoted {
            if !escaped && c == '"' {
                quoted = false;
            }
            escaped = !escaped && c == '\\';
        } else if c == '"' {
            quoted = true;
        } else if spaced && matches!(c, ':' | ',') {
            out.push(' ');
        }
    }
    out
}
pub fn artifact_digest(value: &Value) -> String {
    format!("{:x}", Sha256::digest(canonical(value, false).as_bytes()))
}
