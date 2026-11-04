mod bench;
mod keys;
mod marker_ir;
mod markers;
mod surface;
mod witness;
use serde_json::{Value as J, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};
type Result<T> = egg_layout::pipeline::Result<T>;
fn sha(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}
fn read(path: &Path) -> Result<J> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
fn write(path: &Path, j: &J) -> Result<()> {
    Ok(fs::write(path, serde_json::to_vec(j)?)?)
}
fn bake(src: &Path, out: &Path) -> Result<()> {
    let r = egg_layout::native_analyze::run(src, out, None, None, false)?;
    if !matches!(
        r["status"].as_str(),
        Some("complete" | "observed_unfinished")
    ) {
        return Err("incomplete Bake analysis".into());
    }
    for j in surface::a(&r["saturated_rule_composition"]["jobs"]) {
        let msg = surface::s(&j["reason"]).to_lowercase();
        if [
            "undefined sort",
            "unbound function",
            "unbound symbol",
            "unknown primitive",
            "unknown sort",
        ]
        .iter()
        .any(|s| msg.contains(s))
        {
            return Err(format!("Bake environment error: {msg}").into());
        }
    }
    Ok(())
}
fn validate_environment(core: &J, bake: &J) -> Result<()> {
    for t in surface::a(&bake["tables"]) {
        if t["internal"] != true && !surface::a(&core["tables"]).contains(t) {
            return Err(format!("Bake declaration differs: {}", t["name"]).into());
        }
    }
    for s in surface::a(&bake["rulesets"]) {
        if !surface::a(&s["rules"]).is_empty()
            && !surface::a(&core["rulesets"])
                .iter()
                .any(|r| r["name"] == s["name"] && r["rules"] == s["rules"])
        {
            return Err(format!("Bake rules differ: {}", s["name"]).into());
        }
    }
    Ok(())
}
fn compile(
    source: &str,
    training: Option<&str>,
    out: &Path,
    opts: &BTreeMap<String, String>,
) -> Result<()> {
    if out.exists() {
        return Err(format!("output already exists: {}", out.display()).into());
    }
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let tmp = tempfile::Builder::new()
        .prefix(".cell-compile-")
        .tempdir_in(parent)?;
    let dir = tmp.path();
    fs::write(dir.join("source.egg"), source)?;
    let keyed = keys::rewrite("source.egg", source)?;
    let sk = dir.join("source_sk.egg");
    fs::write(&sk, &keyed)?;
    let train = if let Some(t) = training {
        validate_environment(
            &egg_layout::device_frontend::core("source_sk.egg", &keyed)?,
            &egg_layout::device_frontend::core("bake-source.egg", t)?,
        )?;
        let p = dir.join("bake-source.egg");
        fs::write(&p, t)?;
        p
    } else {
        sk
    };
    eprintln!("Cell compiler: native Bake");
    let analysis = dir.join("bake");
    let status = Command::new(std::env::current_exe()?)
        .arg("internal-bake")
        .arg(train)
        .arg(&analysis)
        .env("EGG_LAYOUT_GRAPHS", "0")
        .status()?;
    if !status.success() {
        return Err(format!("native Bake failed ({status})").into());
    }
    eprintln!("Cell compiler: select structural markers");
    let profile = opts
        .get("--profile")
        .map(|p| read(Path::new(p)))
        .transpose()?;
    let locality = opts
        .get("--marker-locality")
        .map(String::as_str)
        .unwrap_or("identity");
    if !matches!(locality, "identity" | "all") {
        return Err("unknown marker locality".into());
    }
    let cells = witness::load(&analysis)?;
    let (marked, report, names) = markers::rewrite(
        "source_sk.egg",
        &keyed,
        &cells,
        profile.as_ref(),
        locality == "identity",
    )?;
    let core = egg_layout::device_frontend::core("source_sk_mk.egg", &marked)?;
    eprintln!("Cell compiler: lower and validate standalone CPU plans");
    let analysis_bytes = fs::read(analysis.join("cell-recipes.json"))?;
    let marker = marker_ir::build(&core, &report, &marked, &analysis_bytes)?;
    let evidence = opts
        .get("--composition-evidence")
        .map(|f| read(Path::new(f)))
        .transpose()?;
    let mut p = eggfuse_cell_runtime::compiler::compile_with_entry(
        core,
        &names,
        evidence.as_ref(),
        opts.contains_key("--emit-free-join-entry")
            || opts.get("--lane").is_some_and(|s| matches!(s.as_str(), "free-join-entry" | "free-join-planned")),
    )?;
    p.as_object_mut().unwrap().remove("sha256");
    p["marker_program_sha256"] = marker["sha256"].clone();
    p["sha256"] = json!(eggfuse_cell_runtime::checksum::artifact_digest(&p));
    let caps = json!({"backend":"cell-cpu","native_runtime_linked":false,"requirements":p["requirements"],"entry_schedule":{"nodes":surface::a(&p["entry_schedule"]["nodes"]).len()}});

    let mut files = BTreeMap::new();
    for (file, value) in [
        ("program.json", &p),
        ("marker-program.json", &marker),
        ("capabilities.json", &caps),
        ("core.json", &p["source_core"]),
        ("recipe-program.json", &p["recipe"]),
        ("reference-views.json", &p["reference_views"]),
        ("entry-schedule.json", &p["entry_schedule"]),
        ("multi-stage-program.json", &p["multi_stage"]),
        ("merge-contracts.json", &p["merge_contracts"]),
        ("merge-programs.json", &p["merge_programs"]),
        ("known-results.json", &p["known_results"]),
        ("local-congruence-program.json", &p["congruence"]),
        ("marker-selection.json", &report),
    ] {
        let path = dir.join(file);
        write(&path, value)?;
        files.insert(file, sha(&fs::read(path)?));
    }
    if let Some(plan) = p.get("compositions") {
        let path = dir.join("composition-program.json");
        write(&path, plan)?;
        files.insert("composition-program.json", sha(&fs::read(path)?));
    }
    fs::write(dir.join("source_sk_mk.egg"), &marked)?;
    for file in ["source.egg", "source_sk.egg", "source_sk_mk.egg"] {
        files.insert(file, sha(&fs::read(dir.join(file))?));
    }
    if training.is_some() {
        files.insert(
            "bake-source.egg",
            sha(&fs::read(dir.join("bake-source.egg"))?),
        );
    }
    write(
        &dir.join("manifest.json"),
        &json!({"schema":"eggfuse-device-artifact/v1","source_sha256":sha(marked.as_bytes()),"analysis_sha256":sha(&analysis_bytes),"execution":{"native_runtime":false},"backend":"cell-cpu","files":files,"marker_selection":report["cost_model"],"training":"native-analyze","program_sha256":p["sha256"]}),
    )?;
    fs::rename(dir, out)?;
    Ok(())
}
fn options(
    mut args: impl Iterator<Item = String>,
    allowed: &[&str],
) -> Result<BTreeMap<String, String>> {
    let mut opts = BTreeMap::new();
    while let Some(key) = args.next() {
        if key == "--bench" {
            continue;
        }
        if !allowed.contains(&key.as_str()) {
            return Err(format!("unknown option {key}").into());
        }
        if key == "--emit-free-join-entry" {
            if opts.insert(key.clone(), String::new()).is_some() {
                return Err(format!("duplicate {key}").into());
            }
            continue;
        }
        let v = args
            .next()
            .filter(|v| !v.starts_with("--"))
            .ok_or_else(|| format!("{key} requires a value"))?;
        if opts.insert(key.clone(), v).is_some() {
            return Err(format!("duplicate {key}").into());
        }
    }
    Ok(opts)
}
fn suite(name: &str) -> Result<(String, String)> {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../experiments/profile_suite");
    let spec = read(&base.join("suite.json"))?;
    let input = |item: &J| -> Result<String> {
        let mut data = vec![];
        for p in surface::a(&item["parts"]) {
            data.extend(fs::read(
                base.join(p.as_str().ok_or("invalid suite input path")?),
            )?);
        }
        if item["sha256"] != sha(&data) {
            return Err("frozen suite input checksum mismatch".into());
        }
        Ok(String::from_utf8(data)?)
    };
    let case = surface::a(&spec["cases"])
        .iter()
        .find(|c| c["name"] == name)
        .ok_or_else(|| format!("unknown case {name}"))?;
    Ok((input(case)?, input(&spec["bake"])?))
}
fn inputs(opts: &BTreeMap<String, String>) -> Result<(String, Option<String>)> {
    match (
        opts.get("--source"),
        opts.get("--case"),
        opts.get("--bake-source"),
    ) {
        (Some(src), None, b) => Ok((
            fs::read_to_string(src)?,
            b.map(fs::read_to_string).transpose()?,
        )),
        (None, Some(c), None) => {
            let (s, b) = suite(c)?;
            Ok((s, Some(b)))
        }
        _ => Err("use --case NAME, or --source PATH with optional --bake-source PATH".into()),
    }
}
pub fn cli(default_bench: bool) -> Result<()> {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    args.retain(|a| a != "--bench");
    let cmd = if default_bench
        && !matches!(
            args.first().map(String::as_str),
            Some("internal-bake" | "internal-worker" | "internal-stages")
        ) {
        "bench".into()
    } else if args.is_empty() {
        "help".into()
    } else {
        args.remove(0)
    };
    if cmd == "help" || args.iter().any(|a| a == "--help") {
        println!(
            "cargo cell-compile (--case NAME | --source FILE [--bake-source FILE]) --output DIR [--emit-free-join-entry] [--profile FILE] [--marker-locality identity|all] [--composition-evidence FILE]\ncargo cell-bench (--case NAME | --source FILE [--bake-source FILE] | --program FILE) [--repeat N] [--lane baseline|nodes|filtered|free-join|free-join-entry|free-join-planned] [--modules local-congruence,union-graft,stage-spine,static-aliases,slot-transfer,range-guards,fd-keys,scalar-merge] [--slot-transfer off|siblings|parent|both|local-only|parent-check|parent-owned] [--verify-stages]"
        );
        return Ok(());
    }
    egglog::EGraph::set_num_threads(1);
    match cmd.as_str() {
        "internal-bake" if args.len() == 2 => bake(Path::new(&args[0]), Path::new(&args[1])),
        "compile" => {
            let opts = options(
                args.into_iter(),
                &[
                    "--source",
                    "--case",
                    "--output",
                    "--bake-source",
                    "--profile",
                    "--marker-locality",
                    "--composition-evidence",
                    "--emit-free-join-entry",
                ],
            )?;
            let (s, b) = inputs(&opts)?;
            let out = Path::new(opts.get("--output").ok_or("missing --output")?);
            compile(&s, b.as_deref(), out, &opts)?;
            println!("{}", out.join("program.json").display());
            Ok(())
        }
        "internal-stages" if args.len() == 1 => bench::stages(Path::new(&args[0])),
        "bench" => bench::run(args),
        "internal-worker" => bench::worker(args),
        _ => Err(format!("invalid invocation {cmd}").into()),
    }
}
