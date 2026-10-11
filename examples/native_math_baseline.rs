//! Isolate native execution and dependency tracing, without the CS/ripen pipeline.
use egglog::{
    EGraph, TraceSession,
    ast::{Command, GenericSchedule},
};
// Phased mode uses the same native engine as the existing baseline command.
// Expensive graph inspection is only enabled in separate verification processes.
#[derive(serde::Deserialize)]
struct Phases {
    environment: String,
    input: String,
    run: String,
    #[serde(default)]
    export: String,
    constructors: Vec<String>,
}
fn execute_phase(eg: &mut EGraph, source: &str) -> (f64, usize) {
    let tick = std::time::Instant::now();
    let outputs = eg.parse_and_run_program(None, source).unwrap();
    let matches = outputs
        .iter()
        .filter_map(|o| match o {
            egglog::CommandOutput::RunSchedule(r) => {
                Some(r.num_matches_per_rule.values().sum::<usize>())
            }
            _ => None,
        })
        .sum();
    drop(outputs);
    (tick.elapsed().as_secs_f64(), matches)
}
fn shape(eg: &EGraph, names: &[String]) -> serde_json::Value {
    let mut classes = std::collections::BTreeSet::new();
    let mut tables = std::collections::BTreeMap::new();
    for name in names {
        if let Some(f) = eg.get_function(name) {
            let mut count = 0;
            eg.function_for_each(name, |row| {
                count += 1;
                classes.insert((
                    f.schema().output.name().to_owned(),
                    eg.get_canonical_value(*row.vals.last().unwrap(), &f.schema().output),
                ));
            })
            .unwrap();
            tables.insert(name, count);
        }
    }
    serde_json::json!({"enodes":tables.values().sum::<usize>(),"eclasses":classes.len(),"tables":tables})
}
fn phase_bench(path: &str, graph: Option<&str>) {
    use serde_json::json;
    use std::io::Write;
    let tick = std::time::Instant::now();
    let config: Phases = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let environment = std::fs::read_to_string(&config.environment).unwrap();
    let input = std::fs::read_to_string(&config.input).unwrap();
    let read_seconds = tick.elapsed().as_secs_f64();
    let tick = std::time::Instant::now();
    let mut eg = EGraph::default();
    execute_phase(&mut eg, &environment);
    let setup_seconds = tick.elapsed().as_secs_f64();
    let (input_seconds, _) = execute_phase(&mut eg, &input);
    let (run_seconds, matches) = execute_phase(&mut eg, &config.run);
    let mut verification = serde_json::Value::Null;
    if graph.is_some() {
        let names = eg.get_function_names();
        let (mut active, mut hidden) = (0, 0);
        for name in names.iter().filter(|n| n.starts_with("Spine-Summary-")) {
            eg.function_for_each(name, |r| if r.subsumed { hidden += 1 } else { active += 1 })
                .unwrap();
        }
        verification = json!({"original_before_export":shape(&eg, &config.constructors),
            "spine":shape(&eg, &config.constructors.iter().map(|n|format!("Spine-{n}")).collect::<Vec<_>>()),
            "active_summaries":active,"subsumed_summaries":hidden});
    }
    let (export_seconds, export_matches) = if config.export.is_empty() {
        (0., 0)
    } else {
        execute_phase(&mut eg, &config.export)
    };
    if let Some(path) = graph {
        verification["restored"] = shape(&eg, &config.constructors);
        let mut writer = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
        write!(writer, "[").unwrap();
        let mut comma = false;
        for name in &config.constructors {
            let Some(f) = eg.get_function(name) else {
                continue;
            };
            eg.function_for_each(name, |row| {
                assert!(
                    !row.subsumed,
                    "unexpected hidden original fact in positive Math benchmark"
                );
                let args = row
                    .vals
                    .iter()
                    .zip(&f.schema().input)
                    .map(|(v, t)| match t.name() {
                        "Math" => json!(["Math", eg.value_to_class_id(t, *v).to_string()]),
                        "String" => json!([
                            "String",
                            eg.value_to_base::<egglog::sort::S>(*v).to_string()
                        ]),
                        "i64" => json!(["i64", eg.value_to_base::<i64>(*v).to_string()]),
                        _ => panic!("unsupported Math benchmark field"),
                    })
                    .collect::<Vec<_>>();
                let result = eg
                    .value_to_class_id(&f.schema().output, *row.vals.last().unwrap())
                    .to_string();
                if comma {
                    write!(writer, ",").unwrap();
                }
                comma = true;
                serde_json::to_writer(&mut writer, &json!({"op":name,"args":args,"result":result}))
                    .unwrap();
            })
            .unwrap();
        }
        write!(writer, "]").unwrap();
        writer.flush().unwrap();
    }
    let tick = std::time::Instant::now();
    drop(eg);
    let drop_seconds = tick.elapsed().as_secs_f64();
    let seconds = setup_seconds + input_seconds + run_seconds + export_seconds;
    println!(
        "{}",
        json!({"seconds":seconds,"read_seconds":read_seconds,"setup_seconds":setup_seconds,
        "input_seconds":input_seconds,"run_seconds":run_seconds,"export_seconds":export_seconds,
        "drop_seconds":drop_seconds,"through_drop_seconds":seconds+drop_seconds,
        "matches":matches,"export_matches":export_matches,"verification":verification,
        "traced":false,"scope":"phased native execution; graph inspection only in separate verification runs"})
    );
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|s| s == "bench") {
        phase_bench(&args[2], args.get(3).map(String::as_str));
        return;
    }
    let source = &args[1];
    let text = std::fs::read_to_string(source).unwrap();
    let traced = args.get(2).is_some_and(|s| s == "trace");
    let start = std::time::Instant::now();
    let mut eg = EGraph::default();
    let mut outputs = vec![];
    if traced {
        let trace = TraceSession::with_dependencies();
        let mut round = 0;
        for cmd in eg.parse_program(Some(source.clone()), &text).unwrap() {
            let commands = if let Command::RunSchedule(GenericSchedule::Repeat(_, n, inner)) = &cmd
            {
                if matches!(**inner, GenericSchedule::Run(..)) {
                    vec![Command::RunSchedule((**inner).clone()); *n]
                } else {
                    vec![cmd]
                }
            } else {
                vec![cmd]
            };
            for c in commands {
                let scheduled = matches!(c, Command::RunSchedule(_));
                let tick = std::time::Instant::now();
                outputs.extend(eg.run_program_with_trace(vec![c], &trace).unwrap());
                let batch = trace.drain_completed();
                if scheduled {
                    round += 1;
                    eprintln!(
                        "trace_round={} seconds={:.9} matches={} reads={} writes={} unions={} events={}",
                        round,
                        tick.elapsed().as_secs_f64(),
                        batch.matches.len(),
                        batch.reads.len(),
                        batch.writes.len(),
                        batch.unions.len(),
                        batch.len()
                    );
                }
            }
        }
    } else {
        outputs = eg
            .parse_and_run_program(Some(source.clone()), &text)
            .unwrap();
    }
    let seconds = start.elapsed().as_secs_f64();
    if args.get(2).is_some_and(|s| s == "shape") {
        let mut classes = std::collections::BTreeSet::new();
        let mut nodes = 0usize;
        for name in eg.get_function_names() {
            let f = eg.get_function(&name).unwrap();
            if f.is_let_binding() || f.schema().output.name() != "Math" {
                continue;
            }
            let sort = f.schema().output.clone();
            eg.function_for_each(&name, |r| {
                nodes += 1;
                classes.insert(
                    eg.value_to_class_id(&sort, *r.vals.last().unwrap())
                        .to_string(),
                );
            })
            .unwrap();
        }
        eprintln!(
            "shape_enodes={nodes} shape_eclasses={} shape_average={:.9}",
            classes.len(),
            nodes as f64 / classes.len() as f64
        );
    }
    for o in outputs {
        println!("{o}");
    }
    eprintln!("baseline_native_seconds={seconds:.9} traced={traced}");
}
