use egg_layout::{native_analyze, pipeline::Result};
use std::path::{Path, PathBuf};
fn main() -> Result {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("--help" | "help" | "-h") => {
            println!(
                "egg_layout: reuse-bake OUTPUT.json RUN_DIR...\n  reuse-library-check LIBRARY.json\n  reuse-query SOURCE.egg LIBRARY.json QUERY.json\n  reuse-run SOURCE.egg LIBRARY.json [--learn-grafts OUTPUT.json]\n  reuse-cycle SOURCE.egg LIBRARY.json OUTPUT_DIR\n  analyze --source FILE --output DIR [--rounds N] [--save-history]\n  analyze --replay-history FILE --output DIR\n  cc-merge --rules FILE --seeds FILE --output DIR [--generations N]\n  debug-stream FILE\n  debug-patterns FILE\n  parse-check FILE\n  ripen FILE DIR\n  saturated-rule-composition-compare A.json B.json\n  saturated-rule-composition-catalog DIR RIPEN_DIR...\n  ripen-diff A.json B.json REPORT.json [ANCHORS.json]\nOptional analyze flags: --reuse-library FILE; --ripen-sweep-steps N (default 4), --ripen-observe-ms N / --ripen-observe-facts N. Defaults: 50 ms / 200000 facts; native admission pauses unfinished tasks."
            );
            Ok(())
        }
        Some("reuse-cycle") => {
            if args.len()!=4 {return Err("usage: reuse-cycle SOURCE.egg LIBRARY.json OUTPUT_DIR".into());}
            let out=Path::new(&args[3]);
            if out.exists(){return Err("reuse-cycle output already exists".into());}
            let library=egg_layout::reuse_graph::library::Library::load(Path::new(&args[2]))?;
            let (execution,signatures)=egg_layout::reuse_graph::planner::rule_runner::run_observed(&std::fs::read_to_string(&args[1])?,&library,true)?;
            native_analyze::run_with_reuse(Path::new(&args[1]),out,None,None,false,egg_layout::ripen_observation::Limits::from_env()?,Some(Path::new(&args[2])))?;
            let mut learned=egg_layout::reuse_graph::library::Library::load(&out.join("reuse-library.json"))?;
            learned.remember_grafts(signatures);learned.save(&out.join("adaptive-library.json"))?;
            let queue:serde_json::Value=serde_json::from_slice(&std::fs::read(out.join("saturated-rule-composition/queue.json"))?)?;
            let report=serde_json::json!({"execution":execution,"learning":queue["reuse_learning"],"library":out.join("adaptive-library.json"),"scope":"validated positive-program feedback experiment: optimized execution, independent native reference, and native trace/ripen learning are separate passes; not an end-to-end speedup claim"});
            std::fs::write(out.join("reuse-cycle.json"),serde_json::to_vec_pretty(&report)?)?;
            println!("{}",serde_json::to_string_pretty(&report)?);Ok(())
        }
        Some("reuse-run") => {
            if args.len()!=3 && args.len()!=5 {return Err("usage: reuse-run SOURCE.egg LIBRARY.json [--learn-grafts OUTPUT.json]".into());}
            if args.len()==5 && args[3]!="--learn-grafts" {return Err("unknown reuse-run option".into());}
            let mut library=egg_layout::reuse_graph::library::Library::load(Path::new(&args[2]))?;
            let (result,observed)=egg_layout::reuse_graph::planner::rule_runner::run_observed(&std::fs::read_to_string(&args[1])?,&library,args.len()==5)?;
            if args.len()==5 {library.remember_grafts(observed);library.save(Path::new(&args[4]))?;}
            println!("{}",serde_json::to_string_pretty(&result)?);Ok(())
        }
        Some("reuse-query") => {
            if args.len()!=4 {return Err("usage: reuse-query SOURCE.egg LIBRARY.json QUERY.json".into());}
            let library=egg_layout::reuse_graph::library::Library::load(Path::new(&args[2]))?;
            let q:egg_layout::reuse_graph::planner::Query=serde_json::from_slice(&std::fs::read(&args[3])?)?;
            let mut engine=egg_layout::reuse_graph::planner::Engine::default();
            engine.execute(&std::fs::read_to_string(&args[1])?)?;engine.install_library(&library)?;
            let actual=engine.query(&q)?;
            let stats=serde_json::to_value(&engine.stats)?;
            let expected=engine.reference(&q)?;
            if actual!=expected {return Err("fragment plan disagrees with native whole-query join".into());}
            println!("{}",serde_json::json!({"bindings":actual.iter().map(|r|format!("{r:?}")).collect::<Vec<_>>(),"planner":stats,"boundary_demands":engine.boundary_demands(),"reference_equal":true}));
            Ok(())
        }
        Some("reuse-bake") => {
            if args.len() < 3 {
                return Err("usage: reuse-bake OUTPUT.json RUN_DIR...".into());
            }
            let report = egg_layout::reuse_graph::library::bake(&args[2..], Path::new(&args[1]))?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        Some("reuse-library-check") => {
            if args.len() != 2 {
                return Err("usage: reuse-library-check LIBRARY.json".into());
            }
            let library = egg_layout::reuse_graph::library::Library::load(Path::new(&args[1]))?;
            println!("{}", library.counts());
            Ok(())
        }
        Some("cc-merge") => {
            let (mut rules, mut seeds, mut out, mut generations) = (None, None, None, 1);
            let mut i = 1;
            while i < args.len() {
                let key = &args[i];
                i += 1;
                let value = args.get(i).ok_or("missing CC option value")?;
                match key.as_str() {
                    "--rules" => rules = Some(PathBuf::from(value)),
                    "--seeds" => seeds = Some(PathBuf::from(value)),
                    "--output" => out = Some(PathBuf::from(value)),
                    "--generations" => generations = value.parse()?,
                    _ => return Err(format!("unknown CC option {key}").into()),
                }
                i += 1;
            }
            egg_layout::cc_merge::run(
                &rules.ok_or("--rules required")?,
                &seeds.ok_or("--seeds required")?,
                &out.ok_or("--output required")?,
                generations,
            )
        }
        Some("analyze") => {
            let mut source = None;
            let mut out = None;
            let mut rounds = None;
            let mut replay = None;
            let mut save = false;
            let mut reuse_library=None;
            let mut observation = egg_layout::ripen_observation::Limits::from_env()?;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--reuse-library" => {i+=1;reuse_library=Some(PathBuf::from(args.get(i).ok_or("missing reuse library")?));}
                    "--source" => {
                        i += 1;
                        source = Some(PathBuf::from(args.get(i).ok_or("missing source")?));
                    }
                    "--output" => {
                        i += 1;
                        out = Some(PathBuf::from(args.get(i).ok_or("missing output")?));
                    }
                    "--rounds" => {
                        i += 1;
                        rounds = Some(args.get(i).ok_or("missing rounds")?.parse()?);
                    }
                    "--replay-history" => {
                        i += 1;
                        replay = Some(PathBuf::from(args.get(i).ok_or("missing history")?));
                    }
                    "--save-history" => save = true,
                    "--ripen-sweep-steps" => {
                        i += 1;
                        observation.sweep_steps =
                            args.get(i).ok_or("missing sweep steps")?.parse()?;
                    }
                    "--ripen-observe-ms" => {
                        i += 1;
                        observation.milliseconds = Some(
                            args.get(i)
                                .ok_or("missing observation milliseconds")?
                                .parse()?,
                        );
                    }
                    "--ripen-observe-facts" => {
                        i += 1;
                        observation.facts = Some(
                            args.get(i)
                                .ok_or("missing observation fact count")?
                                .parse()?,
                        );
                    }
                    "--recapture-tier0" => {}
                    x => {
                        return Err(format!(
                            "unsupported option {x}; old experimental stages have been removed"
                        )
                        .into());
                    }
                }
                i += 1;
            }
            let source = source.unwrap_or_default();
            if replay.is_none() && source.as_os_str().is_empty() {
                return Err("--source is required".into());
            }
            let r = native_analyze::run_with_reuse(
                &source,
                &out.ok_or("--output is required")?,
                rounds,
                replay.as_deref(),
                save,
                observation,
                reuse_library.as_deref(),
            )?;
            println!("{}", r["summary"]);
            Ok(())
        }
        Some("debug-patterns") if args.len() == 2 => {
            println!(
                "{}",
                native_analyze::debug::patterns(&std::fs::read_to_string(&args[1])?)?
            );
            Ok(())
        }
        Some("parse-check") if args.len() == 2 => {
            println!(
                "{}",
                native_analyze::debug::parse_check(&std::fs::read_to_string(&args[1])?)?
            );
            Ok(())
        }
        Some("debug-stream") if args.len() == 2 => {
            use std::io::Write;
            native_analyze::debug::stream(
                Path::new(env!("CARGO_MANIFEST_DIR")),
                Path::new(&args[1]),
                &mut |row| {
                    println!("{row}");
                    std::io::stdout().flush()?;
                    Ok(())
                },
            )
        }
        Some("ripen") if args.len() == 3 => {
            println!(
                "{}",
                native_analyze::ripen::run(Path::new(&args[1]), Path::new(&args[2]))?
            );
            Ok(())
        }
        Some("saturated-rule-composition-compare") if args.len() == 3 => {
            use egg_layout::saturated_rule_composition::*;
            println!(
                "{}",
                serde_json::to_string(&compare(
                    &read(Path::new(&args[1]))?,
                    &read(Path::new(&args[2]))?
                )?)?
            );
            Ok(())
        }
        Some("saturated-rule-composition-catalog") if args.len() > 2 => {
            let paths = args[2..].iter().map(PathBuf::from).collect::<Vec<_>>();
            println!(
                "{}",
                egg_layout::saturated_rule_composition::catalog(&paths, Path::new(&args[1]))?
            );
            Ok(())
        }
        Some("ripen-diff") if args.len() == 4 || args.len() == 5 => {
            use egg_layout::saturated_rule_composition::read;
            let anchors = if args.len() == 5 {
                serde_json::from_slice(&std::fs::read(&args[4])?)?
            } else {
                vec![]
            };
            let r = egg_layout::state_difference::explain(
                &read(Path::new(&args[1]))?,
                &read(Path::new(&args[2]))?,
                &anchors,
            )?;
            serde_json::to_writer_pretty(
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&args[3])?,
                &r,
            )?;
            Ok(())
        }
        _ => Err("unknown command; use --help for the CS/ripen core".into()),
    }
}
