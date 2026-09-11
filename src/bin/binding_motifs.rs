//! Extract typed, conservative binding relations from recorded source-site witnesses.
use egg_layout::{
    binding_program::{Term, learn},
    visual_rule,
};
use egglog::{
    EGraph,
    ast::{Command, Expr, Fact, Rule},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
fn variables(e: &Expr, out: &mut Vec<String>) {
    match e {
        Expr::Var(_, v) => {
            if !out.contains(v) {
                out.push(v.clone());
            }
        }
        Expr::Call(_, _, a) => {
            for x in a {
                variables(x, out)
            }
        }
        _ => {}
    }
}
fn rule_vars(r: &Rule) -> Vec<String> {
    let mut out = vec![];
    for f in &r.body {
        match f {
            Fact::Eq(_, a, b) => match (a, b) {
                (Expr::Var(..), Expr::Call(..)) => variables(b, &mut out),
                (Expr::Call(..), Expr::Var(..)) => variables(a, &mut out),
                _ => {
                    variables(a, &mut out);
                    variables(b, &mut out);
                }
            },
            Fact::Fact(e) => variables(e, &mut out),
        }
    }
    out
}
fn typed(e: &Expr, sort: &str, role: &str, ports: &BTreeMap<String, usize>, eg: &EGraph) -> Term {
    match e {
        Expr::Var(_, v) => Term::new(
            sort,
            format!(
                "{}{}",
                if role == "c" { "out:" } else { "in:" },
                ports[&format!("{role}.{v}")]
            ),
            vec![],
        ),
        Expr::Lit(_, l) => Term::new(sort, format!("literal:{l:?}"), vec![]),
        Expr::Call(_, op, a) => {
            let f=eg.get_function(op).expect("this experiment only accepts declared table calls; primitives need a separate Calc contract");
            assert_eq!(f.schema().output.name(), sort);
            assert_eq!(a.len(), f.schema().input.len());
            Term::new(
                sort,
                format!("materialized:{op}"),
                a.iter()
                    .zip(&f.schema().input)
                    .map(|(e, s)| typed(e, s.name(), role, ports, eg))
                    .collect(),
            )
        }
    }
}
fn bind_value(t: &Term, value: &str, env: &mut BTreeMap<Term, String>) {
    if let Some(old) = env.insert(t.clone(), value.into()) {
        assert_eq!(
            old, value,
            "same typed term has inconsistent temporal values; needs versioned ports"
        );
    }
}
fn evaluate(t: &Term, env: &BTreeMap<Term, String>) -> String {
    env.get(t)
        .unwrap_or_else(|| panic!("missing explicit materialized witness for {t:?}"))
        .clone()
}
fn build(profile: &Value, source: &str) -> Value {
    let mut eg = EGraph::default();
    let commands = eg.parse_program(None, source).unwrap();
    let mut source_rules = BTreeMap::new();
    let mut rule_index = 0;
    for c in commands {
        if matches!(c, Command::RunSchedule(..)) {
            break;
        }
        if let Command::Rule { rule } = visual_rule::normalize(c.clone(), rule_index) {
            let id = if rule.name.is_empty() {
                format!("R{rule_index}")
            } else {
                rule.name.clone()
            };
            source_rules.insert(id, rule.to_string());
            rule_index += 1;
        }
        eg.run_program(vec![c]).unwrap();
    }
    let mut rules = BTreeMap::new();
    for (name, r) in profile["rule_labels"].as_object().unwrap() {
        assert_eq!(
            source_rules.get(name).map(String::as_str),
            r["definition"].as_str(),
            "profile/source contract mismatch"
        );
        let parsed = eg
            .parse_program(None, r["definition"].as_str().unwrap())
            .unwrap();
        let Command::Rule { rule } = parsed.into_iter().next().unwrap() else {
            panic!("expected normalized rule")
        };
        rules.insert(name.clone(), rule);
    }
    let mut programs = Vec::new();
    let mut class_rows = Vec::new();
    let mut total_verified = 0;
    let mut direct_checks = 0;
    let mut derived_checks = 0;
    for rank in profile["motif_rankings"].as_array().unwrap() {
        let shape = rank["shape"].as_str().unwrap();
        let target = shape.rsplit(" -> ").next().unwrap();
        let entries: Vec<_> = shape
            .strip_prefix('[')
            .unwrap()
            .split_once("] -> ")
            .unwrap()
            .0
            .split(" + ")
            .collect();
        let stages: BTreeMap<String, String> = entries
            .iter()
            .map(|e| {
                let (name, rest) = e.split_once('@').unwrap();
                (rest.split(':').next().unwrap().to_string(), name.into())
            })
            .collect();
        let mut in_ports = BTreeMap::new();
        let mut port_labels = Vec::new();
        for (role, name) in &stages {
            for v in rule_vars(&rules[name]) {
                let id = port_labels.len();
                in_ports.insert(format!("{role}.{v}"), id);
                port_labels.push(format!("{role}.{v}"));
            }
        }
        let cvars = rule_vars(&rules[target]);
        let out_ports: BTreeMap<_, _> = cvars
            .iter()
            .enumerate()
            .map(|(i, v)| (format!("c.{v}"), i))
            .collect();
        let occurrences: Vec<_> = profile["witnesses"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|w| w["motif"] == rank["shape"])
            .collect();
        let first = occurrences[0];
        let mut equations = Vec::new();
        let mut connections = Vec::new();
        let mut input_sorts = vec![None; port_labels.len()];
        let mut output_sorts = vec![None; cvars.len()];
        fn infer_expr(
            e: &Expr,
            expected: Option<&str>,
            found: &mut BTreeMap<String, String>,
            eg: &EGraph,
        ) {
            match e {
                Expr::Var(_, v) => {
                    if let Some(s) = expected {
                        if let Some(old) = found.insert(v.clone(), s.into()) {
                            assert_eq!(old, s);
                        }
                    }
                }
                Expr::Call(_, op, args) => {
                    let f = eg.get_function(op).unwrap();
                    if let Some(s) = expected {
                        assert_eq!(s, f.schema().output.name());
                    }
                    for (a, s) in args.iter().zip(&f.schema().input) {
                        infer_expr(a, Some(s.name()), found, eg);
                    }
                }
                _ => {}
            }
        }
        let infer = |r: &Rule| {
            let mut found = BTreeMap::new();
            for f in &r.body {
                match f {
                    Fact::Eq(_, a, b) => {
                        infer_expr(a, None, &mut found, &eg);
                        infer_expr(b, None, &mut found, &eg);
                    }
                    Fact::Fact(e) => infer_expr(e, None, &mut found, &eg),
                }
            }
            found
        };
        for (role, name) in &stages {
            let types = infer(&rules[name]);
            for (v, s) in types {
                if let Some(i) = in_ports.get(&format!("{role}.{v}")) {
                    input_sorts[*i] = Some(s);
                }
            }
        }
        for (v, s) in infer(&rules[target]) {
            if let Some(i) = out_ports.get(&format!("c.{v}")) {
                output_sorts[*i] = Some(s);
            }
        }

        for (entry, support) in entries.iter().zip(first["support"].as_array().unwrap()) {
            let (_, rest) = entry.split_once('@').unwrap();
            let role = rest.split(':').next().unwrap();
            let producer = &rules[&stages[role]];
            let consumer = &rules[target];
            let ppath = support["producer_sites"][0].as_str().unwrap();
            let cpath = support["consumer_sites"][0].as_str().unwrap();
            let Expr::Call(_, op, pa) = visual_rule::expression_at(producer, ppath).unwrap() else {
                panic!()
            };
            let Expr::Call(_, cop, ca) = visual_rule::expression_at(consumer, cpath).unwrap()
            else {
                panic!()
            };
            assert_eq!(op, cop);
            let f = eg.get_function(op).unwrap();
            let start = equations.len();
            for (a, b) in pa.iter().zip(ca).zip(&f.schema().input) {
                let ((a, b), sort) = (a, b);
                let p = typed(a, sort.name(), role, &in_ports, &eg);
                let c = typed(b, sort.name(), "c", &out_ports, &eg);
                fn types(t: &Term, ins: &mut [Option<String>], outs: &mut [Option<String>]) {
                    let slot = if let Some(i) = t.op.strip_prefix("in:") {
                        Some(&mut ins[i.parse::<usize>().unwrap()])
                    } else if let Some(i) = t.op.strip_prefix("out:") {
                        Some(&mut outs[i.parse::<usize>().unwrap()])
                    } else {
                        None
                    };
                    if let Some(slot) = slot {
                        if let Some(old) = slot {
                            assert_eq!(*old, t.sort);
                        } else {
                            *slot = Some(t.sort.clone());
                        }
                    }
                    for a in &t.args {
                        types(a, ins, outs);
                    }
                }
                types(&p, &mut input_sorts, &mut output_sorts);
                types(&c, &mut input_sorts, &mut output_sorts);
                equations.push((p, c));
            }
            connections.push((role.to_string(), start, pa.len()));
        }
        let (assignments, residual) = egg_layout::binding_program::normalize_equations(&equations);
        let mut ops = Vec::new();
        for (i, t) in &assignments {
            ops.push(Term::new(
                "Assignment",
                format!("assign:{i}"),
                vec![t.clone()],
            ));
        }
        ops.extend(residual.clone());
        // Type signatures are part of identity. Unused ports remain explicit context inputs.
        let ins = Term::new(
            "Signature",
            "inputs",
            input_sorts
                .iter()
                .map(|s| Term::new("Sort", s.as_deref().unwrap_or("context"), vec![]))
                .collect(),
        );
        let outs = Term::new(
            "Signature",
            "outputs",
            output_sorts
                .iter()
                .map(|s| Term::new("Sort", s.as_deref().unwrap_or("context"), vec![]))
                .collect(),
        );
        let program = Term::new(
            "BindingMap",
            "binding-map",
            vec![ins, outs, Term::new("Ops", "ops", ops)],
        );
        for w in &occurrences {
            assert_eq!(w["support"].as_array().unwrap().len(), connections.len());
            for (observed, template) in w["support"]
                .as_array()
                .unwrap()
                .iter()
                .zip(first["support"].as_array().unwrap())
            {
                assert_eq!(observed["producer_sites"], template["producer_sites"]);
                assert_eq!(observed["consumer_sites"], template["consumer_sites"]);
                assert_eq!(observed["table"], template["table"]);
                assert_eq!(
                    observed["kind"], "row",
                    "rebuild/equality epochs require a versioned binding interface"
                );
            }

            let mut env = BTreeMap::new();
            for ((role, start, n), support) in
                connections.iter().zip(w["support"].as_array().unwrap())
            {
                assert_eq!(support["key_values"].as_array().unwrap().len(), *n);
                for j in 0..*n {
                    let (p, c) = &equations[start + j];
                    let value = support["key_values"][j].as_str().unwrap();
                    for (t, bindings, labels) in [
                        (p, &support["producer_bindings"], &port_labels),
                        (c, &w["consumer_bindings"], &cvars),
                    ] {
                        if let Some(i) =
                            t.op.strip_prefix("in:")
                                .or_else(|| t.op.strip_prefix("out:"))
                        {
                            let name = &labels[i.parse::<usize>().unwrap()];
                            let name = if t.op.starts_with("in:") {
                                name.strip_prefix(&format!("{role}.")).unwrap()
                            } else {
                                name.as_str()
                            };
                            assert_eq!(bindings[name].as_str(), Some(value));
                            direct_checks += 1;
                        } else {
                            assert!(
                                t.op.starts_with("materialized:") || t.op.starts_with("literal:")
                            );
                            derived_checks += 1;
                        }
                        bind_value(t, value, &mut env);
                    }
                }
            }
            for (i, t) in &assignments {
                assert_eq!(
                    evaluate(t, &env),
                    w["consumer_bindings"][&cvars[*i]].as_str().unwrap()
                );
            }
            for constraint in &residual {
                assert_eq!(
                    evaluate(&constraint.args[0], &env),
                    evaluate(&constraint.args[1], &env)
                );
            }
            total_verified += 1;
        }
        class_rows.push(json!({"rank":rank["rank"],"shape":shape,"occurrences":occurrences.len(),"producer_port_labels":port_labels,"consumer_port_labels":cvars,"normalized":program.json(),"boundary_consumer_ports":(0..cvars.len()).filter(|i|!assignments.contains_key(i)).collect::<Vec<_>>(),"residual_constraints":residual.len(),"contracts":{"producer_rules":stages,"consumer_rule":target,"rule_definitions":"retained by reference in profile.rule_labels; not erased or included in wiring equivalence"},"verification":"all recorded port equations and mapped consumer variables agree; materialized constructor values are supplied by native write/read witnesses, not independently recomputed"}));
        programs.push(program);
    }
    let mut ids = BTreeMap::new();
    let mut unique = Vec::new();
    for t in &programs {
        if !ids.contains_key(t) {
            ids.insert(t.clone(), unique.len());
            unique.push(t.clone());
        }
    }
    let (defs, compressed, log) = learn(&unique, 8);
    let (control_defs, control_trees, _) =
        egg_layout::binding_program::learn_with_policy(&unique, 8, false);
    let control_cost = control_trees.iter().map(Term::size).sum::<usize>()
        + control_defs.iter().map(|p| p.size() + 1).sum::<usize>();
    for (row, t) in class_rows.iter_mut().zip(&programs) {
        let id = ids[t];
        row["binding_map_id"] = json!(format!("B{id}"));
        row["component_decomposition"] = compressed[id].json();
    }
    let mut composition_edges = Vec::new();
    for (i, a) in unique.iter().enumerate() {
        for (j, b) in unique.iter().enumerate() {
            if let Some(c) = egg_layout::binding_program::compose_routes(a, b) {
                composition_edges.push(json!({"first":format!("B{i}"),"second":format!("B{j}"),"result_existing_id":ids.get(&c).map(|i|format!("B{i}")),"result":c.json(),"identity":egg_layout::binding_program::is_identity_route(&c),"observed_macro_execution":false,"requires_rule_contract_alignment":true,"proof":"total typed port substitution","scope":"wiring only; source rule contracts and egraph effects are not equated"}));
            }
        }
    }
    let raw: usize = programs.iter().map(Term::size).sum();
    let canonical: usize = unique.iter().map(Term::size).sum();
    let after: usize = compressed.iter().map(Term::size).sum::<usize>()
        + defs.iter().map(|p| 1 + p.size()).sum::<usize>();
    json!({"binding_composition_relations":composition_edges,"unrestricted_syntax_control":{"cost_nodes":control_cost,"definitions":control_defs.len()},"component_filter":"requires concrete port wiring, repeated value holes, or materialized constructor shape; rejects signature-only components","scope":"offline typed binding-relation analysis over native witnesses; no new rewrite equivalences or runtime shortcuts","profile_round_limit":profile["profile_round_limit"],"macro_classes":programs.len(),"unique_wiring_relations":unique.len(),"verified_occurrences":total_verified,"direct_variable_checks":direct_checks,"materialized_value_checks":derived_checks,"classes":class_rows,"components":defs.iter().enumerate().map(|(i,p)|json!({"id":format!("F{i}"),"pattern":p.json(),"requires_interface_validation_on_new_calls":true,"kind":"typed syntactic expansion macro, not an inductively proved fractal rule"})).collect::<Vec<_>>(),"learning":log,"description_length":{"unit":"AST nodes, including definition nodes; not bytes/RSS or full proof storage","raw_maps":raw,"unique_maps":canonical,"unique_maps_plus_class_references":canonical+programs.len(),"component_corpus_plus_definitions":after,"component_corpus_plus_definitions_plus_class_references":after+programs.len()},"equivalence_scope":"same normalized wiring only; full source rule guards/actions are retained separately and do not become semantically equivalent","normalization":"orient consumer variables, retain nested e-class equalities, remove exact duplicate constraints; no constructor injectivity assumed"})
}
fn main() {
    let profile_path = std::env::args()
        .nth(1)
        .unwrap_or("experiments/annotated_export/math_microbenchmark/profile.json".into());
    let p: Value = serde_json::from_str(&std::fs::read_to_string(&profile_path).unwrap()).unwrap();
    let source = std::fs::read_to_string(p["source"].as_str().unwrap()).unwrap();
    let result = build(&p, &source);
    let text = serde_json::to_string_pretty(&result).unwrap();
    if let Some(out) = std::env::args().nth(2) {
        std::fs::write(out, text).unwrap();
    } else {
        println!("{text}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn corrupted_binding_is_rejected() {
        let mut p: Value = serde_json::from_str(include_str!(
            "../../experiments/annotated_export/math_microbenchmark/profile.json"
        ))
        .unwrap();
        p["witnesses"][0]["consumer_bindings"]["a"] = json!("Value(999999999)");
        let source = std::fs::read_to_string(p["source"].as_str().unwrap()).unwrap();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build(&p, &source))).is_err()
        );
    }
}
