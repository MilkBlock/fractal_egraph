mod bridge;
mod cache;
mod runner;
mod suite;
use anyhow::{Context, Result, bail};
use std::path::PathBuf;

pub struct Options {
    pub root: PathBuf,
    pub cache: PathBuf,
    pub suite: PathBuf,
    pub tools: PathBuf,
    pub python: PathBuf,
    pub solver: PathBuf,
    pub compiler: Option<PathBuf>,
    pub cases: Vec<String>,
    pub all: bool,
    pub fresh: bool,
    pub only: bool,
    pub plan: bool,
    pub list: bool,
    pub hardware: bool,
    pub capture: bool,
    pub pipeline: bool,
}
fn main() {
    if let Err(e) = run() {
        eprintln!("profile: {e:#}");
        std::process::exit(1)
    }
}
fn run() -> Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let mut o = Options {
        cache: root.join("out/profile-suite"),
        suite: root.join("experiments/profile_suite/suite.json"),
        tools: root.join("target/debug/examples"),
        python: PathBuf::from("python3"),
        solver: PathBuf::from("python3"),
        root,
        compiler: None,
        cases: vec![],
        all: false,
        fresh: false,
        only: false,
        plan: false,
        list: false,
        hardware: false,
        capture: false,
        pipeline: false,
    };
    // Local configuration is optional and untracked; CLI/env can replace tools.
    if let Ok(p) = std::env::var("EGGFUSE_SOLVER_PYTHON") {
        o.solver = p.into();
    } else {
        let local = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join("Repos/egg_related/egg_layout/out/cell-spine-venv/bin/python");
        if local.exists() {
            o.solver = local;
        }
    }
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || {
            args.next()
                .with_context(|| format!("missing argument after {arg}"))
        };
        match arg.as_str() {
            "--case" => o.cases.push(value()?),
            "--all" => o.all = true,
            "--fresh" => o.fresh = true,
            "--cached-only" => o.only = true,
            "--plan" => o.plan = true,
            "--list" => o.list = true,
            "--hardware" => o.hardware = true,
            "--capture" => o.capture = true,
            "--pipeline" => o.pipeline = true,
            "--cache" => o.cache = PathBuf::from(value()?),
            "--suite" => o.suite = PathBuf::from(value()?),
            "--tools" => o.tools = PathBuf::from(value()?),
            "--python" => o.python = PathBuf::from(value()?),
            "--solver-python" => o.solver = PathBuf::from(value()?),
            "--compiler" => o.compiler = Some(PathBuf::from(value()?)),
            "-h" | "--help" => {
                println!(
                    "cargo profile [--case NAME | --all] [--fresh] [--hardware] [--capture]\n\nDefault: fact, loopfact, sum-bits, complete Pass 3; resident-core (no source markers), multi-stage Cell, clean + verified encoder timing.\nRaytrace is opt-in: cargo profile --case raytrace\n\n--pipeline       Explicit full SK + source-marker compiler lane (slow cold build)\n--fresh          Remeasure; retain compiler cache and previous measurements\n--cached-only    Refuse execution on a cache miss\n--plan           Show cache decisions without executing compiler/GPU work\n--hardware       Collect cached live hardware windows selected from phase timings\n--capture        Capture the hottest outer command (Apple replay is separate)\n--list           List all four frozen workloads\n--cache DIR      Content-addressed artifacts/results (default out/profile-suite)\n--tools DIR      Existing native analyzer helpers (default target/debug/examples)\n--solver-python  Python with the current marker solver\n--compiler BIN   With --pipeline: future compiler using the same artifact contract\n--python BIN     Existing Python adapters; never edited by this driver\n--suite FILE     Explicit alternative frozen suite manifest"
                );
                return Ok(());
            }
            _ => bail!("unknown argument {arg}; use --help"),
        }
    }
    if o.fresh && o.only {
        bail!("--fresh conflicts with --cached-only");
    }
    if o.all && !o.cases.is_empty() {
        bail!("choose --all or --case");
    }
    if o.compiler.is_some() && !o.pipeline {
        bail!("--compiler selects the full source pipeline; also pass --pipeline");
    }
    o.cache = std::path::absolute(&o.cache)?;
    o.suite = std::path::absolute(&o.suite)?;
    o.tools = std::path::absolute(&o.tools)?;
    let cases = suite::load(&o.suite, &o.cases, o.all || o.list)?;
    if o.list {
        for c in cases {
            println!(
                "{:<12} Pass 3  {:>8} bytes  {}",
                c.name,
                c.bytes.len(),
                if c.spec["default"] == true {
                    "default"
                } else {
                    "opt-in"
                }
            );
        }
        return Ok(());
    }
    runner::run(&o, &cases)
}
