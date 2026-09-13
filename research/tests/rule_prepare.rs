use egglog::EGraph;
#[test]
fn every_prepared_optimizer_candidate_typechecks_natively() {
    let path=concat!(env!("CARGO_MANIFEST_DIR"),"/../experiments/native_programs/simplify.egg");
    let result=std::process::Command::new(env!("CARGO_BIN_EXE_rule_prepare")).arg(path).output().unwrap();
    assert!(result.status.success(),"{}",String::from_utf8_lossy(&result.stderr));
    let data:serde_json::Value=serde_json::from_slice(&result.stdout).unwrap();
    let mut eg=EGraph::default();
    eg.parse_and_run_program(None,include_str!("../../experiments/native_programs/simplify.egg")).unwrap();
    let mut count=0;
    let mut cross_level=false;
    for batch in data["batches"].as_array().unwrap() {
        for c in batch["candidates"].as_array().unwrap() {
            eg.parse_and_run_program(None,c["command"].as_str().unwrap()).unwrap();
            cross_level |= c["producer"]=="mul-fold" && c["consumer"]=="add-fold";
            count+=1;
        }
    }
    assert!(count>0 && cross_level);
    eg.parse_and_run_program(None,"(run simplify 10) (check (= seed (Const 26)))").unwrap();
}
