use cell_assisted_egglog::table_live::{Runtime,Options};
use egglog::EGraph;
use serde_json::json;
use std::path::{Path,PathBuf};
#[allow(dead_code)]
#[path="../../examples/support/live_table_bench.rs"] mod comparison;
fn count(r:&Runtime,name:&str)->u64{r.report()["cell_work"][name].as_u64().unwrap_or(0)}
fn setup()->Option<(EGraph,Runtime,Runtime)>{
    let worker=std::env::var_os("CELL_MATRIX_WORKER").map(PathBuf::from)?;
    let env=include_str!("../../experiments/commit_spine_markers/environment.egg");
    let mut eg=EGraph::default();eg.parse_and_run_program(None,env).unwrap();
    let commands=eg.parse_program(None,"(run-schedule (seq make-get make-box consume))").unwrap();
    let lib=Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/commit-markers");
    let r=Runtime::new_full_configured(eg.clone(),&commands,Some(&worker),true,None,None,Options{analysis:Some(&lib),profile:true,..Default::default()}).unwrap();
    let cpu=Runtime::new_full(eg.clone(),&commands,None,false,None).unwrap();
    Some((eg,r,cpu))
}
fn input(r:&mut Runtime,cpu:&mut Runtime,text:&str){r.input(text).unwrap();cpu.input(text).unwrap();}
fn run(base:&EGraph,r:&mut Runtime,cpu:&mut Runtime,s:&str){
    let cmds=EGraph::default().parse_program(None,s).unwrap();r.run(&cmds).unwrap();cpu.run(&cmds).unwrap();
    comparison::equal(base,cpu.engine(),r.engine()).unwrap();
}
#[test]
fn committed_receipts_seed_and_extend_spines_without_repeating_prefix_queries(){
    let Some((base,mut r,mut cpu))=setup() else{return};
    let enabled=std::env::var("CELL_COMMIT_MARKERS").as_deref()==Ok("1");
    let mut stages=vec![];
    input(&mut r,&mut cpu,"(Single (Leaf 19))");
    run(&base,&mut r,&mut cpu,"(run make-get 1)");
    run(&base,&mut r,&mut cpu,"(run consume 1)");
    stages.push(json!({"phase":"Get committed; Box absent","report":r.report()}));
    let before=count(&r,"certification_probes");
    run(&base,&mut r,&mut cpu,"(run make-box 1)");
    run(&base,&mut r,&mut cpu,"(run consume 1)");
    r.engine().clone().parse_and_run_program(None,"(check (Short (Single (Leaf 19)) (Leaf 19))) (check (Long (Single (Leaf 19)) (Box (Leaf 19))))").unwrap();
    stages.push(json!({"phase":"Box committed; previous prefix extended","report":r.report()}));
    assert_eq!(count(&r,"certification_probes"),before,"late committed Box must resume from saved row witnesses");
    if enabled{
        assert!(r.report()["commit_markers"]["emitted_hints"].as_u64().unwrap()>0);
        let fields=r.report()["cell_plan_fields"].as_array().unwrap().iter().map(|x|x.as_str().unwrap().to_string()).collect::<Vec<_>>();
        let report=r.report();let plans=report["cells"]["plans"].as_array().unwrap();
        let mut short_used=false;
        for p in plans {if p["internal_joins"]==1 {
            let counts=&report["cell_plan_work"][p["plan"].to_string()];
            if counts[fields.iter().position(|n|n=="receipt_accepted").unwrap()].as_u64().unwrap_or(0)>0 {
                short_used=true;assert_eq!(counts[fields.iter().position(|n|n=="probes").unwrap()],0,"fully supplied spine must do zero certification index reads");
            }
        }}
        assert!(short_used,"a real short spine must consume a receipt");
    }
    // Invalidate a suffix, introduce an alias and then establish a new witness.
    input(&mut r,&mut cpu,"(subsume (Box (Leaf 19))) (Single (Leaf 23))");
    run(&base,&mut r,&mut cpu,"(run consume 1)");
    input(&mut r,&mut cpu,"(union (Leaf 19) (Leaf 23))");
    run(&base,&mut r,&mut cpu,"(run-schedule (seq make-get make-box consume))");
    stages.push(json!({"phase":"subsume and union","report":r.report()}));
    if let Some(out)=std::env::var_os("CELL_MARKER_REPORT"){
        std::fs::write(out,serde_json::to_vec_pretty(&json!({"enabled":enabled,"scope":"actual native commit/GPU matcher; complete native graph checked at every stage","stages":stages})).unwrap()).unwrap();
    }
}
#[test]
fn an_old_receipt_cannot_resurrect_a_subsumed_row_after_commit(){
    let Some((base,mut r,mut cpu))=setup() else{return};
    input(&mut r,&mut cpu,"(Single (Leaf 8))");
    run(&base,&mut r,&mut cpu,"(run make-get 1)");
    // Receipt is queued but has not been uploaded; this later commit invalidates it.
    input(&mut r,&mut cpu,"(subsume (Get (Single (Leaf 8)) 0))");
    run(&base,&mut r,&mut cpu,"(run consume 1)");
    r.engine().clone().parse_and_run_program(None,"(fail (check (Short (Single (Leaf 8)) (Leaf 8))))").unwrap();
    if std::env::var("CELL_COMMIT_MARKERS").as_deref()==Ok("1") {
        assert!(r.report()["cell_plan_work"].as_object().unwrap().values().any(|v|{
            let fields=r.report()["cell_plan_fields"].as_array().unwrap().clone();
            v[fields.iter().position(|f|f=="receipt_rejected").unwrap()].as_u64().unwrap_or(0)>0
        }));
    }
}

