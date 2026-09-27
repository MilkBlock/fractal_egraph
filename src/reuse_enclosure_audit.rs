//! Opt-in causal-boundary audit. Uses pre-step enclosure membership and actual
//! keyed LHS reads. It does not retain full match history or claim an optimal partition.
use crate::{pipeline::Result, reuse_graph::adaptive::Macro};
use egglog::{EGraph, TraceBatch, TraceSession, Value as Native, WriteOutcome};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
type Fact = (String, Vec<Native>);
type Body = Vec<Fact>;
type Graph = BTreeMap<Native, BTreeSet<Fact>>;
#[derive(Default)]
pub struct Audit {
    bodies: BTreeMap<Body, bool>, // any actual aliasing relative to symbolic slots
    owners: BTreeMap<Fact, BTreeSet<usize>>,
    aliases: BTreeSet<usize>,
    rekeyed: BTreeSet<usize>,
    before: usize,
    pre_graph: Graph,
    last_graph: Graph,
    regions: Vec<(BTreeSet<Native>, BTreeSet<Native>)>,
    between_output: BTreeSet<usize>,
    between_input: BTreeSet<usize>,
}
fn canonical(eg: &EGraph, f: &Fact) -> Fact {
    let schema = eg.get_function(&f.0).unwrap().schema();
    (
        f.0.clone(),
        f.1.iter()
            .zip(schema.input.iter().chain([&schema.output]))
            .map(|(v, t)| eg.get_canonical_value(*v, t))
            .collect(),
    )
}
fn graph(eg: &EGraph) -> Graph {
    let mut index = Graph::new();
    for op in eg.get_function_names() {
        let f = eg.get_function(&op).unwrap();
        if f.is_let_binding() || f.schema().output.name() != "Math" {
            continue;
        }
        eg.function_for_each(&op, |r| {
            let row = canonical(eg, &(op.clone(), r.vals.to_vec()));
            index.entry(*row.1.last().unwrap()).or_default().insert(row);
        })
        .unwrap();
    }
    index
}
fn grew(eg: &EGraph, old: &Graph, new: &Graph) -> BTreeSet<Native> {
    let mut out = BTreeSet::new();
    for (id, rows) in old {
        let mapped = rows
            .iter()
            .map(|r| canonical(eg, r))
            .collect::<BTreeSet<_>>();
        let Some(first) = mapped.first() else {
            continue;
        };
        let key = *first.1.last().unwrap();
        if new
            .get(&key)
            .is_some_and(|r| r.iter().any(|r| !mapped.contains(r)))
        {
            out.insert(*id);
        }
    }
    out
}
fn region(eg: &EGraph, body: &Body) -> (BTreeSet<Native>, BTreeSet<Native>) {
    let outputs = body
        .iter()
        .map(|r| *r.1.last().unwrap())
        .collect::<BTreeSet<_>>();
    let mut inputs = BTreeSet::new();
    for (op, row) in body {
        for (v, t) in row.iter().zip(&eg.get_function(op).unwrap().schema().input) {
            if t.name() == "Math" && !outputs.contains(v) {
                inputs.insert(*v);
            }
        }
    }
    (outputs, inputs)
}
impl Audit {
    /// Immutable proof index for one read-only matching boundary. This is sound
    /// only for this runtime's positive constructor/equality rule environment.
    pub fn rhs_filter(
        &self,
        eg: &EGraph,
    ) -> std::sync::Arc<dyn Fn(&str, &[(&str, &[Native])]) -> bool + Send + Sync> {
        let bad = self
            .aliases
            .union(&self.rekeyed)
            .copied()
            .collect::<BTreeSet<_>>();
        let owners = self
            .owners
            .iter()
            .map(|(f, ids)| {
                (
                    f.clone(),
                    ids.difference(&bad).copied().collect::<BTreeSet<_>>(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let arities = eg
            .get_function_names()
            .into_iter()
            .filter_map(|n| {
                let f = eg.get_function(&n).unwrap();
                (f.schema().output.name() == "Math").then_some((n, f.schema().input.len() + 1))
            })
            .collect::<BTreeMap<_, _>>();
        std::sync::Arc::new(move |rule, reads| {
            if reads.is_empty()
                || !(rule.starts_with("adaptive_source_") || rule.starts_with("adaptive_baked_"))
            {
                return false;
            }
            let mut common: Option<BTreeSet<usize>> = None;
            for (op, row) in reads {
                let Some(n) = arities.get(*op) else {
                    return false;
                };
                if row.len() < *n {
                    return false;
                }
                let Some(ids) = owners.get(&(op.to_string(), row[..*n].to_vec())) else {
                    return false;
                };
                common = Some(match common {
                    None => ids.clone(),
                    Some(old) => old.intersection(ids).copied().collect(),
                });
                if common.as_ref().unwrap().is_empty() {
                    return false;
                }
            }
            common.is_some_and(|s| !s.is_empty())
        })
    }
    pub fn observe(&mut self, eg: &EGraph, m: &Macro, bindings: &[Vec<Native>]) -> Result<()> {
        for binding in bindings {
            let mut body = m
                .template
                .iter()
                .map(|(op, cols)| {
                    canonical(
                        eg,
                        &(op.clone(), cols.iter().map(|i| binding[*i]).collect()),
                    )
                })
                .collect::<Vec<_>>();
            body.sort();
            body.dedup();
            let mut seen = BTreeSet::new();
            let mut alias = false;
            // Symbolic template slots are distinct by (sort, canonical value).
            let math = eg
                .get_function(&m.template[0].0)
                .unwrap()
                .schema()
                .output
                .clone();
            for ((_, sort), v) in m.slots.iter().zip(binding) {
                if sort == "Math" && !seen.insert(eg.get_canonical_value(*v, &math)) {
                    alias = true;
                }
            }
            self.bodies
                .entry(body)
                .and_modify(|a| *a |= alias)
                .or_insert(alias);
        }
        Ok(())
    }
    pub fn begin(&mut self, eg: &EGraph) {
        self.pre_graph = graph(eg);
        let external = grew(eg, &self.last_graph, &self.pre_graph);
        let mut normalized: BTreeMap<Body, (bool, bool, bool, bool)> = BTreeMap::new();
        for (old, alias) in &self.bodies {
            let mut new = old.iter().map(|f| canonical(eg, f)).collect::<Vec<_>>();
            new.sort();
            new.dedup();
            let changed = &new != old;
            let (old_o, old_i) = region(eg, old);
            let (new_o, new_i) = region(eg, &new);
            let alias_now = *alias || new_o.len() + new_i.len() < old_o.len() + old_i.len();
            let (o, i) = region(eg, old);
            let outside_o = o.iter().any(|v| external.contains(v));
            let outside_i = i.iter().any(|v| external.contains(v));
            normalized
                .entry(new)
                .and_modify(|a| {
                    a.0 |= alias_now;
                    a.1 |= changed;
                    a.2 |= outside_o;
                    a.3 |= outside_i;
                })
                .or_insert((alias_now, changed, outside_o, outside_i));
        }
        self.regions.clear();
        self.between_output.clear();
        self.between_input.clear();
        self.owners.clear();
        self.aliases.clear();
        self.rekeyed.clear();
        self.bodies.clear();
        for (id, (body, (alias, changed, outside_o, outside_i))) in
            normalized.into_iter().enumerate()
        {
            self.regions.push(region(eg, &body));
            if outside_o {
                self.between_output.insert(id);
            }
            if outside_i {
                self.between_input.insert(id);
            }
            for f in &body {
                self.owners.entry(f.clone()).or_default().insert(id);
            }
            if alias {
                self.aliases.insert(id);
            }
            if changed {
                self.rekeyed.insert(id);
            }
            self.bodies.insert(body, alias);
        }
        self.before = self.bodies.len();
    }
    pub fn finish(&mut self, eg: &EGraph, trace: &TraceSession, batch: TraceBatch) -> Value {
        let names: BTreeMap<_, _> = trace.table_names().into_iter().collect();
        let mut reads: BTreeMap<u64, BTreeSet<Fact>> = BTreeMap::new();
        for r in &batch.reads {
            let name = r.table_name.as_ref().or_else(|| names.get(&r.table));
            let Some(name) = name else { continue };
            let Some(f) = eg.get_function(name) else {
                continue;
            };
            if f.schema().output.name() != "Math" {
                continue;
            }
            let n = f.schema().input.len() + 1;
            if r.row.len() >= n {
                reads
                    .entry(r.match_event_id)
                    .or_default()
                    .insert((name.to_string(), r.row[..n].to_vec()));
            }
        }
        let mut effectful = BTreeSet::new();
        let mut write_events = BTreeMap::<String, usize>::new();
        for w in &batch.writes {
            let math = names
                .get(&w.table)
                .and_then(|n| eg.get_function(n))
                .is_some_and(|f| f.schema().output.name() == "Math");
            if math {
                *write_events.entry(format!("{:?}", w.outcome)).or_default() += 1;
            }
            if math && matches!(w.outcome, WriteOutcome::Inserted | WriteOutcome::Updated) {
                effectful.insert(w.match_event_id);
            }
        }
        for u in &batch.unions {
            if u.displaced.is_some() {
                effectful.insert(u.match_event_id);
            }
        }
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        let mut effects: BTreeMap<&str, usize> = BTreeMap::new();
        let mut touched = BTreeSet::new();
        let mut productive = BTreeSet::new();
        let mut internal_changed = BTreeSet::new();
        let mut by_rule: BTreeMap<String, BTreeMap<&str, usize>> = BTreeMap::new();
        let mut effect_by_rule: BTreeMap<String, BTreeMap<&str, usize>> = BTreeMap::new();
        for m in batch.matches.iter().filter(|m| {
            m.rule.starts_with("adaptive_source_") || m.rule.starts_with("adaptive_baked_")
        }) {
            let rs = reads.get(&m.event_id);
            let mut any = BTreeSet::new();
            let mut all: Option<BTreeSet<usize>> = None;
            let mut outside = false;
            if let Some(rs) = rs {
                for f in rs {
                    let o = self.owners.get(f).cloned().unwrap_or_default();
                    outside |= o.is_empty();
                    any.extend(o.iter().copied());
                    all = Some(match all {
                        None => o,
                        Some(a) => a.intersection(&o).copied().collect(),
                    });
                }
            }
            let covered = all.unwrap_or_default();
            let category = if !m.physical_witness_complete || rs.is_none_or(|r| r.is_empty()) {
                "unknown_witness"
            } else if !covered.is_empty() {
                "inside_one_prior_body"
            } else if any.is_empty() {
                "outside_prior_bodies"
            } else if outside {
                "boundary_with_external_facts"
            } else {
                "cross_prior_bodies"
            };
            *counts.entry(category).or_default() += 1;
            *by_rule
                .entry(m.rule.to_string())
                .or_default()
                .entry(category)
                .or_default() += 1;
            let changed = effectful.contains(&m.event_id);
            if changed {
                *effects.entry(category).or_default() += 1;
                *effect_by_rule
                    .entry(m.rule.to_string())
                    .or_default()
                    .entry(category)
                    .or_default() += 1;
            }
            if category == "boundary_with_external_facts" || category == "cross_prior_bodies" {
                touched.extend(any.iter().copied());
                if changed {
                    productive.extend(any);
                }
            }
            if category == "inside_one_prior_body" && changed {
                internal_changed.extend(covered);
            }
        }
        let post = graph(eg);
        let changed = grew(eg, &self.pre_graph, &post);
        let mut output_changed = self.between_output.clone();
        let mut input_changed = self.between_input.clone();
        for (id, (o, i)) in self.regions.iter().enumerate() {
            if o.iter().any(|v| changed.contains(v)) {
                output_changed.insert(id);
            }
            if i.iter().any(|v| changed.contains(v)) {
                input_changed.insert(id);
            }
        }
        let matched_ids = batch
            .matches
            .iter()
            .map(|m| m.event_id)
            .collect::<BTreeSet<_>>();
        let unassigned = effectful.difference(&matched_ids).count();
        let result = json!({"rhs_skipped":batch.skipped_rhs.len(),"candidate_matches":counts.values().sum::<usize>(),"math_write_events":write_events,"nonredundant_unions":batch.unions.iter().filter(|u|u.displaced.is_some()).count(),"known_bodies_before_step":self.before,"aliased_bindings":self.aliases.len(),"rekeyed_since_last_step":self.rekeyed.len(),"matches":counts,"committed_effectful_matches":effects,"boundary_participating_bodies":touched.len(),"productive_boundary_participating_bodies":productive.len(),"internally_effectful_bodies":internal_changed.len(),"internally_effectful_alias_or_rekeyed":internal_changed.iter().filter(|i|self.aliases.contains(i)||self.rekeyed.contains(i)).count(),"clean_bodies":self.before-self.aliases.union(&self.rekeyed).count(),"productive_boundary_clean_bodies":productive.iter().filter(|i|!self.aliases.contains(i)&&!self.rekeyed.contains(i)).count(),"body_output_regions_changed":output_changed.len(),"lower_input_regions_changed":input_changed.len(),"any_region_changed":output_changed.union(&input_changed).count(),"body_output_changed_between_steps":self.between_output.len(),"effectful_source_ids_not_in_batch":unassigned,"per_rule":by_rule,"effectful_per_rule":effect_by_rule,"scope":"pre-step unique instantiated fact sets; complete keyed LHS witnesses; committed Math inserts/updates or nonredundant unions; notifications excluded; overlapping owners are all counted, not a minimum reopen set"});
        let mut rebased = BTreeMap::new();
        for (body, alias) in std::mem::take(&mut self.bodies) {
            let mut key = body.iter().map(|r| canonical(eg, r)).collect::<Vec<_>>();
            key.sort();
            key.dedup();
            let (old_o, old_i) = region(eg, &body);
            let (new_o, new_i) = region(eg, &key);
            let alias = alias || new_o.len() + new_i.len() < old_o.len() + old_i.len();
            rebased
                .entry(key)
                .and_modify(|a| *a |= alias)
                .or_insert(alias);
        }
        self.bodies = rebased;
        self.last_graph = post;
        result
    }
}

/// Verification-only logical graph, with primitive values decoded across engines.
pub fn export_math(eg: &EGraph) -> Value {
    let mut rows = vec![];
    for op in eg.get_function_names() {
        let f = eg.get_function(&op).unwrap();
        if f.is_let_binding() || f.schema().output.name() != "Math" {
            continue;
        }
        eg.function_for_each(&op,|r|{
            let args=r.vals.iter().zip(&f.schema().input).map(|(v,t)|match t.name(){
                "Math"=>json!(["Math",format!("{:?}",eg.get_canonical_value(*v,t))]),
                "i64"=>json!(["i64",eg.value_to_base::<i64>(*v).to_string()]),
                "String"=>json!(["String",eg.value_to_base::<egglog::sort::S>(*v).to_string()]),
                _=>panic!("verification exporter supports Math/i64/String only"),
            }).collect::<Vec<_>>();
            rows.push(json!({"op":op,"args":args,"result":format!("{:?}",eg.get_canonical_value(*r.vals.last().unwrap(),&f.schema().output))}));
        }).unwrap();
    }
    json!(rows)
}
