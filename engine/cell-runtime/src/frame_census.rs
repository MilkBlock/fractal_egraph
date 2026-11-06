//! Whole producer lifetimes, separate from logical fact and RHS call counts.
use super::*;
#[derive(Default, Serialize)]
struct Counts {
    registered_frames: u64,
    single_fact: u64,
    all_facts_deferred: u64,
    all_facts_deferred_and_unretained: u64,
    empty_after_repair: u64,
    empty_and_unretained_after_repair: u64,
    persistent_single: u64,
    persistent_single_unretained: u64,
    input_slots: u64,
}
#[derive(Default)]
pub(super) struct Census {
    pub enabled: bool,
    pending: BTreeSet<u32>,
    counts: BTreeMap<usize, Counts>,
}
impl Graph {
    pub fn configure_frame_census(&mut self, enabled: bool) {
        self.frame_census.enabled = enabled;
    }
    pub(super) fn census_frame(&mut self, id: u32) {
        if !self.frame_census.enabled {
            return;
        }
        let Some(c) = self.cells.get(&id) else { return };
        if c.started_epoch != self.epoch
            || c.facts.is_empty()
            || !self.frame_census.pending.insert(id)
        {
            return;
        }
        let counts = self.frame_census.counts.entry(c.template).or_default();
        counts.registered_frames += 1;
        counts.single_fact += u64::from(c.facts.len() == 1);
        counts.input_slots += c.values.len() as u64;
        let deferred = c.facts.keys().all(|&op| {
            self.deferred_incidence
                .live
                .contains(&FactRef { cell: id, op })
        });
        counts.all_facts_deferred += u64::from(deferred);
        counts.all_facts_deferred_and_unretained +=
            u64::from(deferred && self.parent_users.get(&id).copied().unwrap_or(0) == 0);
    }
    pub(super) fn census_repaired(&mut self) {
        for id in std::mem::take(&mut self.frame_census.pending) {
            let Some(c) = self.cells.get(&id) else {
                continue;
            };
            let n = self.frame_census.counts.get_mut(&c.template).unwrap();
            let unretained = self.parent_users.get(&id).copied().unwrap_or(0) == 0;
            n.empty_after_repair += u64::from(c.facts.is_empty());
            n.empty_and_unretained_after_repair += u64::from(c.facts.is_empty() && unretained);
            n.persistent_single += u64::from(c.facts.len() == 1);
            n.persistent_single_unretained += u64::from(c.facts.len() == 1 && unretained);
        }
    }
    pub fn frame_census_report(&self) -> J {
        json!({"enabled":self.frame_census.enabled,"templates":self.frame_census.counts.iter().map(|(t,c)|json!({"template":t,"name":self.program.templates[*t].name,"ops":self.program.templates[*t].ops,"counts":c})).collect::<Vec<_>>()})
    }
}
