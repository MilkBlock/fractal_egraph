//! Independent native Bake and post-batch commit/verification. Never called
//! between resident GPU transitions; original smooth queries are oracle-only.
use egg_layout::whole_cell::{Library, Plan, Runtime};
use egglog::{
    EGraph,
    ast::{Expr, Parser},
};
use serde_json::{Value as Json, json};
use std::{collections::BTreeMap, path::Path};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn main() -> Result<()> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    let source = std::fs::read_to_string(&a[1])?;
    let plan: Plan = serde_json::from_slice(&std::fs::read(&a[2])?)?;
    let lib = Library::bake(&source, &plan)?;
    if a[0] == "bake" {
        let mut data = lib.describe();
        let n = data["states"].as_array().unwrap().len();
        data["states"] = json!((0..n).map(|i| lib.body(i)).collect::<Vec<_>>());
        data["plan"] = json!(plan);
        std::fs::write(&a[3], serde_json::to_vec_pretty(&data)?)?;
        return Ok(());
    }
    let cases: Json = serde_json::from_slice(&std::fs::read(&a[3])?)?;
    let out = Path::new(&a[4]);
    std::fs::create_dir_all(out)?;
    let names = cases["parameter_names"].as_array().unwrap();
    let mut results = vec![];
    for (ci, c) in cases["cases"].as_array().unwrap().iter().enumerate() {
        let params = c["parameters"].as_array().unwrap();
        let input = names
            .iter()
            .zip(params)
            .map(|(n, v)| {
                format!(
                    "(let {} (Var {}))",
                    n.as_str().unwrap(),
                    v.as_u64().unwrap()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let mut run = Runtime::new(lib.clone(), &input)?;
        let mut native = EGraph::default();
        native.parse_and_run_program(None, &format!("{source}\n{input}"))?;
        let mut expressions = BTreeMap::<String, Expr>::new();
        let mut checks = vec![];
        for event in c["events"].as_array().unwrap() {
            let i = event.as_u64().ok_or("bad transition ID")? as usize;
            let spec = plan.stages.get(i).ok_or("unregistered transition")?;
            if !spec.premises.is_empty() {
                return Err("fixture oracle does not lower extra seed premises".into());
            }
            let ex = Parser::default().get_expr_from_string(None, &spec.graft)?;
            let ex = ex.visit_exprs(&mut |e| {
                if matches!(&e,Expr::Var(_,n) if n=="parent") {
                    spec.parent
                        .as_ref()
                        .and_then(|p| expressions.get(p))
                        .cloned()
                        .unwrap_or(e)
                } else {
                    e
                }
            });
            expressions.insert(spec.name.clone(), ex.clone());
            let bind = spec
                .parameters
                .iter()
                .map(|p| (p.name.clone(), p.name.clone()))
                .collect();
            let receipt = run.advance(&spec.name, &bind)?;
            native.parse_and_run_program(None, &format!("(let native_root_{i} {ex})"))?;
            let mut smooth = BTreeMap::<String, usize>::new();
            loop {
                let report = native.step_rules(&plan.ruleset)?;
                for (n, c) in report.num_matches_per_rule {
                    *smooth.entry(n.to_string()).or_default() += c;
                }
                if !report.updated {
                    break;
                }
            }

            let (_, root) = native.eval_expr(
                &Parser::default().get_expr_from_string(None, &format!("native_root_{i}"))?,
            )?;
            let comparison = run.verify_against(&native, root)?;
            if serde_json::to_value(&comparison)?["status"] != "equivalent" {
                return Err("native complete graph differs".into());
            }
            checks.push(json!({"transition":i,"comparison":comparison,"receipt":receipt,"native_smooth_matches":smooth}));
        }
        if run.smooth_counts().values().any(|n| *n != 0) {
            return Err("smooth query executed in batch commit".into());
        }
        if run.current().is_none() {
            for name in ["Add", "Mul", "Start"] {
                let mut count = 0;
                run.engine().function_for_each(name, |_| count += 1)?;
                if count != 0 {
                    return Err("unentered Cell emitted an original constructor".into());
                }
            }
            let mut count = 0;
            run.engine().function_for_each("Var", |_| count += 1)?;
            let distinct = params
                .iter()
                .map(|v| v.as_u64().unwrap())
                .collect::<std::collections::BTreeSet<_>>()
                .len();
            if count != distinct {
                return Err("opaque input facts differ".into());
            }
        }
        let snapshot = if run.current().is_some() {
            Some(run.full_snapshot()?)
        } else {
            None
        };
        let state = run.current().map(|i| i.state + 1).unwrap_or(0);
        if c["state"].as_u64() != Some(state as u64) {
            return Err("GPU final state differs from native receipts".into());
        }
        std::fs::write(
            out.join(format!("{ci}.json")),
            serde_json::to_vec(&snapshot)?,
        )?;
        results.push(json!({"case":ci,"state":state,"verified_transitions":checks.len(),"comparisons":checks,"original_smooth_queries":run.smooth_counts(),"snapshot":format!("{ci}.json")}));
    }
    std::fs::write(
        out.join("report.json"),
        serde_json::to_vec_pretty(
            &json!({"passed":true,"cases":results,"scope":"GPU transcript committed through the existing certified native action emitter; full original constructor facts/equivalence independently compared to native saturation after each macro. No CPU calls between GPU steps."}),
        )?,
    )?;
    Ok(())
}
