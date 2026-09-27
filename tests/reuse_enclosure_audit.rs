use egg_layout::reuse_graph::adaptive;
fn run(n: usize, affected: usize, union: bool) -> serde_json::Value {
    let mut s="(datatype Math (V i64) (A Math) (B Math) (Flag Math) (Pair Math Math) (Done Math)) (rewrite (A x) (B x)) (rewrite (Pair (B x) (Flag x)) (Done x))".to_string();
    for i in 0..n {
        s += &format!("(A (V {i}))");
    }
    s += "(run 2)";
    if union {
        s += "(Pair (B (V 0)) (Flag (V 1))) (run 1) (union (V 0) (V 1)) (run 2) (check (= (Pair (B (V 0)) (Flag (V 1))) (Done (V 0))))";
    } else {
        for i in 0..affected {
            s += &format!("(Pair (B (V {i})) (Flag (V {i})))");
        }
        s += "(run 2)";
    }
    let out = std::env::temp_dir().join(format!(
        "audit-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    adaptive::run_audit(&s, None, &out, "adaptive", 2).unwrap()
}
#[test]
fn one_local_graft_touches_one_of_twelve_bodies() {
    let r = run(12, 1, false);
    let a = &r["samples"][2]["enclosure_audit"];
    assert_eq!(a["known_bodies_before_step"], 12);
    assert_eq!(a["productive_boundary_participating_bodies"], 1, "{a}");
}
#[test]
fn broad_graft_is_not_misreported_as_local() {
    let r = run(12, 12, false);
    let a = &r["samples"][2]["enclosure_audit"];
    assert_eq!(a["productive_boundary_participating_bodies"], 12, "{a}");
    assert_eq!(
        a["body_output_regions_changed"], 0,
        "reading all bodies is not changing them: {a}"
    );
}
#[test]
fn union_enabled_match_is_detected_after_rekey() {
    let r = run(12, 0, true);
    let a = &r["samples"][3]["enclosure_audit"];
    assert!(a["rekeyed_since_last_step"].as_u64().unwrap() > 0, "{a}");
    assert!(
        a["productive_boundary_participating_bodies"]
            .as_u64()
            .unwrap()
            > 0,
        "{a}"
    );
}

#[test]
fn external_union_into_one_body_is_a_real_region_change() {
    let mut s="(datatype Math (V i64) (A Math) (B Math) (Extra Math) (Done Math)) (rewrite (A x) (B x)) (rewrite (Extra x) (Done x))".to_string();
    for i in 0..12 {
        s += &format!("(A (V {i}))");
    }
    s += "(run 2) (union (A (V 0)) (Extra (V 0))) (run 2) (check (= (A (V 0)) (Done (V 0))))";
    let out = std::env::temp_dir().join(format!(
        "audit-region-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let r = adaptive::run_audit(&s, None, &out, "adaptive", 2).unwrap();
    let a = &r["samples"][2]["enclosure_audit"];
    assert_eq!(a["body_output_changed_between_steps"], 1, "{a}");
    assert_eq!(a["body_output_regions_changed"], 1, "{a}");
}

#[test]
fn aliasing_can_enable_a_new_match_wholly_inside_a_symbolic_closure() {
    let s = "(datatype Math (V i64) (A Math) (B Math) (Done Math)) (rewrite (A x) (B x)) (rewrite (A (A x)) (Done x)) (A (V 0)) (run 2) (union (A (V 0)) (V 0)) (run 2) (check (= (Done (V 0)) (V 0)))";
    let out = std::env::temp_dir().join(format!(
        "audit-alias-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let r = adaptive::run_audit(s, None, &out, "adaptive", 1).unwrap();
    let a = &r["samples"][2]["enclosure_audit"];
    assert!(
        a["internally_effectful_bodies"].as_u64().unwrap() > 0,
        "{a}"
    );
    assert_eq!(
        a["internally_effectful_bodies"], a["internally_effectful_alias_or_rekeyed"],
        "{a}"
    );
}
