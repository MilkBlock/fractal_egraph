mod bench;
#[cfg(feature = "iterative-bake")]
mod iterate;
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
type Result<T> = eggfuse_bake::pipeline::Result<T>;
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
    let r = eggfuse_bake::native_analyze::bake(src, out)?;
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
    catalog: Option<&J>,
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
            &eggfuse_bake::device_frontend::core("source_sk.egg", &keyed)?,
            &eggfuse_bake::device_frontend::core("bake-source.egg", t)?,
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
    let core = eggfuse_bake::device_frontend::core("source_sk_mk.egg", &marked)?;
    eprintln!("Cell compiler: lower and validate standalone CPU plans");
    let analysis_bytes = fs::read(analysis.join("cell-recipes.json"))?;
    let marker = marker_ir::build(&core, &report, &marked, &analysis_bytes)?;
    let mut p = eggfuse_cell_runtime::compiler::compile(core, &names)?;
    if let Some(catalog) = catalog {
        if catalog["schema"] != "eggfuse-cpu-catalog/v1"
            || catalog["recipe_sha256"] != p["recipe"]["sha256"]
        {
            return Err("catalog does not match the compiled recipe".into());
        }
        for key in ["spine_blocks", "iterative_bake", "context_admission"] {
            p[key] = catalog
                .get(key)
                .ok_or_else(|| format!("missing catalog field {key}"))?
                .clone();
        }
        eggfuse_cell_runtime::program::Program::parse(p.clone())?;
    }
    p.as_object_mut().unwrap().remove("sha256");
    p["marker_program_sha256"] = marker["sha256"].clone();
    p["sha256"] = json!(eggfuse_cell_runtime::checksum::artifact_digest(&p));
    let caps = json!({"backend":"cell-cpu","native_runtime_linked":false,"requirements":p["requirements"],"entry_schedule":{"nodes":surface::a(&p["entry_schedule"]["nodes"]).len()}});

    let mut files = BTreeMap::new();
    for (file, value) in [
        ("program.json", &p),
        ("marker-program.json", &marker),
        ("capabilities.json", &caps),
        ("marker-selection.json", &report),
    ] {
        let path = dir.join(file);
        write(&path, value)?;
        files.insert(file, sha(&fs::read(path)?));
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
struct Inputs {
    source: String,
    training: Option<String>,
    catalog: Option<J>,
}
fn suite(name: &str) -> Result<Inputs> {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../workloads/pass3");
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
    let catalog = case
        .get("catalog")
        .map(|item| -> Result<J> {
            let bytes = fs::read(base.join(item["file"].as_str().ok_or("invalid catalog path")?))?;
            if item["sha256"] != sha(&bytes) {
                return Err("frozen catalog checksum mismatch".into());
            }
            Ok(serde_json::from_slice(&bytes)?)
        })
        .transpose()?;
    Ok(Inputs {
        source: input(case)?,
        training: Some(input(&spec["bake"])?),
        catalog,
    })
}
fn inputs(opts: &BTreeMap<String, String>) -> Result<Inputs> {
    let mut input = match (
        opts.get("--source"),
        opts.get("--case"),
        opts.get("--bake-source"),
    ) {
        (Some(src), None, b) => Inputs {
            source: fs::read_to_string(src)?,
            training: b.map(fs::read_to_string).transpose()?,
            catalog: None,
        },
        (None, Some(case), None) => suite(case)?,
        _ => {
            return Err("use --case NAME, or --source PATH with optional --bake-source PATH".into());
        }
    };
    if let Some(path) = opts.get("--catalog") {
        input.catalog = Some(read(Path::new(path))?);
    }
    Ok(input)
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
    if cmd == "iterate" && args.iter().any(|a| a == "--help") {
        println!(
            "cargo cell-iterate --program SEED --output NEW_DIR [--rounds N] [--objective copies|rhs-lookups] [--max-depth N] [--max-nodes N] [--new-per-round N] [--max-units N] [--candidate-slots N] [--min-support N]"
        );
        return Ok(());
    }
    if cmd == "help" || args.iter().any(|a| a == "--help") {
        println!(
            "cargo cell-compile (--case NAME | --source FILE [--bake-source FILE]) --output DIR [--catalog FILE] [--profile FILE] [--marker-locality identity|all]\ncargo cell-bench (--case NAME | --source FILE | --program FILE) [--repeat N] [--reference-program FILE]\ncargo cell-iterate --program SEED --output DIR --rounds N"
        );
        return Ok(());
    }
    egglog::EGraph::set_num_threads(1);
    match cmd.as_str() {
        #[cfg(feature = "iterative-bake")]
        "iterate" => iterate::run(args),
        "internal-bake" if args.len() == 2 => bake(Path::new(&args[0]), Path::new(&args[1])),
        "compile" => {
            let opts = options(
                args.into_iter(),
                &[
                    "--source",
                    "--case",
                    "--output",
                    "--bake-source",
                    "--catalog",
                    "--profile",
                    "--marker-locality",
                ],
            )?;
            let input = inputs(&opts)?;
            let out = Path::new(opts.get("--output").ok_or("missing --output")?);
            compile(
                &input.source,
                input.training.as_deref(),
                input.catalog.as_ref(),
                out,
                &opts,
            )?;
            println!("{}", out.join("program.json").display());
            Ok(())
        }
        "internal-stages" if args.len() == 1 => bench::stages(Path::new(&args[0])),
        "bench" => bench::run(args),

        _ => Err(format!("invalid invocation {cmd}").into()),
    }
}
