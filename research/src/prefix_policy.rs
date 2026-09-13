//! Compression-guided prefix work selection; not an egglog rule scheduler.
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug)]
pub struct Candidate {
    pub id: u64,
    pub marginal_bytes: i64,
    pub coarse: bool,
}
#[derive(Clone, Debug)]
pub struct Decision {
    pub candidate: u64,
    pub reason: &'static str,
    pub age: u64,
    pub marginal_bytes: i64,
}
pub struct Policy {
    context: Option<u64>,
    epoch: u64,
    tick: u64,
    born: BTreeMap<u64, u64>,
    period: u64,
    max_wait: u64,
    owed_exploration: bool,
    reserved_steps: u8,
}
impl Policy {
    pub fn new(epoch: u64, period: u64, max_wait: u64) -> Self {
        assert!(period > 0 && max_wait > 0);
        Self {
            context: None,
            epoch,
            tick: 0,
            born: BTreeMap::new(),
            period,
            max_wait,
            owed_exploration: false,
            reserved_steps: 0,
        }
    }
    pub fn bind_context(&mut self, context: u64, epoch: u64) -> Result<(), String> {
        if epoch != self.epoch {
            return Err("score model epoch mismatch".into());
        }
        if self.context.is_some_and(|old| old != context) {
            return Err("one policy state must belong to one block/context".into());
        }
        self.context = Some(context);
        Ok(())
    }
    fn synchronize(&mut self, epoch: u64, candidates: &[Candidate]) -> Result<(), String> {
        if epoch != self.epoch {
            return Err("score model epoch mismatch".into());
        }
        let ids: BTreeSet<_> = candidates.iter().map(|c| c.id).collect();
        if ids.len() != candidates.len() {
            return Err("duplicate candidate ID".into());
        }
        self.born.retain(|id, _| ids.contains(id));
        for c in candidates {
            self.born.entry(c.id).or_insert(self.tick);
        }
        Ok(())
    }
    /// Finish at most two protected continuation steps after a selected head.
    /// Any exploration slot crossed here is owed at the next ordinary choice.
    pub fn record_reserved(
        &mut self,
        epoch: u64,
        candidates: &[Candidate],
        id: u64,
    ) -> Result<Decision, String> {
        if self.tick == 0 || self.reserved_steps >= 2 {
            return Err("reservation requires a head and at most two continuation steps".into());
        }
        let c = candidates
            .iter()
            .find(|c| c.id == id)
            .ok_or("reserved continuation is unavailable")?;
        self.synchronize(epoch, candidates)?;
        let d = Decision {
            candidate: id,
            reason: "reserved-bridge-step",
            age: self.tick - self.born[&id],
            marginal_bytes: c.marginal_bytes,
        };
        self.owed_exploration |= self.tick % self.period == self.period - 1;
        self.born.remove(&id);
        self.tick += 1;
        self.reserved_steps += 1;
        Ok(d)
    }
    pub fn choose(
        &mut self,
        epoch: u64,
        candidates: &[Candidate],
    ) -> Result<Option<Decision>, String> {
        self.synchronize(epoch, candidates)?;
        let oldest = |c: &&Candidate| (self.born[&c.id], c.id);
        let aged = candidates
            .iter()
            .filter(|c| self.tick - self.born[&c.id] >= self.max_wait)
            .min_by_key(oldest);
        let exploration = if self.owed_exploration || self.tick % self.period == self.period - 1 {
            candidates
                .iter()
                .filter(|c| c.coarse || c.marginal_bytes <= 0)
                .min_by_key(oldest)
        } else {
            None
        };
        let positive = candidates
            .iter()
            .filter(|c| c.marginal_bytes > 0)
            .max_by_key(|c| {
                (
                    c.marginal_bytes,
                    std::cmp::Reverse(self.born[&c.id]),
                    std::cmp::Reverse(c.id),
                )
            });
        let selected = aged
            .map(|c| (c, "age-priority"))
            .or_else(|| exploration.map(|c| (c, "exploration-quota")))
            .or_else(|| positive.map(|c| (c, "positive-marginal-gain")))
            .or_else(|| {
                candidates
                    .iter()
                    .min_by_key(oldest)
                    .map(|c| (c, "fifo-no-positive-gain"))
            });
        let Some((c, reason)) = selected else {
            return Ok(None);
        };
        let decision = Decision {
            candidate: c.id,
            reason,
            age: self.tick - self.born[&c.id],
            marginal_bytes: c.marginal_bytes,
        };
        if reason == "exploration-quota"
            || (reason == "age-priority" && (c.coarse || c.marginal_bytes <= 0))
        {
            self.owed_exploration = false;
        }
        self.born.remove(&c.id);
        self.tick += 1;
        self.reserved_steps = 0;
        Ok(Some(decision))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn c(id: u64, g: i64, coarse: bool) -> Candidate {
        Candidate {
            id,
            marginal_bytes: g,
            coarse,
        }
    }
    #[test]
    fn coarse_branch_gets_an_opportunity_amid_positive_arrivals() {
        let mut p = Policy::new(1, 3, 10);
        for id in 1..=2 {
            assert_eq!(
                p.choose(1, &[c(99, -50, true), c(id, 100, false)])
                    .unwrap()
                    .unwrap()
                    .candidate,
                id
            );
        }
        let d = p
            .choose(1, &[c(99, -50, true), c(3, 100, false)])
            .unwrap()
            .unwrap();
        assert_eq!(d.candidate, 99);
        assert_eq!(d.reason, "exploration-quota");
    }
    #[test]
    fn age_prevents_a_lower_gain_branch_being_permanently_ignored() {
        let mut p = Policy::new(1, 99, 2);
        for id in 1..=2 {
            p.choose(1, &[c(99, 1, false), c(id, 100, false)]).unwrap();
        }
        assert_eq!(
            p.choose(1, &[c(99, 1, false), c(3, 100, false)])
                .unwrap()
                .unwrap()
                .candidate,
            99
        );
    }
    #[test]
    fn stale_scores_and_duplicate_ids_are_rejected() {
        let mut p = Policy::new(1, 4, 8);
        assert!(p.choose(2, &[c(1, 2, false)]).is_err());
        assert!(p.choose(1, &[c(1, 2, false), c(1, 3, true)]).is_err());
    }
    #[test]
    fn negative_gains_use_fifo_not_the_least_bad_compression() {
        let mut p = Policy::new(1, 4, 8);
        let d = p
            .choose(1, &[c(1, -100, false), c(2, -1, false)])
            .unwrap()
            .unwrap();
        assert_eq!(d.candidate, 1);
        assert_eq!(d.reason, "fifo-no-positive-gain");
    }
    #[test]
    fn policy_context_isolated_and_stale_binding_does_not_claim_it() {
        let mut p = Policy::new(1, 4, 8);
        assert!(p.bind_context(99, 2).is_err());
        p.bind_context(0, 1).unwrap();
        assert!(p.bind_context(1, 1).is_err());
        p.bind_context(0, 1).unwrap();
    }
    #[test]
    fn reservation_defers_but_does_not_erase_exploration() {
        let mut p = Policy::new(1, 2, 99);
        p.choose(1, &[c(1, 10, false), c(99, -1, true)]).unwrap();
        p.record_reserved(1, &[c(2, 20, false), c(99, -1, true)], 2)
            .unwrap();
        let d = p
            .choose(1, &[c(3, 100, false), c(99, -1, true)])
            .unwrap()
            .unwrap();
        assert_eq!(d.candidate, 99);
        assert_eq!(d.reason, "exploration-quota");
    }
    #[test]
    fn protected_steps_are_bounded_and_require_an_available_candidate() {
        let mut p = Policy::new(1, 4, 8);
        assert!(p.record_reserved(1, &[c(1, 1, false)], 1).is_err());
        p.choose(1, &[c(1, 1, false)]).unwrap();
        assert!(p.record_reserved(1, &[c(2, 1, false)], 3).is_err());
        p.record_reserved(1, &[c(2, 1, false)], 2).unwrap();
        p.record_reserved(1, &[c(3, 1, false)], 3).unwrap();
        assert!(p.record_reserved(1, &[c(4, 1, false)], 4).is_err());
    }
}
