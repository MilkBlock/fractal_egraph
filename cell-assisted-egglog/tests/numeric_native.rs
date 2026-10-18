use cell_assisted_egglog::{Part, Session, environment, numeric_native::NumericSession};
use std::path::Path;
fn setup(extension: &str) -> Option<NumericSession> {
    let worker = std::env::var_os("CELL_MATRIX_WORKER")?;
    Some(setup_with_worker(extension, Path::new(&worker), true, true))
}
fn setup_with_worker(extension: &str, worker: &Path, checked: bool, audit: bool) -> NumericSession {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/analyze");
    let mut source = Session::with_extension(
        &dir,
        "",
        "cost-improve",
        vec!["single-get".into()],
        vec![
            Part {
                atoms: vec![0],
                cell: true,
            },
            Part {
                atoms: vec![1],
                cell: false,
            },
            Part {
                atoms: vec![2],
                cell: false,
            },
        ],
        extension,
    )
    .unwrap();
    source.set_audit(audit);
    let env = format!("{}\n{extension}", environment(&dir).unwrap().0);
    let mut session = NumericSession::new(source, &env, worker).unwrap();
    session.set_gpu_check(checked).unwrap();
    session
}
fn extension() -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("matrix-numeric/consumer.egg"),
    )
    .unwrap()
}
fn rounds(s: &mut NumericSession) {
    for _ in 0..4 {
        s.round().unwrap();
    }
}
fn check(s: &NumericSession, text: &str) {
    s.engine()
        .clone()
        .parse_and_run_program(None, text)
        .unwrap();
}
#[test]
fn actual_cell_numeric_guard_and_colliding_merge_match_native() {
    let ext = extension();
    let Some(mut s) = setup(&ext) else { return };
    let input = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("matrix-numeric/input.egg"),
    )
    .unwrap();
    s.input(&input).unwrap();
    rounds(&mut s);
    check(
        &s,
        "(check (= (Best (Leaf 200)) 9999999999999999)) (check (= (Best (Leaf 201)) 2)) (check (= (Best (Leaf 202)) 1))",
    );
    assert_eq!(s.stats.accepted_bindings, 3);
    assert_eq!(s.stats.native_sets, 2);
    assert!(s.stats.verification.batches > 0);
    assert_eq!(s.stats.verification.failures, 0);
    assert_eq!(s.stats.verification.proposals, 3);
    assert!(s.stats.audit_uncovered.iter().all(|v| *v == 0));
    let before = s.stats.device["execution_counts"].clone();
    let writes = s.stats.native_sets;
    s.round().unwrap();
    assert_eq!(before, s.stats.device["execution_counts"]);
    assert_eq!(s.stats.native_sets, writes);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/analyze");
    let mut native = egglog::EGraph::default();
    native
        .parse_and_run_program(
            None,
            &format!(
                "{}\n{ext}\n{input}\n(run-schedule (saturate (run)))",
                environment(&dir).unwrap().0
            ),
        )
        .unwrap();
    for k in [200, 201, 202] {
        let e = egglog::ast::Parser::default()
            .get_expr_from_string(None, &format!("(Best (Leaf {k}))"))
            .unwrap();
        let a = native.eval_expr(&e).unwrap().1;
        let mut actual = s.engine().clone();
        let b = actual.eval_expr(&e).unwrap().1;
        assert_eq!(
            native.value_to_base::<i64>(a),
            actual.value_to_base::<i64>(b)
        );
    }
}
#[test]
fn native_union_rekeys_before_numeric_merge_and_late_rows_reenable_guards() {
    let Some(mut s) = setup(&extension()) else {
        return;
    };
    s.input("(Single (Leaf 1)) (set (Best (Leaf 2)) 10) (set (Best (Leaf 3)) 4)")
        .unwrap();
    rounds(&mut s);
    s.input("(union (Leaf 2) (Leaf 3)) (Offer (Leaf 2) (Single (Leaf 1)) 3) (Offer (Leaf 3) (Single (Leaf 1)) 2)").unwrap();
    rounds(&mut s);
    check(
        &s,
        "(check (= (Best (Leaf 2)) 1)) (check (= (Best (Leaf 3)) 1))",
    );
    assert_eq!(s.stats.native_sets, 1);
    assert_eq!(s.stats.accepted_bindings, 2);
    assert!(s.stats.union_events > 0);
    s.input("(Offer (Leaf 3) (Single (Leaf 1)) 0)").unwrap();
    rounds(&mut s);
    check(&s, "(check (= (Best (Leaf 2)) -1))");
    assert!(s.stats.audit_uncovered.iter().all(|v| *v == 0));
}
#[test]
fn max_merge_and_updated_guard_use_actual_payloads() {
    let ext = extension()
        .replace("min old new", "max old new")
        .replace("(< cost old)", "(> cost old)")
        .replace("(- cost 1)", "(+ cost 1)");
    let Some(mut s) = setup(&ext) else { return };
    s.input("(Single (Leaf 1)) (set (Best (Leaf 2)) -100) (Offer (Leaf 2) (Single (Leaf 1)) 3) (Offer (Leaf 2) (Single (Leaf 1)) 7)").unwrap();
    rounds(&mut s);
    check(&s, "(check (= (Best (Leaf 2)) 8))");
    assert_eq!(s.stats.native_sets, 1);
    s.input("(set (Best (Leaf 2)) 100)").unwrap();
    rounds(&mut s);
    check(&s, "(check (= (Best (Leaf 2)) 100))");
    assert_eq!(s.stats.native_sets, 1);
}
#[test]
fn overflow_commits_no_partial_batch_and_recovery_keeps_valid_completions() {
    let ext = extension()
        .replace("min old new", "max old new")
        .replace("(< cost old)", "(>= cost old)")
        .replace("(- cost 1)", "(+ cost 1)");
    let Some(mut s) = setup(&ext) else { return };
    s.input("(Single (Leaf 1)) (set (Best (Leaf 2)) 0) (Offer (Leaf 2) (Single (Leaf 1)) 4) (Offer (Leaf 2) (Single (Leaf 1)) 9223372036854775807)").unwrap();
    assert!(
        s.round()
            .unwrap_err()
            .to_string()
            .contains("checked i64 RHS failed")
    );
    check(&s, "(check (= (Best (Leaf 2)) 0))");
    let counts = s.stats.device["execution_counts"].clone();
    s.input("(delete (Offer (Leaf 2) (Single (Leaf 1)) 9223372036854775807))")
        .unwrap();
    s.round().unwrap();
    check(&s, "(check (= (Best (Leaf 2)) 5))");
    for (i, c) in counts.as_array().unwrap().iter().enumerate() {
        assert_eq!(*c, s.stats.device["execution_counts"][i]);
    }
}
#[test]
fn non_lattice_merge_and_extra_effects_are_not_miscompiled() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/analyze");
    for ext in [
        extension().replace("min old new", "+ old new"),
        extension().replace(
            "((set (Best dst) (- cost 1)))",
            "((set (Best dst) (- cost 1)) (union dst x))",
        ),
    ] {
        let source = Session::with_extension(
            &dir,
            "",
            "cost-improve",
            vec!["single-get".into()],
            vec![
                Part {
                    atoms: vec![0],
                    cell: true,
                },
                Part {
                    atoms: vec![1],
                    cell: false,
                },
                Part {
                    atoms: vec![2],
                    cell: false,
                },
            ],
            &ext,
        )
        .unwrap();
        let env = format!("{}\n{ext}", environment(&dir).unwrap().0);
        assert!(NumericSession::new(source, &env, Path::new("/not-a-worker")).is_err());
    }
}

