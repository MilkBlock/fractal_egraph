//! Verified producer-local projections and their source-schedule permissions.
//! A projection never certifies uniqueness after union; the executor checks that
//! obligation against the current Run snapshot before bypassing an atom.
use crate::program::*;
use serde::{Deserialize, Serialize};
use serde_json::Value as J;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalRead {
    pub atom: usize,
    pub table: usize,
    pub columns: Vec<Arg>,
    pub effect: Option<usize>,
    #[serde(default)]
    pub(crate) assignments: Vec<(usize, Arg)>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Continuation {
    pub producer: usize,
    pub consumer: usize,
    pub anchor: usize,
    pub reads: Vec<LocalRead>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MultiStage {
    #[serde(default)]
    pub stage_entries:Vec<(usize,usize,Rc<Vec<usize>>)>,
    pub routes: HashMap<(usize, usize, usize, usize), Rc<Continuation>>,
    pub entries: HashMap<(usize, usize), Rc<Vec<usize>>>,
    pub finishes: Vec<Rc<Vec<usize>>>,
}
pub(crate) fn normalize(template: &Template, arg: &Arg) -> Arg {
    if let Arg::Slot(slot) = arg {
        if let Some(op) = template.defs.get(slot) {
            if let Op::Copy(_, source) = &template.ops[*op] {
                return normalize(template, source);
            }
        }
    }
    arg.clone()
}
pub(crate) fn source(
    program: &Program,
    producer: usize,
    kind: &str,
    index: usize,
) -> Result<(usize, Vec<Arg>)> {
    let rule = program.rules.get(producer).ok_or("continuation producer")?;
    let template = &program.templates[rule.template];
    let (table, columns) = match kind {
        "query" => {
            let atom = rule.query.get(index).ok_or("continuation query source")?;
            let Call::Table(t) = atom.call else {
                return Err("primitive is not a local fact".into());
            };
            (t, atom.args.clone())
        }
        "effect" => match template
            .ops
            .get(index)
            .ok_or("continuation effect source")?
        {
            Op::Let(dst, Call::Table(t), args) if program.tables[*t].virtual_fact => {
                let mut columns = args.clone();
                columns.push(Arg::Slot(*dst));
                (*t, columns)
            }
            Op::Set(t, args, value) => {
                let mut columns = args.clone();
                columns.push(value.clone());
                (*t, columns)
            }
            _ => return Err("effect cannot provide a local fact".into()),
        },
        _ => return Err("unknown local fact source".into()),
    };
    Ok((
        table,
        columns.iter().map(|a| normalize(template, a)).collect(),
    ))
}
pub(crate) fn extend(
    atom: &Atom,
    table: usize,
    columns: &[Arg],
    mapping: &mut BTreeMap<usize, Arg>,
) -> Result<()> {
    if !matches!(atom.call,Call::Table(t) if t==table) || atom.args.len() != columns.len() {
        return Err("continuation operator/arity differs".into());
    }
    for (to, from) in atom.args.iter().zip(columns) {
        match to {
            Arg::Slot(slot) => {
                if let Some(old) = mapping.get(slot) {
                    if old != from {
                        return Err("continuation relative binding differs".into());
                    }
                } else {
                    mapping.insert(*slot, from.clone());
                }
            }
            _ if to != from => return Err("continuation literal differs".into()),
            _ => {}
        }
    }
    Ok(())
}
/// extend() verifies literals, repeated slots and agreement with the pivot.
/// A physically identical pivot already bound those slots in the shared packet.
pub(crate) fn assignments(pivot: &Atom, atom: &Atom, columns: &[Arg]) -> Vec<(usize, Arg)> {
    let mut seen: BTreeSet<_> = pivot
        .args
        .iter()
        .filter_map(|a| match a {
            Arg::Slot(i) => Some(*i),
            _ => None,
        })
        .collect();
    atom.args
        .iter()
        .zip(columns)
        .filter_map(|(dst, src)| match dst {
            Arg::Slot(i) if seen.insert(*i) => Some((*i, src.clone())),
            _ => None,
        })
        .collect()
}
impl LocalRead {
    /// Only use a live, unrelocated effect in the same ordinary producer
    /// instance as the pivot. Dynamic ports and uncertain keys keep the matcher.
    pub(crate) fn wire(
        &self,
        g: &mut crate::graph::Graph,
        ctx: crate::views::Context,
        pivot: crate::graph::Dependency,
        inputs: &mut crate::entry_schedule::Inputs,
        trail: &mut Vec<usize>,
        expected: Option<crate::graph::Dependency>,
    ) -> Result<Option<(crate::graph::Dependency, u64)>> {
        use crate::graph::{Dependency, FactRef};
        #[cfg(feature = "slot-transfer-audit")]
        crate::transfer_audit::count(|c| c.sibling_wire_attempts += 1);
        #[cfg(feature = "slot-transfer-audit")]
        let _wire_sample = crate::transfer_audit::Sample::begin(crate::transfer_audit::Region::SiblingWire);
        if g.views_enabled
            || !ctx.completed
            || pivot
                != Dependency::Fact(FactRef {
                    cell: ctx.cell,
                    op: ctx.op as u32,
                })
        {
            return Ok(None);
        }
        let Some(op) = self.effect else {
            return Ok(None);
        };
        let w = Dependency::Fact(FactRef {
            cell: ctx.cell,
            op: op as u32,
        });
        if expected.is_some_and(|other| other != w) {
            return Ok(None);
        }
        let Some(epoch) = g.visible_epoch(w) else {
            return Ok(None);
        };
        let mark = trail.len();
        for (dst, src) in &self.assignments {
            let value = g.argument(ctx.cell, src)?;
            g.stats.slot_transfer.producer_values_read += 1;
            if let Some(bound) = &inputs[*dst] {
                if *bound != value {
                    while trail.len() > mark {
                        inputs[trail.pop().unwrap()] = None;
                    }
                    return Ok(None);
                }
            } else {
                inputs[*dst] = Some(value);
                trail.push(*dst);
            }
        }
        g.stats.slot_transfer.inputs_moved += (trail.len() - mark) as u64;
        Ok(Some((w, epoch)))
    }
}
impl MultiStage {
    pub fn parse(program: &Program, raw: &J) -> Result<Self> {
        if raw.is_null() {
            return Ok(Self::default());
        }
        if !raw["union_entries"].is_null()
            && raw["union_entries"]
                != serde_json::to_value(crate::union_graft::compile(program))
                    .map_err(|e| e.to_string())?
        {
            return Err("union Spine entry differs from typed source query".into());
        }
        if raw["schema"] != "eggfuse-multistage/v1"
            || raw["recipe_sha256"] != program.artifact["recipe"]["sha256"]
            || raw["control"] != program.artifact["recipe"]["control"]
        {
            return Err("multi-stage source/control differs".into());
        }
        let sets = program.artifact["recipe"]["rulesets"]
            .as_array()
            .ok_or("recipe sets")?
            .iter()
            .map(|s| serde_json::json!({"name":s["name"],"rule_ids":s["rule_ids"]}))
            .collect::<Vec<_>>();
        if raw["rule_sets"] != J::Array(sets) {
            return Err("multi-stage permissions differ".into());
        }
        if raw["stage_entries"].is_null() {
            if std::env::var("CELL_CPU_STAGE_SPINE").as_deref()==Ok("1") {
                return Err("stage Spine programs require compiler stage_entries; recompile source_sk_mk through the Cell CPU target".into());
            }
        } else if raw["stage_entries"]!=crate::stage_spine::compile_entries(program) {
            return Err("stage entry program differs from exhaustive typed key flow".into());
        }
        let mut result = Self::default();
        if let Some(entries)=raw["stage_entries"].as_array() {
            for e in entries {
                result.stage_entries.push((e["rule"].as_u64().unwrap() as usize,e["anchor"].as_u64().unwrap() as usize,
                    Rc::new(e["atoms"].as_array().unwrap().iter().map(|i|i.as_u64().unwrap() as usize).collect())));
            }
        }
        let verify_points = |rid: usize,
                             anchor: Option<usize>,
                             raw: &J|
         -> Result<Rc<Vec<usize>>> {
            let rule = program.rules.get(rid).ok_or("point program rule")?;
            let mut known = BTreeSet::new();
            let mut covered = BTreeSet::new();
            for (i, atom) in rule.query.iter().enumerate() {
                if anchor == Some(i) || (anchor.is_none() && matches!(atom.call, Call::Table(_))) {
                    covered.insert(i);
                    for arg in &atom.args {
                        if let Arg::Slot(s) = arg {
                            known.insert(*s);
                        }
                    }
                }
            }
            let mut order = vec![];
            for i in array(raw)? {
                let i = i.as_u64().ok_or("point instruction")? as usize;
                let atom = rule.query.get(i).ok_or("point instruction range")?;
                if !covered.insert(i) || (anchor.is_none() && matches!(atom.call, Call::Table(_))) {
                    return Err("invalid point instruction domain".into());
                }
                let full = !atom.args[..atom.args.len() - 1]
                    .iter()
                    .any(|a| matches!(a,Arg::Slot(s) if !known.contains(s)));
                let inverse = matches!(atom.call,Call::Table(t) if program.tables[t].schema.last().unwrap()["kind"]=="eq")
                    && matches!(atom.args.last(),Some(Arg::Slot(s)) if known.contains(s));
                if !full && !inverse {
                    return Err("point read has unbound inputs".into());
                }
                for arg in &atom.args {
                    if let Arg::Slot(s) = arg {
                        known.insert(*s);
                    }
                }
                order.push(i);
            }
            Ok(Rc::new(order))
        };
        for entry in array(&raw["entries"])? {
            let rid = entry["rule"].as_u64().ok_or("point rule")? as usize;
            let anchor = entry["anchor"].as_u64().ok_or("point anchor")? as usize;
            if !matches!(
                program
                    .rules
                    .get(rid)
                    .and_then(|r| r.query.get(anchor))
                    .map(|a| &a.call),
                Some(Call::Table(_))
            ) {
                return Err("point entry is not a fact".into());
            }
            let order = verify_points(rid, Some(anchor), &entry["atoms"])?;
            if result.entries.insert((rid, anchor), order).is_some() {
                return Err("duplicate point entry".into());
            }
        }
        if array(&raw["finishes"])?.len() != program.rules.len() {
            return Err("point finishes differ".into());
        }
        for (rid, finish) in array(&raw["finishes"])?.iter().enumerate() {
            result.finishes.push(verify_points(rid, None, finish)?);
        }
        for route in array(&raw["routes"])? {
            let number = |name: &str| {
                route[name]
                    .as_u64()
                    .map(|n| n as usize)
                    .ok_or_else(|| format!("invalid continuation {name}"))
            };
            let producer = number("producer")?;
            let effect = number("effect")?;
            let consumer = number("consumer")?;
            let anchor = number("anchor")?;
            let target = program.rules.get(consumer).ok_or("continuation consumer")?;
            let (table, cols) = source(program, producer, "effect", effect)?;
            let mut mapping = BTreeMap::new();
            extend(
                target.query.get(anchor).ok_or("continuation anchor")?,
                table,
                &cols,
                &mut mapping,
            )?;
            let mut covered = BTreeSet::from([anchor]);
            let mut reads = vec![];
            for step in array(&route["steps"])? {
                let atom = step["atom"].as_u64().ok_or("continuation atom")? as usize;
                let index = step["source_index"]
                    .as_u64()
                    .ok_or("continuation source index")? as usize;
                let kind = string(&step["source_kind"])?;
                if !covered.insert(atom) {
                    return Err("duplicate continuation atom".into());
                }
                let (table, columns) = source(program, producer, kind, index)?;
                extend(
                    target.query.get(atom).ok_or("continuation atom index")?,
                    table,
                    &columns,
                    &mut mapping,
                )?;
                let assignments = if kind == "effect" {
                    assignments(&target.query[anchor], &target.query[atom], &columns)
                } else {
                    vec![]
                };
                if let Some(raw) = step.get("assignments") {
                    if *raw != serde_json::json!(assignments) {
                        return Err("invalid producer slot wiring".into());
                    }
                }
                reads.push(LocalRead {
                    assignments,
                    atom,
                    table,
                    columns,
                    effect: (kind == "effect").then_some(index),
                });
            }
            let residual = (0..target.query.len())
                .filter(|i| !covered.contains(i))
                .map(J::from)
                .collect::<Vec<_>>();
            if route["residual"] != J::Array(residual) {
                return Err("continuation residual domain differs".into());
            }
            if result
                .routes
                .insert(
                    (program.rules[producer].template, effect, consumer, anchor),
                    Rc::new(Continuation {
                        producer,
                        consumer,
                        anchor,
                        reads,
                    }),
                )
                .is_some()
            {
                return Err("duplicate continuation".into());
            }
        }
        Ok(result)
    }
}
