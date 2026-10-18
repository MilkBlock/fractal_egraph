use cell_assisted_egglog::{
    numeric::{Compare, Expr, Group as NumGroup, Guard, Merge, Plan, Write},
    program::{self as ir, *},
};
use serde_json::{Value as Json, json};
use std::{
    path::Path,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
fn base(n: usize) -> Program {
    Program {
        version: 1,
        bits: 2,
        slots: 1,
        nodes: vec![Node {
            op: Op::Port,
            args: vec![],
            slot: 0,
        }],
        cells: vec![Cell {
            template: 0,
            binding: Some(vec![0]),
            partition: vec![0],
        }],
        templates: vec![json!({"scope":"numeric unit contract"})],
        conditions: vec![],
        channels: (0..n)
            .map(|i| Channel {
                label: i.to_string(),
                sources: vec![0],
                alternatives: vec![vec![]],
                target: None,
            })
            .collect(),
        stages: vec![(0..n).collect()],
        updates: vec![],
        evidence: json!({"scope":"finite numeric contract"}),
        numeric: Some(Plan {
            inputs: vec![vec![]; n],
            state_bindings: vec![vec![]; n],
            ..Default::default()
        }),
    }
}
fn group(name: &str, merge: Merge, initial: Option<i64>) -> NumGroup {
    NumGroup {
        scope: name.into(),
        key: vec![0],
        merge,
        initial,
    }
}
fn check(p: Program) -> ir::Report {
    let checked = p.check().unwrap();
    let cpu = checked.execute();
    if let Some(worker) = std::env::var_os("CELL_MATRIX_WORKER") {
        static N: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "numeric-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(
            path.join("program.json"),
            serde_json::to_vec(&checked.lower().unwrap()).unwrap(),
        )
        .unwrap();
        assert!(
            Command::new(worker)
                .arg(path.join("program.json"))
                .arg(path.join("report.json"))
                .status()
                .unwrap()
                .success()
        );
        let gpu: Json =
            serde_json::from_slice(&std::fs::read(path.join("report.json")).unwrap()).unwrap();
        assert_eq!(gpu["epochs"], serde_json::to_value(&cpu.epochs).unwrap());
        std::fs::remove_dir_all(path).unwrap();
    }
    cpu
}
#[test]
fn signed_guards_cover_extremes_adjacent_large_integers_and_all_relations() {
    let vals = [
        i64::MIN,
        -10000000000000001,
        -1,
        0,
        1,
        10000000000000000,
        10000000000000001,
        i64::MAX,
    ];
    for (cmp, op) in [
        (Compare::Lt, "<"),
        (Compare::Le, "<="),
        (Compare::Gt, ">"),
        (Compare::Ge, ">="),
        (Compare::Eq, "="),
        (Compare::Ne, "!="),
    ] {
        let mut p = base(64);
        let n = p.numeric.as_mut().unwrap();
        n.exprs = vec![Expr::Input { slot: 0 }, Expr::Input { slot: 1 }];
        n.inputs = vals
            .iter()
            .flat_map(|a| vals.iter().map(|b| vec![*a, *b]))
            .collect();
        n.guards = (0..64)
            .map(|channel| Guard {
                channel,
                left: 0,
                right: 1,
                compare: cmp,
                depth: 0,
            })
            .collect();
        let r = check(p);
        let mut native = egglog::EGraph::default();
        native.parse_and_run_program(None,&format!("(relation A (i64 i64)) (relation B (i64 i64)) (relation Hit (i64 i64)) (rule ((A i a) (B j b) ({op} a b)) ((Hit i j)))")).unwrap();
        for (i, v) in vals.iter().enumerate() {
            native
                .parse_and_run_program(None, &format!("(A {i} {v}) (B {i} {v})"))
                .unwrap();
        }
        native.step_rules("").unwrap();
        for (i, d) in r.epochs[0][0].decisions.iter().enumerate() {
            let a = native.base_to_value((i / 8) as i64);
            let b = native.base_to_value((i % 8) as i64);
            assert_eq!(*d == 1, native.cell_visible_row("Hit", &[a, b]).is_some());
        }
    }
}
#[test]
fn checked_add_sub_mul_are_exact_and_overflow_cannot_commit() {
    let mut values = vec![
        i64::MIN,
        i64::MIN + 1,
        -10000000000000001,
        -4294967296,
        -1,
        0,
        1,
        4294967296,
        10000000000000001,
        i64::MAX,
    ];
    let mut seed = 0x123456789abcdef0u64;
    for _ in 0..8 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        values.push(seed as i64);
    }
    for opcode in 0..3 {
        let nlanes = values.len() * values.len();
        let mut p = base(nlanes);
        let n = p.numeric.as_mut().unwrap();
        n.inputs = values
            .iter()
            .flat_map(|a| values.iter().map(|b| vec![*a, *b]))
            .collect();
        n.exprs = vec![
            Expr::Input { slot: 0 },
            Expr::Input { slot: 1 },
            match opcode {
                0 => Expr::Add { a: 0, b: 1 },
                1 => Expr::Sub { a: 0, b: 1 },
                _ => Expr::Mul { a: 0, b: 1 },
            },
        ];
        n.groups = vec![group("out", Merge::Min, Some(123))];
        n.writes = (0..nlanes)
            .map(|channel| Write {
                channel,
                group: 0,
                value: 2,
            })
            .collect();
        let input = n.inputs.clone();
        let r = check(p);
        let n = r.epochs[0][0].numeric.as_ref().unwrap();
        assert!(n.arithmetic_error);
        assert_eq!(n.values, vec![Some(123)]);
        assert_eq!(n.versions, vec![0]);
        let mut native = egglog::EGraph::default();
        for (i, x) in input.iter().enumerate() {
            let op = ["+", "-", "*"][opcode];
            let e = egglog::ast::Parser::default()
                .get_expr_from_string(None, &format!("({op} {} {})", x[0], x[1]))
                .unwrap();
            let expected = native
                .eval_expr(&e)
                .ok()
                .map(|(_, v)| native.value_to_base::<i64>(v));
            assert_eq!(n.proposals[i], expected);
        }
    }
}
#[test]
fn false_guards_do_not_publish_or_raise_unused_rhs_overflow() {
    let mut p = base(1);
    let n = p.numeric.as_mut().unwrap();
    n.inputs = vec![vec![i64::MAX]];
    n.exprs = vec![
        Expr::Input { slot: 0 },
        Expr::Const { value: 0 },
        Expr::Const { value: 1 },
        Expr::Add { a: 0, b: 2 },
    ];
    n.guards = vec![Guard {
        channel: 0,
        left: 0,
        right: 1,
        compare: Compare::Lt,
        depth: 0,
    }];
    n.groups = vec![group("out", Merge::Min, None)];
    n.writes = vec![Write {
        channel: 0,
        group: 0,
        value: 3,
    }];
    let r = check(p);
    let n = r.epochs[0][0].numeric.as_ref().unwrap();
    assert!(!n.arithmetic_error);
    assert_eq!(n.values, vec![None]);
    assert_eq!(n.status, vec![0]);
    assert_eq!(n.work.proposals, 0);
}
#[test]
fn stage_snapshot_merge_then_successor_guard_and_update() {
    let mut p = base(4);
    p.stages = vec![vec![0, 1, 2], vec![3]];
    let n = p.numeric.as_mut().unwrap();
    n.inputs = vec![vec![12], vec![5], vec![8], vec![0]];
    n.state_bindings = vec![vec![0]; 4];
    n.groups = vec![
        group("min", Merge::Min, Some(10)),
        group("max", Merge::Max, Some(0)),
    ];
    n.exprs = vec![
        Expr::Input { slot: 0 },
        Expr::State { slot: 0 },
        Expr::Const { value: 5 },
        Expr::Const { value: 3 },
        Expr::Mul { a: 1, b: 3 },
        Expr::Const { value: 2 },
        Expr::Add { a: 4, b: 5 },
    ];
    n.guards = vec![Guard {
        channel: 3,
        left: 1,
        right: 2,
        compare: Compare::Eq,
        depth: 0,
    }];
    n.writes = (0..3)
        .map(|channel| Write {
            channel,
            group: 0,
            value: 0,
        })
        .chain([Write {
            channel: 3,
            group: 1,
            value: 6,
        }])
        .collect();
    let r = check(p);
    assert_eq!(
        r.epochs[0][0].numeric.as_ref().unwrap().values,
        vec![Some(5), Some(0)]
    );
    let s = &r.epochs[0][1];
    assert_eq!(s.decisions, vec![1]);
    assert_eq!(s.numeric.as_ref().unwrap().values, vec![Some(5), Some(17)]);
}
#[test]
fn parallel_proposals_min_max_empty_duplicates_and_native_merge_agree() {
    for op in [Merge::Min, Merge::Max] {
        let mut p = base(5);
        let n = p.numeric.as_mut().unwrap();
        n.inputs = vec![vec![9], vec![3], vec![3], vec![-7], vec![i64::MAX]];
        n.exprs = vec![Expr::Input { slot: 0 }];
        n.groups = vec![group("merge", op, Some(4)), group("empty", op, None)];
        n.writes = (0..5)
            .rev()
            .map(|channel| Write {
                channel,
                group: 0,
                value: 0,
            })
            .collect();
        let r = check(p);
        let name = if op == Merge::Min { "min" } else { "max" };
        let mut eg = egglog::EGraph::default();
        eg.parse_and_run_program(
            None,
            &format!("(function F (i64) i64 :merge ({name} old new)) (set (F 0) 4)"),
        )
        .unwrap();
        for v in [9, 3, 3, -7, i64::MAX] {
            eg.parse_and_run_program(None, &format!("(set (F 0) {v})"))
                .unwrap();
        }
        let key = eg.base_to_value(0i64);
        let native =
            eg.value_to_base::<i64>(*eg.cell_visible_row("F", &[key]).unwrap().last().unwrap());
        assert_eq!(
            r.epochs[0][0].numeric.as_ref().unwrap().values,
            vec![Some(native), None]
        );
    }
}
#[test]
fn hot_install_keeps_merged_state_and_never_replays_consumed_writes() {
    let Some(worker) = std::env::var_os("CELL_MATRIX_WORKER") else {
        return;
    };
    let mut p = base(1);
    let n = p.numeric.as_mut().unwrap();
    n.inputs = vec![vec![7]];
    n.state_bindings = vec![vec![0]];
    n.groups = vec![group("best", Merge::Min, Some(10))];
    n.exprs = vec![Expr::Input { slot: 0 }, Expr::State { slot: 0 }];
    n.guards = vec![Guard {
        channel: 0,
        left: 0,
        right: 1,
        compare: Compare::Lt,
        depth: 0,
    }];
    n.writes = vec![Write {
        channel: 0,
        group: 0,
        value: 0,
    }];
    let mut service =
        cell_assisted_egglog::matrix_service::Service::new(Path::new(&worker)).unwrap();
    service.install(&p).unwrap();
    let a = service.step(false, &[]).unwrap();
    assert_eq!(a["numeric"]["values"], json!([7]));
    let b = service.step(true, &[]).unwrap();
    assert_eq!(b["numeric"]["work"]["proposals"], 0);
    assert_eq!(b["numeric"]["work"]["reductions"], 0);
    assert_eq!(a["execution_counts"], b["execution_counts"]);
    p.channels.push(Channel {
        label: "later".into(),
        sources: vec![0],
        alternatives: vec![vec![]],
        target: None,
    });
    p.stages[0].push(1);
    let n = p.numeric.as_mut().unwrap();
    n.inputs.push(vec![3]);
    n.state_bindings.push(vec![0]);
    n.guards.push(Guard {
        channel: 1,
        left: 0,
        right: 1,
        compare: Compare::Lt,
        depth: 0,
    });
    n.writes.push(Write {
        channel: 1,
        group: 0,
        value: 0,
    });
    service.install(&p).unwrap();
    let c = service.step(false, &[]).unwrap();
    assert_eq!(c["numeric"]["values"], json!([3]));
    assert_eq!(c["numeric"]["versions"], json!([2]));
    assert_eq!(c["execution_counts"][0], a["execution_counts"][0]);
}
#[test]
fn invalid_numeric_shapes_cycles_and_duplicate_keys_are_rejected() {
    let mut p = base(1);
    p.numeric.as_mut().unwrap().exprs = vec![Expr::Add { a: 0, b: 0 }];
    assert!(p.check().is_err());
    let mut p = base(1);
    p.numeric.as_mut().unwrap().groups = vec![
        group("same", Merge::Min, None),
        group("same", Merge::Max, None),
    ];
    assert!(p.check().is_err());
    let mut p = base(1);
    p.numeric.as_mut().unwrap().inputs.clear();
    assert!(p.check().is_err());
}

