use egglog::EGraph;
// Phased mode uses the same native engine as the existing baseline command.
// Expensive graph inspection is only enabled in separate verification processes.
#[derive(serde::Deserialize)]
struct Phases {
    environment: String,
    input: String,
    run: String,
    #[serde(default)]
    export: String,
    /// Inspect row/class counts without writing or comparing a complete graph.
    #[serde(default)]
    counts: bool,
    constructors: Vec<String>,
}
fn execute_phase(
    eg: &mut EGraph,
    source: &str,
    diagnose: bool,
) -> (f64, usize, std::collections::BTreeMap<String, usize>) {
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
    let mut per_rule = std::collections::BTreeMap::new();
    if diagnose {
        for output in &outputs {
            if let egglog::CommandOutput::RunSchedule(report) = output {
                for (rule, count) in &report.num_matches_per_rule {
                    *per_rule.entry(rule.to_string()).or_default() += count;
                }
            }
        }
    }
    drop(outputs);
    (tick.elapsed().as_secs_f64(), matches, per_rule)
}
fn shape(eg: &EGraph, names: &[String]) -> serde_json::Value {
    let mut classes = std::collections::BTreeSet::new();
    let mut tables = std::collections::BTreeMap::new();
    let mut by_sort = std::collections::BTreeMap::<
        String,
        (usize, std::collections::BTreeSet<egglog::Value>),
    >::new();
    for name in names {
        if let Some(f) = eg.get_function(name) {
            let mut count = 0;
            eg.function_for_each(name, |row| {
                count += 1;
                let group = by_sort
                    .entry(f.schema().output.name().to_owned())
                    .or_default();
                group.0 += 1;
                group
                    .1
                    .insert(eg.get_canonical_value(*row.vals.last().unwrap(), &f.schema().output));
                classes.insert((
                    f.schema().output.name().to_owned(),
                    eg.get_canonical_value(*row.vals.last().unwrap(), &f.schema().output),
                ));
            })
            .unwrap();
            tables.insert(name, count);
        }
    }
    serde_json::json!({"enodes":tables.values().sum::<usize>(),"eclasses":classes.len(),"tables":tables,"sorts":by_sort.into_iter().map(|(s,(n,c))| (s,serde_json::json!({"enodes":n,"eclasses":c.len()}))).collect::<std::collections::BTreeMap<_,_>>()})
}
pub fn run(path: &str, graph: Option<&str>) {
    run_with_probes(path, graph, Vec::new);
}

