use serde_json::Value;
use std::{path::PathBuf, process::Command};

fn analyze(source: &str, windows: bool) -> (PathBuf, Value) {
    let dir=std::env::temp_dir().join(format!("cell-windows-{}-{}",std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("source.egg"),source).unwrap();
    let result=Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .arg("analyze").arg("--source").arg(dir.join("source.egg"))
        .arg("--output").arg(dir.join("run"))
        .env("EGG_LAYOUT_CELL_WINDOWS",if windows {"1"}else{"0"})
        .env("EGG_LAYOUT_GRAPHS","0").env("EGG_LAYOUT_ANALYSIS_THREADS","1")
        .env("EGG_LAYOUT_RIPEN_OBSERVE_MS","500").env("EGG_LAYOUT_RIPEN_OBSERVE_FACTS","200000")
        .env_remove("EGG_LAYOUT_COMPOSITION_DISCOVERY")
        .output().unwrap();
    assert!(result.status.success(),"{}",String::from_utf8_lossy(&result.stderr));
    let report=serde_json::from_slice(&std::fs::read(dir.join("run/analysis.json")).unwrap()).unwrap();
    (dir,report)
}

#[test]
fn shifted_windows_are_independently_certified_and_share_existing_closures() {
    let source=include_str!("../experiments/cell_windows/chain.egg");
    let (dir,report)=analyze(source,true);
    let q=&report["saturated_rule_composition"];
    let windows=q["cs"]["windows"].as_array().unwrap();
    assert!(windows.iter().any(|w|w["new_entry"]==true && w["kind"]=="producer_consumer"),"{q:#}");
    // At most the two semantic bases per mutation, no parent-subset/all-path enumeration.
    let baseline=q["cs"]["compositions"].as_array().unwrap().len();
    assert!(windows.len()<=baseline*2);
    let jobs=q["jobs"].as_array().unwrap();
    let local=jobs.iter().filter(|j|j["candidate_kind"]=="HistoryWindow").collect::<Vec<_>>();
    assert!(!local.is_empty());
    assert!(local.iter().any(|j|j["state"]=="Saturated"),"{local:#?}");
    assert!(local.iter().any(|j| {
        let ms=j["origin"]["comb_members"].as_array().unwrap();
        ms.len()==2 && ms[0]["rule_name"]=="b-c" && ms[1]["rule_name"]=="c-done"
    }),"a middle B/C window must not pull D/A back into its entry");
    let reuse=&q["cell_egraph"]["reuse_value"];
    assert!(reuse["cells"].as_array().unwrap().iter().any(|r|
        r["new_window_entries"].as_u64().unwrap()>0 &&
        r["registered_constructions"].as_u64().unwrap()>r["new_window_entries"].as_u64().unwrap() &&
        r["interface_instances"].as_u64().unwrap()>=3),"{reuse:#}");
    for r in reuse["cells"].as_array().unwrap() {
        assert!(r["interface_instances"].as_u64().unwrap()<=r["registered_constructions"].as_u64().unwrap());
    }
    std::fs::remove_dir_all(dir).unwrap();
    let (dir,old)=analyze(source,false);
    assert!(old["saturated_rule_composition"]["cs"]["windows"].as_array().unwrap().is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn multi_producer_window_keeps_all_antecedents() {
    let (dir,report)=analyze(r#"
(datatype Term (V i64) (A Term) (B Term) (P Term) (Q Term) (Done Term))
(rule ((A x)) ((P x)) :name "a-p")
(rule ((B x)) ((Q x)) :name "b-q")
(rule ((P x) (Q x)) ((Done x)) :name "join")
(A (V 0)) (B (V 0)) (run 4)
(check (Done (V 0)))
"#,true);
    let q=&report["saturated_rule_composition"];
    assert!(q["cs"]["windows"].as_array().unwrap().iter().any(|w|
        w["kind"]=="producer_consumer" && w["members"].as_array().unwrap().len()==3),"{q:#}");
    std::fs::remove_dir_all(dir).unwrap();
}
