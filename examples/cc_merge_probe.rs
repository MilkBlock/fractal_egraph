//! Exhaustive one-generation CC composition on a finite native egglog demo.
use egg_layout::{
    pipeline::Result,
    saturated_rule_composition::{
        Comparison, Row, SaturatedRuleComposition as State, Vertex, compare,
    },
};
use egglog::{EGraph, Value};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
fn saturate(eg: &mut EGraph) -> Result<usize> {
    let mut sweeps = 0;
    loop {
        sweeps += 1;
        if !eg.step_rules("")?.updated {
            return Ok(sweeps);
        }
    }
}
fn value(eg: &mut EGraph, term: &str) -> Result<Value> {
    let e = egglog::ast::Parser::default().get_expr_from_string(None, term)?;
    Ok(eg.eval_expr(&e)?.1)
}
fn state(eg: &mut EGraph, names: &[String], scope: &str) -> Result<State> {
    let sort = eg.get_arcsort_by(|s| s.name() == "Math");
    let canonical = |v: Value| eg.value_to_class_id(&sort, v).to_string();
    let mut rows = vec![];
    let mut ids = BTreeSet::new();
    for op in ["Add", "Mul"] {
        eg.function_for_each(op, |r| {
            let vs: Vec<_> = r.vals.iter().map(|v| canonical(*v)).collect();
            ids.extend(vs.clone());
            rows.push((op.to_string(), vs));
        })?;
    }
    let mut ports = BTreeMap::new();
    for name in names {
        let v = value(eg, name)?;
        let id = eg.value_to_class_id(&sort, v).to_string();
        ids.insert(id.clone());
        ports.insert(name.clone(), id);
    }
    let ids: BTreeMap<_, _> = ids.into_iter().enumerate().map(|(i, v)| (v, i)).collect();
    Ok(State {
        version: 2,
        local_ids: vec![],
        scope: json!({"demo":scope,"semantics":"opaque parameters; fixed parameter and result ports; Input scaffold excluded"}),
        values: vec![
            Vertex {
                sort: "Math".into(),
                literal: None
            };
            ids.len()
        ],
        rows: rows
            .into_iter()
            .map(|(op, v)| Row {
                op,
                args: v[..2].iter().map(|v| ids[v]).collect(),
                result: ids[&v[2]],
            })
            .collect(),
        ports: ports.into_iter().map(|(n, v)| (n, ids[&v])).collect(),
        subsumed_rows: vec![],
    })
}
fn main() -> Result {
    let source = std::fs::read_to_string("experiments/cc_merge_probe/demo.egg")?;
    let out = std::env::args().nth(1).expect("output directory");
    let out = Path::new(&out);
    std::fs::create_dir(out)?;
    let start = std::time::Instant::now();
    let mut base = EGraph::default();
    base.parse_and_run_program(None, &source)?;
    let mut prototypes = vec![];
    for op in ["Add", "Mul"] {
        let mut eg = base.clone();
        eg.parse_and_run_program(
            None,
            &format!("(let p0 (Input 0)) (let p1 (Input 1)) (let base_result ({op} p0 p1))"),
        )?;
        saturate(&mut eg)?;
        assert_eq!(
            eg.get_size(op),
            2,
            "demo prototype must contain exactly its two commuting rows"
        );
        assert_eq!(eg.get_size(if op == "Add" { "Mul" } else { "Add" }), 0);
        prototypes.push((op, eg));
    }
    let mut outcomes = vec![];
    let mut families: Vec<State> = vec![];
    for (left, provider) in &prototypes {
        for (right, _) in &prototypes {
            // Three provider classes: p0, p1 and base_result. Fresh names are allocated
            // in first-use order; repeating a fresh name covers fresh-variable aliasing.
            for a in 0..4 {
                for b in 0..if a == 3 { 5 } else { 4 } {
                    let mut eg = provider.clone();
                    let mut names = vec!["p0".into(), "p1".into(), "base_result".into()];
                    let terms = ["p0", "p1", "base_result", "p2", "p3"];
                    let fresh = if a == 3 && b == 4 {
                        2
                    } else if a >= 3 || b >= 3 {
                        1
                    } else {
                        0
                    };
                    for i in 0..fresh {
                        let n = i + 2;
                        eg.parse_and_run_program(None, &format!("(let p{n} (Input {n}))"))?;
                        names.push(format!("p{n}"));
                    }
                    // Import the entire saturated right prototype: both commuting rows and
                    // their union, with its two parameters instantiated by this binding.
                    eg.parse_and_run_program(
                        None,
                        &format!(
                            "(let result ({right} {} {})) (union result ({right} {} {}))",
                            terms[a], terms[b], terms[b], terms[a]
                        ),
                    )?;
                    names.push("result".into());
                    let before = state(&mut eg, &names, &source)?;
                    let sweeps = saturate(&mut eg)?;
                    let after = state(&mut eg, &names, &source)?;
                    // Validate saturated-snapshot composition against starting
                    // from the original two unsaturated coarse facts.
                    let mut cold = base.clone();
                    cold.parse_and_run_program(
                        None,
                        &format!(
                            "(let p0 (Input 0)) (let p1 (Input 1)) (let base_result ({left} p0 p1))"
                        ),
                    )?;
                    for n in 2..2 + fresh {
                        cold.parse_and_run_program(None, &format!("(let p{n} (Input {n}))"))?;
                    }
                    cold.parse_and_run_program(
                        None,
                        &format!("(let result ({right} {} {}))", terms[a], terms[b]),
                    )?;
                    saturate(&mut cold)?;
                    if !matches!(
                        compare(&after, &state(&mut cold, &names, &source)?)?,
                        Comparison::Equivalent { .. }
                    ) {
                        return Err(
                            "saturated merge differs from original coarse-fact saturation".into(),
                        );
                    }
                    let id = if let Some(i) = families.iter().position(|s| {
                        matches!(compare(&after, s), Ok(Comparison::Equivalent { .. }))
                    }) {
                        i
                    } else {
                        let i = families.len();
                        families.push(after.clone());
                        i
                    };
                    let changed =
                        !matches!(compare(&before, &after)?, Comparison::Equivalent { .. });
                    outcomes.push(json!({"left":left,"right":right,"binding":[terms[a],terms[b]],"shared_provider_slots":usize::from(a<3)+usize::from(b<3),"fresh_parameters":fresh,"before_facts":before.rows.len(),"after_facts":after.rows.len(),"classes":after.values.len(),"additional_saturation_changed_state":changed,"sweeps":sweeps,"family":id}));
                }
            }
        }
    }
    for (i, s) in families.iter().enumerate() {
        std::fs::write(
            out.join(format!("family-{i}.json")),
            serde_json::to_vec_pretty(s)?,
        )?;
    }
    let report = json!({"scope":"native finite arithmetic demo; all ordered pairs of two saturated prototypes, one generation; all functions from two right parameters to three provider classes or fresh parameters, including aliases; no consumer witness required; not a production tier0 speedup test","elapsed_seconds":start.elapsed().as_secs_f64(),"ordered_pairs":4,"bindings_per_pair":17,"merges":outcomes.len(),"cold_equivalence_checks":outcomes.len(),"families":families.len(),"state_changing_merges":outcomes.iter().filter(|r|r["additional_saturation_changed_state"]==true).count(),"rows":outcomes});
    std::fs::write(
        out.join("summary.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
