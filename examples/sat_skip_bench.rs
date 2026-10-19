//! WORKER NEW_OUTPUT INSTANCES [time|check]. One run per lane, same native
//! schedules and fully materialized graph; Bake/setup/audit are separate.
#[allow(dead_code)]
#[path = "support/live_table_bench.rs"]
mod compare;
#[path = "support/saturation_hook.rs"]
mod saturation_hook;
use cell_assisted_egglog::table_live::Runtime;
use egg_layout::whole_cell::{Library, Plan, automatic::AutomaticRuntime};
use egglog::CommandOutput;
use serde_json::json;
use std::{path::Path, time::Instant};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn flags(outputs: Vec<CommandOutput>) -> (bool, bool) {
    let (mut updated, mut stop) = (false, true);
    for o in outputs {
        if let CommandOutput::RunSchedule(r) = o {
            updated |= r.updated;
            stop &= r.can_stop;
        }
    }
    (updated, stop)
}
fn main() -> Result<()> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    if a.len() != 4 {
        return Err("WORKER NEW_OUTPUT INSTANCES time|check".into());
    }
    let n: usize = a[2].parse()?;
    if n == 0 {
        return Err("positive instance count required".into());
    }
    let audit = match a[3].as_str() {
        "check" => true,
        "time" => false,
        _ => return Err("time|check".into()),
    };
    let out = Path::new(&a[1]);
    std::fs::create_dir(out)?;
    let env = format!(
        "{}\n(constructor Published (Math) Math)\n",
        include_str!("../experiments/whole_cell_automatic/math.egg")
    );
    let graft =
        include_str!("../experiments/whole_cell_automatic/grafts.egg").replace("(run graft 3)", "");
    let observed = "(ruleset observe) (rule ((= a (Add x y))) ((Published a)) :ruleset observe :name \"publish-add\")";
    let plan: Plan =
        serde_json::from_str(include_str!("../experiments/whole_cell_chain/plan.json"))?;
    let t = Instant::now();
    let lib = Library::bake(&env, &plan)?;
    let bake_seconds = t.elapsed().as_secs_f64();
    let input = (0..n)
        .map(|i| {
            format!(
                "(Start (Var {}) (Var {}) (Var {}) (Var {}))\n",
                i * 4 + 1,
                i * 4 + 2,
                i * 4 + 3,
                i * 4 + 4
            )
        })
        .collect::<String>();
    let t = Instant::now();
    let mut cell = AutomaticRuntime::new(lib.clone(), &input)?;
    cell.install_grafts("graft", &graft)?;
    cell.install_grafts("observe", observed)?;
    let (mut base, mut observer) = cell.detach();
    observer.set_audit(audit);
    let shared_preparation_seconds = t.elapsed().as_secs_f64();
    let text = "(run graft 1)\n(run-schedule (saturate math))\n(run observe 1)\n".repeat(3);
    let commands = base.parse_program(None, &text)?;
    std::fs::write(
        out.join("original.egg"),
        format!("{env}\n{graft}\n{observed}\n{input}\n{text}"),
    )?;
    std::fs::write(out.join("library.json"), serde_json::to_vec_pretty(&lib)?)?;
    let mut native = base.clone();
    let mut expected = vec![];
    let mut native_phases = vec![];
    for (i, c) in commands.iter().enumerate() {
        let t = Instant::now();
        let (updated, can_stop) = flags(native.run_program(vec![c.clone()])?);
        let seconds = t.elapsed().as_secs_f64();
        native_phases.push(json!({"boundary":i,"kind":(["coarse","saturate","observe"][i%3]),"seconds":seconds,"updated":updated,"can_stop":can_stop}));
        expected.push(native.clone());
    }
    let smooth = base.cell_rule_names("math")?;
    let native_smooth_matches: usize = smooth
        .iter()
        .map(|r| {
            native
                .get_overall_run_report()
                .num_matches_per_rule
                .get(r.as_str())
                .copied()
                .unwrap_or(0)
        })
        .sum();
    let mut lanes = vec![
        json!({"lane":"native","phases":native_phases,"smooth_matches":native_smooth_matches}),
    ];
    let mut observer = Some(observer);
    let checkpoint = base.clone();
    for (skip, engine) in [(false, base.clone()), (true, base)] {
        let t = Instant::now();
        let mut r = Runtime::new(engine, &commands, Path::new(&a[0]), audit)?;
        if skip {
            r.set_saturation_hook(Box::new(saturation_hook::WholeCellHook(
                observer.take().unwrap(),
            )));
        }
        let setup_seconds = t.elapsed().as_secs_f64();
        let mut phases = vec![];
        for (i, c) in commands.iter().enumerate() {
            let t = Instant::now();
            r.run(std::slice::from_ref(c))?;
            let seconds = t.elapsed().as_secs_f64();
            let verification = compare::equal(&checkpoint, &expected[i], r.engine())?;
            phases.push(json!({"boundary":i,"kind":(["coarse","saturate","observe"][i%3]),"seconds":seconds,"verification":verification}));
        }
        let report = r.report();
        let smooth_matches: usize = report["rules"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|x| x["ruleset"] == "math")
            .map(|x| x["stats"]["emitted_bindings"].as_u64().unwrap() as usize)
            .sum();
        if skip {
            assert_eq!(
                smooth_matches, 0,
                "all known closures should bypass smooth matching"
            );
            assert_eq!(report["runtime"]["skipped_saturations"], 3);
            assert_eq!(report["runtime"]["partial_saturations"], 0);
        }
        lanes.push(json!({"lane":if skip{"gpu_sat_skip"}else{"gpu_full"},"setup_seconds":setup_seconds,"phases":phases,"smooth_matches":smooth_matches,"report":report}));
    }
    let result = json!({"mode":a[3],"instances":n,"bake_seconds":bake_seconds,"shared_preparation_seconds":shared_preparation_seconds,"lanes":lanes,
        "scope":"Actual native/GPU runtime; finite positive Math A-B-C Cell family. Ordinary graft/observer runs preserved. Every boundary checks full original rows, values, equivalence and visibility. Not raytrace or support for custom-merge terms. Setup, Bake, input and verification excluded from per-boundary timers. No averaging."});
    std::fs::write(
        out.join("results.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    for l in result["lanes"].as_array().unwrap() {
        let sum: f64 = l["phases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["seconds"].as_f64().unwrap())
            .sum();
        eprintln!(
            "{}: {:.3} ms; smooth matches {}",
            l["lane"],
            sum * 1000.,
            l["smooth_matches"]
        );
    }
    Ok(())
}
