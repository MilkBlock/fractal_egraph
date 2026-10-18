use cell_assisted_egglog::{
    layout::{Block, Method, Plan, Planner},
    program::*,
};
use serde_json::{Value, json};
use std::{path::Path, process::Command};

fn mixed() -> Program {
    let mut p = Program {
        version: 1,
        bits: 32,
        slots: 1,
        nodes: vec![Node {
            op: Op::Port,
            args: vec![],
            slot: 0,
        }],
        templates: vec![json!({"scope":"layout correctness fixture"})],
        cells: (0..52)
            .map(|i| Cell {
                template: 0,
                binding: Some(vec![i % 4]),
                partition: vec![0],
            })
            .collect(),
        conditions: vec![],
        channels: vec![],
        stages: vec![vec![]],
        updates: vec![vec![Update {
            cell: 0,
            slot: 0,
            value: 31,
        }]],
        numeric: None,
        evidence: json!({}),
    };
    for (a, b) in (0..8)
        .flat_map(|a| (8..20).map(move |b| (a, b)))
        .chain((0..16).map(|i| (20 + 2 * i, 21 + 2 * i)))
    {
        let id = p.conditions.len();
        p.conditions.push(Condition {
            left: Wire(a, 0),
            right: Wire(b, 0),
        });
        p.channels.push(Channel {
            label: format!("pair-{id}"),
            sources: vec![a, b],
            alternatives: vec![vec![Requirement {
                condition: id,
                depth: 0,
            }]],
            target: None,
        });
        p.stages[0].push(id);
    }
    p
}
fn gpu(c: &Checked, plan: &Plan) -> Option<Value> {
    let worker = std::env::var_os("CELL_MATRIX_WORKER")?;
    let dir = std::env::temp_dir().join(format!(
        "cell-layout-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("plan.json");
    let out = dir.join("result.json");
    std::fs::write(
        &path,
        serde_json::to_vec(&c.lower_with_layout(plan).unwrap()).unwrap(),
    )
    .unwrap();
    assert!(
        Command::new(worker)
            .arg(&path)
            .arg(&out)
            .status()
            .unwrap()
            .success()
    );
    let result: Value = serde_json::from_slice(&std::fs::read(out).unwrap()).unwrap();
    let expected = serde_json::to_value(c.execute()).unwrap();
    for (e, epoch) in expected["epochs"].as_array().unwrap().iter().enumerate() {
        for (s, stage) in epoch.as_array().unwrap().iter().enumerate() {
            for field in [
                "decisions",
                "live",
                "cells",
                "target_status",
                "specialization_targets",
            ] {
                assert_eq!(stage[field], result["epochs"][e][s][field], "{field}");
            }
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
    Some(result)
}
#[test]
fn paired_rectangular_and_mixed_layouts_preserve_updates_and_full_bindings() {
    let c = mixed().check().unwrap();
    for method in [Method::Gram, Method::Paired, Method::Rectangular] {
        let plan = Plan::uniform(&c, method.clone());
        if let Some(r) = gpu(&c, &plan) {
            if method == Method::Paired {
                assert_eq!(r["host_count_reads"], 0);
                assert!(r["layout_work"]["paired_comparisons"].as_u64().unwrap() > 0);
            }
        }
    }
    gpu(
        &c,
        &Plan {
            blocks: vec![
                Block {
                    method: Method::Rectangular,
                    conditions: (0..96).collect(),
                },
                Block {
                    method: Method::Paired,
                    conditions: (96..112).collect(),
                },
            ],
            report: json!({}),
        },
    );
}
#[test]
fn incomplete_or_duplicate_physical_plans_cannot_change_semantics() {
    let c = mixed().check().unwrap();
    let mut p = Plan::uniform(&c, Method::Paired);
    p.blocks[0].conditions.pop();
    assert!(c.lower_with_layout(&p).is_err());
    p.blocks[0].conditions.push(0);
    assert!(c.lower_with_layout(&p).is_err());
}
#[test]
fn layouts_preserve_moment_features_branch_scopes_and_target_gathers() {
    let mut p = mixed();
    p.bits = 4;
    p.slots = 2;
    p.cells = vec![
        Cell {
            template: 0,
            binding: Some(vec![1, 2]),
            partition: vec![0, 1],
        },
        Cell {
            template: 0,
            binding: Some(vec![2, 1]),
            partition: vec![0, 1],
        },
        Cell {
            template: 0,
            binding: Some(vec![3, 4]),
            partition: vec![0, 1],
        },
        Cell {
            template: 0,
            binding: None,
            partition: vec![0, 1],
        },
    ];
    p.nodes = vec![
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
    ];
    p.conditions = vec![
        Condition {
            left: Wire(0, 4),
            right: Wire(1, 4),
        },
        Condition {
            left: Wire(0, 0),
            right: Wire(2, 0),
        },
        Condition {
            left: Wire(0, 4),
            right: Wire(2, 4),
        },
        Condition {
            left: Wire(0, 0),
            right: Wire(1, 1),
        },
    ];
    p.channels = vec![
        Channel {
            label: "no-half-bindings".into(),
            sources: vec![0, 1, 2],
            alternatives: vec![
                vec![
                    Requirement {
                        condition: 0,
                        depth: 0,
                    },
                    Requirement {
                        condition: 1,
                        depth: 1,
                    },
                ],
                vec![
                    Requirement {
                        condition: 2,
                        depth: 0,
                    },
                    Requirement {
                        condition: 3,
                        depth: 1,
                    },
                ],
            ],
            target: None,
        },
        Channel {
            label: "symmetric-pair".into(),
            sources: vec![0, 1],
            alternatives: vec![vec![Requirement {
                condition: 0,
                depth: 0,
            }]],
            target: Some(Target {
                certified: true,
                cell: 3,
                bindings: vec![Wire(0, 0), Wire(0, 1)],
                specializations: vec![],
            }),
        },
    ];
    p.stages = vec![vec![0, 1]];
    p.updates = vec![vec![Update {
        cell: 0,
        slot: 0,
        value: 4,
    }]];
    let c = p.check().unwrap();
    assert_eq!(c.execute().epochs[0][0].decisions, vec![2, 1]);
    for method in [Method::Paired, Method::Rectangular] {
        gpu(&c, &Plan::uniform(&c, method));
    }
}
#[test]
fn solver_selects_dense_and_sparse_blocks_and_reuses_identical_shape_plan() {
    let Some(python) = std::env::var_os("CELL_LAYOUT_PYTHON") else {
        return;
    };
    let c = mixed().check().unwrap();
    let mut planner = Planner::default();
    planner.mode = "solver".into();
    planner.python = Path::new(&python).into();
    let plan = planner.plan(&c).unwrap();
    assert!(plan.blocks.iter().any(|b| b.method == Method::Paired));
    assert!(plan.blocks.iter().any(|b| b.method == Method::Rectangular));
    planner.plan(&c).unwrap();
    assert_eq!(planner.solves, 1);
    assert_eq!(planner.cache_hits, 1);
    gpu(&c, &plan);
}
