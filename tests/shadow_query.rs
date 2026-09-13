use egglog::{EGraph, Error};
#[test]
fn check_variables_do_not_escape_and_repeated_variables_are_allowed() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
 (function f () i64 :no-merge) (set (f) 2)
 (check (= (f) x) (= x 2))
 (check (= (f) x) (= x 2))
 (function x () i64 :no-merge)
 "#,
    )
    .unwrap();
}
#[test]
fn check_still_rejects_function_and_global_alias_shadowing() {
    for source in [
        "(function f () i64 :no-merge) (set (f) 2) (check (= (f) f) (= f 2))",
        "(let $value 41) (check (= $value value))",
    ] {
        let error = EGraph::default()
            .parse_and_run_program(None, source)
            .unwrap_err();
        assert!(matches!(error, Error::Shadowing(..)), "{error:?}");
    }
}
