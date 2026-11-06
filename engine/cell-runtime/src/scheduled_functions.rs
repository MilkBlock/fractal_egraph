//! Function versions remain committed in Graph. Derived query representations
//! cross their publication frontier only when a source-scheduled reader runs.
use crate::{
    graph::{Dependency, Graph},
    program::*,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value as J, json};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Plan {
    pub tables: Vec<bool>,
    pub reads: Vec<Vec<usize>>,
    pub recursive: Vec<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        change_batch::Publication, neighbor::Context, physical_plan::Bound, query::QueryIndexes,
        range_index::RangeIndex, value::Value,
    };
    #[test]
    fn delayed_range_publication_preserves_current_membership_without_rebuilding() {
        let table = json!({"name":"f","schema":["i64","i64","i64"],"constructor":false,"virtual_fact":false,"internal":false,"merge":{"var":"new"}});
        let mut p:Program=serde_json::from_value(json!({"multi_stage":crate::multistage::MultiStage::default(),"entry_schedule":crate::entry_schedule::Plan::default(),"tables":[table],"templates":[],"rules":[],"sets":{},"control":[],"initial":[],"table_templates":[],"artifact":{}})).unwrap();
        p.templates.push(Template {
            name: "reader".into(),
            sorts: vec![json!({"kind":"i64"}); 3],
            defs: Default::default(),
            ops: Default::default(),
            parent_columns: Default::default(),
        });
        p.rules.push(Rule {
            constructor_only: false,
            template: 0,
            query: std::rc::Rc::new(vec![Atom {
                call: Call::Table(0),
                args: vec![Arg::Slot(0), Arg::Slot(1), Arg::Slot(2)],
            }]),
            inputs: std::rc::Rc::new(vec![0, 1, 2]),
            seminaive: true,
            direct: None,
        });
        let mut state = State::new(&p);
        state.enabled = true;
        let mut g = Graph::new(p).unwrap();
        g.merge_programs_enabled = false;
        for i in 0..64 {
            g.set_function(0, vec![Value::Int(0), Value::Int(i)], Value::Int(i))
                .unwrap();
        }
        g.changes.clear();
        let mut range = RangeIndex::default();
        let mut ix = QueryIndexes::default();
        let mut publication = Publication::default();
        let mut neighbor = Context::new(&g.program, false);
        let rows = std::rc::Rc::new(g.witnesses(0));
        assert_eq!(
            range
                .select(&g, 0, (0, Value::Int(0)), 2, Bound::Lt, 16, &rows)
                .unwrap()
                .len(),
            16
        );
        for v in [55, 60] {
            g.epoch += 1;
            g.set_function(0, vec![Value::Int(0), Value::Int(5)], Value::Int(v))
                .unwrap();
            state.capture(&g, |_| true);
            g.changes.clear();
        }
        let ids = state.take(0);
        assert_eq!(ids.len(), 1);
        assert_eq!(state.stats.coalesced_headers, 1);
        publication
            .activate_columns(&g, 0, &ids, &mut ix, Some(&mut range), &mut neighbor)
            .unwrap();
        let found = range
            .select(&g, 0, (0, Value::Int(0)), 2, Bound::Lt, 16, &rows)
            .unwrap();
        assert_eq!(found.len(), 15);
        assert!(!found.contains(&Dependency::Function(5)));
        assert_eq!(range.stats.builds, 1);
        assert_eq!(range.stats.update_rows, 1);
        assert_eq!(range.stats.invalidated_axes, 0);
    }
}
impl Plan {
    pub fn derive(p: &Program) -> Self {
        let reads: Vec<Vec<usize>> = p
            .rules
            .iter()
            .map(|r| {
                r.query
                    .iter()
                    .filter_map(|a| {
                        if let Call::Table(t) = a.call
                            && !p.tables[t].virtual_fact
                        {
                            Some(t)
                        } else {
                            None
                        }
                    })
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect()
            })
            .collect();
        let mut tables = vec![false; p.tables.len()];
        for r in &reads {
            for &t in r {
                tables[t] = true;
            }
        }
        // A cost policy, not a semantic restriction: cross-key recursive
        // propagation usually consumes its own delta in the next iteration.
        let mut recursive = vec![false; p.tables.len()];
        for r in &p.rules {
            let template = &p.templates[r.template];
            for op in template.ops.iter() {
                let Op::Set(t, key, _) = op else { continue };
                if !tables[*t] {
                    continue;
                }
                let target: Vec<_> = key
                    .iter()
                    .map(|a| crate::multistage::normalize(template, a))
                    .collect();
                if r.query.iter().any(|a| {
                    matches!(a.call,Call::Table(table) if table==*t)
                        && a.args[..a.args.len() - 1]
                            .iter()
                            .map(|v| crate::multistage::normalize(template, v))
                            .collect::<Vec<_>>()
                            != target
                }) {
                    recursive[*t] = true;
                }
            }
        }
        for (t, flag) in tables.iter_mut().enumerate() {
            *flag &= !recursive[t];
        }
        let reads = reads
            .into_iter()
            .map(|r| r.into_iter().filter(|t| tables[*t]).collect())
            .collect();
        Self {
            tables,
            reads,
            recursive,
        }
    }
}
#[derive(Default, Serialize)]
struct Counts {
    unobserved_headers: u64,
    captured_headers: u64,
    coalesced_headers: u64,
    flushed_headers: u64,
    retired_index_rows: u64,
    activations: u64,
    peak_pending: usize,
    capture_seconds: f64,
    activation_seconds: f64,
}
pub(crate) struct State {
    pub enabled: bool,
    plan: Plan,
    pending: Vec<Vec<usize>>,
    dirty: Vec<u64>,
    live: usize,
    stats: Counts,
    by_table: Vec<[u64; 4]>,
}
impl State {
    pub fn new(p: &Program) -> Self {
        Self {
            enabled: false,
            plan: Plan::derive(p),
            pending: vec![vec![]; p.tables.len()],
            dirty: vec![],
            live: 0,
            stats: Default::default(),
            by_table: vec![[0; 4]; p.tables.len()],
        }
    }
    #[inline]
    pub fn defers(&self, t: usize) -> bool {
        self.enabled && self.plan.tables[t]
    }
    pub fn has_work(&self) -> bool {
        self.enabled && self.live > 0
    }
    pub fn capture(&mut self, g: &Graph, observed: impl Fn(usize) -> bool) {
        if !self.enabled {
            return;
        }
        let start = std::time::Instant::now();
        for (t, changes) in g.changes.tables() {
            if !self.defers(t) {
                continue;
            }
            if !observed(t) {
                // Cold readers build from the authoritative current Graph.
                self.stats.unobserved_headers += changes.len() as u64;
                self.by_table[t][3] += changes.len() as u64;
                continue;
            }
            self.by_table[t][0] += changes.len() as u64;
            for &w in changes.keys() {
                let Dependency::Function(i) = w else {
                    unreachable!("compiler-certified function table")
                };
                self.stats.captured_headers += 1;
                let word = i / 64;
                let bit = 1u64 << (i % 64);
                if self.dirty.len() <= word {
                    self.dirty.resize(word + 1, 0);
                }
                if self.dirty[word] & bit != 0 {
                    self.stats.coalesced_headers += 1;
                    self.by_table[t][1] += 1;
                } else {
                    self.dirty[word] |= bit;
                    self.pending[t].push(i);
                    self.live += 1;
                }
            }
        }
        self.stats.peak_pending = self.stats.peak_pending.max(self.live);
        self.stats.capture_seconds += start.elapsed().as_secs_f64();
    }
    pub fn reads(&self, rid: usize) -> &[usize] {
        &self.plan.reads[rid]
    }
    pub fn take(&mut self, t: usize) -> Vec<usize> {
        let ids = std::mem::take(&mut self.pending[t]);
        self.live -= ids.len();
        for &i in &ids {
            self.dirty[i / 64] &= !(1u64 << (i % 64));
        }
        self.stats.flushed_headers += ids.len() as u64;
        self.by_table[t][2] += ids.len() as u64;
        ids
    }
    pub fn recycle(&mut self, t: usize, mut ids: Vec<usize>) {
        ids.clear();
        self.pending[t] = ids;
    }
    pub fn has_pending(&self, t: usize) -> bool {
        !self.pending[t].is_empty()
    }
    pub fn retired(&mut self, n: u64) {
        self.stats.retired_index_rows += n;
    }
    pub fn activated(&mut self, seconds: f64) {
        self.stats.activations += 1;
        self.stats.activation_seconds += seconds;
    }
    pub fn report(&self, p: &Program) -> J {
        json!({"enabled":self.enabled,"certified_tables":self.plan.tables.iter().filter(|v|**v).count(),
            "stats":self.stats,"pending":self.live,
            "pending_id_capacity_bytes":self.pending.iter().map(|v|v.capacity()*std::mem::size_of::<usize>()).sum::<usize>(),
            "dirty_capacity_bytes":self.dirty.capacity()*8,
            "eager_recursive_tables":self.plan.recursive.iter().enumerate().filter(|(_,v)|**v).map(|(t,_)|p.tables[t].name.as_str()).collect::<Vec<_>>(),
            "tables":self.by_table.iter().enumerate().filter(|(_,v)|v[0]+v[3]>0).map(|(t,v)|json!({"table":p.tables[t].name,"captured":v[0],"coalesced":v[1],"flushed":v[2],"unobserved":v[3],"pending":self.pending[t].len()})).collect::<Vec<_>>(),
            "scope":"derived columns, ranges, neighbor ports and shared-entry notifications; Graph merges and Spine publication remain eager"})
    }
}
