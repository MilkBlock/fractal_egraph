//! Single entry point for the current analysis pipeline.
use egg_layout::pipeline::{self, Result};
use std::path::PathBuf;

const HELP: &str = "egg_layout — native rule-combination analysis

  cargo run -- analyze --reuse-tier0
  cargo run -- analyze --recapture-tier0 --source PATH.egg --rounds 11 --output out/math11
  cargo run -- view                   Regenerate the fractal viewer from saved results

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
        Some("relations" | "schema" | "fixture") if args.len() == 3 => match args[0].as_str() {
            "relations" => pipeline::relations(&args[1], &args[2]),
            "schema" => pipeline::schema(&args[1], &args[2]),
            _ => pipeline::fixture(&args[1], &args[2]),
        },
        Some("higher") if args.len() == 1 => pipeline::higher(),
        Some("reduce") if args.len() == 1 => pipeline::reduce(),
        Some("observations") if args.len() == 1 => pipeline::observations(),
        Some("analyze") => {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            let mut fresh = false;
            let mut reuse = false;
            let mut source = None;
            let mut rounds = None;
            let mut out = None;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--recapture-tier0" => fresh = true,
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
            if fresh && reuse || !fresh && (source.is_some() || rounds.is_some()) {
                return Err(
                    "source/rounds require recapture; recapture and reuse are exclusive".into(),
                );
            }
            if fresh {
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
                let report = egg_layout::native_analyze::run(&root, &source, rounds, &out)?;
                println!("{}", serde_json::to_string_pretty(&report["summary"])?);
                println!("{}", out.join("fractal.html").display());
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
