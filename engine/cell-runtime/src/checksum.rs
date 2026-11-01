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
