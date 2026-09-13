//! Single entry point for the current analysis pipeline.
use egg_layout::pipeline::{self, Result};
use std::process::Command;

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

analyze defaults to the saved tier-1 snapshot. --recapture-tier0 runs Math afresh.
Legacy experiments: cargo run --manifest-path research/Cargo.toml --bin NAME.
";
fn python(script: &str, args: &[String]) -> Result {
    let python = std::env::var_os("PYTHON").unwrap_or_else(|| "python3".into());
    let status = Command::new(python)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .arg(script)
        .args(args)
        .status()?;
    if !status.success() {
        return Err(format!("{script} failed: {status}").into());
    }
    Ok(())
}
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
            let mut forwarded = vec![
                "--driver".into(),
                std::env::current_exe()?.to_string_lossy().into_owned(),
            ];
            forwarded.extend_from_slice(&args[1..]);
            python("experiments/tier2/run.py", &forwarded)
        }
        Some("view") if args.len() == 1 => {
            python("experiments/tier2/fractal_view.py", &[])?;
            println!(
                "{}/experiments/tier2/fractal.html",
                env!("CARGO_MANIFEST_DIR")
            );
            Ok(())
        }
        _ => Err(format!("invalid command or arguments\n\n{HELP}").into()),
    }
}
