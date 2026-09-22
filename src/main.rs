//! Single entry point for the current analysis pipeline.
use egg_layout::pipeline::{self, Result};
use std::path::PathBuf;

const HELP: &str = "egg_layout — native rule-combination analysis

  cargo run -- analyze --reuse-tier0
  cargo run -- analyze --recapture-tier0 --source PATH.egg --rounds 11 --output out/math11
  cargo run -- debug-patterns SOURCE.egg    Parse source ranges, Typst, and DOT
  cargo run -- debug-stream SOURCE.egg      Stream native events + default budgeted ripen
  cargo run -- saturated-rule-composition-compare A/saturated-rule-composition.json B/saturated-rule-composition.json [--budget N]
  cargo run -- saturated-rule-composition-catalog OUTPUT_DIR RIPEN_DIR... [--budget N]
  cargo run -- ripen-diff LEFT.json RIGHT.json REPORT.json [ANCHORS.json]
  cargo run -- ripen-probe OUTPUT_DIR MAX_ROUNDS [--baseline] INPUT.egg...
  cargo run -- ripen INPUT.egg OUTPUT_DIR [--max-rounds N]
  cargo run -- ripen-use HISTORY.json USE_ID OUTPUT_DIR [--max-rounds N]
  cargo run -- bake-format OLD_LIBRARY NEW_LIBRARY.egg
  cargo run -- bake MANIFEST.json OUTPUT_DIR
  cargo run -- bake-use LIBRARY.egg SOURCE.egg OUTPUT_DIR [--save-history]
  cargo run -- bake-eval LIBRARY.egg TEMPLATE START LIMIT OUTPUT.json
  cargo run -- bake-eval LIBRARY.egg TEMPLATE START --depth N OUTPUT.json
  cargo run -- arrays INPUT.egg OUTPUT.json
  cargo run -- embed-dag SMALL.json LARGE.json OUT.json [BUDGET]
  cargo run -- view                   Regenerate the fractal viewer from saved results

  analyze --recapture-tier0 --build online --save-history --source PATH.egg --output out/online
  analyze --recapture-tier0 --build offline --save-history --source PATH.egg --output out/offline
  analyze --replay-history out/online/history.json --output out/replayed

Capture defaults to online imports at round boundaries. --save-history is optional.
Replay rebuilds tier-1/tier-2 from resolved application history without executing tier-0.

Native operations (run from the repository root):
  cargo run -- relations INPUT OUTPUT
  cargo run -- schema INPUT OUTPUT
  cargo run -- fixture INPUT OUTPUT    Additive-recurrence test adapter
  cargo run -- higher
  cargo run -- reduce
  cargo run -- observations            Additive-recurrence test adapter

