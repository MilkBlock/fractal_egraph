//! Diagnostic only. R and Z count rows reaching and passing a projection.
//! K counts distinct selector keys per Run, rule, pivot and projection.
use super::*;
use rustc_hash::{FxHashMap, FxHashSet};

type Key = SmallVec<[u64; 4]>;
#[derive(Default, serde::Serialize)]
struct Counts {
    r: u64,
    k: u64,
    z: u64,
    version_reprobes: u64,
    same_membership: u64,
    still_empty: u64,
}
#[derive(Default)]
pub(crate) struct Survey {
    epoch: Option<(u64, usize, usize)>,
    seen: FxHashSet<(usize, Key)>,
    stats: BTreeMap<(usize, usize, usize), Counts>,
    previous: FxHashMap<(usize, Key), (u64, Option<Rows>)>,
}
impl Survey {
    pub fn observe(
        &mut self,
        epoch: u64,
        rule: usize,
        pivot: usize,
        projection: usize,
        version: u64,
        key: Key,
        result: &Option<Rows>,
    ) {
        if self.epoch != Some((epoch, rule, pivot)) {
            self.seen.clear();
            self.epoch = Some((epoch, rule, pivot));
        }
        let stats = self.stats.entry((rule, pivot, projection)).or_default();
        stats.r += 1;
        stats.z += u64::from(result.is_some());
        stats.k += u64::from(self.seen.insert((projection, key.clone())));
        // Compare the complete, deterministically ordered witness sets. An
        // unchanged empty set is also a valid negative result across versions.
        let prior = self
            .previous
            .entry((projection, key))
            .or_insert_with(|| (version, result.clone()));
        if prior.0 != version {
            stats.version_reprobes += 1;
            if prior.1 == *result {
                stats.same_membership += 1;
                stats.still_empty += u64::from(result.is_none());
            }
            *prior = (version, result.clone());
        }
    }
    pub fn report(&self) -> J {
        J::Array(
            self.stats
                .iter()
                .map(|(&(r, a, p), s)| json!({"rule":r,"pivot":a,"projection":p,"counts":s}))
                .collect(),
        )
    }
}
