//! Fresh-process off/on measurements on the two frozen programs. Diagnostic
//! binaries use the same driver with zero timed rounds and --audit.
use serde_json::{Value as J, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

fn hash(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    use std::io::Read;
    let mut file = fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = [0; 65536];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}
fn median(mut xs: Vec<f64>) -> f64 {
    xs.sort_by(f64::total_cmp);
    (xs[(xs.len() - 1) / 2] + xs[xs.len() / 2]) / 2.
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if ![5,6,8].contains(&args.len()) {
        return Err("expected BINARY NEW_OUTPUT SUM_PROGRAM RAY_PROGRAM ROUNDS [--audit]".into());
    }
    let binary = fs::canonicalize(&args[0])?;
    let output = PathBuf::from(&args[1]);
    fs::create_dir(&output)?;
    let rounds: usize = args[4].parse()?;
    let receipts=args.get(5).is_some_and(|s|s=="--block-receipts" || s=="--audit-block-receipts");
    let combination=args.get(5).is_some_and(|s|s=="--receipt-combination" || s=="--audit-receipt-combination");
    let learned=args.get(5).is_some_and(|s|s=="--iterative-lhs" || s=="--audit-iterative-lhs");
    let catalog=args.get(5).is_some_and(|s|s=="--catalog-pair" || s=="--audit-catalog-pair");
    if (learned||catalog||combination) && args.len()!=8 {return Err("learned catalog modes require SUM_BASE RAY_BASE after MODE".into())}
    let lhs=args.get(5).is_some_and(|s|s=="--block-lhs" || s=="--audit-block-lhs");
    let multi=args.get(5).is_some_and(|s|s=="--multi-run" || s=="--audit-multi-run");
    let fusion = args
        .get(5)
        .is_some_and(|s| s == "--two-run" || s == "--audit-two-run");
    let audit = args.get(5).is_some_and(|s|s=="--audit-block-receipts" || s=="--audit-receipt-combination" || s=="--audit-iterative-lhs" || s=="--audit-catalog-pair") || args
        .get(5)
        .is_some_and(|s| s == "--audit" || s == "--audit-ports" || s == "--audit-two-run" || s=="--audit-multi-run" || s=="--audit-block-lhs");
    let ports = receipts || combination || learned || catalog || lhs || multi || fusion
        || args
            .get(5)
            .is_some_and(|s| s == "--ports" || s == "--audit-ports");
    if rounds == 0 && !audit {
        return Err("zero rounds only for audit".into());
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let gold: J = serde_json::from_slice(&fs::read(root.join("docs/view-consumer-results.json"))?)?;
    let mut log = fs::File::create(output.join("samples.jsonl"))?;
    let mut samples = vec![];
    for (case, input) in [("sum-bits", &args[2]), ("raytrace", &args[3])] {
        let base=if learned||catalog||combination {&args[if case=="sum-bits"{6}else{7}]}else{input};
        let mut original: J = serde_json::from_slice(&fs::read(base)?)?;
        let mut admitted = original
            .as_object_mut()
            .unwrap()
            .remove("deferred_incidence_admission")
            .is_some();
        admitted |= original
            .as_object_mut()
            .unwrap()
            .remove("two_run_blocks")
            .is_some();
        admitted |= original.as_object_mut().unwrap().remove("multi_run_blocks").is_some();
        admitted |= learned || catalog || combination;
        original.as_object_mut().unwrap().remove("sha256");
        let original_sha = eggfuse_cell_runtime::checksum::artifact_digest(&original);
        for round in 0..=rounds {
            let mut modes = if combination {vec!["none","receipts","iterative","combined"]}else if multi {vec!["legacy2","early2","early8"]} else if ports {
                vec!["off", "on"]
            } else {
                vec!["off", "edges", "on"]
            };
            let n = modes.len();
            modes.rotate_left(round % n);
            for mode in modes {
                let run = output.join(format!("{case}-{round}-{mode}"));
                let mut cmd = Command::new(&binary);
                let program=if catalog && mode=="off" {base}else{input};
                cmd.args(["--program", program, "--output"])
                    .arg(&run)
                    .args(["--deferred-incidence", if ports { "off" } else { mode }]);
                if receipts || combination || learned || catalog || lhs || multi || fusion || (ports && mode == "on") {
                    cmd.arg("--port-frames");
                }
                if fusion && mode == "on" {
                    cmd.arg("--two-run-blocks");
                }
                if multi {match mode {
                    "legacy2"=>{cmd.arg("--two-run-blocks");},
                    "early2"=>{cmd.args(["--multi-run-blocks","--port-block-depth","2"]);},
                    _=>{cmd.arg("--multi-run-blocks");},
                }}
                if lhs {cmd.arg("--multi-run-blocks");if mode=="on" {cmd.arg("--block-lhs");}}
                if learned||catalog {cmd.arg("--multi-run-blocks");}
                if learned && mode=="off" {cmd.arg("--no-iterative-lhs");}
                if receipts {cmd.args(["--multi-run-blocks","--block-lhs"]);if mode=="on"{cmd.arg("--block-receipts");}}
                if combination {
                    cmd.arg("--multi-run-blocks");
                    if mode=="none" || mode=="receipts" {cmd.arg("--no-iterative-lhs");}
                    if mode=="receipts" || mode=="combined" {cmd.arg("--block-receipts");}
                }
                for (k, _) in std::env::vars() {
                    if k.starts_with("CELL_CPU_") || k.starts_with("RAYON_") {
                        cmd.env_remove(k);
                    }
                }
                cmd.env("RAYON_NUM_THREADS", "1");
                let start = Instant::now();
                let result = cmd.output()?;
                let cli_ms = start.elapsed().as_secs_f64() * 1000.;
                if !result.status.success() {
                    return Err(String::from_utf8_lossy(&result.stderr).into_owned().into());
                }
                let report: J = serde_json::from_slice(&fs::read(run.join("report.json"))?)?;
                let state_hash = hash(&run.join("state.json"))?;
                assert_eq!(
                    state_hash,
                    gold["audits"][case]["state_sha256"].as_str().unwrap()
                );
                let fields = [
                    "ordinal",
                    "ruleset",
                    "path",
                    "iterations",
                    "epoch",
                    "logical_matches",
                    "executed_bindings",
                    "changed",
                ];
                let steps = report["steps"].as_array().unwrap();
                let schedule = json!(
                    steps
                        .iter()
                        .map(|s| fields.iter().map(|k| s[*k].clone()).collect::<Vec<_>>())
                        .collect::<Vec<_>>()
                );
                let schedule_hash = format!(
                    "{:x}",
                    Sha256::digest(
                        eggfuse_cell_runtime::checksum::canonical(&schedule, true).as_bytes()
                    )
                );
                assert_eq!(
                    schedule_hash,
                    gold["audits"][case]["schedule_sha256"].as_str().unwrap()
                );
                let mut digests = json!({"state":state_hash,"schedule":schedule_hash});
                if audit {
                    for (name, expected) in gold["audits"][case]["digests"].as_object().unwrap() {
                        let path = run.join(format!("{name}.json"));
                        let mut got = hash(&path)?;
                        if name == "spine-shapes" && admitted {
                            digests["spine-shapes_raw"] = json!(got);
                            let mut shape: J = serde_json::from_slice(&fs::read(&path)?)?;
                            assert_eq!(shape["program_sha256"], report["program_sha256"]);
                            // Only the admission hint changes artifact identity;
                            // compare every static entry and trajectory unchanged.
                            shape["program_sha256"] = json!(original_sha);
                            got = format!("{:x}", Sha256::digest(serde_json::to_vec(&shape)?));
                        }
                        assert_eq!(got, expected.as_str().unwrap(), "{case} {mode} {name}");
                        digests[name] = json!(got);
                    }
                }
                let mut phases = json!({});
                for name in ["search", "rhs", "repair", "delivery_gc", "query_update"] {
                    phases[name] = json!(
                        steps
                            .iter()
                            .map(|s| s["phases"][name].as_f64().unwrap())
                            .sum::<f64>()
                            * 1000.
                    );
                }
                let sample = json!({"case":case,"round":round,"mode":mode,"audit":audit,
                    "input_sha256":hash(Path::new(program))?,"inference_ms":report["inference_seconds"].as_f64().unwrap()*1000.,"cli_ms":cli_ms,
                    "rss_bytes":report["peak_rss_through_inference_bytes"],"phases_ms":phases,
                    "stats":report["stats"],"block_lhs":report["block_lhs"],"key_lookup_calls":report["key_lookup_calls"],"singleton_fast_lookups":report["singleton_fast_lookups"],"fact_hash_lookups":report["fact_hash_lookups"],"deferred_incidence":report["deferred_incidence"],"port_frames":report["port_frames"],"two_run_blocks":report["two_run_blocks"],"storage":report["storage"],
                    "spine":report["stage_spine"]["stats"],"runs":steps.len(),"digests":digests});
                writeln!(log, "{sample}")?;
                log.flush()?;
                println!(
                    "{case} {mode} round {round}: {} ms; deferred {}",
                    sample["inference_ms"],
                    sample["deferred_incidence"]["stats"]["deferred_births"]
                );
                samples.push(sample);
                if lhs && audit && mode=="on" {
                    fs::copy(run.join("block-lhs-plans.json"),output.join(format!("{case}-plans.json")))?;
                }
                // Only this freshly created child output is disposable; failed
                // executions and failed checks above retain their complete data.
                fs::remove_dir_all(&run)?;
            }
        }
    }
    let mut summary = json!({"binary_sha256":hash(&binary)?,"rounds":rounds,"audit":audit,
        "inputs":{"sum-bits":hash(Path::new(&args[2]))?,"raytrace":hash(Path::new(&args[3]))?},"cases":{}});
    if rounds > 0 {
        for case in ["sum-bits", "raytrace"] {
            for mode in if combination {vec!["none","receipts","iterative","combined"]}else if multi {vec!["legacy2","early2","early8"]} else if ports {
                vec!["off", "on"]
            } else {
                vec!["off", "edges", "on"]
            } {
                let selected: Vec<_> = samples
                    .iter()
                    .filter(|s| {
                        s["case"] == case && s["mode"] == mode && s["round"].as_u64().unwrap() > 0
                    })
                    .collect();
                let mut values = json!({});
                for key in ["inference_ms", "cli_ms", "rss_bytes"] {
                    values[key] = json!(median(
                        selected.iter().map(|s| s[key].as_f64().unwrap()).collect()
                    ));
                }
                values["deferred_incidence"] = selected[0]["deferred_incidence"].clone();
                values["port_frames"] = selected[0]["port_frames"].clone();
                values["storage"] = selected[0]["storage"].clone();
                values["two_run_blocks"]=selected[0]["two_run_blocks"].clone();
                values["key_lookup_calls"]=selected[0]["key_lookup_calls"].clone();
                values["block_lhs"]=selected[0]["block_lhs"].clone();
                for name in ["search","rhs","repair","delivery_gc","query_update"] {
                    values["phases_ms"][name]=json!(median(selected.iter().map(|s|s["phases_ms"][name].as_f64().unwrap()).collect()));
                }
                summary["cases"][case][mode] = values;
            }
        }
    }
    fs::write(
        output.join("summary.json"),
        serde_json::to_vec_pretty(&summary)?,
    )?;
    Ok(())
}
