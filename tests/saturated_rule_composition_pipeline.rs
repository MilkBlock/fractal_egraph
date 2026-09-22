use serde_json::Value;
use std::{fs, path::Path, process::Command};
fn read(p: impl AsRef<Path>) -> Value {
    serde_json::from_slice(&fs::read(p).unwrap()).unwrap()
}
fn source(base: &Path) -> std::path::PathBuf {
    fs::create_dir_all(base).unwrap();
    let p = base.join("input.egg");
    let mut s = String::from(
        "(datatype Expr (V String) (A Expr) (B Expr) (C Expr) (D Expr))\n(rewrite (A x) (B x))\n(rewrite (B x) (C x))\n(rewrite (C x) (D x))\n",
    );
    for i in 0..16 {
        s += &format!("(A (V \"v{i}\"))\n");
    }
    s += "(run 5)\n";
    fs::write(&p, s).unwrap();
    p
}
fn command() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_egg_layout"));
    c.env("EGG_LAYOUT_RIPEN_MILLISECONDS", "100000")
        .env("EGG_LAYOUT_RIPEN_PER_BOUNDARY", "8");
    c
}
#[test]
fn normal_capture_and_debug_stream_include_closed_without_trace_files() {
    let base = std::env::temp_dir().join(format!("closed-auto-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let src = source(&base);
    let out = base.join("analyze");
    let r = command()
        .args(["analyze", "--recapture-tier0", "--source"])
        .arg(&src)
        .arg("--output")
        .arg(&out)
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    assert!(!out.join("history.json").exists());
    let queue = read(out.join("saturated-rule-composition/queue.json"));
    assert!(
        queue["counts"]["Saturated"].as_u64().unwrap_or(0) > 0,
        "{queue}"
    );
    assert!(
        queue["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|j| j["cache_hit"] == true),
        "exact interface cache should be reused"
    );
    let catalog = read(out.join("catalog/catalog.json"));
    assert!(catalog["saturated_rule_compositions"].as_u64().unwrap() > 0);
    for e in fs::read_dir(out.join("saturated-rule-composition/cells")).unwrap() {
        let p = e.unwrap().path();
        assert!(!p.join("history.json").exists());
        assert!(!p.join("work").exists());
    }
    let frames = read(out.join("rounds/manifest.json"));
    let last = frames.as_array().unwrap().last().unwrap()["stem"]
        .as_str()
        .unwrap();
    assert_eq!(
        read(out.join(format!("rounds/{last}.json")))["saturated_rule_composition"],
        queue
    );
    assert!(out.join(format!("rounds/{last}.saturated_rule_composition.dot")).exists());
    let offline = base.join("offline");
    let r = command()
        .args([
            "analyze",
            "--recapture-tier0",
            "--build",
            "offline",
            "--source",
        ])
        .arg(&src)
        .arg("--output")
        .arg(&offline)
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let offline_queue = read(offline.join("saturated-rule-composition/queue.json"));
    assert_eq!(offline_queue["counts"], queue["counts"]);
    assert_eq!(
        offline_queue["catalog"]["states"],
        queue["catalog"]["states"]
    );
    let statuses = |q: &Value| {
        q["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|j| {
                (
                    j["use_id"].clone(),
                    j["state"].clone(),
                    j["cache_hit"].clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(statuses(&offline_queue), statuses(&queue));
    let stream = base.join("stream");
    let r = command()
        .env("EGG_LAYOUT_SATURATED_RULE_COMPOSITION_OUTPUT", &stream)
        .arg("debug-stream")
        .arg(&src)
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let rows: Vec<Value> = String::from_utf8(r.stdout)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    let snapshots: Vec<_> = rows
        .iter()
        .filter(|r| r["kind"] == "layer_snapshot")
        .collect();
    assert!(!snapshots.is_empty());
    assert_eq!(
        snapshots.last().unwrap()["saturated_rule_composition"]["counts"],
        queue["counts"]
    );
    assert!(snapshots.last().unwrap()["saturated_rule_composition"]["catalog"].is_object());
    fs::remove_dir_all(base).unwrap();
}
#[test]
fn budgets_preserve_pending_and_suspended_as_distinct_states() {
    let base = std::env::temp_dir().join(format!("closed-budget-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let src = source(&base);
    let out = base.join("run");
    let r = command()
        .env("EGG_LAYOUT_RIPEN_JOBS", "1")
        .env("EGG_LAYOUT_RIPEN_ROUNDS", "1")
        .env("EGG_LAYOUT_SATURATED_RULE_COMPOSITION_OUTPUT", &out)
        .arg("debug-stream")
        .arg(src)
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let q = read(out.join("saturated-rule-composition/queue.json"));
    assert!(q["counts"]["Pending"].as_u64().unwrap_or(0) > 0, "{q}");
    assert_eq!(q["counts"]["Suspended"], 1, "{q}");
    assert!(q["catalog"].is_null());
    assert!(!out.join("catalog").exists());
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn standalone_debug_stream_ripens_without_output_environment() {
    let base=std::env::temp_dir().join(format!("default-ripen-{}",std::process::id()));
    fs::create_dir_all(&base).unwrap();let src=source(&base);
    let r=command().env_remove("EGG_LAYOUT_SATURATED_RULE_COMPOSITION_OUTPUT")
        .env("EGG_LAYOUT_RIPEN_JOBS","8").arg("debug-stream").arg(src).output().unwrap();
    assert!(r.status.success(),"{}",String::from_utf8_lossy(&r.stderr));
    let frames:Vec<Value>=String::from_utf8(r.stdout).unwrap().lines().filter_map(|l|serde_json::from_str::<Value>(l).ok()).filter(|v|v.get("saturated_rule_composition").is_some()).collect();
    let last=frames.last().expect("default ripen frames");
    assert!(last["saturated_rule_composition"]["counts"]["Saturated"].as_u64().unwrap_or(0)>0);
    let out=std::path::PathBuf::from(last["ripen_output_directory"].as_str().unwrap());
    assert!(out.starts_with(Path::new(env!("CARGO_MANIFEST_DIR")).join("out/debug-stream")));
    assert!(out.join("catalog/catalog.json").is_file());
    fs::remove_dir_all(out).unwrap();fs::remove_dir_all(base).unwrap();
}

#[test]
fn coarse_without_observed_smooth_is_ripened() {
    let base=std::env::temp_dir().join(format!("coarse-first-ripen-{}",std::process::id()));fs::create_dir_all(&base).unwrap();
    let src=base.join("input.egg");fs::write(&src,"(datatype E (V) (A E) (B E))\n(rewrite (A x) (B x))\n(A (V))\n(run 1)\n").unwrap();
    let out=base.join("run");if out.exists(){fs::remove_dir_all(&out).unwrap();}
    let r=command().env("EGG_LAYOUT_SATURATED_RULE_COMPOSITION_OUTPUT",&out).arg("debug-stream").arg(src).output().unwrap();assert!(r.status.success(),"{}",String::from_utf8_lossy(&r.stderr));
    let q=read(out.join("saturated-rule-composition/queue.json"));
    let job=q["jobs"].as_array().unwrap().iter().find(|j|j["candidate_kind"]=="CSUnit"&&j["state"]=="Saturated").expect("coarse entry should ripen before tier0 records smooth consequences");
    let id=job["candidate_id"].as_u64().unwrap() as usize;assert!(q["cs"]["units"][id]["smooth"].as_array().unwrap().is_empty());
    assert_eq!(q["limits"]["dependency_slots"]["CSCS"],32);
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn cscs_has_reserved_admission_under_math_candidate_pressure() {
    let base=std::env::temp_dir().join(format!("ripen-fair-admission-{}",std::process::id()));
    if base.exists(){fs::remove_dir_all(&base).unwrap();}
    let src=Path::new(env!("CARGO_MANIFEST_DIR")).join("egglog/tests/math-microbenchmark.egg");
    let r=command().env("EGG_LAYOUT_RIPEN_JOBS","16").env("EGG_LAYOUT_RIPEN_PER_BOUNDARY","16")
        .args(["analyze","--recapture-tier0","--source"]).arg(src).args(["--rounds","4","--output"]).arg(&base).output().unwrap();
    assert!(r.status.success(),"{}",String::from_utf8_lossy(&r.stderr));
    let q=read(base.join("saturated-rule-composition/queue.json"));let jobs=q["jobs"].as_array().unwrap();
    assert_eq!(jobs.iter().filter(|j|j["candidate_kind"]=="CSUnit").count(),64);
    assert!(jobs.iter().any(|j|j["candidate_kind"]=="CSCS"),"CSCS was starved before entering the queue");
    assert!(jobs.iter().filter(|j|j["candidate_kind"]=="CCSS").count()<=32);
    fs::remove_dir_all(base).unwrap();
}
