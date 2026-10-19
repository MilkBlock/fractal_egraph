//! WORKER NEW_OUTPUT_DIR [instances_per_family=128] [profile|check|time]
//! Change only the frozen Bake library, never the test facts, rules or schedule.
#[allow(dead_code)]
#[path = "support/live_table_bench.rs"]
mod bench;
use cell_assisted_egglog::table_live::{Options, PortLocality, QueryRouting, Runtime};
use egg_layout::whole_cell::{Library, Plan};
use egglog::EGraph;
use serde_json::{Value, json};
use std::{path::Path, time::Instant};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const HELPERS: &str = include_str!("../experiments/bake_quality/helpers.egg");
const TRAINING: &str = include_str!("../experiments/bake_quality/training.json");
fn diff(a: &Value, b: &Value) -> Value {
    let mut out = serde_json::Map::new();
    for (k, v) in b.as_object().unwrap() {
        if let (Some(x), Some(y)) = (a.get(k).and_then(Value::as_f64), v.as_f64()) {
            out.insert(k.clone(), json!(y - x));
        }
    }
    Value::Object(out)
}
fn root(kind: usize, x: usize, y: usize) -> String {
    match kind {
        0 => format!("(Single (Leaf {x}))"),
        1 => format!("(Pair (Leaf {x}) (Leaf {y}))"),
        2 => format!("(Single (Pair (Leaf {x}) (Leaf {y})))"),
        _ => format!("(Pair (Single (Leaf {x})) (Leaf {y}))"),
    }
}
fn main() -> Result<()> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    if !(2..=4).contains(&a.len()) {
        return Err("WORKER NEW_OUTPUT [instances_per_family] [profile|check|time]".into());
    }
    let n: usize = a.get(2).map(|x| x.parse()).transpose()?.unwrap_or(128);
    let mode = a.get(3).map(String::as_str).unwrap_or("profile");
    if n == 0 || !["profile", "check", "time"].contains(&mode) {
        return Err("invalid workload/mode".into());
    }
    let out = Path::new(&a[1]);
    std::fs::create_dir(out)?;
    let training: Plan = serde_json::from_str(TRAINING)?;
    let mut libraries = vec![];
    for (name, stages) in [
        ("single", vec![training.stages[0].clone()]),
        (
            "repeated_single",
            (0..8)
                .map(|i| {
                    let mut s = training.stages[0].clone();
                    s.name = format!("single-{i}");
                    s
                })
                .collect(),
        ),
        ("base_cells", training.stages[..2].to_vec()),
        ("composed_cells", training.stages.clone()),
    ] {
        let plan = Plan {
            ruleset: "make".into(),
            stages,
        };
        let start = Instant::now();
        let lib = Library::bake(HELPERS, &plan)?;
        let seconds = start.elapsed().as_secs_f64();
        let path = out.join(name);
        lib.export_projection_catalog(&path)?;
        libraries.push((name,path,json!({"seconds":seconds,"training_contexts":plan.stages.len(),"distinct_states":lib.describe()["states"].as_array().unwrap().len(),"body_rows":lib.describe()["states"].as_array().unwrap().iter().enumerate().map(|(i,_)|lib.body(i).rows.len()).sum::<usize>()})));
    }
    // This fixed consumer/test program is built only AFTER all libraries freeze.
    // Each family has its own Boundary table so root-shape overlap cannot silently
    // change the intended workload mix. None of these relations is in training.
    let mut source = String::from(HELPERS);
    source += "(relation Offer (Expr i64)) (relation Allowed (i64)) (ruleset consume)\n";
    let bodies = [
        "(= object (Single x)) (= x (Get object 0))",
        "(= object (Pair x y)) (= y (Get object 1))",
        "(= inner (Get object 0)) (= object (Single inner)) (= x (Get inner 0)) (= y (Get inner 1)) (= inner (Pair x y))",
        "(= inner (Get object 0)) (= y (Get object 1)) (= object (Pair inner y)) (= x (Get inner 0)) (= inner (Single x))",
    ];
    for (k, body) in bodies.iter().enumerate() {
        source += &format!("(relation Boundary{k} (Expr i64))\n");
        for c in 0..2 {
            source += &format!(
                "(relation Seen{k}_{c} (Expr i64))\n(rule ((Boundary{k} object n) {body} (Offer object choice) (Allowed choice) (>= n {})) ((Seen{k}_{c} object n)) :ruleset consume :name \"family-{k}-consumer-{c}\")\n",
                14 + c
            );
        }
    }
    let mut data = String::new();
    let (mut intact, mut disrupted) = (String::new(), String::new());
    for k in 0..4 {
        for i in 0..n {
            let x = 10000 + 2 * (k * n + i);
            let term = root(k, x, x + 1);
            let key = 3 * (k * n + i);
            data += &format!(
                "(Boundary{k} {term} {}) (Offer {term} {key}) (Offer {term} {}) (Allowed {key})\n",
                i % 16,
                key + 1
            );
            intact += &format!("(Boundary{k} {term} 64)\n");
            disrupted += &format!("(Boundary{k} {term} 65)\n");
            if i % 8 == 0 {
                let index = if k == 1 { 1 } else { 0 };
                disrupted += &format!("(subsume (Get {term} {index}))\n");
            }
            if i % 8 == 1 {
                disrupted += &format!("(union {term} {})\n", root(k, x + 1000000, x + 1000001));
            }
        }
    }
    std::fs::write(out.join("environment.egg"), &source)?;
    std::fs::write(out.join("heldout.egg"), &data)?;
    std::fs::write(out.join("intact.egg"), &intact)?;
    std::fs::write(out.join("disrupted.egg"), &disrupted)?;
    let mut base = EGraph::default();
    base.parse_and_run_program(None, &source)?;
    let input = base.parse_program(None, &data)?;
    base.run_ground_import(&input)?;
    base.parse_and_run_program(None, "(run-schedule (saturate make))")?;
    let consume = base.parse_program(None, "(run consume 1)")?;
    let mut native = base.clone();
    native.run_program(consume.clone())?;
    native.parse_and_run_program(None, &intact)?;
    native.run_program(consume.clone())?;
    let ref_intact = native.clone();
    native.parse_and_run_program(None, &disrupted)?;
    native.run_program(consume.clone())?;
    let mut variants = vec![];
    let none = out.join("unused");
    let mut cases = vec![(
        "none",
        none,
        json!({"seconds":0.0,"training_contexts":0,"distinct_states":0,"body_rows":0}),
    )];
    cases.extend(libraries);
    for (name, path, bake) in cases {
        let mut run = Runtime::configured(
            base.clone(),
            &consume,
            Path::new(&a[0]),
            mode == "check",
            Options {
                analysis: (name != "none").then_some(path.as_path()),
                profile: mode == "profile",
                port_locality: PortLocality::SharedJoins,
                query_routing: QueryRouting::CellPriority,
                ..Options::default()
            },
        )?;
        run.run(&consume)?;
        run.input(&intact)?;
        let before = run.report();
        run.run(&consume)?;
        let first = run.report();
        let eq1 = bench::equal(&base, &ref_intact, run.engine())?;
        run.input(&disrupted)?;
        let before2 = run.report();
        run.run(&consume)?;
        let second = run.report();
        let eq2 = bench::equal(&base, &native, run.engine())?;
        variants.push(json!({"library":name,"bake":bake,"intact":{"runtime":diff(&before["runtime"],&first["runtime"]),"work":diff(&before["cell_work"],&first["cell_work"]),"verification":eq1},"disrupted":{"runtime":diff(&before2["runtime"],&second["runtime"]),"work":diff(&before2["cell_work"],&second["cell_work"]),"verification":eq2},"report":second}));
        eprintln!(
            "{name}: {} states, intact ready {} / {}, disrupted {} / {}",
            bake["distinct_states"],
            variants.last().unwrap()["intact"]["work"]["routed_ready"],
            variants.last().unwrap()["intact"]["work"]["routing_valid_seeds"],
            variants.last().unwrap()["disrupted"]["work"]["routed_ready"],
            variants.last().unwrap()["disrupted"]["work"]["routing_valid_seeds"]
        );
    }
    // Duplicate training must not manufacture distinct bodies or coverage.
    assert_eq!(
        variants[1]["bake"]["distinct_states"],
        variants[2]["bake"]["distinct_states"]
    );
    for phase in ["intact", "disrupted"] {
        for field in [
            "routed_ready",
            "routed_suspected",
            "routed_unmapped",
            "ready_covered_atom_sum",
            "ready_reuse_seeds",
        ] {
            assert_eq!(
                variants[1][phase]["work"][field], variants[2][phase]["work"][field],
                "duplicate training changed coverage"
            );
        }
    }
    let result = json!({"schema":"bake-quality-ablation/v1","mode":mode,"instances_per_family":n,"families":["Single","Pair","Single(Pair)","Pair(Single,leaf)"],"consumer_atoms":[5,5,8,8],"variants":variants,"scope":"Same held-out facts, consumers, execution and boundary changes; only frozen native-Bake catalog varies. All positive bodies come from actual helper saturation and independent delta replay. Not a whole eggcc benchmark; consumer guards/effects and boundary relations were never baked."});
    std::fs::write(
        out.join("results.json"),
        serde_json::to_string_pretty(&result)?,
    )?;
    Ok(())
}