#[test]
fn new_function_row_can_be_created_without_an_old_value_guard() {
    let Some(worker) = std::env::var_os("CELL_MATRIX_WORKER") else {
        return;
    };
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/analyze");
    let ext = "(function Best (Expr) i64 :merge (min old new)) (relation Offer (Expr Expr i64)) (rule ((Offer dst (Single x) cost)) ((set (Best dst) cost)) :name \"cost-improve\")";
    let mut source = Session::with_extension(
        &dir,
        "",
        "cost-improve",
        vec!["single-get".into()],
        vec![
            Part {
                atoms: vec![0],
                cell: true,
            },
            Part {
                atoms: vec![1],
                cell: false,
            },
        ],
        ext,
    )
    .unwrap();
    source.set_audit(true);
    let env = format!("{}\n{ext}", environment(&dir).unwrap().0);
    let mut s = NumericSession::new(source, &env, Path::new(&worker)).unwrap();
    s.set_gpu_check(true).unwrap();
    s.input("(Single (Leaf 1)) (Offer (Leaf 2) (Single (Leaf 1)) -9223372036854775808)")
        .unwrap();
    rounds(&mut s);
    check(&s, "(check (= (Best (Leaf 2)) -9223372036854775808))");
    assert_eq!(s.stats.native_sets, 1);
}

#[test]
fn gpu_check_can_be_disabled_and_reenabled_without_replaying_completed_matches() {
    let Some(worker) = std::env::var_os("CELL_MATRIX_WORKER") else {
        return;
    };
    let mut s = setup_with_worker(&extension(), Path::new(&worker), false, false);
    s.input("(Single (Leaf 1)) (set (Best (Leaf 2)) 100) (Offer (Leaf 2) (Single (Leaf 1)) 4)")
        .unwrap();
    rounds(&mut s);
    check(&s, "(check (= (Best (Leaf 2)) 3))");
    assert_eq!(s.stats.verification.batches, 0);
    assert!(s.stats.audit_uncovered.is_empty());
    let counts = s.stats.device["execution_counts"].clone();
    s.set_gpu_check(true).unwrap();
    s.input("(Offer (Leaf 2) (Single (Leaf 1)) 2)").unwrap();
    rounds(&mut s);
    check(&s, "(check (= (Best (Leaf 2)) 1))");
    assert!(s.stats.verification.batches > 0);
    for (i, c) in counts.as_array().unwrap().iter().enumerate() {
        assert_eq!(*c, s.stats.device["execution_counts"][i]);
    }
    s.set_gpu_check(false).unwrap();
    let verified = s.stats.verification.batches;
    s.input("(Offer (Leaf 2) (Single (Leaf 1)) 0)").unwrap();
    rounds(&mut s);
    check(&s, "(check (= (Best (Leaf 2)) -1))");
    assert_eq!(s.stats.verification.batches, verified);
    assert!(!s.stats.verification.enabled);
}