#[test]
fn merge_proposals_are_not_mistaken_for_the_committed_function_value(){
    let Some(worker)=std::env::var_os("CELL_MATRIX_WORKER").map(PathBuf::from) else{return};
    let mut eg=EGraph::default();
    let source=include_str!("../fixtures/function-update/source.egg");
    eg.parse_and_run_program(None,source).unwrap();
    eg.parse_and_run_program(None,r#"(relation BestSeen (Expr i64)) (rule ((Proposal dst src cost) (= old (Best dst))) ((BestSeen dst old)) :name "read-best")"#).unwrap();
    eg.parse_and_run_program(None,r#"(relation SpineSeen (Expr Expr)) (rule ((= s (Single x)) (= y (Get s 0))) ((SpineSeen s y)) :name "read-spine")"#).unwrap();
    let base=eg.clone();let cmds=eg.parse_program(None,"(run 1)").unwrap();
    let lib=Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/function-update/analyze");
    let mut r=Runtime::new_full_configured(eg.clone(),&cmds,Some(&worker),true,None,None,Options{analysis:Some(&lib),profile:true,..Default::default()}).unwrap();
    let mut cpu=Runtime::new_full(eg,&cmds,None,false,None).unwrap();
    input(&mut r,&mut cpu,"(Proposal (Leaf 10) (Single (Leaf 1)) 20)");
    run(&base,&mut r,&mut cpu,"(run 1)");
    run(&base,&mut r,&mut cpu,"(run 1)");
    r.engine().clone().parse_and_run_program(None,"(check (= (Best (Leaf 10)) 3))").unwrap();
    if std::env::var("CELL_COMMIT_MARKERS").as_deref()==Ok("1"){
        assert!(r.report()["commit_markers"]["rejected_rows"].as_u64().unwrap()>0,"min kept 3; proposed 20 is not a committed row: markers={} library={} plans={}",r.report()["commit_markers"],r.report()["cells"]["library"],r.report()["cells"]["plans"]);
    }
}
#[test]
fn failed_actions_never_publish_a_receipt(){
    let Some(worker)=std::env::var_os("CELL_MATRIX_WORKER").map(PathBuf::from) else{return};
    let mut eg=EGraph::default();
    eg.parse_and_run_program(None,r#"
      (datatype Expr (Leaf i64) (Single Expr) (Get Expr i64))
      (relation Seen (Expr)) (ruleset bad) (ruleset read)
      (rule ((= s (Single x))) ((union (Get s 0) x) (panic "stop")) :ruleset bad)
      (rule ((= s (Single x)) (= x (Get s 0))) ((Seen s)) :ruleset read)
    "#).unwrap();
    let cmds=eg.parse_program(None,"(run-schedule (seq bad read))").unwrap();
    let lib=Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/analyze");
    let mut r=Runtime::new_full_configured(eg,&cmds,Some(&worker),true,None,None,Options{analysis:Some(&lib),profile:true,..Default::default()}).unwrap();
    r.input("(Single (Leaf 1))").unwrap();assert!(r.run(&cmds).is_err());assert!(r.run(&cmds).is_err());
    if std::env::var("CELL_COMMIT_MARKERS").as_deref()==Ok("1"){assert_eq!(r.report()["commit_markers"]["emitted_hints"],0);}
}
#[test]
fn duplicate_effects_can_confirm_a_spine_without_extending_saturation(){
    let Some(worker)=std::env::var_os("CELL_MATRIX_WORKER").map(PathBuf::from) else{return};
    let mut eg=EGraph::default();eg.parse_and_run_program(None,include_str!("../../experiments/commit_spine_markers/environment.egg")).unwrap();
    eg.parse_and_run_program(None,"(ruleset repeat-get) (rule ((= s (Single x))) ((union (Get s 0) x)) :ruleset repeat-get)").unwrap();
    let base=eg.clone();let cmds=eg.parse_program(None,"(run-schedule (seq make-get consume repeat-get))").unwrap();
    let lib=Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/commit-markers");
    let mut r=Runtime::new_full_configured(eg.clone(),&cmds,Some(&worker),true,None,None,Options{analysis:Some(&lib),profile:true,..Default::default()}).unwrap();
    let mut cpu=Runtime::new_full(eg,&cmds,None,false,None).unwrap();
    input(&mut r,&mut cpu,"(Single (Leaf 1))");
    run(&base,&mut r,&mut cpu,"(run-schedule (seq make-get consume))");
    let before=r.stats.ruleset_runs;
    run(&base,&mut r,&mut cpu,"(run-schedule (saturate repeat-get))");
    assert_eq!(r.stats.ruleset_runs-before,1,"receipt callbacks must not manufacture a graph update");
    input(&mut r,&mut cpu,"(Box (Leaf 99))");run(&base,&mut r,&mut cpu,"(run consume 1)");
    if std::env::var("CELL_COMMIT_MARKERS").as_deref()==Ok("1") {
        let report=r.report();let field=report["cell_plan_fields"].as_array().unwrap().iter().position(|s|s=="receipt_redundant").unwrap();
        assert!(report["cell_plan_work"].as_object().unwrap().values().any(|v|v[field].as_u64().unwrap_or(0)>0));
    }
}

#[test]
fn cpu_only_configuration_never_collects_unused_receipts(){
    let mut eg=EGraph::default();eg.parse_and_run_program(None,include_str!("../../experiments/commit_spine_markers/environment.egg")).unwrap();
    let cmds=eg.parse_program(None,"(run-schedule (seq make-get make-box consume))").unwrap();
    let lib=Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/commit-markers");
    let mut r=Runtime::new_full_configured(eg,&cmds,None,false,None,None,Options{analysis:Some(&lib),..Default::default()}).unwrap();
    r.input("(Single (Leaf 1))").unwrap();r.run(&cmds).unwrap();
    assert!(r.report()["commit_markers"].is_null());
}
