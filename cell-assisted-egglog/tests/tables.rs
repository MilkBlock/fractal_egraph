use cell_assisted_egglog::{
    Part, Session, environment, layout::Planner, numeric_native::NumericSession,
};
use std::path::Path;

fn extension() -> String {
    include_str!("../matrix-numeric/consumer.egg").into()
}
fn session(ext: &str, group_get: bool) -> Option<NumericSession> {
    let worker = std::env::var_os("CELL_MATRIX_WORKER")?;
    Some(with_worker(ext, group_get, Path::new(&worker)))
}
fn with_worker(ext: &str, group_get: bool, worker: &Path) -> NumericSession {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/analyze");
    let env = format!("{}\n{ext}", environment(&dir).unwrap().0);
    let mut inspect = egglog::EGraph::default();
    inspect.parse_and_run_program(None, &env).unwrap();
    let sink = inspect.prepare_cell_action("", "cost-improve").unwrap();
    let mut parts = vec![];
    let mut cell = vec![];
    for (i, a) in sink.atoms.iter().enumerate() {
        if a.function == "Single" || (group_get && a.function == "Get") {
            cell.push(i);
        } else {
            parts.push(Part {
                atoms: vec![i],
                cell: false,
            });
        }
    }
    parts.insert(
        0,
        Part {
            atoms: cell,
            cell: true,
        },
    );
    let source = Session::with_extension(
        &dir,
        "",
        "cost-improve",
        vec!["single-get".into()],
        parts,
        ext,
    )
    .unwrap();
    let mut s = NumericSession::new(source, &env, worker).unwrap();
    s.set_gpu_check(true).unwrap();
    let mut p = Planner::default();
    p.mode = "tables".into();
    s.configure_layout("device", p).unwrap();
    s
}
fn rounds(s: &mut NumericSession) {
    for _ in 0..4 {
        s.round().unwrap();
    }
}
fn check(s: &NumericSession, code: &str) {
    s.engine()
        .clone()
        .parse_and_run_program(None, code)
        .unwrap();
}

