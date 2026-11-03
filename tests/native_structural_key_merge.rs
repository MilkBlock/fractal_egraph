#[test]
fn equal_merge_values_do_not_bypass_nested_function_reads() {
    let mut eg=egglog::EGraph::default();
    eg.parse_and_run_program(None,r#"
      (function Lookup (i64) i64 :merge new)
      (function Total () i64 :merge (+ (Lookup old) new))
      (set (Lookup 1) 10)
      (set (Total) 1) (set (Total) 1)
      (check (= (Total) 11))
    "#).unwrap();
}

#[test]
fn equal_candidate_reads_keep_score_and_key_types() {
    let mut eg=egglog::EGraph::default();
    egg_layout::structural_key::source::install(&mut eg,r#"; cell-structural-key-runtime/v1 {"nodes":[{"primitive":"cell-key-node-N","label":"E:N","arity":1}],"bases":["i64"]}"#).unwrap();
    let mut src=String::from(r#"
      (datatype E (N i64))
      (function Rank (E) i64 :merge (max old new))
      (function K (E) CellStructuralKey :merge (cell-key-min old new))
      (function Winner () E :merge (cell-key-select old new
          (cell-key-prefer-left (Rank old) (Rank new) (K old) (K new))))
    "#);
    for i in 0..100 {src.push_str(&format!("(N {i})\n"));}
    src.push_str(r#"
      (set (Rank (N 99)) 10000000000000000)
      (set (K (N 99)) (cell-key-node-N (cell-key-base-i64 99)))
      (set (Winner) (N 99)) (set (Winner) (N 99))
      (check (= (Winner) (N 99)))
    "#);
    eg.parse_and_run_program(None,&src).unwrap();
}
