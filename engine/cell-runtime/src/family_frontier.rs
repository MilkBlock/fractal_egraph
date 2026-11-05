//! Counterfactual replay of a complete entry admission across consumers.
//! Same Run, watermark, entry node, Sieve conditions and ordered Free Join
//! projections (including owner tie direction). Never replay a Spine-owned pivot.
use super::*;
use crate::entry_schedule::{Batch, Seed};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Default, Serialize)]
struct Costs {
    calls: u64,
    builds: u64,
    hits: u64,
    rows: u64,
    baseline_hit_ns: u64,
    copy_ns: u64,
    release_ns: u64,
    setup_ns: u64,
    invalid: u64,
    oversize: u64,
    union_store_ns: u64,
}
struct Saved {
    rid: usize,
    rows: Vec<(Dependency, Vec<(usize, Rc<Vec<Dependency>>)>)>,
}
#[derive(Default)]
struct State {
    epoch: Option<u64>,
    banks: BTreeMap<u64, Saved>,
    retained: usize,
    cost: Costs,
}
pub(super) struct Trace {
    pub batches: Vec<[u64; 9]>,
    signatures: Vec<Vec<usize>>,
    projections: Vec<Vec<Vec<usize>>>,
    states: BTreeMap<(String, usize, usize), State>,
    members: BTreeMap<(String, usize, usize), BTreeSet<usize>>,
}
impl Trace {
    pub fn new(p: &Program, sieve: &crate::sieve::Sieve) -> Self {
        let plan = p
            .entry_join
            .clone()
            .unwrap_or_else(|| Rc::new(crate::entry_join::Plan::derive(p)));
        let mut ids = BTreeMap::new();
        let mut signatures = vec![];
        let mut projections = vec![];
        for (r, atoms) in plan.entries.iter().enumerate() {
            let mut sig = vec![];
            let mut targets = vec![];
            for (pivot, steps) in atoms.iter().enumerate() {
                let key = (
                    p.rules[r].direct.is_some(),
                    sieve.survey_guards(r, pivot),
                    steps
                        .iter()
                        .map(|s| (s.projection, s.atom < pivot))
                        .collect::<Vec<_>>(),
                );
                let new = ids.len();
                sig.push(*ids.entry(key).or_insert(new));
                targets.push(steps.iter().map(|s| s.atom).collect());
            }
            signatures.push(sig);
            projections.push(targets);
        }
        let mut members = BTreeMap::<_, BTreeSet<_>>::new();
        for (set, rs) in &p.sets {
            for &r in rs {
                for (atom, node) in p.entry_schedule.survey_nodes(r) {
                    members
                        .entry((set.clone(), node, signatures[r][atom]))
                        .or_default()
                        .insert(r);
                }
            }
        }
        members.retain(|_, v| v.len() > 1);
        Self {
            batches: vec![],
            signatures,
            projections,
            states: BTreeMap::new(),
            members,
        }
    }
    pub fn observe(
        &mut self,
        set: &str,
        epoch: u64,
        union: bool,
        rid: usize,
        batch: Batch<'_>,
        join: bool,
        actual: &[Seed],
        domains: &[super::free_join::Domains],
        base_ns: u64,
    ) {
        let atom = batch.atom();
        let key = (set.to_owned(), batch.node(), self.signatures[rid][atom]);
        if !self.members.contains_key(&key) {
            return;
        }
        let s = self.states.entry(key).or_default();
        s.cost.calls += 1;
        if s.epoch != Some(epoch) {
            let at = Instant::now();
            s.banks.clear();
            s.retained = 0;
            s.epoch = Some(epoch);
            s.cost.release_ns += at.elapsed().as_nanos() as u64;
        }
        let wm = batch.watermark();
        let t = Instant::now();
        if let Some(saved) = s.banks.get(&wm) {
            if saved.rid == rid {
                return;
            }
            let mut packets = Vec::with_capacity(saved.rows.len());
            let mut subtries = Vec::with_capacity(if join { saved.rows.len() } else { 0 });
            for (w, ds) in &saved.rows {
                let Some(packet) = batch.packet(*w) else {
                    s.cost.invalid += 1;
                    return;
                };
                packets.push(batch.seed(packet));
                if join {
                    if ds.len() != self.projections[rid][atom].len() {
                        s.cost.invalid += 1;
                        return;
                    }
                    subtries.push(
                        ds.iter()
                            .zip(&self.projections[rid][atom])
                            .map(|((n, rows), to)| Subtrie {
                                atom: *to,
                                projected_columns: *n,
                                rows: rows.clone(),
                            })
                            .collect::<Vec<_>>(),
                    );
                }
            }
            let ns = t.elapsed().as_nanos() as u64;
            let same = packets.len() == actual.len()
                && packets
                    .iter()
                    .zip(actual)
                    .all(|(a, b)| a.packet.row.witness == b.packet.row.witness && a.atom == b.atom)
                && subtries.len() == domains.len()
                && subtries.iter().zip(domains).all(|(a, b)| {
                    a.len() == b.len()
                        && a.iter().zip(b).all(|(x, y)| {
                            x.atom == y.atom
                                && x.projected_columns == y.projected_columns
                                && x.rows == y.rows
                        })
                });
            if same {
                s.cost.hits += 1;
                s.cost.rows += packets.len() as u64;
                s.cost.baseline_hit_ns += base_ns;
                s.cost.copy_ns += ns
            } else {
                s.cost.invalid += 1
            }
        } else {
            if s.retained + actual.len() > 65536 {
                s.cost.oversize += 1;
                return;
            }
            let rows = actual
                .iter()
                .enumerate()
                .map(|(i, seed)| {
                    (
                        seed.packet.row.witness,
                        if join {
                            domains[i]
                                .iter()
                                .map(|d| (d.projected_columns, d.rows.clone()))
                                .collect()
                        } else {
                            vec![]
                        },
                    )
                })
                .collect();
            s.banks.insert(wm, Saved { rid, rows });
            s.retained += actual.len();
            let ns = t.elapsed().as_nanos() as u64;
            s.cost.builds += 1;
            s.cost.setup_ns += ns;
            if union {
                s.cost.union_store_ns += ns
            }
        }
    }
    pub fn report(&self) -> J {
        J::Array(self.states.iter().map(|(k,s)|json!({"set":k.0,"node":k.1,"signature":k.2,"members":self.members[k],"cost":s.cost})).collect())
    }
}