analyze defaults to re-rendering the saved tier-1 analysis. Recapture runs all stages in one Rust process.
Legacy experiments: cargo run --manifest-path research/Cargo.toml --bin NAME.
";
fn main() -> Result {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("--help" | "-h" | "help") => {
            print!("{HELP}");
            Ok(())
        }
        Some("debug-patterns") if args.len() == 2 => {
            println!(
                "{}",
                egg_layout::native_analyze::debug::patterns(&std::fs::read_to_string(&args[1])?)?
            );
            Ok(())
        }
        Some("debug-stream") if args.len() == 2 => {
            use std::io::Write;
            egg_layout::native_analyze::debug::stream(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")),
                std::path::Path::new(&args[1]),
                &mut |row| {
                    println!("{row}");
                    std::io::stdout().flush()?;
                    Ok(())
                },
            )
        }
        Some("bake-format") if args.len() == 3 => {
            egg_layout::native_analyze::bake::convert_library(
                std::path::Path::new(&args[1]),
                std::path::Path::new(&args[2]),
            )
        }
        Some("bake") if args.len() == 3 => {
            let result = egg_layout::native_analyze::bake::train(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")),
                std::path::Path::new(&args[1]),
                std::path::Path::new(&args[2]),
            )?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        Some("bake-use") if args.len() == 4 || (args.len() == 5 && args[4] == "--save-history") => {
            let result = egg_layout::native_analyze::bake::apply(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")),
                std::path::Path::new(&args[1]),
                std::path::Path::new(&args[2]),
                std::path::Path::new(&args[3]),
                args.len() == 5,
            )?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        Some("bake-eval") if args.len() == 7 && args[4] == "--depth" => {
            let result = egg_layout::native_analyze::bake::query_frontier(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")),
                std::path::Path::new(&args[1]),
                &args[2],
                args[3].parse()?,
                args[5].parse()?,
                std::path::Path::new(&args[6]),
            )?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        Some("bake-eval") if args.len() == 6 => {
            let result = egg_layout::native_analyze::bake::query(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")),
                std::path::Path::new(&args[1]),
                &args[2],
                args[3].parse()?,
                args[4].parse()?,
                std::path::Path::new(&args[5]),
            )?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
        Some("arrays") if args.len() == 3 => pipeline::arrays(&args[1], &args[2]),
        Some("embed-dag") if args.len() == 4 || args.len() == 5 => {
            let small: egg_layout::dag_embedding::Dag =
                serde_json::from_slice(&std::fs::read(&args[1])?)?;
            let large: egg_layout::dag_embedding::Dag =
                serde_json::from_slice(&std::fs::read(&args[2])?)?;
            let budget = args
                .get(4)
                .map(|v| v.parse::<usize>())
                .transpose()?
                .unwrap_or(100_000);
            let outcome = egg_layout::dag_embedding::embed(&small, &large, budget)
                .map_err(std::io::Error::other)?;
            let projection = if let egg_layout::dag_embedding::Outcome::Found { node_map, .. } =
                &outcome
            {
                small.interface.iter().map(|p|serde_json::json!({"name":p.name,"node":node_map[p.node],"port":p.port})).collect::<Vec<_>>()
            } else {
                vec![]
            };
            let report = serde_json::json!({"embedding":outcome,"interface_projection":projection,"scope":"finite labelled DAG embedding, not executable equivalence"});
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&args[3])?;
            serde_json::to_writer_pretty(file, &report)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        Some("relations" | "schema" | "fixture") if args.len() == 3 => match args[0].as_str() {
            "relations" => pipeline::relations(&args[1], &args[2]),
            "schema" => pipeline::schema(&args[1], &args[2]),
            _ => pipeline::fixture(&args[1], &args[2]),
        },
        Some("higher") if args.len() == 1 => pipeline::higher(),
        Some("reduce") if args.len() == 1 => pipeline::reduce(),
        Some("observations") if args.len() == 1 => pipeline::observations(),
        Some("saturated-rule-composition-compare") if args.len() == 3 || args.len() == 5 => {
            let budget = if args.len() == 5 {
                if args[3] != "--budget" {
                    return Err("expected --budget".into());
                }
                args[4].parse()?
            } else {
                100_000
            };
            let a = egg_layout::saturated_rule_composition::read(&PathBuf::from(&args[1]))?;
            let b = egg_layout::saturated_rule_composition::read(&PathBuf::from(&args[2]))?;
            println!(
                "{}",
                serde_json::to_string_pretty(&egg_layout::saturated_rule_composition::compare(&a, &b, budget)?)?
            );
            Ok(())
        }
        Some("saturated-rule-composition-catalog") if args.len() >= 3 => {
            let mut end = args.len();
            let budget = if args.len() >= 5 && args[args.len() - 2] == "--budget" {
                end -= 2;
                args[end + 1].parse()?
            } else {
                100_000
            };
            let inputs = args[2..end].iter().map(PathBuf::from).collect::<Vec<_>>();
            let r = egg_layout::saturated_rule_composition::catalog(&inputs, &PathBuf::from(&args[1]), budget)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "saturated_rule_compositions":r["saturated_rule_compositions"],"triggers":r["triggers"].as_array().map(Vec::len),
                    "unresolved_comparisons":r["unresolved_comparisons"],"output":args[1]
                }))?
            );
            Ok(())
        }
        Some("ripen-diff") if args.len()==4 || args.len()==5 => {
            let a=egg_layout::saturated_rule_composition::read(&PathBuf::from(&args[1]))?;
            let b=egg_layout::saturated_rule_composition::read(&PathBuf::from(&args[2]))?;
            let anchors:Vec<(usize,usize)>=if args.len()==5{serde_json::from_slice(&std::fs::read(&args[4])?)?}else{vec![]};
            let report=egg_layout::state_difference::explain(&a,&b,&anchors)?;
            let file=std::fs::OpenOptions::new().write(true).create_new(true).open(&args[3])?;
            serde_json::to_writer_pretty(file,&report)?;
            println!("{}",report["status"]);Ok(())
        }
        Some("ripen-probe") if args.len() >= 4 => {
            let baseline = args[3] == "--baseline";
            let sources = args[if baseline {4} else {3}..].iter().map(PathBuf::from).collect::<Vec<_>>();
            if sources.is_empty() { return Err("ripen-probe needs an input".into()); }
            let report = egg_layout::native_analyze::ripen::probe_mode(&sources, &PathBuf::from(&args[1]), args[2].parse()?, !baseline)?;
            println!("{}", serde_json::to_string_pretty(&report["index"])?);
            Ok(())
        }
        Some("ripen-use") if args.len() == 4 || args.len() == 6 => {
            let budget = if args.len() == 6 {
                if args[4] != "--max-rounds" {
                    return Err("expected --max-rounds".into());
                }
                args[5].parse()?
            } else {
                32
            };
            let report = egg_layout::native_analyze::ripen::from_use(
                &PathBuf::from(&args[1]),
                args[2].parse()?,
                &PathBuf::from(&args[3]),
                budget,
            )?;
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"ripen":report["ripen"],"checks":report["checks"],"saturated_rule_composition":report["saturated_rule_composition"],"source_use":args[2],"symbolic_boundary":true,"output":args[3]})
                )?
            );
            Ok(())
        }
        Some("ripen") if args.len() == 3 || args.len() == 5 => {
            let budget = if args.len() == 5 {
                if args[3] != "--max-rounds" {
                    return Err("expected --max-rounds".into());
                }
                args[4].parse()?
            } else {
                32
            };
            let report = egg_layout::native_analyze::ripen::run(
                &PathBuf::from(&args[1]),
                &PathBuf::from(&args[2]),
                budget,
            )?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "ripen":report["ripen"],"checks":report["checks"],"saturated_rule_composition":report["saturated_rule_composition"],
                    "imported_applies":report["imported_applies"],"output":args[2]
                }))?
            );
            Ok(())
        }
        Some("analyze") => {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            let mut online = true;
            let mut build_selected = false;
            let mut history = false;
            let mut replay = None;
            let mut fresh = false;
            let mut reuse = false;
            let mut source = None;
            let mut rounds = None;
            let mut out = None;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--recapture-tier0" => fresh = true,
                    "--save-history" => history = true,
                    "--build" | "--replay-history" => {
                        let key = &args[i];
                        i += 1;
                        let value = args.get(i).ok_or("missing option value")?;
                        if key == "--build" {
                            build_selected = true;
                            online = match value.as_str() {
                                "online" => true,
                                "offline" => false,
                                _ => return Err("build must be online or offline".into()),
                            };
                        } else {
                            replay = Some(PathBuf::from(value));
                        }
                    }
                    "--reuse-tier0" => reuse = true,
                    "--source" | "--rounds" | "--output" => {
                        let key = &args[i];
                        i += 1;
                        let value = args.get(i).ok_or("missing option value")?;
                        match key.as_str() {
                            "--source" => source = Some(PathBuf::from(value)),
                            "--output" => out = Some(PathBuf::from(value)),
                            _ => {
                                let n: usize = value.parse()?;
                                if n == 0 {
                                    return Err("rounds must be positive".into());
                                }
                                rounds = Some(n);
                            }
                        }
                    }
                    "--help" => {
                        print!("{HELP}");
                        return Ok(());
                    }
                    other => return Err(format!("invalid command or arguments: {other}").into()),
                }
                i += 1;
            }
            if build_selected && (!fresh && replay.is_none() || replay.is_some() && online) {
                return Err("build mode requires recapture, or offline history replay".into());
            }
            if replay.is_some() && (fresh || reuse || source.is_some() || rounds.is_some()) {
                return Err(
                    "history replay cannot be combined with capture/reuse/source/rounds".into(),
                );
            }
            if history && !fresh && replay.is_none() {
                return Err("save-history requires capture or replay".into());
            }
            if fresh && reuse || !fresh && (source.is_some() || rounds.is_some()) {
                return Err(
                    "source/rounds require recapture; recapture and reuse are exclusive".into(),
                );
            }
            if fresh || replay.is_some() {
                let source = root.join(source.unwrap_or_else(|| {
                    PathBuf::from("experiments/annotated_export/math_microbenchmark/source.egg")
                }));
                let out = root.join(out.unwrap_or_else(|| {
                    PathBuf::from(format!(
                        "out/native-{}",
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_millis()
                    ))
                }));
                let report = egg_layout::native_analyze::run_with_options(
                    &root,
                    &source,
                    rounds,
                    &out,
                    online && replay.is_none(),
                    history,
                    replay.as_ref().map(|p| root.join(p)).as_deref(),
                )?;
                println!("{}", serde_json::to_string_pretty(&report["summary"])?);
                println!("{}", out.join("fractal.html").display());
                if !report["recursive_patterns"].is_null() {
                    println!("{}", out.join("recursive_patterns.html").display());
                }
                if !report["dependency_growth"].is_null() {
                    println!("{}", out.join("dependency_growth.html").display());
                }
                Ok(())
            } else {
                egg_layout::native_analyze::reuse(
                    &root,
                    out.as_ref().map(|p| root.join(p)).as_deref(),
                )
            }
        }
        Some("view") if args.len() == 1 => {
            egg_layout::native_analyze::reuse(&PathBuf::from(env!("CARGO_MANIFEST_DIR")), None)
        }
        _ => Err(format!("invalid command or arguments\n\n{HELP}").into()),
    }
}
