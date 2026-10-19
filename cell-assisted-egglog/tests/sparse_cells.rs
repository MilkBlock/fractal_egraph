//! Native frontier auditing stays enabled: allocation changes must not change
//! bindings, including late dependencies and non-idempotent RHS effects.
use cell_assisted_egglog::table_live::{Options, QueryRouting, Runtime};
use egglog::EGraph;
use serde_json::Value;
use std::path::{Path, PathBuf};

fn worker() -> Option<PathBuf> {
    std::env::var_os("CELL_MATRIX_WORKER").map(Into::into)
}
fn library() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/analyze")
}
fn setup() -> EGraph {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
      (datatype Expr (Leaf i64) (Single Expr) (Get Expr i64))
      (relation Boundary (Expr i64)) (relation LeafSeen (i64))
      (function Count (i64) i64 :merge (+ old new))
      (ruleset consume)
      (rule ((Boundary s n) (= s (Single x)) (= x (Get s 0)))
            ((set (Count n) 1)) :ruleset consume)
      ; Force unrelated rows into the real GPU mirror, not just the native IR.
      (rule ((Leaf n)) ((LeafSeen n)) :ruleset consume)
      (let x (Leaf 0)) (let s (Single x))
      (union (Get s 0) x) (Boundary s 0)
    "#,
    )
    .unwrap();
    eg
}
fn number(v: &Value, name: &str) -> u64 {
    v["device"][name].as_u64().unwrap()
}

#[test]
fn unrelated_rows_do_not_multiply_cell_or_dependency_storage() {
    let Some(w) = worker() else { return };
    // Check both incremental event states and the legacy cache-only path.
    for cache_only in [false, true] {
        let mut eg = setup();
        let cmds = eg.parse_program(None, "(run consume 1)").unwrap();
        let mut r = Runtime::configured(
            eg,
            &cmds,
            &w,
            true,
            Options {
                analysis: Some(&library()),
                cache_only,
                profile: true,
                query_routing: QueryRouting::CellPriority,
                ..Options::default()
            },
        )
        .unwrap();
        r.run(&cmds).unwrap();
        let before = r.report();
        r.input(&(1..4097).map(|n| format!("(Leaf {n})")).collect::<String>())
            .unwrap();
        r.run(&cmds).unwrap();
        let after = r.report();
        assert!(number(&after, "row_slots") > number(&before, "row_slots") + 4000);
        assert!(number(&before, "cell_instances") > 0);
        for field in [
            "cell_plans",
            "cell_instances",
            "cell_instance_slots",
            "cell_instance_capacity",
            "cell_cache_bytes",
            "watch_capacity",
            "port_capacity",
        ] {
            assert_eq!(
                after["device"][field], before["device"][field],
                "{field}, cache_only={cache_only}"
            );
        }
        assert_eq!(r.engine().get_size("LeafSeen"), 4097);
        r.engine()
            .clone()
            .parse_and_run_program(None, "(check (= (Count 0) 1))")
            .unwrap();
        r.input(
            &(10000..10128)
                .map(|n| format!("(Get (Leaf {n}) 99)"))
                .collect::<String>(),
        )
        .unwrap();
        r.run(&cmds).unwrap();
        assert_eq!(
            number(&r.report(), "cell_instances"),
            number(&before, "cell_instances"),
            "an anchor's table alone is insufficient: Get(_,99) cannot allocate a Get(_,0) instance"
        );
    }
}

#[test]
fn retired_slots_are_recycled_without_reusing_stale_guarantees() {
    let Some(w) = worker() else { return };
    let mut eg = setup();
    let cmds = eg.parse_program(None, "(run consume 1)").unwrap();
    let mut r = Runtime::configured(
        eg,
        &cmds,
        &w,
        true,
        Options {
            analysis: Some(&library()),
            profile: true,
            query_routing: QueryRouting::CellPriority,
            ..Options::default()
        },
    )
    .unwrap();
    r.run(&cmds).unwrap();
    let initial = r.report();
    for n in 1..17 {
        let old = n - 1;
        // A deletion alone can leave every consumer below its watermark, so
        // force a real dispatch to flush the queued retirement to the device.
        r.input(&format!(
            "(delete (Get (Single (Leaf {old})) 0)) (delete (Single (Leaf {old}))) (Leaf {})",
            100000 + n
        ))
        .unwrap();
        r.run(&cmds).unwrap();
        assert_eq!(number(&r.report(), "cell_instances"), 0);
        r.input(&format!("(Boundary (Single (Leaf {n})) {n})"))
            .unwrap();
        r.run(&cmds).unwrap();
        assert_eq!(r.engine().get_size("Count"), n as usize);
        // Waiting instances must subscribe to the missing key, then become
        // Ready when that exact fact arrives. Old slot revisions cannot help.
        r.input(&format!("(union (Get (Single (Leaf {n})) 0) (Leaf {n}))"))
            .unwrap();
        r.run(&cmds).unwrap();
        r.engine()
            .clone()
            .parse_and_run_program(None, &format!("(check (= (Count {n}) 1))"))
            .unwrap();
        assert_eq!(
            number(&r.report(), "cell_instance_slots"),
            number(&initial, "cell_instance_slots")
        );
        assert_eq!(
            number(&r.report(), "cell_cache_bytes"),
            number(&initial, "cell_cache_bytes")
        );
    }
    assert!(number(&r.report(), "cell_slots_reused") >= 16);
    assert!(number(&r.report(), "watch_count") <= number(&r.report(), "watch_capacity"));
    // Also retire and allocate in ONE upload, while old watch hits can already
    // have queued the recycled slot. Each replacement must evaluate only once.
    for n in 17..33 {
        let old = n - 1;
        r.input(&format!(
            "(delete (Get (Single (Leaf {old})) 0)) (delete (Single (Leaf {old}))) \
            (Boundary (Single (Leaf {n})) {n}) (union (Get (Single (Leaf {n})) 0) (Leaf {n}))"
        ))
        .unwrap();
        r.run(&cmds).unwrap();
        r.engine()
            .clone()
            .parse_and_run_program(None, &format!("(check (= (Count {n}) 1))"))
            .unwrap();
        assert_eq!(
            number(&r.report(), "cell_instance_slots"),
            number(&initial, "cell_instance_slots")
        );
    }
}
