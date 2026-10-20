//! Native effects and scheduling checks, with no GPU or concrete Cell catalog.
#[allow(dead_code)]
#[path = "../examples/support/live_table_bench.rs"]
mod compare;
use cell_assisted_egglog::parameterized_cell::Runtime;
use egglog::EGraph;

const TERMS: &str = include_str!("../experiments/parameterized_cell/terms.egg");
const SETS: &[&str] = &["terms", "terms-helpers", "terms-helpers-helpers"];
const SAT: &str = "(run-schedule (saturate (seq terms (saturate (seq terms-helpers (saturate terms-helpers-helpers))))))";
struct Pair {
    base: EGraph,
    native: EGraph,
    cell: Runtime,
}
impl Pair {
    fn new(source: &str, sets: &[&str]) -> Self {
        let mut base = EGraph::default();
        base.parse_and_run_program(None, source).unwrap();
        Self {
            native: base.clone(),
            cell: Runtime::new(base.clone(), sets).unwrap(),
            base,
        }
    }
    fn equal(&self) {
        self.cell.audit_mirror().unwrap();
        compare::equal(&self.base, &self.native, self.cell.engine()).unwrap();
    }
    fn input(&mut self, source: &str) {
        self.native.parse_and_run_program(None, source).unwrap();
        self.cell.input(source).unwrap();
        self.equal();
    }
    fn run(&mut self, source: &str) {
        let commands = self.native.parse_program(None, source).unwrap();
        self.native.run_program(commands.clone()).unwrap();
        self.cell.run(&commands).unwrap();
        self.equal();
    }
    fn check(&self, source: &str) {
        self.cell
            .engine()
            .clone()
            .parse_and_run_program(None, source)
            .unwrap();
    }
    fn bindings(&self) -> u64 {
        self.cell.report()["programs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["emitted_bindings"].as_u64().unwrap())
            .sum()
    }
}

#[test]
fn terms_respects_every_finite_run_boundary() {
    let mut p = Pair::new(TERMS, SETS);
    p.input(
        "(set (ExtractedExpr (V 0)) (TCPair (TermLeaf 0) 2))
             (set (ExtractedExpr (V 1)) (TCPair (TermLeaf 1) 3))
             (Bop (Add) (V 0) (V 1))",
    );
    p.run("(run terms 1)");
    assert_eq!(p.cell.engine().get_size("ExtractedExpr"), 2);
    assert_eq!(p.cell.engine().get_size("PotentialExtractedExpr"), 1);
    p.run("(run terms-helpers 1)");
    p.check(
        "(check (= (ExtractedExpr (Bop (Add) (V 0) (V 1))) (TCPair (NoTerm) 10000000000000000)))",
    );
    p.run("(run terms-helpers 1)");
    p.check("(check (= (ExtractedExpr (Bop (Add) (V 0) (V 1)))
                  (Smaller (TCPair (NoTerm) 10000000000000000) (TCPair (TermBop (Add) (TermLeaf 0) (TermLeaf 1)) 6))))");
    p.run("(run terms-helpers-helpers 1)");
    p.check("(check (= (ExtractedExpr (Bop (Add) (V 0) (V 1))) (TCPair (TermBop (Add) (TermLeaf 0) (TermLeaf 1)) 6)))");
    p.run(SAT);
    let before = (p.cell.stats.keyed_reads, p.bindings());
    p.run(SAT);
    p.input("(TermLeaf 999)"); // not a read port of any kernel
    p.run(SAT);
    assert_eq!(before, (p.cell.stats.keyed_reads, p.bindings()));
    assert_eq!(p.cell.stats.program_compilations, 1);
    assert_eq!(p.cell.report()["concrete_closure_catalog_entries"], 0);
}