#[test]
fn strict_check_detects_missing_cell_coverage_even_without_a_gpu_dispatch() {
    let Some(worker) = std::env::var_os("CELL_MATRIX_WORKER") else {
        return;
    };
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/analyze");
    let ext = extension();
    let source = Session::with_extension(
        &dir,
        "",
        "cost-improve",
        vec!["single-get".into()],
        vec![
            Part {
                atoms: vec![0],
                cell: true,
            },
            Part {
                atoms: vec![1],
                cell: false,
            },
            Part {
                atoms: vec![2],
                cell: false,
            },
        ],
        &ext,
    )
    .unwrap();
    let env = format!("{}\n{ext}", environment(&dir).unwrap().0);
    let mut s = NumericSession::new(source, &env, Path::new(&worker)).unwrap();
    s.set_gpu_check(true).unwrap();
    // The LHS is present, but its baked Single/Get certificate is not visible.
    s.input("(Single (Leaf 1)) (subsume (Get (Single (Leaf 1)) 0)) (set (Best (Leaf 2)) 100) (Offer (Leaf 2) (Single (Leaf 1)) 3)")
        .unwrap();
    assert!(s.round().unwrap_err().to_string().contains("1 missing"));
    assert_eq!(s.stats.channels, 0);
    assert_eq!(s.stats.native_sets, 0);
    check(&s, "(check (= (Best (Leaf 2)) 100))");
}

#[cfg(unix)]
#[test]
fn corrupted_real_gpu_packets_are_rejected_before_any_native_set() {
    use std::os::unix::fs::PermissionsExt;
    let Some(worker) = std::env::var_os("CELL_MATRIX_WORKER") else {
        return;
    };
    let dir = std::env::temp_dir().join(format!(
        "cell-gpu-fault-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    for (fault, expected) in [
        ("rhs", "RHS/key mismatch"),
        ("merge", "merge mismatch"),
        ("missing", "binding mismatch"),
        ("version", "change/version mismatch"),
    ] {
        let proxy = dir.join(fault);
        // Forward the actual Metal worker protocol and corrupt just its output.
        let script = format!(
            r#"#!/usr/bin/env python3
import json, struct, subprocess, sys
p = subprocess.Popen([{worker}, '--serve'], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
def read(f):
    h=f.read(4)
    if not h: return None
    n=struct.unpack('<I',h)[0]
    return json.loads(f.read(n))
def write(f,v):
    b=json.dumps(v).encode();f.write(struct.pack('<I',len(b))+b);f.flush()
try:
    write(sys.stdout.buffer,read(p.stdout))
    while True:
        q=read(sys.stdin.buffer)
        if q is None: break
        write(p.stdin,q);r=read(p.stdout)
        if q['op']=='advance' and 'numeric' in r:
            n=r['numeric']
            if '{fault}'=='rhs':
                i=n['proposals'].index(4);n['proposals'][i]=99
            elif '{fault}'=='merge':
                i=next(i for i,v in enumerate(n['versions']) if v>0);n['values'][i]+=1
            elif '{fault}'=='version':
                i=next(i for i,v in enumerate(n['versions']) if v>0);n['versions'][i]=0
            else:
                for i,d in enumerate(r['decisions']):
                    if d==1:r['decisions'][i]=2
                n['status']=[0]*len(n['status'])
        write(sys.stdout.buffer,r)
finally:
    p.stdin.close();p.wait()
"#,
            worker = serde_json::to_string(&worker.to_string_lossy()).unwrap()
        );
        std::fs::write(&proxy, script).unwrap();
        std::fs::set_permissions(&proxy, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut s = setup_with_worker(&extension(), &proxy, true, false);
        s.input("(Single (Leaf 1)) (set (Best (Leaf 2)) 100) (Offer (Leaf 2) (Single (Leaf 1)) 3) (Offer (Leaf 2) (Single (Leaf 1)) 5)").unwrap();
        let error = s.round().unwrap_err().to_string();
        assert!(error.contains(expected), "{fault}: {error}");
        check(&s, "(check (= (Best (Leaf 2)) 100))");
        assert_eq!(s.stats.native_sets, 0);
        assert_eq!(s.stats.verification.failures, 1);
        assert!(
            s.round()
                .unwrap_err()
                .to_string()
                .contains("start a new session")
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}
