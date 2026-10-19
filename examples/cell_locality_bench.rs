//! Actual GPU/native execution; frozen analyzed Single/Get Cell, held-out IDs.
//! WORKER ANALYSIS OUTPUT_DIR [instances=8192] [consumers=16] [time|profile|check] [boundary_fanout=0]
#[allow(dead_code)]
#[path = "support/live_table_bench.rs"]
mod bench;
use cell_assisted_egglog::table_live::{Options, PortLocality, QueryRouting, Runtime};
use egglog::EGraph;
use serde_json::{Value, json};
use std::{path::Path, time::Instant};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn diff(a: &Value, b: &Value) -> Value {
    let mut out = serde_json::Map::new();
    for (k, v) in b.as_object().unwrap() {
        if let (Some(x), Some(y)) = (a.get(k).and_then(Value::as_f64), v.as_f64()) {
            out.insert(k.clone(), json!(y - x));
        }
    }
    Value::Object(out)
}
fn main() -> Result<()> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    if a.len() < 3 {
        return Err("WORKER ANALYSIS OUTPUT [instances] [consumers] [time|profile|check] [boundary_fanout=0]".into());
    }
    let n: usize = a.get(3).map(|x| x.parse()).transpose()?.unwrap_or(8192);
    let consumers: usize = a.get(4).map(|x| x.parse()).transpose()?.unwrap_or(16);
    let mode = a.get(5).map(String::as_str).unwrap_or("time");
    let profile = mode == "profile";
    let audit = mode == "check";
    if n == 0 || consumers == 0 || !["time", "profile", "check"].contains(&mode) {
        return Err("invalid benchmark shape/mode".into());
    }
    let mixed = a.get(7).is_some_and(|x| x == "mixed");
    let fanout: usize = a.get(6).map(|x| x.parse()).transpose()?.unwrap_or(0);
    let out = Path::new(&a[2]);
    std::fs::create_dir(out)?;
    let mut source = String::from(
        "(datatype Expr (Leaf i64) (Single Expr) (Get Expr i64))\n(relation Boundary0 (Expr i64)) (relation Boundary1 (Expr i64))\n(ruleset make) (ruleset consume)\n(rule ((Single value)) ((union (Get (Single value) 0) value)) :ruleset make :name \"complete\")\n",
    );
    if fanout > 0 {
        source += "(relation Offer (Expr i64)) (relation Allowed (i64))\n";
    }
    for r in 0..consumers {
        source += &format!(
            "(relation Seen{r} (Expr i64))\n(rule ((Boundary{} object{r} number{r}) (= object{r} (Single value{r})) (= value{r} (Get object{r} 0)) (>= number{r} {})) ((Seen{r} value{r} number{r})) :ruleset consume :name \"consume-{r}\")\n",
            r % 2,
            63 - r % 4
        );
    }
    if fanout > 0 {
        // Extend each consumer with the SAME relational tail, preserving its
        // own original numeric guard and RHS. Most offered keys are rejected.
        for r in 0..consumers {
            source = source.replace(
                &format!("(>= number{r}"),
                &format!("(Offer object{r} candidate{r}) (Allowed candidate{r}) (>= number{r}"),
            );
        }
    }
    for i in 0..n {
        source += &format!(
            "(Boundary0 (Single (Leaf {i})) {}) (Boundary1 (Single (Leaf {i})) {})\n",
            i % 64,
            i % 64
        );
    }
    if fanout > 0 {
        for i in 0..n {
            for j in 0..fanout {
                source += &format!("(Offer (Single (Leaf {i})) {})\n", i * fanout + j);
            }
            source += &format!("(Allowed {})\n", i * fanout + fanout - 1);
        }
    }
    std::fs::write(out.join("source.egg"), &source)?;
    let mut base = EGraph::default();
    let mut definitions = vec![];
    let mut data = vec![];
    for c in base.parse_program(None, &source)? {
        if matches!(c, egglog::ast::Command::Action(..)) {
            data.push(c);
        } else {
            definitions.push(c);
        }
    }
    base.run_program(definitions)?;
    base.run_ground_import(&data)?;
    eprintln!(
        "loaded {} instances, {} consumers, fanout {}",
        n, consumers, fanout
    );
    let warm = base.parse_program(None, "(run consume 1)")?;
    let ready = base.parse_program(None, "(run-schedule (seq make consume))")?;
    let consume = base.parse_program(None, "(run consume 1)")?;
    let mut all = warm.clone();
    all.extend(ready.clone());
    let mut extra = (0..n)
        .map(|i| format!("(Boundary0 (Single (Leaf {i})) 64) (Boundary1 (Single (Leaf {i})) 64)\n"))
        .collect::<String>();
    if mixed {
        for i in 0..n {
            if i % 8 == 0 {
                extra += &format!("(subsume (Get (Single (Leaf {i})) 0))\n");
            }
            if i % 8 == 1 {
                extra += &format!("(union (Single (Leaf {i})) (Single (Leaf {})))\n", n + i);
            }
            if i % 8 == 7 && fanout > 1 {
                extra += &format!("(Allowed {})\n", i * fanout);
            }
        }
    }
    std::fs::write(out.join("external.egg"), &extra)?;
    let mut cpu = Runtime::new_cpu(base.clone(), &all)?;
    cpu.run(&warm)?;
    let before = cpu.report();
    cpu.run(&ready)?;
    let after = cpu.report();
    let cpu_ready = cpu.engine().clone();
    cpu.input(&extra)?;
    let before_external = cpu.report();
    cpu.run(&consume)?;
    let after_external = cpu.report();
    // Also compare with the ordinary original schedule, not only the assisted
    // CPU's cached query and sorted-commit policy.
    let mut pristine = base.clone();
    pristine.run_program(warm.clone())?;
    let t = Instant::now();
    pristine.run_program(ready.clone())?;
    let native_ready_seconds = t.elapsed().as_secs_f64();
    let native_ready = bench::equal(&base, &pristine, &cpu_ready)?;
    pristine.parse_and_run_program(None, &extra)?;
    pristine.run_program(consume.clone())?;
    let native_final = bench::equal(&base, &pristine, cpu.engine())?;
    let mut variants = vec![];
    for (name, cells, locality) in [
        ("full_gpu", false, PortLocality::Independent),
        ("cell_order_only", true, PortLocality::Independent),
        ("cell_independent", true, PortLocality::Independent),
        ("cell_shared_context", true, PortLocality::SharedContext),
        ("cell_shared_joins", true, PortLocality::SharedJoins),
        ("cell_compact_seeds", true, PortLocality::SharedJoins),
        ("cell_priority_seeds", true, PortLocality::SharedJoins),
    ] {
        let mut run = Runtime::configured(
            base.clone(),
            &all,
            Path::new(&a[0]),
            audit,
            Options {
                analysis: cells.then(|| Path::new(&a[1])),
                profile,
                plan_only: name == "cell_order_only",
                query_routing: match name {
                    "cell_compact_seeds" => QueryRouting::Compact,
                    "cell_priority_seeds" => QueryRouting::CellPriority,
                    _ => QueryRouting::Direct,
                },
                port_locality: locality,
                ..Options::default()
            },
        )?;
        run.run(&warm)?;
        let a0 = run.report();
        run.run(&ready)?;
        let b0 = run.report();
        let checked_ready = bench::equal(&base, &cpu_ready, run.engine())?;
        let input_clock = Instant::now();
        run.input(&extra)?;
        let input_seconds = input_clock.elapsed().as_secs_f64();
        let a1 = run.report();
        run.run(&consume)?;
        let b1 = run.report();
        let checked_final = bench::equal(&base, cpu.engine(), run.engine())?;
        variants.push(json!({"name":name,"ready":{"runtime":diff(&a0["runtime"],&b0["runtime"]),"work":diff(&a0["cell_work"],&b0["cell_work"]),"verification":checked_ready},"external":{"input_seconds":input_seconds,"runtime":diff(&a1["runtime"],&b1["runtime"]),"work":diff(&a1["cell_work"],&b1["cell_work"]),"verification":checked_final},"report":b1}));
        eprintln!(
            "{name}: ready {} external {}",
            variants.last().unwrap()["ready"]["runtime"]["run_seconds"],
            variants.last().unwrap()["external"]["runtime"]["run_seconds"]
        );
    }
    let results = json!({"schema":"cell-locality/v1","instances":n,"consumers":consumers,"boundary_fanout":fanout,"mixed_boundary":mixed,"mode":mode,"analysis":a[1],"cpu":{"ready":diff(&before["runtime"],&after["runtime"]),"external":diff(&before_external["runtime"],&after_external["runtime"]),"native_ready_seconds":native_ready_seconds,"native_ready_verification":native_ready,"native_final_verification":native_final},"variants":variants,
    "scope":"Native committed events, GPU matching and native RHS. Frozen analyzed Single/Get interface, synthetic sparse multi-consumer workload using that real structural fragment; not whole eggcc and not arbitrary whole-Cell GPU transitions. Independent Cell mode already shares certificates and indexes. Timings include event ingest, transfer, device query, output decode and native RHS; setup and shader compilation are separate in each report."});
    std::fs::write(
        out.join("results.json"),
        serde_json::to_string_pretty(&results)?,
    )?;
    Ok(())
}
