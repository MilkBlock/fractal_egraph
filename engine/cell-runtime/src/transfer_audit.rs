//! Untimed inference counters. The release benchmark compiles without this feature.
use crate::value::Value;
use std::{cell::Cell, time::Instant};
#[derive(Clone, Copy, Default, serde::Serialize)]
pub struct Counts {
    pub decoded_rows: u64,
    pub row_fields: u64,
    pub fresh_bind_copies: u64,
    pub binding_seal_copies: u64,
    pub parent_projected_fields: u64,
    pub owned_frame_fields: u64,
    pub value_clones: u64,
    pub spine_owner_checks: u64,
    pub spine_route_lookups: u64,
    pub spine_route_hits: u64,
    pub sibling_wire_attempts: u64,
    pub stored_parent_frames: u64,
    pub parent_reads_rhs: u64,
    pub parent_reads_other: u64,
}
thread_local! {
    static DIGESTS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    static IN_RHS: Cell<bool> = const { Cell::new(false) };
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    static COUNTS: Cell<Counts> = Cell::default();
}
// Measure a sample of outermost calls. Nested projections are included in
// the outer read and cannot be charged a second time.
#[derive(Clone, Copy, Default, serde::Serialize)]
pub struct Cost {
    calls: u64,
    samples: u64,
    elapsed_ns: u64,
    empty_clock_ns: u64,
}
#[derive(Clone, Copy)]
pub(crate) enum Region {
    SpineProof,
    RhsParent,
    OtherParent,
    RhsOwned,
    OtherOwned,
    RhsReplay,
    OtherReplay,
    SiblingWire,
    SiblingFallback,
}
thread_local! {
    static COSTS: Cell<[Cost; 9]> = Cell::default();
    static IN_REGION: Cell<bool> = const { Cell::new(false) };
}
pub(crate) struct Sample {
    region: Region,
    start: Option<Instant>,
}
impl Sample {
    pub(crate) fn begin(region: Region) -> Option<Self> {
        if !active() || IN_REGION.replace(true) {
            return None;
        }
        let mut costs = COSTS.get();
        let c = &mut costs[region as usize];
        c.calls += 1;
        let sample = c.calls % 127 == 64;
        if sample {
            c.samples += 1;
            c.empty_clock_ns += Instant::now().elapsed().as_nanos() as u64;
        }
        COSTS.set(costs);
        Some(Self {
            region,
            start: sample.then(Instant::now),
        })
    }
    pub(crate) fn sampled(&self) -> bool {
        self.start.is_some()
    }
    pub(crate) fn read(owned: bool) -> Option<Self> {
        let region = match (IN_RHS.get(), owned) {
            (true, false) => Region::RhsParent,
            (false, false) => Region::OtherParent,
            (true, true) => Region::RhsOwned,
            (false, true) => Region::OtherOwned,
        };
        Self::begin(region)
    }
}
impl Drop for Sample {
    fn drop(&mut self) {
        if let Some(start) = self.start {
            let ns = start.elapsed().as_nanos() as u64;
            let mut costs = COSTS.get();
            costs[self.region as usize].elapsed_ns += ns;
            COSTS.set(costs);
        }
        IN_REGION.set(false);
    }
}
/// Replay the very same column against its pinned parent, after timing the
/// owned lookup. The extra diagnostic read must not enter any work counter.
pub(crate) fn check_parent(
    value: &Value,
    read: impl FnOnce() -> Result<Value, String>,
) -> Result<(), String> {
    let region = if IN_RHS.get() {
        Region::RhsReplay
    } else {
        Region::OtherReplay
    };
    ACTIVE.set(false);
    let empty_ns = Instant::now().elapsed().as_nanos() as u64;
    let start = Instant::now();
    let replay = read();
    let elapsed_ns = start.elapsed().as_nanos() as u64;
    ACTIVE.set(true);
    let mut costs = COSTS.get();
    let c = &mut costs[region as usize];
    c.samples += 1;
    c.calls += 1;
    c.empty_clock_ns += empty_ns;
    c.elapsed_ns += elapsed_ns;
    COSTS.set(costs);
    if &replay? != value {
        return Err("owned and projected slot values differ".into());
    }
    Ok(())
}
pub fn costs() -> serde_json::Value {
    let [
        proof,
        rhs_parent,
        other_parent,
        rhs_owned,
        other_owned,
        rhs_replay,
        other_replay,
        sibling_wire,
        sibling_fallback,
    ] = COSTS.get();
    serde_json::json!({"spine_proof":proof, "rhs_parent":rhs_parent, "other_parent":other_parent,
        "rhs_owned":rhs_owned, "other_owned":other_owned, "rhs_replay":rhs_replay, "other_replay":other_replay, "sibling_wire":sibling_wire, "sibling_fallback":sibling_fallback})
}
pub fn active() -> bool {
    ACTIVE.get()
}
pub fn set_active(on: bool) {
    ACTIVE.set(on);
}
pub fn counts() -> Counts {
    COUNTS.get()
}
pub(crate) fn count(f: impl FnOnce(&mut Counts)) {
    if active() {
        let mut c = counts();
        f(&mut c);
        COUNTS.set(c);
    }
}
pub(crate) struct RhsScope(bool);
impl RhsScope {
    pub fn enter() -> Self {
        Self(IN_RHS.replace(true))
    }
}
impl Drop for RhsScope {
    fn drop(&mut self) {
        IN_RHS.set(self.0);
    }
}
pub(crate) fn parent_read() {
    count(|c| {
        if IN_RHS.get() {
            c.parent_reads_rhs += 1
        } else {
            c.parent_reads_other += 1
        }
    });
}
pub(crate) fn row(fields: usize) {
    count(|c| {
        c.decoded_rows += 1;
        c.row_fields += fields as u64;
    });
}
impl Clone for Value {
    fn clone(&self) -> Self {
        count(|c| c.value_clones += 1);
        match self {
            Self::Eq(x) => Self::Eq(*x),
            Self::Int(x) => Self::Int(*x),
            Self::Float(x) => Self::Float(*x),
            Self::Text(x) => Self::Text(x.clone()),
            Self::Set(x) => Self::Set(x.clone()),
            Self::Key(x) => Self::Key(x.clone()),
        }
    }
}
pub(crate) fn record_run(rows: impl Iterator<Item = impl serde::Serialize>) -> Result<(), String> {
    if active() {
        use sha2::Digest;
        let mut digest = sha2::Sha256::new();
        for row in rows {
            serde_json::to_writer(&mut digest, &row).map_err(|e| e.to_string())?;
        }
        DIGESTS.with_borrow_mut(|runs| runs.push(format!("{:x}", digest.finalize())));
    }
    Ok(())
}
pub fn take_digests() -> Vec<String> {
    DIGESTS.take()
}
