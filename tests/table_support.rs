use serde_json::Value;
use std::{fs, path::Path, process::Command};
fn read(p: impl AsRef<Path>) -> Value {
    serde_json::from_slice(&fs::read(p).unwrap()).unwrap()
}
fn run(src: &str, out: &Path, args: &[&str]) {
    let source = out.with_extension("egg");
    fs::write(&source, src).unwrap();
    let p = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args(args)
        .arg(&source)
        .arg(out)
        .output()
        .unwrap();
    assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
}
#[test]
fn relation_facts_are_part_of_closed_state() {
    let base = std::env::temp_dir().join(format!("table-ripen-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    let text = "(datatype E (A) (B))\n(relation Marked (E))\n(rule ((= a (A))) ((Marked a)))\n(A)\n(check (Marked (A)))";
    run(text, &base.join("relation"), &["ripen"]);
    let s = read(base.join("relation/closed-state.json"));
    assert!(
        s["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["op"] == "Marked")
    );
    assert_eq!(
        read(base.join("relation/ripen.json"))["tables"]["Marked"],
        1
    );
    let function = "(datatype E (A))\n(function score (E) i64 :merge (max old new))\n(rule ((= a (A))) ((set (score a) 7)))\n(A)\n(check (= (score (A)) 7))";
    run(function, &base.join("function"), &["ripen"]);
    let r = read(base.join("function/ripen.json"));
    assert_eq!(r["ripen"]["state"], "Closed");
    assert_eq!(r["tables"]["score"], 1);
    assert_eq!(r["closed_state"]["status"], "unavailable");
    let no_merge = function.replace(":merge (max old new)", ":no-merge");
    run(&no_merge, &base.join("no-merge"), &["ripen"]);
    let state = read(base.join("no-merge/closed-state.json"));
    assert!(
        state["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["op"] == "score")
    );
    fs::remove_dir_all(base).unwrap();
}
#[test]
fn pure_tables_execute_and_history_replays() {
    let base = std::env::temp_dir().join(format!("table-capture-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    let source = base.join("tables.egg");
    fs::write(&source,"(relation P (i64))\n(relation Q (i64))\n(function score (i64) i64 :merge (max old new))\n(P 2)\n(rule ((P x)) ((Q x) (set (score x) (+ x 1))))\n(run 2)\n(check (Q 2))\n(check (= (score 2) 3))").unwrap();
    let out = base.join("capture");
    let r = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args(["analyze", "--recapture-tier0", "--save-history", "--source"])
        .arg(&source)
        .arg("--output")
        .arg(&out)
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    assert!(
        read(out.join("history.json"))["datatype"]
            .as_str()
            .unwrap()
            .contains("(relation")
    );
    let r = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .args(["analyze", "--replay-history"])
        .arg(out.join("history.json"))
        .arg("--output")
        .arg(base.join("replay"))
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn automatic_entries_keep_relation_declarations_and_scalar_bindings() {
    let base = std::env::temp_dir().join(format!("table-auto-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    let source = base.join("input.egg");
    let mut text="(datatype Expr (Num i64) (Add Expr Expr) (Seed i64))\n(relation Marked (Expr))\n(rule ((= s (Seed n))) ((Add (Num n) (Num n))))\n(rule ((= e (Add a b))) ((Marked e)))\n".to_string();
    for i in 1..=8 {
        text += &format!("(Seed {i})\n");
    }
    text += "(run 3)";
    fs::write(&source, text).unwrap();
    let out = base.join("run");
    let r = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .env("EGG_LAYOUT_RIPEN_MILLISECONDS", "100000")
        .args(["analyze", "--recapture-tier0", "--save-history", "--source"])
        .arg(&source)
        .arg("--output")
        .arg(&out)
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let q = read(out.join("closed/queue.json"));
    assert_eq!(q["counts"]["Rejected"], Value::Null);
    assert!(q["counts"]["Closed"].as_u64().unwrap() > 0);
    assert!(q["catalog"]["states"].as_array().unwrap().iter().all(|s| {
        s["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["op"] == "Marked")
    }));
    let r = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .env("EGG_LAYOUT_RIPEN_MILLISECONDS", "100000")
        .args(["analyze", "--replay-history"])
        .arg(out.join("history.json"))
        .arg("--output")
        .arg(base.join("replay"))
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    assert_eq!(
        read(base.join("replay/closed/queue.json"))["counts"],
        q["counts"]
    );
    fs::remove_dir_all(base).unwrap();
}
