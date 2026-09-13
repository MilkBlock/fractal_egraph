use egg_layout::{
    binding_program::Term, effect_program::Fact, fractal_frontier::*,
    trigger_bridge::BindingRelation,
};
use egglog::EGraph;
use serde_json::{Value, json};
use std::collections::BTreeMap;
fn val(n: i64) -> Term {
    Term::new("i64", format!("literal:{n}"), vec![])
}
fn port(n: usize, sort: &str) -> Term {
    Term::new(sort, format!("in:{n}"), vec![])
}
fn state(n: i64) -> State {
    State {
        family: "advance".into(),
        binding: vec![val(n)],
    }
}
fn family() -> Family {
    let n = port(0, "i64");
    let m = port(1, "i64");
    let g = port(2, "Token");
    let f = |name: &str, args| Fact::Relation(name.into(), args);
    let trigger = BindingRelation {
        sorts: vec!["i64".into(), "i64".into(), "Token".into()],
        inputs: vec![0],
        outputs: vec![0],
        facts: vec![
            f("At", vec![n.clone()]),
            f("Permit", vec![n.clone()]),
            f("Gate", vec![n.clone(), g.clone()]),
            f("Have", vec![g]),
        ],
        equalities: vec![],
    };
    let mut recurrence = trigger.clone();
    recurrence.outputs = vec![1];
    recurrence
        .facts
        .extend([f("Next", vec![n, m.clone()]), f("At", vec![m])]);
    Family {
        name: "advance".into(),
        trigger,
        recurrence,
    }
}
// Fixture adapter compiles supplied relation contracts into native existential checks.
fn text(t: &Term, b: &BTreeMap<usize, Term>) -> String {
    if let Some(i) = t.op.strip_prefix("in:") {
        let i = i.parse().unwrap();
        return b.get(&i).map(|v| text(v, b)).unwrap_or(format!("v{i}"));
    }
    if let Some(n) = t.op.strip_prefix("literal:") {
        return n.into();
    }
    format!(
        "({} {})",
        t.op,
        t.args
            .iter()
            .map(|a| text(a, b))
            .collect::<Vec<_>>()
            .join(" ")
    )
}
fn check(eg: &mut EGraph, query: &str) -> Result<bool, String> {
    match eg.parse_and_run_program(None, &format!("(check {query})")) {
        Ok(_) => Ok(true),
        Err(egglog::Error::CheckError(..)) => Ok(false),
        Err(e) => Err(e.to_string()),
    }
}
fn relation(
    eg: &mut EGraph,
    r: &BindingRelation,
    b: &BTreeMap<usize, Term>,
) -> Result<bool, String> {
    let mut clauses: Vec<_> = r
        .facts
        .iter()
        .map(|f| match f {
            Fact::Relation(name, args) => format!(
                "({name} {})",
                args.iter()
                    .map(|t| text(t, b))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            Fact::Node(t) => text(t, b),
        })
        .collect();
    clauses.extend(
        r.equalities
            .iter()
            .map(|(a, c)| format!("(= {} {})", text(a, b), text(c, b))),
    );
    check(eg, &clauses.join(" "))
}
struct Native<'a> {
    eg: &'a mut EGraph,
}
impl Oracle for Native<'_> {
    fn canonicalize(&self, s: &State) -> Result<State, String> {
        Ok(s.clone())
    } // only i64 interface in this fixture
    fn inspect(&mut self, f: &Family, s: &State) -> Result<Observation, String> {
        let n: i64 = s.binding[0]
            .op
            .strip_prefix("literal:")
            .unwrap()
            .parse()
            .unwrap();
        let mut next = vec![];
        self.eg
            .function_for_each("Next", |r| {
                let a = self.eg.value_to_base::<i64>(r.vals[0]);
                if a == n {
                    next.push(self.eg.value_to_base::<i64>(r.vals[1]));
                }
            })
            .map_err(|e| e.to_string())?;
        if next.is_empty() {
            return Ok(Observation {
                transitions: vec![],
                pending: None,
                complete: true,
            });
        }
        let mut b = BTreeMap::from([(0, val(n))]);
        if !relation(self.eg, &f.trigger, &b)? {
            let permit = check(self.eg, &format!("(Permit {n})"))?;
            let gate = check(self.eg, &format!("(Gate {n} g) (Have g)"))?;
            return Ok(Observation {
                transitions: vec![],
                pending: Some(Pending::TriggerGap(
                    json!({"permit_missing":!permit,"gate_join_missing":!gate,"union_may_enable_join":!gate}),
                )),
                complete: true,
            });
        }
        let mut transitions = vec![];
        let mut missing = false;
        for m in next {
            b.insert(1, val(m));
            if relation(self.eg, &f.recurrence, &b)? {
                transitions.push(Transition {
                    target: state(m),
                    kind: EdgeKind::Recurrence,
                    witness: format!(
                        "native conjunction: At({n}), Next({n},{m}), Permit/Gate/Have and At({m})"
                    ),
                });
            } else {
                missing = true;
            }
        }
        Ok(Observation {
            transitions,
            pending: missing.then_some(Pending::EnabledUnmaterialized),
            complete: true,
        })
    }
}
fn snapshot(eg: &EGraph) -> BTreeMap<String, Vec<String>> {
    eg.get_function_names()
        .into_iter()
        .map(|name| {
            let mut rows = vec![];
            eg.function_for_each(&name, |r| rows.push(format!("{:?}", r.vals)))
                .unwrap();
            rows.sort();
            (name, rows)
        })
        .collect()
}
fn count(eg: &EGraph) -> usize {
    let mut n = 0;
    eg.function_for_each("At", |_| n += 1).unwrap();
    n
}
fn run(interleaved: bool) -> Value {
    let mut eg = EGraph::default();
    eg.parse_and_run_program(
        None,
        include_str!("../../experiments/fractal_frontier/interleaved.egg"),
    )
    .unwrap();
    let mut tracker = Frontier::new([family()]).unwrap();
    tracker.seed(state(0)).unwrap();
    let mut phases = vec![];
    let mut epoch = 0;
    let mut schedule = vec![];
    phases.push(
        tracker
            .refresh(
                epoch,
                Limits {
                    states: 32,
                    edges: 64,
                },
                &mut Native { eg: &mut eg },
            )
            .unwrap(),
    );
    let mut apply = |name: &str, eg: &mut EGraph, tracker: &mut Frontier| {
        eg.parse_and_run_program(None, &format!("(run {name} 1)"))
            .unwrap();
        epoch += 1;
        schedule.push(name.to_string());
        let before = snapshot(eg);
        let r = tracker
            .refresh(
                epoch,
                Limits {
                    states: 32,
                    edges: 64,
                },
                &mut Native { eg },
            )
            .unwrap();
        assert_eq!(snapshot(eg), before, "observer changed graph");
        phases.push(r);
    };
    for i in 0..8 {
        if i == 2 {
            apply("coarse", &mut eg, &mut tracker);
        }
        if i == 4 {
            apply("unlock", &mut eg, &mut tracker);
        }
        apply("grow", &mut eg, &mut tracker);
        if interleaved {
            apply("noise", &mut eg, &mut tracker);
        }
    }
    if !interleaved {
        for _ in 0..8 {
            apply("noise", &mut eg, &mut tracker);
        }
    }
    assert!(check(&mut eg, "(At 8)").unwrap());
    assert_eq!(phases.last().unwrap()["visited_states"], 9);
    assert_eq!(phases.last().unwrap()["witnessed_edges"], 8);
    assert_eq!(phases.last().unwrap()["snapshot_candidate_closure"], true);
    assert!(
        phases
            .iter()
            .any(|r| r.to_string().contains("enabled_unmaterialized"))
    );
    assert!(
        phases
            .iter()
            .any(|r| r.to_string().contains("\"permit_missing\":true"))
    );
    assert!(
        phases
            .iter()
            .any(|r| r.to_string().contains("\"gate_join_missing\":true"))
    );
    let mut longest = 0;
    let mut consecutive = 0;
    for name in &schedule {
        if name == "grow" {
            consecutive += 1;
            longest = longest.max(consecutive);
        } else {
            consecutive = 0;
        }
    }
    json!({"schedule":schedule,"longest_consecutive_grow_invocations":longest,"phases":phases,"final_at_count":count(&eg),"native_final_check":true})
}
#[test]
fn native_interleaving_retains_family_and_reopens_fact_and_union_gaps() {
    let focused = run(false);
    let mixed = run(true);
    assert_eq!(focused["longest_consecutive_grow_invocations"], 4);
    assert_eq!(mixed["longest_consecutive_grow_invocations"], 1);
    assert_eq!(
        focused["phases"].as_array().unwrap().last().unwrap()["edges"],
        mixed["phases"].as_array().unwrap().last().unwrap()["edges"]
    );
    if let Ok(path) = std::env::var("FRACTAL_FRONTIER_REPORT") {
        std::fs::write(path,serde_json::to_string_pretty(&json!({"scope":"native egglog fixture and read-only structural frontier; supplied finite candidate, no trace causality or infinite induction","focused":focused,"interleaved":mixed})).unwrap()).unwrap();
    }
}

