#[path = "../experiments/eggcc_passes/schedule_profile.rs"]
mod schedule_profile;

#[test]
fn boundary_profiler_preserves_native_repeat_and_saturation() {
    for schedule in [
        "(run r 1) (run s 1)",
        "(run-schedule (repeat 5 (seq r (saturate s))))",
        "(run-schedule (repeat 10 (repeat 3 (seq r s))))",
        "(run-schedule (saturate (seq r (saturate s))))",
        "(run-schedule (repeat 8 (run r :until (Q 2)))) (run-schedule (saturate s))",
    ] {
        let src = format!(
            r#"
          (relation P (i64)) (relation Q (i64)) (relation R (i64))
          (ruleset r) (ruleset s)
          (rule ((P x) (< x 3)) ((P (+ x 1)) (Q x)) :ruleset r)
          (rule ((Q x)) ((R x)) :ruleset s)
          (P 0) {schedule}
        "#
        );
        let mut native = egglog::EGraph::default();
        native.parse_and_run_program(None, &src).unwrap();
        let mut measured = egglog::EGraph::default();
        let profile = schedule_profile::execute(&mut measured, &src).unwrap();
        assert_eq!(
            native.get_overall_run_report().num_matches_per_rule,
            measured.get_overall_run_report().num_matches_per_rule,
            "{schedule}"
        );
        for table in ["P", "Q", "R"] {
            assert_eq!(native.get_size(table), measured.get_size(table));
            for value in 0..5 {
                let check = format!("(check ({table} {value}))");
                assert_eq!(
                    native.clone().parse_and_run_program(None, &check).is_ok(),
                    measured.clone().parse_and_run_program(None, &check).is_ok()
                );
            }
        }
        let packets = profile["packets"].as_array().unwrap();
        if schedule == "(run-schedule (saturate (seq r (saturate s))))" {
            assert_eq!(packets.len(), 1, "nested saturation must not double count");
            assert_eq!(packets[0]["kind"], "saturate");
        }
        let sum: f64 = packets
            .iter()
            .map(|p| p["wall_seconds"].as_f64().unwrap())
            .sum();
        let grouped = profile["saturate_seconds"].as_f64().unwrap()
            + profile["run_seconds"].as_f64().unwrap();
        assert!((sum - grouped).abs() < 1e-9);
    }
}
