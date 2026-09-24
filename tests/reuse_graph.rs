use egg_layout::{
    coarse_smooth::{Comb, CombKind, RelativeBinding},
    reuse_graph::*,
    saturated_rule_composition::{Row, SaturatedRuleComposition, Vertex},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    process::Command,
};

// Synthetic state/metadata for the data-model tests. The final test imports
// real native artifacts instead of treating these fixtures as runtime evidence.
fn state() -> SaturatedRuleComposition {
    SaturatedRuleComposition {
        version: 2,
        local_ids: vec![],
        scope: json!({"rules":["add-mul"],"datatype":"E"}),
        values: vec![
            Vertex {
                sort: "E".into(),
                literal: None
            };
            3
        ],
        rows: vec![
            Row {
                op: "Add".into(),
                args: vec![0, 1],
                result: 2,
            },
            Row {
                op: "Mul".into(),
                args: vec![1, 0],
                result: 2,
            },
        ],
        subsumed_rows: vec![],
        ports: BTreeMap::new(),
    }
}
fn report(g: &SaturatedRuleComposition) -> Value {
    json!({"ripen":{"state":"Saturated","updated":false},"checks":"passed","native_stop_reason":null,"saturated_rule_composition":{"status":"exported","values":g.values.len(),"facts":g.rows.len()}})
}
fn body() -> RipSatEgraph {
    let g = state();
    let r = report(&g);
    RipSatEgraph::from_native_export(g, &r).unwrap()
}
fn origin(rule: &str) -> Comb {
    Comb {
        kind: CombKind::CoarseRuleComposition,
        rule: rule.into(),
        input_roles: vec!["root".into()],
        output_roles: vec!["root".into()],
        parents: vec![],
        binding: vec![RelativeBinding::External { slot: 0 }],
        aliases: vec![0, 0, 0],
        input_count: 1,
        output_count: 1,
        external_count: 1,
        requirements: vec![],
        effects: vec![],
        external_facts: vec![],
    }
}
fn descriptor(body: RipSatId, rule: &str) -> EnclosureDescriptor {
    EnclosureDescriptor {
        body,
        interface: Interface {
            upper: vec![2],
            lower: vec![0, 1],
        },
        origin: vec![origin(rule)],
    }
}
fn value(id: u64) -> RuntimeValue {
    RuntimeValue {
        sort: "E".into(),
        id,
    }
}
fn fact(table: &str, id: u64) -> RuntimeFact {
    RuntimeFact {
        run: 10,
        table: table.into(),
        id,
        version: 0,
    }
}
fn bind_reads(r: &mut ReuseGraph, i: InstanceId, rev: u64) {
    r.bind_fact_slot(i, rev, 0, fact("Add", 1)).unwrap();
    r.bind_fact_slot(i, rev, 1, fact("Mul", 2)).unwrap();
}
fn setup() -> (ReuseGraph, EnclosureId, InstanceId) {
    let mut graph = ReuseGraph::default();
    let b = graph.intern_body(body()).unwrap().id;
    let e = graph
        .register_enclosure(descriptor(b, "Add-to-Mul"), Admission::InitialBake)
        .unwrap();
    let i = graph
        .instantiate(e, 10, vec![value(100)], vec![value(101), value(102)])
        .unwrap();
    bind_reads(&mut graph, i, 0);
    (graph, e, i)
}
#[test]
fn completed_body_gate_rejects_prefixes_interruption_and_mismatched_exports() {
    for field in ["state", "updated", "checks", "stop", "facts", "observed"] {
        let mut g = state();
        let mut r = report(&g);
        match field {
            "state" => r["ripen"]["state"] = json!("ObservedUnfinished"),
            "updated" => r["ripen"]["updated"] = json!(true),
            "checks" => r["checks"] = json!("deferred"),
            "stop" => r["native_stop_reason"] = json!("time"),
            "facts" => r["saturated_rule_composition"]["facts"] = json!(99),
            _ => g.scope["observation_only"] = json!(true),
        }
        assert!(RipSatEgraph::from_native_export(g, &r).is_err(), "{field}");
    }
}
#[test]
fn equivalent_bodies_share_storage_but_different_origins_do_not_merge() {
    let mut registry = ReuseGraph::default();
    let first = registry.intern_body(body()).unwrap();
    let mut g = state();
    let map = [2, 0, 1];
    for r in &mut g.rows {
        r.args.iter_mut().for_each(|v| *v = map[*v]);
        r.result = map[r.result];
    }
    let r = report(&g);
    let second = registry
        .intern_body(RipSatEgraph::from_native_export(g, &r).unwrap())
        .unwrap();
    assert_eq!(first.id, second.id);
    assert!(!second.inserted);
    for i in 0..3 {
        assert_eq!(second.value_map[map[i]], i);
    }
    let a = registry
        .register_enclosure(descriptor(first.id, "Add-to-Mul"), Admission::InitialBake)
        .unwrap();
    let b = registry
        .register_enclosure(
            descriptor(second.id, "Mul-to-Add"),
            Admission::RuntimeSupplement,
        )
        .unwrap();
    assert_ne!(a, b);
    assert_eq!(registry.metrics().new_enclosure_count, 1);
    assert_eq!(
        registry.report()["rip_sat_egraphs"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let mut same = descriptor(first.id, "Mul-to-Add");
    same.origin[0].aliases = vec![71, 71, 71];
    assert_eq!(
        registry
            .register_enclosure(same, Admission::RuntimeSupplement)
            .unwrap(),
        b
    );
    assert_eq!(registry.metrics().new_enclosure_count, 1);
}
#[test]
fn parameter_renaming_creates_instances_not_new_enclosures() {
    let (mut r, e, i) = setup();
    assert_eq!(
        r.instantiate(e, 10, vec![value(100)], vec![value(101), value(102)])
            .unwrap(),
        i
    );
    assert_ne!(
        r.instantiate(e, 10, vec![value(200)], vec![value(201), value(202)])
            .unwrap(),
        i
    );
    assert_ne!(
        r.instantiate(e, 11, vec![value(100)], vec![value(101), value(102)])
            .unwrap(),
        i
    );
    assert_eq!(r.metrics(), &Metrics::default()); // registering is not applied reuse
    assert!(r.instantiate(e, 10, vec![], vec![]).is_err());
    assert!(
        r.instantiate(
            e,
            10,
            vec![RuntimeValue {
                sort: "Other".into(),
                id: 100
            }],
            vec![value(101), value(102)]
        )
        .is_err()
    );
}
#[test]
fn reuse_is_idempotent_run_scoped_and_revision_checked() {
    let (mut r, _, i) = setup();
    let event = RuntimeEvent { run: 10, id: 7 };
    assert!(
        r.record_applied_reuse(i, 0, event, ReuseKind::BodyStorage)
            .unwrap()
    );
    assert!(
        !r.record_applied_reuse(i, 0, event, ReuseKind::BodyStorage)
            .unwrap()
    );
    assert!(
        r.record_applied_reuse(
            i,
            0,
            RuntimeEvent { run: 99, id: 8 },
            ReuseKind::BodyStorage
        )
        .is_err()
    );
    assert_eq!(r.advance_revision(i).unwrap(), 1);
    assert!(
        r.record_applied_reuse(i, 0, event, ReuseKind::BodyStorage)
            .is_err()
    );
    assert!(
        r.record_applied_reuse(i, 1, event, ReuseKind::BodyStorage)
            .is_err()
    );
    assert_eq!(r.metrics().reuse_count, 1);
}
#[test]
fn graft_degree_counts_slots_and_break_counts_causal_episodes() {
    let (mut r, _, i) = setup();
    let g = Graft {
        event: RuntimeEvent { run: 10, id: 20 },
        episode: 19,
        instance: i,
        revision: 0,
        rule: "graft".into(),
        upper: BTreeSet::from([0, 0]),
        lower: BTreeSet::from([0, 1, 0]),
        inner_facts: BTreeMap::from([(0, fact("Add", 1)), (1, fact("Mul", 2))]),
    };
    assert_eq!(
        g.connectivity(),
        Connectivity {
            upper: 1,
            lower: 2,
            inner_nodes_used: 2
        }
    );
    assert!(r.observe_graft(g.clone(), true).unwrap());
    assert!(!r.observe_graft(g.clone(), true).unwrap());
    let mut other = g.clone();
    other.event.id = 21;
    r.observe_graft(other, true).unwrap();
    r.advance_revision(i).unwrap();
    bind_reads(&mut r, i, 1);
    let mut follow = g.clone();
    follow.revision = 1;
    follow.event.id = 22;
    r.observe_graft(follow, true).unwrap();
    assert_eq!(r.metrics().unexpected_break_count, 1);
    assert_eq!(r.report()["grafts"].as_array().unwrap().len(), 3);
    let mut expected = g.clone();
    expected.revision = 1;
    expected.event.id = 23;
    expected.episode = 23;
    r.observe_graft(expected, false).unwrap();
    assert_eq!(r.metrics().unexpected_break_count, 1);
    let mut bad = g;
    bad.revision = 1;
    bad.event.id = 24;
    bad.lower.insert(99);
    assert!(r.observe_graft(bad, true).is_err());
    assert_eq!(r.metrics().unexpected_break_count, 1);
}
#[test]
fn interface_aliases_are_preserved_without_collapsing_port_degree() {
    let mut r = ReuseGraph::default();
    let body = r.intern_body(body()).unwrap().id;
    let mut d = descriptor(body, "R");
    d.interface.lower = vec![0, 0];
    let e = r.register_enclosure(d, Admission::InitialBake).unwrap();
    assert!(
        r.instantiate(e, 1, vec![value(20)], vec![value(21), value(22)])
            .is_err()
    );
    let i = r
        .instantiate(e, 1, vec![value(20)], vec![value(21), value(21)])
        .unwrap();
    let g = Graft {
        event: RuntimeEvent { run: 1, id: 1 },
        episode: 1,
        instance: i,
        revision: 0,
        rule: "R".into(),
        upper: BTreeSet::new(),
        lower: BTreeSet::from([0, 1]),
        inner_facts: BTreeMap::new(),
    };
    assert_eq!(g.connectivity().lower, 2);
    r.observe_graft(g, false).unwrap();
}
#[test]
fn malformed_origin_does_not_register_or_increment_counts() {
    let mut r = ReuseGraph::default();
    let b = r.intern_body(body()).unwrap().id;
    let mut d = descriptor(b, "R");
    d.origin[0].parents = vec![0];
    assert!(
        r.register_enclosure(d, Admission::RuntimeSupplement)
            .is_err()
    );
    assert_eq!(r.metrics().new_enclosure_count, 0);
}
#[test]
fn actual_native_artifacts_admit_saturation_but_not_growth_prefixes() {
    let root = std::env::temp_dir().join(format!("reuse-native-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let finite = root.join("finite.egg");
    fs::write(
        &finite,
        "(datatype E (V i64) (A E) (B E)) (rewrite (A x) (B x)) (A (V 1)) (run 1)",
    )
    .unwrap();
    for (label, path) in [
        ("finite", finite),
        (
            "grow",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("experiments/reentry_forms/dabc.egg"),
        ),
    ] {
        let out = root.join(label);
        let run = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
            .args([
                "analyze",
                "--source",
                path.to_str().unwrap(),
                "--output",
                out.to_str().unwrap(),
                "--ripen-observe-ms",
                "60000",
                "--ripen-observe-facts",
                "80",
            ])
            .env("EGG_LAYOUT_GRAPHS", "0")
            .output()
            .unwrap();
        assert!(
            run.status.success(),
            "{}",
            String::from_utf8_lossy(&run.stderr)
        );
        let cell = out.join("saturated-rule-composition/cells/CSUnit-000000");
        let r: Value = serde_json::from_slice(&fs::read(cell.join("ripen.json")).unwrap()).unwrap();
        let file = if label == "finite" {
            "saturated-rule-composition.json"
        } else {
            "unfinished-state.json"
        };
        let g = serde_json::from_slice(&fs::read(cell.join(file)).unwrap()).unwrap();
        assert_eq!(
            RipSatEgraph::from_native_export(g, &r).is_ok(),
            label == "finite"
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn aliased_fact_slots_count_one_actual_node_and_stale_maps_are_rejected() {
    let mut g = state();
    g.rows[1].op = "Add".into();
    let r = report(&g);
    let mut graph = ReuseGraph::default();
    let body = graph
        .intern_body(RipSatEgraph::from_native_export(g, &r).unwrap())
        .unwrap()
        .id;
    let e = graph
        .register_enclosure(descriptor(body, "R"), Admission::InitialBake)
        .unwrap();
    let i = graph
        .instantiate(e, 10, vec![value(100)], vec![value(101), value(101)])
        .unwrap();
    let row = fact("Add", 1);
    graph.bind_fact_slot(i, 0, 0, row.clone()).unwrap();
    graph.bind_fact_slot(i, 0, 1, row.clone()).unwrap();
    let graft = Graft {
        event: RuntimeEvent { run: 10, id: 1 },
        episode: 1,
        instance: i,
        revision: 0,
        rule: "graft".into(),
        upper: BTreeSet::new(),
        lower: BTreeSet::from([0, 1]),
        inner_facts: BTreeMap::from([(0, row.clone()), (1, row.clone())]),
    };
    assert_eq!(
        graft.connectivity(),
        Connectivity {
            upper: 0,
            lower: 2,
            inner_nodes_used: 1
        }
    );
    graph.observe_graft(graft.clone(), true).unwrap();
    assert!(graph.bind_fact_slot(i, 0, 1, fact("Mul", 1)).is_err());
    graph.advance_revision(i).unwrap();
    assert!(graph.instance(i).unwrap().fact_slots.is_empty());
    let mut stale = graft;
    stale.revision = 1;
    stale.event.id = 2;
    assert!(graph.observe_graft(stale, true).is_err());
    assert_eq!(graph.metrics().unexpected_break_count, 1);
}