#[test]
fn computed_guard_overflow_is_a_nonmatch_and_absent_merge_can_store_min_i64() {
    let mut p = base(3);
    let n = p.numeric.as_mut().unwrap();
    n.inputs = vec![vec![i64::MAX], vec![-1], vec![1]];
    n.exprs = vec![
        Expr::Input { slot: 0 },
        Expr::Const { value: 1 },
        Expr::Add { a: 0, b: 1 },
        Expr::Const { value: 0 },
    ];
    n.guards = (0..3)
        .map(|channel| Guard {
            channel,
            left: 2,
            right: 3,
            compare: Compare::Gt,
            depth: 0,
        })
        .collect();
    let r = check(p);
    assert_eq!(r.epochs[0][0].decisions, vec![2, 2, 1]);
    assert!(!r.epochs[0][0].numeric.as_ref().unwrap().arithmetic_error);
    let mut native = egglog::EGraph::default();
    native.parse_and_run_program(None,"(relation P (i64)) (relation Hit (i64)) (rule ((P n) (= m (+ n 1)) (> m 0)) ((Hit n))) (P 9223372036854775807) (P -1) (P 1) (run 1)").unwrap();
    assert_eq!(native.get_size("Hit"), 1);
    for merge in [Merge::Min, Merge::Max] {
        let mut p = base(1);
        let n = p.numeric.as_mut().unwrap();
        n.exprs = vec![Expr::Const { value: i64::MIN }];
        n.groups = vec![group("empty", merge, None)];
        n.writes = vec![Write {
            channel: 0,
            group: 0,
            value: 0,
        }];
        let r = check(p);
        assert_eq!(
            r.epochs[0][0].numeric.as_ref().unwrap().values,
            vec![Some(i64::MIN)]
        );
        assert_eq!(r.epochs[0][0].numeric.as_ref().unwrap().versions, vec![1]);
    }
}
