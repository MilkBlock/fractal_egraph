use serde_json::Value;
use sha2::{Digest, Sha256};
pub fn digest(value: &Value) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).expect("JSON value serialization"))
    )
}
