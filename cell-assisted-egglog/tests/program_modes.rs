use std::{
    path::{Path, PathBuf},
    process::Command,
};
fn folder() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let p = std::env::temp_dir().join(format!(
        "cell-program-modes-{}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&p).unwrap();
    p
}
fn invoke(source: &Path, out: &Path, backend: &str, extra: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_cell-assisted-egglog"))
        .args(["--mode", "program", "--backend", backend, "--source"])
        .arg(source)
        .arg("--output")
        .arg(out)
        .args(extra)
        .output()
        .unwrap()
}
#[test]
fn complete_program_and_failed_check_are_reported_separately() {
    let dir = folder();
    let source = dir.join("source.egg");
    std::fs::write(&source,"(datatype E (N i64)) (function f (E) i64 :merge (min old new)) (ruleset s) (rule ((= x (N n))) ((set (f x) n)) :ruleset s) (N 3) (run s 1) (check (= (f (N 3)) 3))").unwrap();
    let out = dir.join("native");
    let result = invoke(&source, &out, "native", &[]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["backend"], "native-fork");
    assert_eq!(report["complete"], true);
    assert_eq!(report["snapshot_complete"], true);
    std::fs::write(&source, "(check (= 1 2))").unwrap();
    let bad = dir.join("bad");
    assert!(!invoke(&source, &bad, "native", &[]).status.success());
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(bad.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["complete"], false);
    assert!(report["error"].is_string());
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn unverified_binary_cannot_be_called_pristine() {
    let dir = folder();
    let source = dir.join("source.egg");
    std::fs::write(&source, "").unwrap();
    let runner = dir.join("fake");
    std::fs::write(&runner, "not the baseline").unwrap();
    std::fs::write(
        runner.with_extension("json"),
        r#"{"binary_sha256":"invalid"}"#,
    )
    .unwrap();
    let r = invoke(
        &source,
        &dir.join("out"),
        "pristine",
        &["--pristine-runner", runner.to_str().unwrap()],
    );
    assert!(!r.status.success());
    assert!(String::from_utf8_lossy(&r.stderr).contains("fingerprint mismatch"));
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn gpu_program_preserves_post_schedule_checks() {
    let Some(worker) = std::env::var_os("CELL_MATRIX_WORKER") else {
        return;
    };
    let dir = folder();
    let source = dir.join("source.egg");
    std::fs::write(&source,"(relation P (i64)) (relation Q (i64)) (ruleset s) (rule ((P x)) ((Q (+ x 1))) :ruleset s) (P 7) (run s 1) (check (Q 8)) (fail (check (Q 9)))").unwrap();
    let r = invoke(
        &source,
        &dir.join("out"),
        "gpu",
        &[
            "--gpu-worker",
            worker.to_str().unwrap(),
            "--query-audit",
            "on",
        ],
    );
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cache_preflight_uses_native_parser_and_does_not_read_includes() {
    let dir = folder();
    let source = dir.join("source.egg");
    std::fs::write(
        &source,
        "; include in a comment is harmless\n(datatype E (N i64))",
    )
    .unwrap();
    let r = invoke(&source, &dir.join("unused"), "inspect", &[]);
    assert!(r.status.success());
    let v: serde_json::Value = serde_json::from_slice(&r.stdout).unwrap();
    assert_eq!(v["cache_safe"], true);
    std::fs::write(&source, "(include \"missing.egg\")").unwrap();
    let r = invoke(&source, &dir.join("unused"), "inspect", &[]);
    assert!(r.status.success());
    let v: serde_json::Value = serde_json::from_slice(&r.stdout).unwrap();
    assert_eq!(v["cache_safe"], false);
    assert!(!dir.join("unused").exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn explicit_native_helpers_keep_the_same_source_schedule() {
    let Some(worker) = std::env::var_os("CELL_MATRIX_WORKER") else {
        return;
    };
    let dir = folder();
    let source = dir.join("source.egg");
    std::fs::write(&source,"(relation P (i64)) (relation Q (i64)) (relation R (i64)) (ruleset helper) (ruleset consumer) (rule ((P x)) ((Q (+ x 1))) :ruleset helper) (rule ((Q x)) ((R (+ x 1))) :ruleset consumer) (P 7) (run-schedule (seq (run helper) (run consumer))) (check (R 9)) (fail (check (R 10)))").unwrap();
    let out = dir.join("out");
    let r = invoke(
        &source,
        &out,
        "gpu",
        &[
            "--gpu-worker",
            worker.to_str().unwrap(),
            "--query-audit",
            "on",
            "--native-rulesets",
            "helper",
        ],
    );
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("report.json")).unwrap()).unwrap();
    assert_eq!(
        report["explicit_native_rulesets"],
        serde_json::json!(["helper"])
    );
    let rules = report["runtime"]["rules"].as_array().unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0]["ruleset"], "consumer");
    let bad = invoke(
        &source,
        &dir.join("bad"),
        "gpu",
        &[
            "--gpu-worker",
            worker.to_str().unwrap(),
            "--native-rulesets",
            "unknown",
        ],
    );
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("unknown explicit native ruleset"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn explicit_source_attaches_cell_closures_without_reinstalling_keys() {
    let Some(worker) = std::env::var_os("CELL_MATRIX_WORKER") else { return };
    let dir=folder();let source=dir.join("source.egg");
    let text=format!("; cell-structural-key-runtime/v1 {{\"nodes\":[],\"bases\":[]}}\n{}\n\
        (ruleset cell-stable-keys) (ruleset cell-stable-freeze)\n\
        (set (ExtractedExpr (V 0)) (TCPair (TermLeaf 0) 2))\n\
        (set (ExtractedExpr (V 1)) (TCPair (TermLeaf 1) 3))\n\
        (Bop (Add) (V 0) (V 1))\n{}\n\
        (check (= (ExtractedExpr (Bop (Add) (V 0) (V 1))) (TCPair (TermBop (Add) (TermLeaf 0) (TermLeaf 1)) 6)))",
        include_str!("../../experiments/parameterized_cell/terms.egg"),
        cell_assisted_egglog::parameterized_cell::joint::SCHEDULE);
    std::fs::write(&source,text).unwrap();
    let out=dir.join("assisted");
    let args=["--gpu-worker",worker.to_str().unwrap(),"--cell-assistance","on","--native-rulesets","cell-stable-keys,cell-stable-freeze"];
    let r=invoke(&source,&out,"gpu",&args);
    assert!(r.status.success(),"{}",String::from_utf8_lossy(&r.stderr));
    let report:serde_json::Value=serde_json::from_slice(&std::fs::read(out.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["cell_assistance"],true);
    assert_eq!(report["source_rewritten_by_runner"],false);
    assert!(report["runtime"]["runtime"]["skipped_saturations"].as_u64().unwrap()>0);
    assert!(report["runtime"]["saturation_diagnostics"]["joint"]["stats"]["macro_instances"].as_u64().unwrap()>0);
    let mut duplicate=args.to_vec();duplicate.extend(["--setup-policy","legacy-stable"]);
    let bad=invoke(&source,&dir.join("duplicate"),"gpu",&duplicate);
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("cannot also install legacy"));
    std::fs::remove_dir_all(dir).unwrap();
}
