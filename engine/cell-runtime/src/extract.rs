//! A diagnostic extractor over virtual members. No native EGraph or restored
//! fact tables. This explicit unit-cost model is not eggcc's effectful extractor.
use crate::{
    graph::*,
    program::Result,
    value::{Value, exported, root},
};
use serde_json::{Value as J, json};
use std::collections::{BTreeMap, BTreeSet};
impl Graph {
    pub fn extract_unit_cost(&self, operator: &str) -> Result<J> {
        let table = self
            .program
            .tables
            .iter()
            .position(|t| t.name == operator)
            .ok_or("extraction operator missing")?;
        if self.program.tables[table].schema.last().unwrap()["kind"] != "eq" {
            return Err("extraction root must be equality sort".into());
        }
        let mut candidates = vec![];
        for t in 0..self.program.tables.len() {
            if self.program.tables[t].constructor {
                for w in self.witnesses(t) {
                    if let Some(row) = self.row(w)? {
                        if !row.hidden {
                            candidates.push(row)
                        }
                    }
                }
            }
        }
        let mut best: BTreeMap<u64, (u64, Dependency)> = BTreeMap::new();
        loop {
            let mut changed = false;
            for row in &candidates {
                let Value::Eq(id) = row.values.last().unwrap() else {
                    continue;
                };
                let mut cost = Some(1u64);
                for arg in &row.values[..row.values.len() - 1] {
                    if let Value::Eq(child) = arg {
                        cost =
                            cost.and_then(|n| best.get(child).and_then(|(c, _)| n.checked_add(*c)));
                    } else if matches!(arg, Value::Set(_)) {
                        cost = None;
                    }
                }
                if let Some(cost) = cost {
                    if best.get(id).is_none_or(|(old, _)| cost < *old) {
                        best.insert(*id, (cost, row.witness));
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        let mut roots = BTreeSet::new();
        for w in self.witnesses(table) {
            if let Some(row) = self.row(w)? {
                if !row.hidden {
                    if let Value::Eq(id) = row.values.last().unwrap() {
                        roots.insert(*id);
                    }
                }
            }
        }
        fn visit(
            g: &Graph,
            id: u64,
            best: &BTreeMap<u64, (u64, Dependency)>,
            ids: &mut BTreeMap<u64, usize>,
            nodes: &mut Vec<J>,
        ) -> Result<Option<usize>> {
            if let Some(i) = ids.get(&id) {
                return Ok(Some(*i));
            }
            let Some((_, w)) = best.get(&id) else {
                return Ok(None);
            };
            let row = g.row(*w)?.ok_or("extract witness disappeared")?;
            let mut args = vec![];
            for (v, s) in row.values[..row.values.len() - 1]
                .iter()
                .zip(&g.program.tables[row.table].schema)
            {
                args.push(if let Value::Eq(c) = v {
                    json!({"node":visit(g,*c,best,ids,nodes)?.ok_or("unextractable child")?})
                } else {
                    json!({"literal":exported(v,s)})
                });
            }
            let i = nodes.len();
            nodes.push(json!({"id":i,"operator":g.program.tables[row.table].name,"args":args}));
            ids.insert(id, i);
            Ok(Some(i))
        }
        let mut ids = BTreeMap::new();
        let mut nodes = vec![];
        let mut outputs = vec![];
        for id in roots {
            let id = root(&self.parents, id);
            let node = visit(self, id, &best, &mut ids, &mut nodes)?;
            outputs.push(json!({"eclass":format!("Cell({id})"),"node":node,"cost":best.get(&id).map(|x|x.0)}));
        }
        Ok(
            json!({"model":"diagnostic unit constructor cost; scalar literals cost zero; visible constructors only; not eggcc custom/effectful extraction","roots":outputs,"nodes":nodes}),
        )
    }
}
