//! Independently verified storage projections. No learned containment claim.
use crate::{graph::FactRef, program::*};
use serde::{Deserialize, Serialize};
use serde_json::Value as J;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Carrier {
    pub atom: usize,
    pub table: usize,
    pub columns: Vec<Arg>,
}
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct Layout {
    pub reference_only: bool,
    pub slots: BTreeMap<usize, (usize, usize)>,
    pub carriers: Vec<Carrier>,
}
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct Views {
    pub layouts: BTreeMap<usize, Layout>,
    pub markers: BTreeSet<usize>,
    pub exports: BTreeMap<usize, BTreeMap<usize, Vec<(usize, usize)>>>,
}
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewStats {
    pub recovered_continuations: u64,
    pub projected_values: u64,
    pub marker_rows: u64,
    pub absorption_attempts: u64,
    pub absorbed_cells: u64,
    pub relocated_facts: u64,
    pub uncovered_sources: u64,
    pub retained_function_owners: u64,
    pub type_rejections: u64,
}
impl Views {
    pub fn parse(p: &Program, j: &J) -> Result<Self> {
        if j.is_null() {
            return Ok(Self::default());
        }
        if j["schema"] != "eggfuse-reference-views/v1"
            || j["recipe_sha256"] != p.artifact["recipe"]["sha256"]
        {
            return Err("reference view recipe mismatch".into());
        }
        let mut v = Self::default();
        for name in array(&j["marker_tables"])? {
            let t = p
                .tables
                .iter()
                .position(|t| name == &t.name)
                .ok_or("unknown marker table")?;
            let table = &p.tables[t];
            if table.constructor
                || table.schema.last().unwrap()["kind"] != "Unit"
                || table.merge != serde_json::json!({"var":"old"})
            {
                return Err("unsafe reference marker".into());
            }
            fn mentions(j: &J, name: &str) -> bool {
                match j {
                    J::Object(m) => j["table"] == name || m.values().any(|x| mentions(x, name)),
                    J::Array(a) => a.iter().any(|x| mentions(x, name)),
                    _ => false,
                }
            }
            if p.tables.iter().any(|x| mentions(&x.merge, &table.name))
                || p.templates[..p.rules.len()].iter().any(|r| {
                    r.ops
                        .iter()
                        .any(|op| matches!(op,Op::Let(_,Call::Table(i),_)|Op::Hide(i,_,_) if *i==t))
                })
            {
                return Err("observable marker read/removal".into());
            }
            v.markers.insert(t);
        }
        if array(&j["rules"])?.len() != p.rules.len() {
            return Err("missing reference rule".into());
        }
        for (rid, r) in p.rules.iter().enumerate() {
            let spec = &j["rules"][rid];
            if spec["rule"].as_u64() != Some(rid as u64) {
                return Err("reference rule order".into());
            }
            let mut layout = Layout::default();
            let ops = &p.templates[r.template].ops;
            layout.reference_only = ops
                .iter()
                .any(|e| matches!(e,Op::Set(t,_,_) if v.markers.contains(t)))
                && ops.iter().all(|e| {
                    matches!(e, Op::Copy(..) | Op::Let(_, Call::Primitive(..), _))
                        || matches!(e,Op::Set(t,_,_) if v.markers.contains(t))
                });
            if spec["storage"]
                != if layout.reference_only {
                    "reference-view"
                } else {
                    "owned-recipe"
                }
            {
                return Err("reference storage role differs".into());
            }
            for (ai, a) in r.query.iter().enumerate() {
                let Call::Table(t) = a.call else { continue };
                if !(p.tables[t].virtual_fact || v.markers.contains(&t)) {
                    continue;
                }
                for (col, arg) in a.args.iter().enumerate() {
                    if let Arg::Slot(s) = arg {
                        layout.slots.entry(*s).or_insert((ai, col));
                    }
                }
                layout.carriers.push(Carrier {
                    atom: ai,
                    table: t,
                    columns: a.args.clone(),
                });
            }
            let cs = array(&spec["carriers"])?;
            if cs.len() != layout.carriers.len() {
                return Err("carrier count mismatch".into());
            }
            for (s, c) in cs.iter().zip(&layout.carriers) {
                if s["atom"].as_u64() != Some(c.atom as u64)
                    || s["table"] != p.tables[c.table].name
                    || array(&s["columns"])?
                        .iter()
                        .map(arg)
                        .collect::<Result<Vec<_>>>()?
                        != c.columns
                {
                    return Err("carrier wiring differs from query".into());
                }
            }
            let mut slots = BTreeMap::new();
            for s in array(&spec["slots"])? {
                let get = |k: &str| {
                    s[k].as_u64()
                        .map(|x| x as usize)
                        .ok_or("invalid projection".to_string())
                };
                if slots
                    .insert(get("slot")?, (get("atom")?, get("column")?))
                    .is_some()
                {
                    return Err("duplicate projection".into());
                }
            }
            if slots != layout.slots {
                return Err("slot projection differs from query".into());
            }
            v.layouts.insert(r.template, layout);
        }
        for (tid, t) in p.templates.iter().enumerate() {
            let mut exports: BTreeMap<usize, Vec<(usize, usize)>> = BTreeMap::new();
            let mut rows = vec![];
            for (op, e) in t.ops.iter().enumerate() {
                let (table, columns) = match e {
                    Op::Let(dst, Call::Table(t), args) => {
                        let mut c = args.clone();
                        c.push(Arg::Slot(*dst));
                        (*t, c)
                    }
                    Op::Set(t, args, value) => {
                        let mut c = args.clone();
                        c.push(value.clone());
                        (*t, c)
                    }
                    _ => continue,
                };
                if p.tables[table].virtual_fact || v.markers.contains(&table) {
                    rows.push((op, columns));
                }
            }
            if let Some(l) = v.layouts.get(&tid) {
                rows.extend(
                    l.carriers
                        .iter()
                        .enumerate()
                        .map(|(i, c)| (t.ops.len() + i, c.columns.clone())),
                );
            }
            for (op, cols) in rows {
                for (col, a) in cols.iter().enumerate() {
                    if let Arg::Slot(s) = a {
                        exports.entry(*s).or_default().push((op, col));
                    }
                }
            }
            if tid < p.rules.len() {
                let expected=exports.iter().map(|(s,cs)|serde_json::json!({"slot":s,"sources":cs.iter().map(|(op,col)|serde_json::json!({"op":op,"column":col})).collect::<Vec<_>>()})).collect::<Vec<_>>();
                if j["rules"][tid]["continuation_slots"] != serde_json::json!(expected) {
                    return Err("continuation slot certificate differs".into());
                }
            }
            v.exports.insert(tid, exports);
        }
        Ok(v)
    }
    pub fn carrier(&self, template: usize, op: usize, base: usize) -> Option<&Carrier> {
        op.checked_sub(base)
            .and_then(|i| self.layouts.get(&template)?.carriers.get(i))
    }
}
// FactRefs stay logical handles; only parameter dereferences follow relocations.
pub fn resolve(map: &BTreeMap<FactRef, FactRef>, mut f: FactRef) -> FactRef {
    while let Some(n) = map.get(&f) {
        f = *n;
    }
    f
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct RetiredContext {
    pub template: usize,
    pub depth: u32,
}
#[derive(Clone, Copy, Debug)]
pub struct Context {
    pub cell: u32,
    pub template: usize,
    pub depth: u32,
    pub op: usize,
}
