use serde_json::Value;
#[test]
fn math_connections_are_proved_by_source_sites() {
    let profile: Value = serde_json::from_str(include_str!(
        "../experiments/annotated_export/math_microbenchmark/profile.json"
    ))
    .unwrap();
    let mut edges = 0;
    for w in profile["witnesses"].as_array().unwrap() {
        for s in w["support"].as_array().unwrap() {
            assert_eq!(s["producer_sites"].as_array().unwrap().len(), 1);
            assert_eq!(s["consumer_sites"].as_array().unwrap().len(), 1);
            edges += 1;
        }
    }
    assert!(edges > 0);
    let nodes: Vec<Value> =
        include_str!("../experiments/annotated_export/math_microbenchmark/combined.egg")
            .lines()
            .filter_map(|l| l.strip_prefix("; @egg-viz-json "))
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
    let mut parser = egglog::EGraph::default();
    let commands = parser
        .parse_program(
            None,
            include_str!("../experiments/annotated_export/math_microbenchmark/combined.egg"),
        )
        .unwrap();
    let rules: std::collections::BTreeMap<_, _> = commands
        .into_iter()
        .filter_map(|c| {
            if let egglog::ast::Command::Rule { rule } = c {
                Some((rule.name.clone(), rule))
            } else {
                None
            }
        })
        .collect();
    let mut n = 0;
    for m in nodes
        .iter()
        .filter(|m| m["kind"] == "combined_witness_bundle")
    {
        assert_eq!(m["ast_connections_complete"], true);
        n += 1;
        for c in m["connections"].as_array().unwrap() {
            assert_eq!(c["mapping_method"], "compiler_source_span");
            let rule = &rules[m["id"].as_str().unwrap()];
            for field in ["producer_combined_position", "consumer_combined_position"] {
                let e = egg_layout::visual_rule::expression_at(rule, c[field].as_str().unwrap())
                    .unwrap();
                let egglog::ast::Expr::Call(_, op, _) = e else {
                    panic!("endpoint must be a call")
                };
                assert_eq!(op, c["table"].as_str().unwrap());
            }

            assert_eq!(c["producer_position"], c["evidence"]["producer_sites"][0]);
            assert_eq!(c["consumer_position"], c["evidence"]["consumer_sites"][0]);
        }
    }
    assert_eq!(n, profile["motif_rankings"].as_array().unwrap().len());
    assert!(n > profile["coarse_motif_classes"].as_u64().unwrap() as usize);
}
