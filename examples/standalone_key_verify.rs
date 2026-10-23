//! Compare the reused Rust policy with the self-contained emitted source.
//! Original tables and key orders are exported for the existing bench checker.
use cell_assisted_egglog::{Result, program_io, stable_choice};
use egglog::{
    EGraph,
    ast::{Command, Expr, Literal},
};
use serde_json::json;
use std::{path::Path, time::Instant};
fn main() -> Result<()> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    if a.len() != 3 {
        return Err("ORIGINAL PORTABLE OUTPUT".into());
    }
    let out = Path::new(&a[2]);
    std::fs::create_dir(out)?;
    EGraph::set_num_threads(1);
    let source = std::fs::read_to_string(&a[0])?;
    let mut parser = EGraph::default();
    let cs = egg_layout::visual_rule::surface_program(
        parser.parse_program(Some(a[0].clone()), &source)?,
    );
    let mut count = 0;
    let cs = cs
        .into_iter()
        .map(|c| {
            let c = egg_layout::visual_rule::normalize(c, count);
            if matches!(c, Command::Rule { .. }) {
                count += 1
            }
            c
        })
        .collect::<Vec<_>>();
    let split = stable_choice::initialization_end(&cs)?;
    let mut eg = EGraph::default();
    stable_choice::setup_full(&mut eg, &cs[..split])?;
    // Apply the same reused source rewrites to post-setup rules (normally none).
    let (tail, _) = stable_choice::rewrite_ties(&cs[split..])?;
    let commands = stable_choice::schedule_full(&mut eg, &tail)?;
    let t = Instant::now();
    eg.run_program(commands)?;
    let reference_seconds = t.elapsed().as_secs_f64();
    std::fs::write(
        out.join("reference.json"),
        serde_json::to_vec(&program_io::snapshot(&eg)?)?,
    )?;
    let mut keys = vec![];
    for name in eg
        .get_function_names()
        .into_iter()
        .filter(|n| n.starts_with("CellStructuralKey-"))
    {
        let f = eg.get_function(&name).unwrap();
        eg.function_for_each(&name,|r|{
            let literal=stable_choice::key_literal(&eg,r.vals[1]);
            let e=egglog::ast::Parser::default().get_expr_from_string(None,&literal).unwrap();
            let Expr::Call(_,_,args)=e else{unreachable!()};
            let Expr::Lit(_,Literal::String(s))=&args[0] else{unreachable!()};
            keys.push(json!({"function":name,"sort":f.schema().input[0].name(),"value":format!("{:?}",eg.get_canonical_value(r.vals[0],&f.schema().input[0])),"key":serde_json::from_str::<serde_json::Value>(s).unwrap()}));
        })?;
    }
    std::fs::write(out.join("reference-keys.json"), serde_json::to_vec(&keys)?)?;
    let mut portable = EGraph::default();
    let t = Instant::now();
    portable.parse_and_run_program(Some(a[1].clone()), &std::fs::read_to_string(&a[1])?)?;
    let portable_seconds = t.elapsed().as_secs_f64();
    std::fs::write(
        out.join("portable.json"),
        serde_json::to_vec(&program_io::snapshot(&portable)?)?,
    )?;
    std::fs::write(
        out.join("result.json"),
        serde_json::to_vec_pretty(
            &json!({"reference_completed":true,"portable_completed":true,"reference_run_seconds":reference_seconds,"portable_parse_setup_run_seconds":portable_seconds,"same_timing_scope":false,"reference_key_rows":keys.len()}),
        )?,
    )?;
    Ok(())
}
