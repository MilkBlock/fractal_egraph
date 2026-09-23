use egg_layout::{native_analyze, pipeline::Result};
use std::path::{Path, PathBuf};
fn main() -> Result {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("--help" | "help" | "-h") => {
            println!(
                "egg_layout: analyze --source FILE --output DIR [--rounds N] [--save-history]\n  analyze --replay-history FILE --output DIR\n  cc-merge --rules FILE --seeds FILE --output DIR [--generations N]\n  debug-stream FILE\n  debug-patterns FILE\n  parse-check FILE\n  ripen FILE DIR\n  saturated-rule-composition-compare A.json B.json\n  saturated-rule-composition-catalog DIR RIPEN_DIR...\n  ripen-diff A.json B.json REPORT.json [ANCHORS.json]\nOptional analyze flags: --ripen-sweep-steps N (default 4), --ripen-observe-ms N / --ripen-observe-facts N. No hidden limits; observation pauses unfinished tasks between sweeps."
            );
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
            let mut observation = egg_layout::ripen_observation::Limits::from_env()?;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
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
            let r = native_analyze::run_with_observation(
                &source,
                &out.ok_or("--output is required")?,
                rounds,
                replay.as_deref(),
                save,
                observation,
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
