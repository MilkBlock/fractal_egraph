//! Exact RipSat equivalence certificates shared by analysis and execution.
use super::{
    Bake, Rule,
    transitions::{self, Binding, Compiled, FactIndex},
};
use crate::{
    pipeline::Result,
    saturated_rule_composition::{
        self as states, Comparison, Row, SaturatedRuleComposition as Graph, Vertex,
    },
};
use egglog::{
    EGraph,
    ast::{Action, Command, Expr, Parser},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Clone, Default)]
pub(super) struct Catalog {
    // rule -> (semantic eclass, local slot -> shared slot)
    maps: BTreeMap<usize, (usize, Vec<usize>)>,
    reads: BTreeMap<usize, Vec<usize>>,
    pub report: Value,
}
impl Catalog {
    pub fn shared_slot(&self, rule: usize, slot: usize) -> usize {
        self.maps.get(&rule).map(|(_, m)| m[slot]).unwrap_or(slot)
    }
    pub fn read_slots(&self, rule: usize) -> &[usize] {
        self.reads.get(&rule).map(Vec::as_slice).unwrap_or(&[])
    }
    pub fn key(&self, rule: usize, binding: &[egglog::Value]) -> (usize, Vec<egglog::Value>) {
        let Some((class, map)) = self.maps.get(&rule) else {
            return (rule, binding.to_vec());
        };
        let mut shared = binding.to_vec();
        for (slot, to) in map.iter().enumerate() {
            shared[*to] = binding[slot];
        }
        (*class, shared)
    }
    pub fn variants(
        &self,
        rule: usize,
        binding: &[egglog::Value],
    ) -> Vec<(usize, Vec<egglog::Value>)> {
        let Some((class, _)) = self.maps.get(&rule) else {
            return vec![(rule, binding.to_vec())];
        };
        let (_, shared) = self.key(rule, binding);
        self.maps
            .iter()
            .filter(|(_, (c, _))| c == class)
            .map(|(r, (_, m))| (*r, m.iter().map(|i| shared[*i]).collect()))
            .collect()
    }
    pub fn expand(&self, plans: &mut Vec<Compiled>, bake: &Bake) {
        let original = plans.len();
        let mut seen: BTreeSet<_> = plans
            .iter()
            .map(|p| {
                (
                    p.spec.source_rule,
                    p.spec.consumer_rule,
                    p.spec.cut_path.clone(),
                    p.spec.anchor_path.clone(),
                )
            })
            .collect();
        for i in 0..original {
            let p = &plans[i];
            let Some((class, from)) = self.maps.get(&p.spec.source_rule) else {
                continue;
            };
            let mut additions = vec![];
            for (rule, (q, to)) in &self.maps {
                if q != class
                    || !seen.insert((
                        *rule,
                        p.spec.consumer_rule,
                        p.spec.cut_path.clone(),
                        p.spec.anchor_path.clone(),
                    ))
                {
                    continue;
                }
                let mut inverse = vec![0; to.len()];
                for (i, j) in to.iter().enumerate() {
                    inverse[*j] = i;
                }
                let remap =
                    |cols: &Vec<usize>| cols.iter().map(|i| inverse[from[*i]]).collect::<Vec<_>>();
                let mut next = p.clone();
                next.spec.source_rule = *rule;
                next.definitions.clear();
                next.rows = p.rows.iter().map(|(a, b)| (remap(a), remap(b))).collect();
                next.projections = next
                    .rows
                    .iter()
                    .map(|(row, keys)| {
                        let terms = |xs: &[usize]| {
                            xs.iter()
                                .map(|i| bake.macros[rule].slots[*i].0.as_str())
                                .collect::<Vec<_>>()
                                .join(" ")
                        };
                        format!(
                            "(set ({} {}) {})\n({} {})",
                            next.relation,
                            terms(&row[..row.len() - 1]),
                            terms(&row[row.len() - 1..]),
                            next.cover,
                            terms(keys)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                next.proof["source_cell"] = json!(rule);
                next.proof["spec"] = json!(next.spec);
                next.proof["equivalent_source"] =
                    json!({"from":p.spec.source_rule,"eclass":class,"slot_map":to});
                next.proof["symbolic_binding_rows"] = json!(next.rows);
                additions.push(next);
            }
            plans.extend(additions);
        }
    }
}
fn environment(bake: &Bake) -> Result<Value> {
    let mut parser = EGraph::default();
    let commands = parser.parse_program(None, &bake.environment.join("\n"))?;
    let mut constructors = vec![];
    let mut rules = vec![];
    for (i, c) in commands.into_iter().enumerate() {
        match crate::visual_rule::normalize(c, i) {
            Command::Datatype { name, variants, .. } if name == "Math" => {
                for v in variants {
                    constructors.push(json!({"name":v.name,"types":v.types,"cost":v.cost.map(|x|x.to_string()),"unextractable":v.unextractable}));
                }
            }
            Command::Rule { mut rule } => {
                rule.name.clear();
                rule.ruleset.clear();
                rule.naive = false;
                rules.push(rule.to_string());
            }
            _ => return Err("runtime catalog requires the positive Math environment".into()),
        }
    }
    constructors.sort_by_key(|v| v["name"].as_str().unwrap().to_owned());
    rules.sort();
    rules.dedup();
    Ok(json!({"datatype":"Math","constructors":constructors,"rules":rules}))
}
fn runtime_graph(
    bake: &Bake,
    rule: &Rule,
    id: usize,
    scope: &Value,
) -> Result<(Graph, Vec<usize>)> {
    let cell = &bake.macros[&id];
    let values = cell
        .slots
        .iter()
        .map(|(literal, sort)| Vertex {
            sort: sort.clone(),
            literal: if sort == "Math" {
                None
            } else {
                Some(literal.clone())
            },
        })
        .collect();
    let rows: Vec<_> = cell
        .template
        .iter()
        .map(|(op, c)| Row {
            op: op.clone(),
            args: c[..c.len() - 1].to_vec(),
            result: *c.last().unwrap(),
        })
        .collect();
    let facts: FactIndex = rows
        .iter()
        .map(|r| ((r.op.clone(), r.args.clone()), r.result))
        .collect();
    let literals: Binding = cell
        .slots
        .iter()
        .enumerate()
        .map(|(i, (s, _))| (s.clone(), i))
        .collect();
    let mut bindings = literals.clone();
    // Resolve anchor aliases explicitly recorded by the certified recipe.
    let commands = Parser::default().get_program_from_string(None, &cell.actions)?;
    loop {
        let before = bindings.len();
        for c in &commands {
            match c {
                Command::Action(Action::Let(_, v, e)) => {
                    if let Some(i) = transitions::evaluate(e, &bindings, &literals, &facts) {
                        bindings.insert(v.clone(), i);
                    }
                }
                Command::Action(Action::Union(_, a, b)) => {
                    for (a, b) in [(a, b), (b, a)] {
                        if let Expr::Var(_, v) = a {
                            if let Some(i) = transitions::evaluate(b, &bindings, &literals, &facts)
                            {
                                bindings.entry(v.clone()).or_insert(i);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if bindings.len() == before {
            break;
        }
    }
    let mut ports = BTreeMap::new();
    for (i, name) in transitions::vars(&rule.lhs).iter().enumerate() {
        ports.insert(
            format!("input{i}"),
            *bindings.get(name).ok_or("missing interface alias")?,
        );
    }
    fn collect(
        e: &Expr,
        bindings: &Binding,
        literals: &Binding,
        facts: &FactIndex,
        reads: &mut BTreeSet<usize>,
    ) {
        if let Expr::Call(_, _, args) = e {
            if let Some(slot) = transitions::evaluate(e, bindings, literals, facts) {
                reads.insert(slot);
            }
            for a in args {
                collect(a, bindings, literals, facts, reads);
            }
        }
    }
    let mut reads = BTreeSet::new();
    collect(&rule.lhs, &bindings, &literals, &facts, &mut reads);
    if let Some(root) = transitions::evaluate(&rule.lhs, &bindings, &literals, &facts) {
        reads.insert(root);
    }
    Ok((
        Graph {
            version: 2,
            local_ids: vec![],
            scope: scope.clone(),
            values,
            rows,
            subsumed_rows: vec![],
            ports,
        },
        reads.into_iter().collect(),
    ))
}
fn imported(mut graph: Graph, marker: &str, expected: &Value) -> Result<Graph> {
    if graph.scope["semantics"] != "positive-constructor-equality/v1"
        || graph.scope["symbolic_boundary"] != true
        || !graph.subsumed_rows.is_empty()
        || !graph.ports.is_empty()
    {
        return Err("unsupported catalog semantics or named interface".into());
    }
    let mut constructors = graph.scope["constructors"]
        .as_array()
        .ok_or("missing constructors")?
        .clone();
    constructors.retain(|v| v["name"] != marker);
    let scope = json!({"datatype":graph.scope["datatype"],"constructors":constructors,"rules":graph.scope["rules"]});
    if &scope != expected {
        return Err("catalog rule environment mismatch".into());
    }
    let mut ports = BTreeMap::new();
    for r in graph.rows.iter().filter(|r| r.op == marker) {
        if r.args.len() != 1 {
            return Err("invalid marker arity".into());
        }
        let n: usize = graph.values[r.args[0]]
            .literal
            .as_ref()
            .ok_or("missing marker label")?
            .parse()?;
        if ports.insert(format!("input{n}"), r.result).is_some() {
            return Err("duplicate marker".into());
        }
    }
    graph.rows.retain(|r| r.op != marker);
    let used: BTreeSet<_> = graph
        .rows
        .iter()
        .flat_map(|r| r.args.iter().copied().chain([r.result]))
        .chain(ports.values().copied())
        .collect();
    let ids: BTreeMap<_, _> = used
        .iter()
        .enumerate()
        .map(|(new, old)| (*old, new))
        .collect();
    graph.values = used.iter().map(|i| graph.values[*i].clone()).collect();
    for r in &mut graph.rows {
        r.args = r.args.iter().map(|i| ids[i]).collect();
        r.result = ids[&r.result];
    }
    graph.ports = ports.into_iter().map(|(p, i)| (p, ids[&i])).collect();
    graph.local_ids.clear();
    graph.scope = expected.clone();
    Ok(graph)
}
pub(super) fn compile(bake: &Bake, rules: &[Rule], directory: Option<&Path>) -> Result<Catalog> {
    let scope = environment(bake)?;
    let mut representatives = vec![];
    let mut imported_count = 0;
    let mut skipped = vec![];
    if let Some(dir) = directory {
        let manifest: Value = serde_json::from_slice(&std::fs::read(dir.join("catalog.json"))?)?;
        let mut markers = BTreeMap::new();
        for t in manifest["triggers"]
            .as_array()
            .ok_or("missing catalog triggers")?
        {
            if let (Some(q), Some(marker)) = (
                t["saturated_rule_composition"].as_u64(),
                t["binding_origin"]["parameter_marker"].as_str(),
            ) {
                markers.entry(q).or_insert(marker.to_owned());
            }
        }
        for (q, marker) in markers {
            let graph = states::read(&dir.join(format!("states/state-{q:04}.json")))?;
            match imported(graph, &marker, &scope) {
                Ok(g) => {
                    representatives.push((Some(q), g));
                    imported_count += 1;
                }
                Err(e) => skipped.push(json!({"catalog_state":q,"reason":e.to_string()})),
            }
        }
    }
    let mut catalog = Catalog::default();
    let mut entries = vec![];
    for (&id, cell) in &bake.macros {
        let (graph, reads) = runtime_graph(bake, &rules[id], id, &scope)?;
        catalog.reads.insert(id, reads);
        let mut found = None;
        for (q, (_, known)) in representatives.iter().enumerate() {
            if let Comparison::Equivalent { value_map, .. } = states::compare(&graph, known)? {
                found = Some((q, value_map));
                break;
            }
        }
        let (q, map) = found.unwrap_or_else(|| {
            let q = representatives.len();
            representatives.push((None, graph));
            (q, (0..cell.slots.len()).collect())
        });
        entries.push(
            json!({"rule":id,"eclass":q,"analysis_state":representatives[q].0,"slot_map":map}),
        );
        catalog.maps.insert(id, (q, map));
    }
    catalog.report = json!({"schema":"runtime-ripsat-equivalences/v1","environment":scope,"entries":entries,"imported_states":imported_count,"skipped_states":skipped,"scope":"exact complete graph isomorphism with opaque inputs fixed; mappings drive runtime identity and transition adapters"});
    Ok(catalog)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn imported_states_preserve_input_ports_and_reject_incompatible_semantics() {
        let scope = json!({"datatype":"Math","constructors":[{"name":"A","types":["Math"],"cost":null,"unextractable":false}],"rules":[]});
        let mut original = scope.clone();
        original["semantics"] = json!("positive-constructor-equality/v1");
        original["symbolic_boundary"] = json!(true);
        original["constructors"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name":"RipenInput","types":["i64"],"cost":null,"unextractable":false}));
        let graph = Graph {
            version: 2,
            local_ids: vec![],
            scope: original,
            values: vec![
                Vertex {
                    sort: "i64".into(),
                    literal: Some("0".into()),
                },
                Vertex {
                    sort: "Math".into(),
                    literal: None,
                },
                Vertex {
                    sort: "Math".into(),
                    literal: None,
                },
            ],
            rows: vec![
                Row {
                    op: "RipenInput".into(),
                    args: vec![0],
                    result: 1,
                },
                Row {
                    op: "A".into(),
                    args: vec![1],
                    result: 2,
                },
            ],
            ports: BTreeMap::new(),
            subsumed_rows: vec![],
        };
        let good = imported(graph.clone(), "RipenInput", &scope).unwrap();
        assert_eq!(good.ports["input0"], 0);
        assert_eq!(good.values.len(), 2);
        let mut bad = graph.clone();
        bad.scope["rules"] = json!(["different"]);
        assert!(imported(bad, "RipenInput", &scope).is_err());
        let mut bad = graph.clone();
        bad.subsumed_rows.push(1);
        assert!(imported(bad, "RipenInput", &scope).is_err());
        let mut bad = graph;
        bad.scope["semantics"] = json!("observed-constructor-graph/v1");
        assert!(imported(bad, "RipenInput", &scope).is_err());
    }
}