// The shared harness still compiles against pristine egglog. Native diagnostic
// callers may supply existing counters without adding instrumentation here.
pub fn run_with_probes(
    path: &str,
    graph: Option<&str>,
    probes: impl Fn() -> Vec<(&'static str, usize)>,
) {
    run_diagnostic(path, graph, probes, false, |_| {});
}

pub fn run_diagnostic(
    path: &str,
    graph: Option<&str>,
    probes: impl Fn() -> Vec<(&'static str, usize)>,
    plans: bool,
    configure: impl FnOnce(&mut EGraph),
) {
    use serde_json::json;
    use std::io::Write;
    let tick = std::time::Instant::now();
    let config: Phases = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let inspect = config.counts || graph.is_some();
    let environment = std::fs::read_to_string(&config.environment).unwrap();
    let input = std::fs::read_to_string(&config.input).unwrap();
    let read_seconds = tick.elapsed().as_secs_f64();
    let tick = std::time::Instant::now();
    let mut eg = EGraph::default();
    configure(&mut eg);
    execute_phase(&mut eg, &environment, false);
    let setup_seconds = tick.elapsed().as_secs_f64();
    let (input_seconds, _, _) = execute_phase(&mut eg, &input, false);
    let initial = inspect.then(|| shape(&eg, &config.constructors));
    let before_probes = inspect.then(|| probes());
    let (run_seconds, matches, rule_matches) = execute_phase(&mut eg, &config.run, inspect);
    let join_probes = before_probes.map(|before| {
        let before: std::collections::BTreeMap<_, _> = before.into_iter().collect();
        probes()
            .into_iter()
            .map(|(name, count)| (name, count - before.get(name).copied().unwrap_or(0)))
            .collect::<std::collections::BTreeMap<_, _>>()
    });
    let mut verification = serde_json::Value::Null;
    if inspect {
        let names = eg.get_function_names();
        let (mut active, mut hidden) = (0, 0);
        let mut summary_tables = std::collections::BTreeMap::new();
        for name in names.iter().filter(|n| n.starts_with("Spine-Summary-")) {
            let (mut live, mut retired) = (0, 0);
            eg.function_for_each(name, |r| if r.subsumed { retired += 1 } else { live += 1 })
                .unwrap();
            active += live;
            hidden += retired;
            summary_tables.insert(name, json!({"active":live,"subsumed":retired}));
        }
        let raw: Vec<_> = config
            .constructors
            .iter()
            .map(|n| format!("Spine-{n}"))
            .collect();
        let canonical: Vec<_> = config
            .constructors
            .iter()
            .map(|n| format!("Spine-Cano-{n}"))
            .collect();
        let all: Vec<_> = raw.iter().chain(&canonical).cloned().collect();
        let mut table_rows = std::collections::BTreeMap::new();
        for name in &names {
            let mut n = 0;
            eg.function_for_each(name, |_| n += 1).unwrap();
            table_rows.insert(name, n);
        }
        verification = json!({"initial_original":initial,"original_before_export":shape(&eg, &config.constructors),
            "spine":shape(&eg, &all),"raw_spine":shape(&eg, &raw),"canonical_spine":shape(&eg, &canonical),
            "active_summaries":active,"subsumed_summaries":hidden,"summary_tables":summary_tables,
            "native_total_rows_before_export":table_rows.values().sum::<usize>(),"native_tables_before_export":table_rows,
            "rule_matches":rule_matches,"run_probe_counters":join_probes});
        if plans {
            verification["run_report"] = serde_json::to_value(eg.get_overall_run_report()).unwrap();
        }
    }
    let (export_seconds, export_matches) = if config.export.is_empty() {
        (0., 0)
    } else {
        let (seconds, matches, _) = execute_phase(&mut eg, &config.export, false);
        (seconds, matches)
    };
    if inspect {
        verification["restored"] = shape(&eg, &config.constructors);
        verification["check_mode"] = json!(if graph.is_some() {
            "graph_dump"
        } else {
            "counts_only"
        });
    }
    if let Some(path) = graph {
        let mut writer = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
        write!(writer, "[").unwrap();
        let eq_sorts = config
            .constructors
            .iter()
            .filter_map(|n| eg.get_function(n))
            .map(|f| f.schema().output.name().to_owned())
            .collect::<std::collections::BTreeSet<_>>();
        let mut comma = false;
        for name in &config.constructors {
            let Some(f) = eg.get_function(name) else {
                continue;
            };
            eg.function_for_each(name, |row| {
                assert!(
                    !row.subsumed,
                    "unexpected hidden original fact in positive constructor benchmark"
                );
                let args = row
                    .vals
                    .iter()
                    .zip(&f.schema().input)
                    .map(|(v, t)| {
                        if eq_sorts.contains(t.name()) {
                            json!([
                                t.name(),
                                format!("{}:{:?}", t.name(), eg.get_canonical_value(*v, t))
                            ])
                        } else {
                            match t.name() {
                                "String" => json!([
                                    "String",
                                    eg.value_to_base::<egglog::sort::S>(*v).to_string()
                                ]),
                                "i64" => json!(["i64", eg.value_to_base::<i64>(*v).to_string()]),
                                "bool" => json!(["bool", eg.value_to_base::<bool>(*v).to_string()]),
                                "f64" => json!([
                                    "f64",
                                    eg.value_to_base::<egglog::sort::F>(*v).to_string()
                                ]),
                                _ => panic!("unsupported primitive benchmark field: {}", t.name()),
                            }
                        }
                    })
                    .collect::<Vec<_>>();
                let result = format!(
                    "{}:{:?}",
                    f.schema().output.name(),
                    eg.get_canonical_value(*row.vals.last().unwrap(), &f.schema().output)
                );
                if comma {
                    write!(writer, ",").unwrap();
                }
                comma = true;
                serde_json::to_writer(
                    &mut writer,
                    &json!({"op":name,"sort":f.schema().output.name(),"args":args,"result":result}),
                )
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
