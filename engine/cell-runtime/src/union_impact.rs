//! Diagnostic-only union workload survey. Runtime recipe frames are NOT
//! automatically certified RipSat instances. No scheduling or matching changes.
use crate::{
    graph::Graph,
    program::{Call, Op, Result},
    value::{Value, root},
};
use serde_json::{Value as J, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
pub struct Hit {
    frame: u32,
    template: usize,
    kind: &'static str,
    depth: u32,
    completed: bool,
    inputs: Vec<(usize, u64, bool)>,
    outputs: Vec<(u32, u64)>,
    scalars: Vec<(usize, i64)>,
    touches: Vec<J>,
    before_ops: BTreeMap<usize, BTreeMap<String, usize>>,
    owned_fact_affected: bool,
    born_this_run: bool,
}
struct Batch {
    hit: Hit,
    events: BTreeSet<usize>,
    repairs: u64,
}
#[derive(Default)]
pub struct Survey {
    pending: BTreeMap<u32, Batch>,
    pub rows: Vec<J>,
    pub scanned_frames: u64,
    pub unready_inputs: u64,
    pub effective_unions: u64,
    pub run_start_cell: u32,
}
fn operator_counts(g: &Graph, id: u64) -> Result<BTreeMap<String, usize>> {
    let tables = g
        .program
        .tables
        .iter()
        .enumerate()
        .filter_map(|(i, t)| t.constructor.then_some(i))
        .collect();
    let mut result = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for row in g.equality_neighbors(id, &tables)? {
        if row.values.last() == Some(&Value::Eq(root(&g.parents, id)))
            && seen.insert((row.table, row.values[..row.values.len() - 1].to_vec()))
        {
            *result
                .entry(g.program.tables[row.table].name.clone())
                .or_default() += 1;
        }
    }
    Ok(result)
}
pub fn collect(g: &Graph, a: u64, b: u64) -> Result<(Vec<Hit>, u64, u64)> {
    let owners = g.union_affected_owners(a, b);
    let ops = [operator_counts(g, a)?, operator_counts(g, b)?];
    let mut hits = vec![];
    let mut unready = 0;
    for (&frame, c) in &g.cells {
        let template = &g.program.templates[c.template];
        let (slots, kind, defined) = if let Some(rule) = g.program.rules.get(c.template) {
            let set = g.program.artifact["recipe"]["rules"][c.template]["metadata"]["source_set"]
                .as_str()
                .unwrap_or("");
            let kind = if rule.query.is_empty() {
                "loader"
            } else if g
                .program
                .views
                .layouts
                .get(&c.template)
                .is_some_and(|x| x.reference_only)
            {
                "marker"
            } else if set.starts_with("cell-") {
                "auxiliary_rule"
            } else {
                "original_rule"
            };
            let defined = rule
                .query
                .iter()
                .filter_map(|a| {
                    if let Call::Table(_) = a.call {
                        if let Some(crate::program::Arg::Slot(s)) = a.args.last() {
                            Some(*s)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect::<BTreeSet<_>>();
            (rule.inputs.to_vec(), kind, defined)
        } else {
            (
                (0..template.sorts.len().saturating_sub(1)).collect(),
                "constructor",
                BTreeSet::new(),
            )
        };
        let mut inputs = vec![];
        let mut scalars = vec![];
        for s in slots {
            match g.slot(frame, s) {
                Ok(Value::Eq(v)) => inputs.push((s, v, !defined.contains(&s))),
                Ok(Value::Int(v)) => scalars.push((s, v)),
                Err(_) => unready += 1,
                _ => (),
            }
        }
        let touched = inputs
            .iter()
            .filter(|(_, v, _)| *v == a || *v == b)
            .copied()
            .collect::<Vec<_>>();
        if touched.is_empty() && !owners.contains(&frame) {
            continue;
        }
        let mut outputs = vec![];
        for (&op, _) in &c.facts {
            if let Some(Op::Let(dst, Call::Table(t), _)) = template.ops.get(op as usize)
                && g.program.tables[*t].constructor
            {
                if let Ok(Value::Eq(v)) = g.slot(frame, *dst) {
                    outputs.push((op, v));
                }
            }
        }
        let mut touches = vec![];
        let mut before_ops = BTreeMap::new();
        for (s, v, leaf) in touched {
            let side = usize::from(v == b);
            let other = if side == 0 { b } else { a };
            let peer_slots = inputs
                .iter()
                .filter_map(|(i, x, _)| (*x == other).then_some(*i))
                .collect::<Vec<_>>();
            let peer_outputs = outputs
                .iter()
                .filter_map(|(i, x)| (*x == other).then_some(*i))
                .collect::<Vec<_>>();
            let class = if !peer_slots.is_empty() {
                "input_input"
            } else if !peer_outputs.is_empty() {
                "input_local_output"
            } else {
                "input_external"
            };
            touches.push(json!({"slot":s,"sort":template.sorts[s],"leaf":leaf,"kind":class,"peer_slots":peer_slots,"peer_outputs":peer_outputs,"opponents":ops[1-side]}));
        }
        // Capture the entire input projection at the FIRST relevant event.
        // Do not fill missing columns from later congruence-derived touches.
        for (s, v, _) in &inputs {
            before_ops.insert(*s, operator_counts(g, *v)?);
        }
        hits.push(Hit {
            frame,
            template: c.template,
            kind,
            depth: c.recipe_depth,
            completed: c.completed,
            inputs,
            outputs,
            scalars,
            touches,
            before_ops,
            owned_fact_affected: owners.contains(&frame),
            born_this_run: frame >= g.union_impact.as_ref().unwrap().run_start_cell,
        });
    }
    Ok((hits, g.cells.len() as u64, unready))
}
fn partition(xs: impl Iterator<Item = (usize, u64)>) -> Vec<(usize, usize)> {
    let mut ids = BTreeMap::new();
    xs.map(|(slot, v)| {
        let n = ids.len();
        (slot, *ids.entry(v).or_insert(n))
    })
    .collect()
}
impl Survey {
    pub fn record(&mut self, event: usize, hits: Vec<Hit>, scanned: u64, unready: u64) {
        self.effective_unions += 1;
        self.scanned_frames += scanned;
        self.unready_inputs += unready;
        for mut hit in hits {
            for touch in &mut hit.touches {
                touch["event"] = json!(event);
            }
            if let Some(old) = self.pending.get_mut(&hit.frame) {
                old.events.insert(event);
                old.hit.owned_fact_affected |= hit.owned_fact_affected;
                old.hit.touches.extend(hit.touches);
            } else {
                self.pending.insert(
                    hit.frame,
                    Batch {
                        hit,
                        events: BTreeSet::from([event]),
                        repairs: 0,
                    },
                );
            }
        }
    }
    pub fn repair(&mut self, frame: u32) {
        if let Some(b) = self.pending.get_mut(&frame) {
            b.repairs += 1;
        }
    }
    pub fn finish(&mut self, g: &Graph, set: &str) -> Result<()> {
        for (_, b) in std::mem::take(&mut self.pending) {
            let h = b.hit;
            let before = partition(h.inputs.iter().map(|(s, v, _)| (*s, *v)));
            let after = partition(h.inputs.iter().map(|(s, v, _)| (*s, root(&g.parents, *v))));
            let out_before = partition(h.outputs.iter().map(|(s, v)| (*s as usize, *v)));
            let out_after = partition(
                h.outputs
                    .iter()
                    .map(|(s, v)| (*s as usize, root(&g.parents, *v))),
            );
            let input_output_aliases = h
                .inputs
                .iter()
                .flat_map(|(s, a, _)| {
                    h.outputs.iter().filter_map(move |(op, b)| {
                        (*a != *b && root(&g.parents, *a) == root(&g.parents, *b))
                            .then_some((*s, *op))
                    })
                })
                .collect::<Vec<_>>();
            let mut refinements = vec![];
            for (s, old) in &h.before_ops {
                let id = h.inputs.iter().find(|(slot, _, _)| slot == s).unwrap().1;
                let new = operator_counts(g, id)?;
                if *old != new {
                    refinements.push(json!({"slot":s,"before":old,"after":new}));
                }
            }
            let mut touches = h.touches;
            touches.sort_by_key(|v| v.to_string());
            touches.dedup();
            let meta = &g.program.artifact["recipe"]["rules"][h.template];
            self.rows.push(json!({"epoch":g.epoch,"active_ruleset":set,"frame":h.frame,"template":h.template,"kind":h.kind,
                "producer_ruleset":meta["metadata"]["source_set"],"producer_name":g.program.templates[h.template].name,
                "completed":h.completed,"continuation_depth":h.depth,"input_slots":h.inputs.iter().map(|(s,_,leaf)|json!({"slot":s,"leaf":leaf})).collect::<Vec<_>>(),
                "born_this_run":h.born_this_run,
                "scalar_values":h.scalars,"owned_fact_affected":h.owned_fact_affected,"events":b.events,
                "touches":touches,"before_input_partition":before,"after_input_partition":after,
                "before_output_partition":out_before,"after_output_partition":out_after,"new_input_output_aliases":input_output_aliases,
                "input_operator_refinements":refinements,"owned_fact_repair_visits":b.repairs,
                "input_operator_before":h.before_ops,
                "owner_alive_after_repair":g.cells.contains_key(&h.frame),"certified_cell_upgrade":false}));
        }
        Ok(())
    }
    pub fn report(&self) -> J {
        json!({"schema":2,"timing_eligible":false,"effective_unions":self.effective_unions,"scanned_frames":self.scanned_frames,"unready_inputs":self.unready_inputs,"rows":self.rows,
            "scope":"diagnostic live runtime recipe frames, not certified RipSat instances; (Run epoch, frame) deduplicated impacts; leaf means not defined by a query-table result"})
    }
}
