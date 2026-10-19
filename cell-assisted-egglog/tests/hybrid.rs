use cell_assisted_egglog::table_live::Runtime;
use egglog::EGraph;
#[test]
fn native_unsupported_steps_feed_gpu_and_keep_the_original_schedule() {
    let Some(worker) = std::env::var_os("CELL_MATRIX_WORKER") else {
        return;
    };
    let source = r#"
      (datatype E (N i64)) (relation Ready (E)) (relation Seen (E))
      (ruleset native) (ruleset selected) (ruleset retire)
      ; Arithmetic in an LHS, plus a destructive step, remain native.
      (rule ((= e (N n)) (= m (+ n 1)) (< m 5)) ((Ready (N m))) :ruleset native)
      (rule ((Ready e)) ((Seen e)) :ruleset selected)
      (rule ((Ready (N 4))) ((subsume (N 4)) (union (N 1) (N 2))) :ruleset retire)
      (N 0)
    "#;
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None, source).unwrap();
    let cmds = eg
        .parse_program(
            None,
            "(run-schedule (saturate (seq native selected))) (run retire 1) (run selected 1)",
        )
        .unwrap();
    let mut native = eg.clone();
    native.run_program(cmds.clone()).unwrap();
    let mut r = Runtime::new_hybrid(
        eg,
        &cmds,
        Some(std::path::Path::new(&worker)),
        true,
        &["selected".into()],
    )
    .unwrap();
    r.run(&cmds).unwrap();
    for f in ["(check (Seen (N 3)))", "(check (= (N 1) (N 2)))"] {
        native.parse_and_run_program(None, f).unwrap();
        r.engine().clone().parse_and_run_program(None, f).unwrap();
    }
    for table in ["N", "Ready", "Seen"] {
        assert_eq!(native.get_size(table), r.engine().get_size(table));
    }
    assert!(r.report()["runtime"]["native_only_runs"].as_u64().unwrap() > 0);
    assert_eq!(r.report()["rules"].as_array().unwrap().len(), 1);
}
