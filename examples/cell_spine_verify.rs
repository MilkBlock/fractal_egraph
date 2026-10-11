//! Execute the generated prototype in the actual local egglog runtime.
use egglog::EGraph;

fn rows(eg: &EGraph, name: &str) -> usize {
    let mut count = 0;
    eg.function_for_each(name, |_| count += 1).unwrap();
    count
}

fn main() {
    let source = std::fs::read_to_string("experiments/cell_spine/demo.egg").unwrap();
    let mut eg = EGraph::default();
    eg.parse_and_run_program(None, &source).unwrap();
    assert_eq!(rows(&eg, "Add"), 0, "ordinary rules must not enter the spine");
    assert_eq!(rows(&eg, "Spine-Add"), 3);
    assert_eq!(rows(&eg, "Summary-Add3"), 1);
    assert_eq!(rows(&eg, "Summary-Add4"), 1);
    // Repeated graft has one structural result, with an unchanged shared prefix.
    eg.parse_and_run_program(None, "(Extend abc d) (run cell-transition 4)").unwrap();
    assert_eq!(rows(&eg, "Spine-Add"), 3);
    // Export only when requested. Compare to the concrete selected source path.
    eg.parse_and_run_program(None,
        "(Export (Spine-Add abc (Input d))) (run export 8)
         (check (= (Reified (Spine-Add abc (Input d))) (Add (Add (Add a b) c) d)))"
    ).unwrap();
    assert_eq!(rows(&eg, "Add"), 3);
    eg.parse_and_run_program(None,
        "(run original 8)
         (check (= (Reified (Spine-Add abc (Input d))) (Add a (Add b (Add c d)))))"
    ).unwrap();
    println!("PASS: isolated spine, shared prefix, idempotent graft, explicit export and original rewrite equivalence");
}
