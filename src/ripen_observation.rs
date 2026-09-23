//! Explicit observation limits, checked between native sweeps; never a saturation proof.
use crate::pipeline::Result;
use serde::{Deserialize, Serialize};
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Limits {
    pub milliseconds: Option<u64>,
    pub facts: Option<usize>,
}
impl Limits {
    pub fn from_env() -> Result<Self> {
        let limits = Self {
            milliseconds: std::env::var("EGG_LAYOUT_RIPEN_OBSERVE_MS")
                .ok()
                .map(|s| s.parse())
                .transpose()?,
            facts: std::env::var("EGG_LAYOUT_RIPEN_OBSERVE_FACTS")
                .ok()
                .map(|s| s.parse())
                .transpose()?,
        };
        limits.validate()?;
        Ok(limits)
    }
    pub fn validate(&self) -> Result {
        if self.milliseconds == Some(0) || self.facts == Some(0) {
            return Err("ripen observation limits must be positive".into());
        }
        Ok(())
    }
    pub fn reached(&self, seconds: f64, facts: usize) -> Vec<&'static str> {
        let mut reasons = vec![];
        if self
            .milliseconds
            .is_some_and(|ms| seconds * 1000. >= ms as f64)
        {
            reasons.push("time")
        }
        if self.facts.is_some_and(|n| facts >= n) {
            reasons.push("facts")
        }
        reasons
    }
}