struct GraphOracle {
    rows: BTreeMap<State, Observation>,
    collapse: bool,
}
impl Oracle for GraphOracle {
    fn canonicalize(&self, s: &State) -> Result<State, String> {
        let mut c = s.clone();
        if self.collapse && c.binding == vec![val(1)] {
            c.binding = vec![val(0)];
        }
        Ok(c)
    }
    fn inspect(&mut self, _: &Family, s: &State) -> Result<Observation, String> {
        Ok(self.rows.get(s).cloned().unwrap_or(Observation {
            transitions: vec![],
            pending: None,
            complete: true,
        }))
    }
}
fn edge(target: State, kind: EdgeKind) -> Transition {
    Transition {
        target,
        kind,
        witness: "test supplied witness".into(),
    }
}
#[test]
fn branches_budgets_and_union_recanonicalization_do_not_force_a_winner() {
    let mut b = family();
    b.name = "other".into();
    let mut tracker = Frontier::new([family(), b]).unwrap();
    tracker.seed(state(0)).unwrap();
    let other = State {
        family: "other".into(),
        binding: vec![val(2)],
    };
    let mut oracle = GraphOracle {
        collapse: false,
        rows: BTreeMap::from([
            (
                state(0),
                Observation {
                    transitions: vec![
                        edge(state(1), EdgeKind::Recurrence),
                        edge(other.clone(), EdgeKind::CoarseBridge),
                    ],
                    pending: None,
                    complete: true,
                },
            ),
            (
                state(1),
                Observation {
                    transitions: vec![edge(state(0), EdgeKind::Recurrence)],
                    pending: None,
                    complete: true,
                },
            ),
        ]),
    };
    let narrow = tracker
        .refresh(
            0,
            Limits {
                states: 8,
                edges: 1,
            },
            &mut oracle,
        )
        .unwrap();
    assert_eq!(narrow["budget_exhausted"], true);
    assert_eq!(narrow["snapshot_candidate_closure"], false);
    let wide = tracker
        .refresh(
            0,
            Limits {
                states: 8,
                edges: 8,
            },
            &mut oracle,
        )
        .unwrap();
    assert_eq!(wide["visited_states"], 3);
    assert_eq!(wide["witnessed_edges"], 3);
    assert_eq!(wide["infinite_fractal_certified"], false);
    oracle.collapse = true;
    let merged = tracker
        .refresh(
            1,
            Limits {
                states: 8,
                edges: 8,
            },
            &mut oracle,
        )
        .unwrap();
    assert_eq!(merged["visited_states"], 2);
    assert_eq!(merged["witnessed_edges"], 2);
    assert!(
        tracker
            .refresh(
                0,
                Limits {
                    states: 8,
                    edges: 8
                },
                &mut oracle
            )
            .is_err()
    );
}
#[test]
fn incomplete_enumeration_and_zero_budget_are_not_turning_point_certificates() {
    let mut t = Frontier::new([family()]).unwrap();
    t.seed(state(0)).unwrap();
    let mut oracle = GraphOracle {
        collapse: false,
        rows: BTreeMap::from([(
            state(0),
            Observation {
                transitions: vec![],
                pending: Some(Pending::Unknown(
                    "external binding candidates truncated".into(),
                )),
                complete: false,
            },
        )]),
    };
    let r = t
        .refresh(
            0,
            Limits {
                states: 0,
                edges: 0,
            },
            &mut oracle,
        )
        .unwrap();
    assert_eq!(r["budget_exhausted"], true);
    let r = t
        .refresh(
            0,
            Limits {
                states: 8,
                edges: 8,
            },
            &mut oracle,
        )
        .unwrap();
    assert_eq!(r["snapshot_candidate_closure"], false);
    assert_eq!(r["incomplete_observations"], 1);
}

#[test]
fn repeated_interface_ports_do_not_admit_inconsistent_bindings() {
    let mut f = family();
    f.trigger.inputs = vec![0, 0];
    f.recurrence.inputs = vec![0, 0];
    f.recurrence.outputs = vec![1, 1];
    let mut t = Frontier::new([f]).unwrap();
    assert!(
        t.seed(State {
            family: "advance".into(),
            binding: vec![val(0), val(1)]
        })
        .is_err()
    );
    t.seed(State {
        family: "advance".into(),
        binding: vec![val(0), val(0)],
    })
    .unwrap();
}
