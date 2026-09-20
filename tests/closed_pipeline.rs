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
    let queue = read(out.join("closed/queue.json"));
    assert!(
        queue["counts"]["Closed"].as_u64().unwrap_or(0) > 0,
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
    assert!(catalog["closed_states"].as_u64().unwrap() > 0);
    for e in fs::read_dir(out.join("closed/cells")).unwrap() {
        let p = e.unwrap().path();
        assert!(!p.join("history.json").exists());
        assert!(!p.join("work").exists());
    }
    let frames = read(out.join("rounds/manifest.json"));
    let last = frames.as_array().unwrap().last().unwrap()["stem"]
        .as_str()
        .unwrap();
    assert_eq!(
        read(out.join(format!("rounds/{last}.json")))["closed"],
        queue
    );
    assert!(out.join(format!("rounds/{last}.closed.dot")).exists());
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
    let offline_queue = read(offline.join("closed/queue.json"));
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
        .env("EGG_LAYOUT_CLOSED_OUTPUT", &stream)
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
        snapshots.last().unwrap()["closed"]["counts"],
        queue["counts"]
    );
    assert!(snapshots.last().unwrap()["closed"]["catalog"].is_object());
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
        .env("EGG_LAYOUT_CLOSED_OUTPUT", &out)
        .arg("debug-stream")
        .arg(src)
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let q = read(out.join("closed/queue.json"));
    assert!(q["counts"]["Pending"].as_u64().unwrap_or(0) > 0, "{q}");
    assert_eq!(q["counts"]["Suspended"], 1, "{q}");
    assert!(q["catalog"].is_null());
    assert!(!out.join("catalog").exists());
    fs::remove_dir_all(base).unwrap();
}
