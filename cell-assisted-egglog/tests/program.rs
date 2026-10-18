use cell_assisted_egglog::program::*;
use serde_json::json;
use std::{
    path::Path,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
fn base() -> Program {
    Program {
        version: 1,
        bits: 4,
        slots: 2,
        nodes: vec![
            Node {
                op: Op::Port,
                args: vec![],
                slot: 0,
            },
            Node {
                op: Op::Port,
                args: vec![],
                slot: 1,
            },
        ],
        templates: vec![json!({"domain":"unit coordinate test"})],
        cells: vec![
            Cell {
                template: 0,
                binding: Some(vec![1, 2]),
                partition: vec![0, 1],
            },
            Cell {
                template: 0,
                binding: Some(vec![1, 3]),
                partition: vec![0, 1],
            },
        ],
        conditions: vec![
            Condition {
                left: Wire(0, 0),
                right: Wire(1, 0),
            },
            Condition {
                left: Wire(0, 1),
                right: Wire(1, 1),
            },
        ],
        channels: vec![Channel {
            label: "whole-binding".into(),
            sources: vec![0, 1],
            alternatives: vec![vec![
                Requirement {
                    condition: 0,
                    depth: 0,
                },
                Requirement {
                    condition: 1,
                    depth: 1,
                },
            ]],
            target: None,
        }],
        stages: vec![vec![0]],
        updates: vec![],
        evidence: json!({"scope":"unit coordinate contract"}),
    }
}
fn compare_gpu(p: Program) -> Report {
    let checked = p.check().unwrap();
    let cpu = checked.execute();
    if let Some(worker) = std::env::var_os("CELL_MATRIX_WORKER") {
        static N: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "cell-matrix-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let input = root.join("program.json");
        let out = root.join("report.json");
        std::fs::write(
            &input,
            serde_json::to_vec(&checked.lower().unwrap()).unwrap(),
        )
        .unwrap();
        assert!(
            Command::new(Path::new(&worker))
                .arg(input)
                .arg(&out)
                .status()
                .unwrap()
                .success()
        );
        let gpu: serde_json::Value = serde_json::from_slice(&std::fs::read(out).unwrap()).unwrap();
        assert_eq!(gpu["epochs"], serde_json::to_value(&cpu.epochs).unwrap());
        assert_eq!(gpu["intermediate_binding_readbacks"], 0);
        std::fs::remove_dir_all(root).unwrap();
    }
    cpu
}
#[test]
fn unknown_survives_shallow_success_then_deep_failure() {
    let r = compare_gpu(base());
    let s = &r.epochs[0][0];
    assert_eq!(s.levels[0].continued, 1);
    assert_eq!(s.levels[1].continued, 1);
    assert_eq!(s.levels[2].rejected, 1);
}
#[test]
fn separate_or_branches_cannot_contribute_half_a_match() {
    let mut p = base();
    p.conditions.push(Condition {
        left: Wire(0, 0),
        right: Wire(1, 1),
    });
    p.conditions.push(Condition {
        left: Wire(1, 1),
        right: Wire(1, 1),
    });
    p.channels[0].alternatives.push(vec![
        Requirement {
            condition: 2,
            depth: 0,
        },
        Requirement {
            condition: 3,
            depth: 0,
        },
    ]);
    assert_eq!(compare_gpu(p).epochs[0][0].decisions, vec![2]);
}
fn pair(a: Vec<u32>, b: Vec<u32>) -> Program {
    let mut p = base();
    for (c, v) in p.cells.iter_mut().zip([a, b]) {
        c.partition = (0..v.len())
            .map(|i| v.iter().position(|x| *x == v[i]).unwrap())
            .collect();
        c.binding = Some(v);
    }
    p.nodes.extend([
        Node {
            op: Op::Lift2,
            args: vec![0],
            slot: 0,
        },
        Node {
            op: Op::Lift2,
            args: vec![1],
            slot: 0,
        },
        Node {
            op: Op::Mean,
            args: vec![2, 3],
            slot: 0,
        },
    ]);
    p.conditions = vec![Condition {
        left: Wire(0, 4),
        right: Wire(1, 4),
    }];
    p.channels[0].alternatives = vec![vec![Requirement {
        condition: 0,
        depth: 0,
    }]];
    p
}
#[test]
fn symmetric_and_repeated_pairs_have_exact_marker_codes() {
    assert_eq!(
        compare_gpu(pair(vec![1, 2], vec![2, 1])).epochs[0][0].decisions,
        vec![1]
    );
    assert_eq!(
        compare_gpu(pair(vec![1, 1], vec![1, 1])).epochs[0][0].decisions,
        vec![1]
    );
    assert_eq!(
        compare_gpu(pair(vec![0, 3], vec![1, 2])).epochs[0][0].decisions,
        vec![2]
    );
}
#[test]
fn stale_source_partition_is_pending_not_a_false_match() {
    let mut p = base();
    p.cells[0].binding = Some(vec![1, 1]);
    let r = compare_gpu(p);
    assert_eq!(r.epochs[0][0].live[0], 2);
    assert_eq!(r.epochs[0][0].decisions, vec![0]);
    assert_eq!(r.epochs[0][0].levels[1].work.matrix_calls, 0);
}
#[test]
fn successful_match_with_alias_target_is_preserved_and_blocks_consumers() {
    let mut p = base();
    p.cells.push(Cell {
        template: 0,
        binding: None,
        partition: vec![0, 1],
    });
    p.channels[0].alternatives = vec![vec![Requirement {
        condition: 0,
        depth: 0,
    }]];
    p.channels[0].target = Some(Target {
        certified: true,
        cell: 2,
        bindings: vec![Wire(0, 0), Wire(1, 0)],
        specializations: vec![],
    });
    p.channels.push(Channel {
        label: "next".into(),
        sources: vec![2],
        alternatives: vec![vec![]],
        target: None,
    });
    p.stages.push(vec![1]);
    let r = compare_gpu(p);
    assert_eq!(r.epochs[0][0].decisions, vec![1]);
    assert_eq!(r.epochs[0][0].target_status, vec![2]);
    assert_eq!(r.epochs[0][1].decisions, vec![0]);
}
#[test]
fn input_updates_invalidate_only_dependent_coordinates() {
    let mut p = pair(vec![1, 2], vec![2, 1]);
    p.updates = vec![
        vec![],
        vec![Update {
            cell: 0,
            slot: 1,
            value: 3,
        }],
    ];
    let r = compare_gpu(p);
    assert_eq!(r.epochs[1][0].levels[1].work.matrix_calls, 0);
    assert_eq!(r.epochs[1][0].levels[1].work.computed_features, 0);
    assert_eq!(r.epochs[2][0].decisions, vec![2]);
    assert!(
        r.epochs[2][0].levels[1].work.computed_features
            < r.epochs[0][0].levels[1].work.computed_features
    );
}
#[test]
fn unsafe_shape_and_effect_wiring_are_rejected() {
    let mut p = base();
    p.nodes[1].args = vec![1];
    assert!(p.check().is_err());
    let mut p = base();
    p.channels[0].target = Some(Target {
        certified: true,
        cell: 0,
        bindings: vec![Wire(0, 0), Wire(0, 1)],
        specializations: vec![],
    });
    assert!(p.check().is_err());
    let mut p = base();
    p.conditions[0].left = Wire(8, 0);
    assert!(p.check().is_err());
    let mut p = base();
    p.channels[0].alternatives[0][0].depth = u32::MAX as usize;
    assert!(p.check().is_err());
    let mut p = base();
    p.cells[0].binding = Some(vec![u32::MAX, 2]);
    assert!(p.check().is_err());
}
#[test]
fn layout_is_generated_from_matrix_shapes() {
    let p = base().check().unwrap();
    let lowered = p.lower().unwrap();
    for (name, m) in &lowered.layout {
        assert!(m.offset + m.rows * m.cols <= lowered.data.len());
        assert_eq!(m.offset % 4, 0);
        assert!(lowered.shader.contains(&format!(
            "#define {}_COLS {}u",
            name.to_uppercase(),
            m.cols
        )));
    }
}

fn specialization() -> Program {
    let mut p = base();
    p.templates
        .push(json!({"domain":"unit alias specialization"}));
    p.cells.extend([
        Cell {
            template: 0,
            binding: None,
            partition: vec![0, 1],
        },
        Cell {
            template: 1,
            binding: None,
            partition: vec![0],
        },
    ]);
    p.channels[0].alternatives[0].truncate(1);
    p.channels[0].target = Some(Target {
        certified: true,
        cell: 2,
        bindings: vec![Wire(0, 0), Wire(1, 0)],
        specializations: vec![Specialization {
            cell: 3,
            partition: vec![0, 0],
            projection: vec![0],
            certificate: json!({"scope":"unit coordinate contract; native math proof is checked separately"}),
        }],
    });
    p.conditions.push(Condition {
        left: Wire(3, 0),
        right: Wire(0, 0),
    });
    p.channels.push(Channel {
        label: "consume-specialized-target".into(),
        sources: vec![3, 0],
        alternatives: vec![vec![Requirement {
            condition: 2,
            depth: 0,
        }]],
        target: None,
    });
    p.stages.push(vec![1]);
    p
}
#[test]
fn certified_alias_resumes_without_repeating_the_match() {
    let mut p = specialization();
    p.updates = vec![
        vec![],
        vec![Update {
            cell: 1,
            slot: 0,
            value: 4,
        }],
    ];
    let r = compare_gpu(p);
    assert_eq!(r.epochs[0][0].decisions, vec![1]);
    assert_eq!(r.epochs[0][0].target_status, vec![3]);
    assert_eq!(r.epochs[0][0].specialization_targets, vec![Some(3)]);
    assert_eq!(r.epochs[0][1].decisions, vec![1]);
    assert!(
        r.epochs[1]
            .iter()
            .flat_map(|s| &s.levels)
            .all(|l| l.work.matrix_calls == 0 && l.work.computed_features == 0)
    );
    assert_eq!(r.epochs[2][0].decisions, vec![2]);
    assert_eq!(r.epochs[2][0].specialization_targets, vec![None]);
    assert_eq!(r.epochs[2][1].live[3], 0);
    assert_eq!(r.epochs[2][1].decisions, vec![2]);
}
#[test]
fn specialization_requires_unique_safe_frames_and_distinct_targets() {
    let mut p = specialization();
    p.channels[0].target.as_mut().unwrap().specializations[0].partition = vec![0, 1];
    assert!(p.check().is_err());
    let mut p = specialization();
    p.channels[0].target.as_mut().unwrap().specializations[0].cell = 2;
    assert!(p.check().is_err());
    let mut p = specialization();
    p.channels[0].target.as_mut().unwrap().specializations[0].projection = vec![2];
    assert!(p.check().is_err());
    let mut p = specialization();
    p.channels[0].target.as_mut().unwrap().specializations[0].certificate = serde_json::Value::Null;
    assert!(p.check().is_err());
}

#[test]
fn live_install_preserves_a_completed_match_and_resumes_only_the_target() {
    let Some(worker) = std::env::var_os("CELL_MATRIX_WORKER") else {
        return;
    };
    let mut service =
        cell_assisted_egglog::matrix_service::Service::new(Path::new(&worker)).unwrap();
    let mut p = specialization();
    p.stages.truncate(1);
    p.channels.truncate(1);
    p.conditions.truncate(2);
    p.cells.truncate(3);
    p.channels[0]
        .target
        .as_mut()
        .unwrap()
        .specializations
        .clear();
    p.channels[0].target.as_mut().unwrap().certified = false;
    service.install(&p).unwrap();
    let before = service.step(false, &[]).unwrap();
    assert_eq!(before["outputs"][0]["status"], 2);
    assert_eq!(before["execution_counts"][0][1], 1);
    // Real runtime performs native Bake at this boundary. The already matched
    // target and both source bindings stay in the same worker.
    p.cells.push(Cell {
        template: 1,
        binding: None,
        partition: vec![0],
    });
    p.channels[0].target.as_mut().unwrap().specializations = vec![Specialization {
        cell: 3,
        partition: vec![0, 0],
        projection: vec![0],
        certificate: json!({"scope":"unit contract"}),
    }];
    service.install(&p).unwrap();
    let after = service.step(true, &[]).unwrap();
    assert_eq!(after["outputs"][0]["status"], 3);
    assert_eq!(after["outputs"][0]["specialized"], 3);
    assert_eq!(after["execution_counts"], before["execution_counts"]);
    assert_eq!(after["host_count_reads"], 0);
    let mut bad = p.clone();
    bad.channels[0].alternatives.clear();
    assert!(service.install(&bad).is_err());
    let retired = service.step(true, &[0]).unwrap();
    assert!(retired["outputs"].as_array().unwrap().is_empty());
    assert_eq!(retired["live"][3], 0);
    assert_eq!(retired["execution_counts"], before["execution_counts"]);
}
