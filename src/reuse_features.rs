//! Conservative operator/arity filters. A positive is never a match certificate.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Probe {
    No,
    Maybe,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    /// Every requested atom must be provided by this immutable stored body.
    BodyOnly,
    /// Query may read through an input, a parent enclosure, or an unknown graft.
    ThroughBoundary,
}
#[derive(Clone, Debug, Serialize)]
pub struct Features {
    bits: [u64; 4],
}
impl Features {
    pub fn bake(body: &RipSatEgraph) -> Self {
        let mut out = Self { bits: [0; 4] };
        for row in &body.graph().rows {
            out.insert(&row.op, row.args.len());
        }
        out
    }
    fn positions(op: &str, arity: usize) -> [usize; 3] {
        // Stable FNV seed; OR membership, never XOR (duplicates cannot cancel).
        let mut h = 14695981039346656037u64;
        for b in op.bytes().chain([0]).chain((arity as u64).to_le_bytes()) {
            h = (h ^ u64::from(b)).wrapping_mul(1099511628211);
        }
        [
            (h & 255) as usize,
            (h.rotate_right(21) & 255) as usize,
            (h.rotate_right(43) & 255) as usize,
        ]
    }
    fn insert(&mut self, op: &str, arity: usize) {
        for p in Self::positions(op, arity) {
            self.bits[p / 64] |= 1 << (p % 64);
        }
    }
    pub fn probe<'a>(
        &self,
        required: impl IntoIterator<Item = (&'a str, usize)>,
        region: Region,
    ) -> Probe {
        if region == Region::ThroughBoundary {
            return Probe::Maybe;
        }
        for (op, arity) in required {
            if Self::positions(op, arity)
                .iter()
                .any(|p| self.bits[p / 64] & (1 << (p % 64)) == 0)
            {
                return Probe::No;
            }
        }
        Probe::Maybe
    }
}
