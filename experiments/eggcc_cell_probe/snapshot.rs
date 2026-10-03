//! Native diagnostic snapshot adapter shared by the standalone experiments.
use egg_layout::{
    pipeline::Result,
    saturated_rule_composition::{Row, SaturatedRuleComposition as State, Vertex},
};
use egglog::EGraph;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
pub fn snapshot_filtered(
    eg: &EGraph,
    markers: &BTreeSet<String>,
    scope: &str,
    ignored: &BTreeSet<String>,
) -> Result<State> {
    let serialized = eg.serialize(egglog::SerializeConfig::default());
    if !serialized.is_complete() {
        return Err("incomplete native serialization".into());
    }
    let g = serialized.egraph;
    let mut raw = vec![];
    let mut boundaries = vec![];
    let mut used = BTreeSet::new();
    for n in g.nodes.values() {
        if ignored.contains(&n.op) {
            continue;
        }
        if markers.contains(&n.op) {
            boundaries.push(n.eclass.clone());
            used.insert(n.eclass.clone());
            continue;
        }
        let ty = g.class_data[&n.eclass]
            .typ
            .as_deref()
            .ok_or("missing sort")?;
        let sort = eg.get_sort_by_name(ty).ok_or("missing native sort")?;
        // Native serialization expands Set Expr using unordered membership.
        if sort.is_container_sort() {
            if n.op != "set-of" {
                return Err(format!("probe does not compare container {ty}/{}", n.op).into());
            }
            raw.push((format!("@set:{ty}"), vec![], n.eclass.clone(), false));
            for child in &n.children {
                let c = g.nodes[child].eclass.clone();
                raw.push((format!("@member:{ty}"), vec![c], n.eclass.clone(), false));
            }
        } else if eg.get_function(&n.op).is_some() {
            raw.push((
                n.op.clone(),
                n.children
                    .iter()
                    .map(|c| g.nodes[c].eclass.clone())
                    .collect(),
                n.eclass.clone(),
                n.subsumed,
            ));
        }
    }
    for (_, args, result, _) in &raw {
        used.extend(args.iter().cloned());
        used.insert(result.clone());
    }
    let ids: BTreeMap<_, _> = used
        .iter()
        .cloned()
        .enumerate()
        .map(|(i, c)| (c, i))
        .collect();
    let mut values = vec![];
    for c in &used {
        let ty = g.class_data[c].typ.clone().ok_or("missing sort")?;
        let sort = eg.get_sort_by_name(&ty).unwrap();
        let literal = if sort.is_eq_sort() || sort.is_container_sort() {
            None
        } else {
            Some(
                g.nodes
                    .values()
                    .find(|n| &n.eclass == c && eg.get_function(&n.op).is_none())
                    .ok_or("missing primitive representation")?
                    .op
                    .clone(),
            )
        };
        values.push(Vertex { sort: ty, literal });
    }
    let mut rows: BTreeMap<Row, bool> = BTreeMap::new();
    for (op, args, result, hidden) in raw {
        let row = Row {
            op,
            args: args.iter().map(|c| ids[c]).collect(),
            result: ids[&result],
        };
        rows.entry(row)
            .and_modify(|h| *h &= hidden)
            .or_insert(hidden);
    }
    // Keep the number and aliases of boundary slots, while permitting renaming.
    for c in boundaries {
        let slot = values.len();
        values.push(Vertex {
            sort: format!("@input:{}", values[ids[&c]].sort),
            literal: None,
        });
        rows.insert(
            Row {
                op: "@binding".into(),
                args: vec![ids[&c]],
                result: slot,
            },
            false,
        );
    }
    let hidden = rows
        .values()
        .enumerate()
        .filter_map(|(i, h)| h.then_some(i))
        .collect();
    Ok(State {
        version: 2,
        local_ids: vec![],
        scope: json!({"probe_environment":scope,"semantics":"observed snapshot isomorphism only; literals retained; input slots may be renamed; no general merge proof"}),
        values,
        rows: rows.into_keys().collect(),
        subsumed_rows: hidden,
        ports: BTreeMap::new(),
    })
}