#[test]
fn missing_port_and_union_wake_the_existing_template() {
    let mut p = Pair::new(TERMS, SETS);
    p.input(
        "(Bop (Add) (V 0) (V 1))
             (set (ExtractedExpr (V 0)) (TCPair (TermLeaf 0) 2))",
    );
    p.run(SAT);
    assert_eq!(p.cell.engine().get_size("PotentialExtractedExpr"), 0);
    p.input("(union (V 0) (V 1))");
    p.run(SAT);
    p.check("(check (= (ExtractedExpr (Bop (Add) (V 0) (V 1))) (TCPair (TermBop (Add) (TermLeaf 0) (TermLeaf 0)) 5)))");
    p.input("(union (TCPair (TermLeaf 0) 2) (TCPair (TermLeaf 7) 2))");
    p.run(SAT);
    p.check("(check (PotentialExtractedExpr (Bop (Add) (V 0) (V 1)) (TCPair (TermBop (Add) (TermLeaf 7) (TermLeaf 0)) 5)))
             (check (PotentialExtractedExpr (Bop (Add) (V 0) (V 1)) (TCPair (TermBop (Add) (TermLeaf 0) (TermLeaf 7)) 5)))");
    // Empty bootstrap. Later canonical changes were all journal deltas.
    assert_eq!(p.cell.stats.bootstrap_rows, 0);
    assert_eq!(p.cell.stats.program_compilations, 1);
}

#[test]
fn visibility_and_reinsert_preserve_non_idempotent_effects() {
    let mut p = Pair::new(
        r#"
        (relation P (i64 i64)) (relation Ready (i64))
        (function Count (i64) i64 :merge (+ old new)) (ruleset s)
        (rule ((P x c) (Ready x)) ((set (Count x) 1)) :ruleset s)
    "#,
        &["s"],
    );
    p.input("(P 1 10)");
    p.run("(run s 1)");
    p.input("(subsume (P 1 10)) (Ready 1)");
    p.run("(run s 1)");
    assert_eq!(p.cell.engine().get_size("Count"), 0);
    p.input("(P 1 11)");
    p.run("(run s 1)");
    p.check("(check (= (Count 1) 1))");
    p.input("(delete (P 1 11)) (P 1 11)");
    p.run("(run s 1)");
    p.check("(check (= (Count 1) 2))");
    p.input("(delete (Ready 1))");
    p.run("(run s 1)");
    p.input("(Ready 1)");
    p.run("(run s 1)");
    p.check("(check (= (Count 1) 3))");
    let count = p.bindings();
    p.run("(run s 1)");
    assert_eq!(p.bindings(), count);
}

#[test]
fn frozen_ruleset_and_native_handoff_do_not_replay_actions() {
    let mut p = Pair::new(
        r#"
        (relation P (i64)) (relation Q (i64)) (relation R (i64))
        (function Count (i64) i64 :merge (+ old new))
        (ruleset s) (ruleset external)
        (rule ((P x)) ((Q x) (set (Count x) 1)) :ruleset s)
        (rule ((Q x)) ((R x)) :ruleset s)
        (rule ((R x)) ((P (+ x 1))) :ruleset external)
        (P 1)
    "#,
        &["s"],
    );
    p.run("(run s 1)");
    assert_eq!(p.cell.engine().get_size("R"), 0);
    p.run("(run-schedule (run s :until (R 1)))"); // native path
    p.run("(run s 1)");
    p.check("(check (= (Count 1) 1)) (check (R 1))");
    p.run("(run external 1)");
    p.run("(run s 1)");
    p.check("(check (= (Count 2) 1))");
    assert_eq!(p.cell.engine().get_size("R"), 1);
    p.run("(run s 1)");
    p.check("(check (R 2)) (check (= (Count 1) 1)) (check (= (Count 2) 1))");
    assert_eq!(p.cell.stats.native_fallback_runs, 2);
}

#[test]
fn failed_native_effects_fail_the_program_and_prevent_resume() {
    for (input, schedule) in [
        (
            "(PotentialExtractedExpr (V 0) (TCPair (TermLeaf 0) -1))",
            "(run terms-helpers 1)",
        ),
        (
            "(Bop (Add) (V 0) (V 1))
          (set (ExtractedExpr (V 0)) (TCPair (TermLeaf 0) 9223372036854775807))
          (set (ExtractedExpr (V 1)) (TCPair (TermLeaf 1) 1))",
            "(run terms 1)",
        ),
    ] {
        let mut p = Pair::new(TERMS, SETS);
        p.input(input);
        let commands = p.native.parse_program(None, schedule).unwrap();
        let native_error = p
            .native
            .run_program(commands.clone())
            .unwrap_err()
            .to_string();
        let cell_error = p.cell.run(&commands).unwrap_err().to_string();
        assert_eq!(native_error, cell_error);
        assert!(
            p.cell
                .run(&commands)
                .unwrap_err()
                .to_string()
                .contains("cannot resume")
        );
        assert!(
            p.cell
                .input("(V 5)")
                .unwrap_err()
                .to_string()
                .contains("cannot resume")
        );
    }
}

#[test]
fn compiler_refuses_unconstrained_cartesian_join() {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        r#"
        (relation P (i64)) (relation Q (i64)) (relation R (i64 i64)) (ruleset s)
        (rule ((P x) (Q y)) ((R x y)) :ruleset s) (P 1) (Q 2)
    "#,
    )
    .unwrap();
    let Err(error) = Runtime::new(eg.clone(), &["s"]) else {
        panic!("compiled an unbound lookup")
    };
    assert!(error.to_string().contains("0 fixed seed plans"));
    eg.parse_and_run_program(None, "(run s 1) (check (R 1 2))")
        .unwrap();
}