#[test]
fn binary_table_deltas_keep_exact_large_integers_and_group_commits() {
    let Some(mut s) = session(&extension(), false) else {
        return;
    };
    s.input(include_str!("../matrix-numeric/input.egg"))
        .unwrap();
    rounds(&mut s);
    check(
        &s,
        "(check (= (Best (Leaf 200)) 9999999999999999)) (check (= (Best (Leaf 201)) 2)) (check (= (Best (Leaf 202)) 1))",
    );
    assert_eq!(s.stats.accepted_bindings, 3);
    assert_eq!(s.stats.native_sets, 2);
    assert_eq!(s.stats.channels, 0);
    assert_eq!(s.stats.phase_seconds.routing_seconds, 0.);
    assert_eq!(s.stats.physical_layout["solver_calls"], 0);
    assert_eq!(s.stats.install_breakdown["shader_compiles"], 1);
    assert!(s.stats.table_uploaded_bytes < 1024);
    let bytes = s.stats.table_uploaded_bytes;
    let records = s.stats.table_match_records;
    rounds(&mut s);
    assert_eq!(bytes, s.stats.table_uploaded_bytes);
    assert_eq!(records, s.stats.table_match_records);
}
#[test]
fn table_union_rekey_and_late_rows_do_not_recompute_completed_matches() {
    let Some(mut s) = session(&extension(), false) else {
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
    assert_eq!(s.stats.accepted_bindings, 2);
    assert_eq!(s.stats.native_sets, 1);
    assert_eq!(s.stats.table_match_records, 2);
    s.input("(Offer (Leaf 3) (Single (Leaf 1)) 0)").unwrap();
    rounds(&mut s);
    check(&s, "(check (= (Best (Leaf 2)) -1))");
    assert_eq!(s.stats.table_match_records, 3);
}
#[test]
fn overflow_recovery_keeps_completed_gpu_rows_and_never_partially_commits() {
    let ext = extension()
        .replace("min old new", "max old new")
        .replace("(< cost old)", "(>= cost old)")
        .replace("(- cost 1)", "(+ cost 1)");
    let Some(mut s) = session(&ext, false) else {
        return;
    };
    s.input("(Single (Leaf 1)) (set (Best (Leaf 2)) 0) (Offer (Leaf 2) (Single (Leaf 1)) 4) (Offer (Leaf 2) (Single (Leaf 1)) 9223372036854775807)").unwrap();
    assert!(
        s.round()
            .unwrap_err()
            .to_string()
            .contains("checked i64 RHS failed")
    );
    check(&s, "(check (= (Best (Leaf 2)) 0))");
    assert_eq!(s.stats.table_match_records, 2);
    s.input("(delete (Offer (Leaf 2) (Single (Leaf 1)) 9223372036854775807))")
        .unwrap();
    rounds(&mut s);
    check(&s, "(check (= (Best (Leaf 2)) 5))");
    assert_eq!(s.stats.table_match_records, 2);
    assert_eq!(s.stats.accepted_bindings, 1);
}
#[test]
fn completed_noop_matches_remain_auditable_and_deleted_rows_can_be_reinserted() {
    let ext = "(function Best (Expr) i64 :merge (min old new)) (relation Offer (Expr Expr i64)) (rule ((Offer dst (Single x) cost)) ((set (Best dst) cost)) :name \"cost-improve\")";
    let Some(mut s) = session(ext, false) else {
        return;
    };
    s.input("(Single (Leaf 1)) (Offer (Leaf 2) (Single (Leaf 1)) 3)")
        .unwrap();
    rounds(&mut s);
    s.input("(delete (Offer (Leaf 2) (Single (Leaf 1)) 3))")
        .unwrap();
    rounds(&mut s);
    s.input("(Offer (Leaf 2) (Single (Leaf 1)) 3)").unwrap();
    rounds(&mut s);
    assert_eq!(s.stats.accepted_bindings, 2);
    assert_eq!(s.stats.native_sets, 1);
    assert_eq!(s.stats.table_match_records, 2);
    check(&s, "(check (= (Best (Leaf 2)) 3))");
}
#[test]
fn real_cartesian_results_grow_storage_without_truncation() {
    let ext = "(function Best () i64 :merge (min old new)) (relation Pick (i64)) (rule ((Single x) (Pick cost)) ((set (Best) cost)) :name \"cost-improve\")";
    let Some(mut s) = session(ext, false) else {
        return;
    };
    let input = (0..32)
        .map(|i| format!("(Single (Leaf {i})) (Pick {i})"))
        .collect::<Vec<_>>()
        .join("\n");
    s.input(&input).unwrap();
    rounds(&mut s);
    check(&s, "(check (= (Best) 0))");
    assert_eq!(s.stats.accepted_bindings, 1024);
    assert_eq!(s.stats.table_match_records, 1024);
    assert!(s.stats.table_retries > 0);
    s.input("(Pick 32)").unwrap();
    rounds(&mut s);
    assert_eq!(s.stats.table_match_records, 1056);
    assert_eq!(s.stats.accepted_bindings, 1056);
    assert_eq!(s.stats.native_sets, 1);
}
#[test]
fn a_cell_can_supply_multiple_lhs_atoms_as_one_device_table() {
    let ext = extension().replace("(Single x) cost", "(Get (Single x) 0) cost");
    let Some(mut s) = session(&ext, true) else {
        return;
    };
    s.input("(Single (Leaf 1)) (set (Best (Leaf 2)) 100) (Offer (Leaf 2) (Leaf 1) 3)")
        .unwrap();
    rounds(&mut s);
    check(&s, "(check (= (Best (Leaf 2)) 2))");
    assert_eq!(s.stats.accepted_bindings, 1);
}
#[test]
fn table_numeric_dag_uses_exact_signed_multiplication() {
    let ext = extension().replace("(- cost 1)", "(* (- cost 2) 3)");
    let Some(mut s) = session(&ext, false) else {
        return;
    };
    s.input("(Single (Leaf 1)) (set (Best (Leaf 2)) 1000000000000000000) (Offer (Leaf 2) (Single (Leaf 1)) 9007199254740995)").unwrap();
    rounds(&mut s);
    check(&s, "(check (= (Best (Leaf 2)) 27021597764222979))");
    s.input("(Offer (Leaf 2) (Single (Leaf 1)) -5)").unwrap();
    rounds(&mut s);
    check(&s, "(check (= (Best (Leaf 2)) -21))");
}

#[test]
fn table_check_can_toggle_and_false_guards_do_not_raise_rhs_overflow() {
    let ext = extension().replace("(- cost 1)", "(* cost cost)");
    let Some(mut s) = session(&ext, false) else {
        return;
    };
    s.set_gpu_check(false).unwrap();
    s.input("(Single (Leaf 1)) (set (Best (Leaf 2)) 0) (Offer (Leaf 2) (Single (Leaf 1)) 9223372036854775807)").unwrap();
    rounds(&mut s);
    assert_eq!(s.stats.accepted_bindings, 0);
    assert_eq!(s.stats.verification.batches, 0);
    s.set_gpu_check(true).unwrap();
    s.input("(Offer (Leaf 2) (Single (Leaf 1)) -1)").unwrap();
    rounds(&mut s);
    check(&s, "(check (= (Best (Leaf 2)) 0))");
    assert_eq!(s.stats.accepted_bindings, 1);
    assert_eq!(s.stats.native_sets, 0);
}

#[test]
fn table_check_rejects_missing_cell_coverage_instead_of_silently_accepting_no_matches() {
    let Some(mut s) = session(&extension(), false) else {
        return;
    };
    s.input("(Single (Leaf 1)) (subsume (Get (Single (Leaf 1)) 0)) (set (Best (Leaf 2)) 100) (Offer (Leaf 2) (Single (Leaf 1)) 3)").unwrap();
    assert!(s.round().unwrap_err().to_string().contains("missing"));
    check(&s, "(check (= (Best (Leaf 2)) 100))");
    assert_eq!(s.stats.native_sets, 0);
}
#[cfg(unix)]
#[test]
fn corrupted_binary_proposals_and_reductions_never_reach_native_storage() {
    use std::os::unix::fs::PermissionsExt;
    let Some(worker) = std::env::var_os("CELL_MATRIX_WORKER") else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("table-packet-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (fault, message) in [
        ("rhs", "RHS/key mismatch"),
        ("group", "reduction mismatch"),
        ("receipt", "match receipt"),
        ("missing", "omitted a pending key"),
    ] {
        let path = dir.join(fault);
        std::fs::write(
            &path,
            format!(
                r#"#!/usr/bin/env python3
import json,struct,subprocess,sys
p=subprocess.Popen([{worker},'--serve'],stdin=subprocess.PIPE,stdout=subprocess.PIPE)
def raw(f):
 h=f.read(4)
 if not h:return None
 n=struct.unpack('<I',h)[0];return f.read(n)
def send(f,b):f.write(struct.pack('<I',len(b))+b);f.flush()
try:
 send(sys.stdout.buffer,raw(p.stdout))
 while True:
  b=raw(sys.stdin.buffer)
  if b is None:break
  q=json.loads(b);send(p.stdin,b)
  if q['op']=='table_delta':send(p.stdin,raw(sys.stdin.buffer))
  r=json.loads(raw(p.stdout));body=None
  if q['op']=='table_delta' and 'error' not in r:
   body=raw(p.stdout);v=list(struct.unpack('<'+'I'*(len(body)//4),body));n=r['audit_rows'];stride=8
   if r['pending_matches']:
    if '{fault}'=='rhs':
     i=next(i for i in range(n) if v[i*stride+3]==4);v[i*stride+3]=99
    elif '{fault}'=='group':v[n*stride+1]+=1
    elif '{fault}'=='receipt':v[1]=0
    else:r['groups']=0;v=v[:n*stride]
    body=struct.pack('<'+'I'*len(v),*v)
  send(sys.stdout.buffer,json.dumps(r).encode())
  if body is not None:send(sys.stdout.buffer,body)
finally:p.stdin.close();p.wait()
"#,
                worker = serde_json::to_string(&worker.to_string_lossy()).unwrap()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut s = with_worker(&extension(), false, &path);
        s.input("(Single (Leaf 1)) (set (Best (Leaf 2)) 100) (Offer (Leaf 2) (Single (Leaf 1)) 3) (Offer (Leaf 2) (Single (Leaf 1)) 5)").unwrap();
        let e = s.round().unwrap_err().to_string();
        assert!(e.contains(message), "{fault}: {e}");
        check(&s, "(check (= (Best (Leaf 2)) 100))");
        assert_eq!(s.stats.native_sets, 0);
    }
    std::fs::remove_dir_all(dir).unwrap();
}
