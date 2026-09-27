//! Observational cell transitions. Patterns are evidence-backed hypotheses, not
//! executable summaries or saturation certificates. No native action is changed.
use super::Macro;
use egglog::{EGraph, TraceBatch, TraceSession, Value as Native, WriteOutcome};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

type Fact = (String, Vec<Native>);
#[derive(Clone)]
struct Cell {
    family: usize,
    binding: Vec<Native>,
    facts: BTreeSet<Fact>,
}
#[derive(Default)]
struct Pattern {
    descriptor: Value,
    occurrences: usize,
    new_target_occurrences: usize,
    sources: BTreeSet<usize>,
    targets: BTreeSet<usize>,
    rounds: BTreeSet<usize>,
    events: BTreeSet<(usize, u64)>,
    sample: Value,
}
#[derive(Default)]
pub struct Evolution {
    families: Vec<Macro>,
    family_keys: BTreeMap<String, usize>,
    cells: Vec<Cell>,
    instances: BTreeMap<(usize, Vec<Native>), usize>,
    fact_owners: BTreeMap<Fact, BTreeSet<usize>>,
    port_owners: BTreeMap<Native, BTreeSet<usize>>,
    input_owners: BTreeMap<Native, BTreeSet<usize>>,
    loose_contacts: usize,
    patterns: BTreeMap<String, Pattern>,
    rounds: Vec<Value>,
    matches: usize,
    incomplete_matches: usize,
    births: usize,
    linked_births: BTreeSet<usize>,
    boundary_events: BTreeSet<(usize, u64)>,
    multi_parent_events: BTreeSet<(usize, u64)>,
    alias_changes: usize,
    coalesced_cells: BTreeSet<usize>,
}
fn canon(eg: &EGraph, sort: &str, v: Native) -> Native {
    if sort != "Math" {
        return v;
    }
    eg.get_canonical_value(v, &eg.get_arcsort_by(|s| s.name() == "Math"))
}
fn fact(eg: &EGraph, name: &str, row: &[Native]) -> Option<Fact> {
    let f = eg.get_function(name)?;
    if f.is_let_binding() || f.schema().output.name() != "Math" {
        return None;
    }
    let n = f.schema().input.len() + 1;
    (row.len() >= n).then(|| (name.to_string(), row[..n].to_vec()))
}
fn partition(binding: &[Native], m: &Macro) -> Vec<Option<usize>> {
    let mut ids = BTreeMap::new();
    binding
        .iter()
        .zip(&m.slots)
        .enumerate()
        .map(|(i, (v, (_, ty)))| (ty == "Math").then(|| *ids.entry(*v).or_insert(i)))
        .collect()
}
impl Evolution {
    fn family(&mut self, m: &Macro) -> usize {
        let key=json!({"rows":m.template,"sorts":m.slots.iter().map(|s| &s.1).collect::<Vec<_>>(),
            "root_slots":m.slots.iter().enumerate().filter(|(_,s)|s.0=="adaptive_root").map(|(i,_)|i).collect::<Vec<_>>()}).to_string();
        if let Some(id) = self.family_keys.get(&key) {
            return *id;
        }
        let id = self.families.len();
        self.family_keys.insert(key, id);
        self.families.push(m.clone());
        id
    }
    fn admit(&mut self, eg: &EGraph, m: &Macro, binding: &[Native]) -> (usize, bool) {
        let family = self.family(m);
        let binding = binding
            .iter()
            .zip(&m.slots)
            .map(|(v, (_, ty))| canon(eg, ty, *v))
            .collect::<Vec<_>>();
        if let Some(id) = self.instances.get(&(family, binding.clone())) {
            return (*id, false);
        }
        let id = self.cells.len();
        let facts = m
            .template
            .iter()
            .map(|(op, cs)| (op.clone(), cs.iter().map(|i| binding[*i]).collect()))
            .collect();
        self.cells.push(Cell {
            family,
            binding: binding.clone(),
            facts,
        });
        self.instances.insert((family, binding), id);
        self.births += 1;
        (id, true)
    }
    /// Canonicalize only existing identities before matching. The index describes
    /// prior cells; cells emitted in this step cannot masquerade as prior causes.
    pub fn begin(&mut self, eg: &EGraph) {
        self.instances.clear();
        self.fact_owners.clear();
        self.port_owners.clear();
        self.input_owners.clear();
        for (id, c) in self.cells.iter_mut().enumerate() {
            let m = &self.families[c.family];
            let before = partition(&c.binding, m);
            for (v, (_, ty)) in c.binding.iter_mut().zip(&m.slots) {
                *v = canon(eg, ty, *v);
            }
            if partition(&c.binding, m) != before {
                self.alias_changes += 1;
            }
            c.facts = m
                .template
                .iter()
                .map(|(op, cs)| (op.clone(), cs.iter().map(|i| c.binding[*i]).collect()))
                .collect();
            // Union can coalesce instances. Keep stable old IDs, but index one
            // canonical representative so duplicate identities do not inflate reuse.
            if self.instances.contains_key(&(c.family, c.binding.clone())) {
                self.coalesced_cells.insert(id);
                continue;
            }
            self.instances.insert((c.family, c.binding.clone()), id);
            for f in &c.facts {
                self.fact_owners.entry(f.clone()).or_default().insert(id);
            }
            // Original LHS variables remain lower ports even when saturation
            // unifies an input with a body output. Generated names are reserved
            // by the positive Math runtime, so this is a representation invariant.
            for (v, (term, ty)) in c.binding.iter().zip(&m.slots) {
                if ty == "Math" && !term.starts_with("adaptive_") {
                    self.input_owners.entry(*v).or_default().insert(id);
                }
            }
            for (v, (_, ty)) in c.binding.iter().zip(&m.slots) {
                if ty == "Math" {
                    self.port_owners.entry(*v).or_default().insert(id);
                }
            }
        }
    }
    pub fn finish(
        &mut self,
        eg: &EGraph,
        trace: &TraceSession,
        batch: &TraceBatch,
        macros: &BTreeMap<usize, Macro>,
        round: usize,
        native_unions: usize,
    ) {
        for m in macros.values() {
            self.family(m);
        }
        // Rebase lookup keys without modifying the pre-step cells used as guards.
        self.instances.clear();
        for (id, c) in self.cells.iter().enumerate() {
            let m = &self.families[c.family];
            let key = c
                .binding
                .iter()
                .zip(&m.slots)
                .map(|(v, (_, ty))| canon(eg, ty, *v))
                .collect();
            self.instances.entry((c.family, key)).or_insert(id);
        }
        let names: BTreeMap<_, _> = trace.table_names().into_iter().collect();
        let mut reads: BTreeMap<u64, Vec<Fact>> = BTreeMap::new();
        for r in &batch.reads {
            if r.planner_summary {
                continue;
            }
            if let Some(name) = r.table_name.as_ref().or_else(|| names.get(&r.table)) {
                if let Some(f) = fact(eg, name, &r.row) {
                    reads.entry(r.match_event_id).or_default().push(f);
                }
            }
        }
        let mut emitted: BTreeMap<u64, Vec<(usize, bool)>> = BTreeMap::new();
        let mut effects: BTreeMap<u64, Vec<(&'static str, Fact)>> = BTreeMap::new();
        for w in &batch.writes {
            // A notification is an observed macro instance, not proof that its
            // Math rows were newly inserted. Require a committed write outcome.
            if w.outcome == WriteOutcome::Unsupported {
                continue;
            }
            let Some(name) = names.get(&w.table) else {
                continue;
            };
            if let Some(id) = name
                .strip_prefix("adaptive_instance_")
                .and_then(|s| s.parse::<usize>().ok())
            {
                if let Some(m) = macros.get(&id) {
                    if w.actual.len() >= m.slots.len() {
                        let cell = self.admit(eg, m, &w.actual[..m.slots.len()]);
                        emitted.entry(w.match_event_id).or_default().push(cell);
                    }
                }
            } else if let Some(f) = fact(eg, name, &w.actual) {
                let kind = match w.outcome {
                    WriteOutcome::Inserted => "inserted",
                    WriteOutcome::Updated => "updated",
                    _ => continue,
                };
                effects.entry(w.match_event_id).or_default().push((kind, f));
            }
        }
        let mut linked = 0;
        let mut unlinked = 0;
        for event in &batch.matches {
            if !(event.rule.starts_with("adaptive_source_")
                || event.rule.starts_with("adaptive_baked_"))
            {
                continue;
            }
            self.matches += 1;
            if !event.physical_witness_complete {
                self.incomplete_matches += 1;
                continue;
            }
            let Some(rs) = reads.get(&event.event_id) else {
                continue;
            };
            let mut support = BTreeSet::<usize>::new();
            let mut contacts = BTreeSet::<usize>::new();
            for f in rs {
                if let Some(ids) = self.fact_owners.get(f) {
                    support.extend(ids);
                }
                if let Some(ids) = f.1.last().and_then(|v| self.input_owners.get(v)) {
                    support.extend(ids);
                }
                let schema = eg.get_function(&f.0).unwrap().schema();
                for (v, ty) in f.1.iter().zip(schema.input.iter().chain([&schema.output])) {
                    if ty.name() == "Math" {
                        if let Some(ids) = self.port_owners.get(v) {
                            contacts.extend(ids);
                        }
                    }
                }
            }
            let Some(outputs) = emitted.get(&event.event_id) else {
                continue;
            };
            let mut unique_outputs = BTreeMap::<usize, bool>::new();
            for &(id, new) in outputs {
                unique_outputs
                    .entry(id)
                    .and_modify(|seen| *seen |= new)
                    .or_insert(new);
            }
            let outputs = unique_outputs;
            self.loose_contacts += contacts.difference(&support).count();
            if support.is_empty() {
                unlinked += outputs.len();
                continue;
            }
            if support.len() > 1 {
                self.multi_parent_events.insert((round, event.event_id));
            }
            for &source in &support {
                let parent = &self.cells[source];
                let pm = &self.families[parent.family];
                let outside = rs.iter().any(|f| !parent.facts.contains(f));
                if !outside {
                    continue;
                } // internal confirmations are not boundary evolution
                self.boundary_events.insert((round, event.event_id));
                for (&target, &new_instance) in &outputs {
                    let child = &self.cells[target];
                    let cm = &self.families[child.family];
                    let mut enc = Encoder::new(eg, &parent, pm);
                    let encoded_reads = rs
                        .iter()
                        .map(|f| {
                            let location = if parent.facts.contains(f) {
                                "body_fact"
                            } else {
                                "external_fact"
                            };
                            json!({"location":location,"fact":enc.row(eg,f,false)})
                        })
                        .collect::<Vec<_>>();
                    // Target slot order, not native ID or commit order, names newly
                    // introduced values. Actual primitive constants are retained.
                    let binding = child
                        .binding
                        .iter()
                        .zip(&cm.slots)
                        .map(|(v, (_, ty))| enc.value(eg, ty, *v, true))
                        .collect::<Vec<_>>();
                    let target_rows = cm
                        .template
                        .iter()
                        .enumerate()
                        .map(|(i, (op, cs))| {
                            (
                                (
                                    op.clone(),
                                    cs.iter()
                                        .map(|j| canon(eg, &cm.slots[*j].1, child.binding[*j]))
                                        .collect::<Vec<_>>(),
                                ),
                                i,
                            )
                        })
                        .collect::<BTreeMap<_, _>>();
                    let mut mutations = effects
                        .get(&event.event_id)
                        .into_iter()
                        .flatten()
                        .map(|(k, f)| {
                            let schema = eg.get_function(&f.0).unwrap().schema();
                            let canonical = (
                                f.0.clone(),
                                f.1.iter()
                                    .zip(schema.input.iter().chain([&schema.output]))
                                    .map(|(v, t)| canon(eg, t.name(), *v))
                                    .collect(),
                            );
                            if let Some(row) = target_rows.get(&canonical) {
                                json!({"outcome":k,"target_row":row})
                            } else {
                                json!({"outcome":k,"residual_fact":enc.row(eg,f,true)})
                            }
                        })
                        .collect::<Vec<_>>();
                    mutations.sort_by_key(Value::to_string);
                    mutations.dedup();
                    let current = parent
                        .binding
                        .iter()
                        .zip(&pm.slots)
                        .map(|(v, (_, ty))| canon(eg, ty, *v))
                        .collect::<Vec<_>>();
                    let input_slots = pm
                        .slots
                        .iter()
                        .enumerate()
                        .filter(|(_, s)| s.1 == "Math" && !s.0.starts_with("adaptive_"))
                        .map(|(i, _)| i)
                        .collect::<Vec<_>>();
                    let touches = rs
                        .iter()
                        .filter(|f| !parent.facts.contains(*f))
                        .flat_map(|f| {
                            let schema = eg.get_function(&f.0).unwrap().schema();
                            f.1.iter()
                                .zip(schema.input.iter().chain([&schema.output]))
                                .filter(|(_, s)| s.name() == "Math")
                                .flat_map(|(v, _)| {
                                    parent
                                        .binding
                                        .iter()
                                        .enumerate()
                                        .filter(move |(i, b)| pm.slots[*i].1 == "Math" && *b == v)
                                        .map(|(i, _)| i)
                                })
                                .collect::<Vec<_>>()
                        })
                        .collect::<BTreeSet<_>>();
                    let input_refinement =
                        rs.iter().filter(|f| !parent.facts.contains(*f)).any(|f| {
                            input_slots
                                .iter()
                                .any(|i| Some(&parent.binding[*i]) == f.1.last())
                        });
                    let mut unions = batch
                        .unions
                        .iter()
                        .filter(|u| u.match_event_id == event.event_id && u.displaced.is_some())
                        .filter_map(|u| {
                            let left = enc.before.get(&u.lhs)?;
                            let right = enc.before.get(&u.rhs)?;
                            let mut pair = vec![left.clone(), right.clone()];
                            pair.sort();
                            Some(pair)
                        })
                        .collect::<Vec<_>>();
                    unions.sort();
                    unions.dedup();
                    let descriptor = json!({"source_family":parent.family,"rule":event.rule,"source_support":if rs.iter().any(|f|parent.facts.contains(f)) {"body_read"} else {"input_refinement"},
                        "before_aliases":partition(&parent.binding,pm),"after_aliases":partition(&current,pm),
                        "read_guard":encoded_reads,"touched_slots":touches,"input_refinement":input_refinement,
                        "target_family":child.family,"target_binding":binding,"committed_math_rows_postcanonical":mutations,"traced_read_value_unions":unions});
                    let key = descriptor.to_string();
                    let entry = self.patterns.entry(key).or_default();
                    entry.descriptor = descriptor;
                    entry.occurrences += 1;
                    entry.new_target_occurrences += usize::from(new_instance);
                    entry.sources.insert(source);
                    entry.targets.insert(target);
                    entry.rounds.insert(round);
                    entry.events.insert((round, event.event_id));
                    if entry.sample.is_null() {
                        let support_ports = support
                            .iter()
                            .map(|id| {
                                let c = &self.cells[*id];
                                let m = &self.families[c.family];
                                let known_slots = c
                                    .binding
                                    .iter()
                                    .enumerate()
                                    .filter(|(i, _)| m.slots[*i].1 == "Math")
                                    .filter_map(|(i, v)| {
                                        enc.before.get(v).map(|name| json!([i, name]))
                                    })
                                    .collect::<Vec<_>>();
                                json!({"cell":id,"family":c.family,"known_slots":known_slots})
                            })
                            .collect::<Vec<_>>();
                        entry.sample = json!({"round":round,"match_event_id":event.event_id,
                        "source_cell":source,"target_cell":target,"support_cells":support,"support_ports":support_ports,"new_instance":new_instance});
                    }
                    if new_instance {
                        self.linked_births.insert(target);
                    }
                    linked += 1;
                }
            }
        }
        self.rounds.push(json!({"round":round,"known_cells":self.cells.len(),"anchored_boundary_links":linked,
            "emissions_without_prior_support":unlinked,"native_successful_unions":native_unions,"traced_committed_unions":batch.unions.iter().filter(|u|u.displaced.is_some()).count(),
            "note":"support is observed read/port incidence, not a minimal causal parent set; union trace is not complete"}));
    }
    pub fn report(&self) -> Value {
        let mut patterns = self.patterns.values().collect::<Vec<_>>();
        patterns.sort_by_key(|p| {
            (
                std::cmp::Reverse(p.sources.len()),
                std::cmp::Reverse(p.occurrences),
            )
        });
        let repeated = patterns.iter().filter(|p| p.sources.len() > 1).count();
        let repeated_events = patterns
            .iter()
            .filter(|p| p.sources.len() > 1)
            .flat_map(|p| p.events.iter().copied())
            .collect::<BTreeSet<_>>();
        json!({"schema":"cell-evolution/v1","scope":"observed anchored transition patterns in one exact rule environment; not an executable state machine or proven minimal guards",
            "loose_interface_contacts_not_expanded":self.loose_contacts,"logical_matches":self.matches,"incomplete_matches":self.incomplete_matches,"observed_instance_births":self.births,"canonical_instances":self.instances.len(),"coalesced_cells":self.coalesced_cells.len(),
            "boundary_linked_births":self.linked_births.len(),"boundary_events":self.boundary_events.len(),"multi_support_events":self.multi_parent_events.len(),
            "boundary_matches_in_repeated_patterns":repeated_events.len(),"observed_slot_partition_changes":self.alias_changes,"transition_patterns":patterns.len(),"patterns_repeated_across_instances":repeated,
            "families":self.families.iter().enumerate().map(|(id,m)|json!({"id":id,"origin_rule":m.rule,"slots":m.slots,"rows":m.template})).collect::<Vec<_>>(),
            "patterns":patterns.iter().enumerate().map(|(id,p)|json!({"id":id,"occurrences":p.occurrences,"new_target_occurrences":p.new_target_occurrences,"distinct_matches":p.events.len(),"distinct_source_cells":p.sources.len(),"distinct_target_cells":p.targets.len(),"rounds":p.rounds,"transition":p.descriptor,"example":p.sample})).collect::<Vec<_>>(),"rounds":self.rounds,
            "limitations":["Only macro notification instances are cells; source-only effects without a notification are not assigned a new cell.","Within-step newly born cells are not used as parents; evidence is from the read-only matching boundary.","Input refinement is an observed external constructor on a lower slot, not proof that it was newly created or solely responsible.","Primitive constants, read occurrence order and source family slot numbering are retained; no graph-isomorphism or guard minimization is claimed.","Implicit union provenance is incomplete; alias partitions report observed equalities, not a complete union proof.","Patterns are anchored projections, not multi-parent graph isomorphism; example support_ports retains the other families and their shared slot mappings."]})
    }
}
struct Encoder {
    before: BTreeMap<Native, String>,
    after: BTreeMap<Native, Vec<String>>,
    fresh: usize,
    // New to this local encoding, not necessarily a newly allocated native value.
    new: BTreeMap<Native, usize>,
}
impl Encoder {
    fn new(eg: &EGraph, c: &Cell, m: &Macro) -> Self {
        let mut e = Self {
            before: BTreeMap::new(),
            after: BTreeMap::new(),
            fresh: 0,
            new: BTreeMap::new(),
        };
        for (i, (v, (_, ty))) in c.binding.iter().zip(&m.slots).enumerate() {
            if ty == "Math" {
                e.before.entry(*v).or_insert_with(|| format!("port_{i}"));
            }
        }
        for (v, s) in &e.before {
            e.after
                .entry(canon(eg, "Math", *v))
                .or_default()
                .push(s.clone());
        }
        e
    }
    fn value(&mut self, eg: &EGraph, ty: &str, v: Native, post: bool) -> Value {
        match ty {
            "i64" => json!({"i64":eg.value_to_base::<i64>(v)}),
            "String" => json!({"String":eg.value_to_base::<egglog::sort::S>(v).to_string()}),
            "Math" => {
                if post {
                    let v = canon(eg, ty, v);
                    if let Some(symbols) = self.after.get(&v) {
                        let mut s = symbols.clone();
                        s.sort();
                        s.dedup();
                        json!({"post_of":s})
                    } else {
                        let n = self.new.len();
                        json!({"new":*self.new.entry(v).or_insert(n)})
                    }
                } else {
                    let s = self
                        .before
                        .entry(v)
                        .or_insert_with(|| {
                            let s = format!("external_{}", self.fresh);
                            self.fresh += 1;
                            s
                        })
                        .clone();
                    self.after
                        .entry(canon(eg, ty, v))
                        .or_default()
                        .push(s.clone());
                    json!({"pre":s})
                }
            }
            _ => panic!("positive Math runtime supports Math/i64/String only"),
        }
    }
    fn row(&mut self, eg: &EGraph, f: &Fact, post: bool) -> Value {
        let schema = eg.get_function(&f.0).unwrap().schema();
        let cols =
            f.1.iter()
                .zip(schema.input.iter().chain([&schema.output]))
                .map(|(v, t)| self.value(eg, t.name(), *v, post))
                .collect::<Vec<_>>();
        json!({"op":f.0,"columns":cols})
    }
}