#[test]
fn unchanged_operand_is_reused_but_a_dropped_subscription_cannot_leave_a_stale_port() {
    let mut p = Pair::new(
        r#"
        (relation Root (i64 i64))
        (function Val (i64) i64 :merge (max old new))
        (relation Out (i64 i64 i64 i64))
        (ruleset s)
        (rule ((Root x y) (= vx (Val x)) (= vy (Val y)))
              ((Out x y vx vy)) :ruleset s)
        "#,
        &["s"],
    );
    p.input("(Root 1 2) (set (Val 1) 3) (set (Val 2) 10)");
    p.run("(run s 1)");
    let before = p.cell.stats.keyed_reads;
    p.input("(set (Val 1) 4)");
    p.run("(run s 1)");
    p.check("(check (Out 1 2 4 10))");
    assert_eq!(
        p.cell.stats.keyed_reads - before,
        1,
        "unchanged y was looked up again"
    );
    assert_eq!(p.cell.report()["port_reuse"]["reused_reads"], 1);

    p.input("(delete (Val 1))");
    p.run("(run s 1)"); // Stops at the missing first port; y is no longer watched.
    p.input("(set (Val 2) 20)");
    p.run("(run s 1)");
    p.input("(set (Val 1) 5)");
    p.run("(run s 1)");
    p.check("(check (Out 1 2 5 20)) (fail (check (Out 1 2 5 10)))");
}

#[test]
fn a_changed_binding_rewires_later_ports_even_when_the_old_row_did_not_change() {
    let mut p = Pair::new(
        r#"
        (relation Root (i64 i64))
        (function Next (i64) i64 :merge (max old new))
        (function Weight (i64) i64 :merge (max old new))
        (relation Out (i64 i64 i64 i64))
        (ruleset s)
        (rule ((Root x z) (= y (Next x)) (= c (Weight y)))
              ((Out x z y c)) :ruleset s)
        "#,
        &["s"],
    );
    p.input("(Root 7 9) (set (Next 7) 1) (set (Weight 1) 10) (set (Weight 2) 20)");
    p.run("(run s 1)");
    p.input("(set (Next 7) 2)");
    p.run("(run s 1)");
    p.check("(check (Out 7 9 2 20)) (fail (check (Out 7 9 2 10)))");
}
