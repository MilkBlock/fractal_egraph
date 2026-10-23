//! Complete rooted boundary queries, maintained from native row commits.
//! This is source-derived context reuse, not a new saturation/absence theorem.
use super::*;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub ruleset: String,
    pub rule: String,
    pub root: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<Plan>,
}
/// A solver-selected layout. Shape is checked against the native compiler;
/// no concrete Value IDs or training bindings are portable.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    shape: artifact::Shape,
    pub seed: usize,
    steps: Vec<(usize, Access)>,
    pub prefix_len: usize,
    pub carried_slots: Vec<usize>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    schema: String,
    rules: Vec<Spec>,
    #[serde(default)]
    setup: Option<Vec<String>>,
}
pub(super) fn load(commands: &[Command]) -> Result<Vec<Spec>> {
    let Some(path) = std::env::var_os("CELL_BOUNDARY_PROGRAMS") else {
        return Ok(vec![]);
    };
    let f: File = serde_json::from_slice(&std::fs::read(path)?)?;
    if !["cell-boundary-programs/v1", "cell-boundary-programs/v2"].contains(&f.schema.as_str()) {
        return Err("unknown boundary program format".into());
    }
    if (f.schema == "cell-boundary-programs/v1" && f.rules.iter().any(|s| s.plan.is_some()))
        || (f.schema == "cell-boundary-programs/v2"
            && (f.setup.is_none() || f.rules.iter().any(|s| s.plan.is_none())))
    {
        return Err("boundary schema does not match its plan contract".into());
    }
    if f.setup
        .is_some_and(|s| s != commands.iter().map(ToString::to_string).collect::<Vec<_>>())
    {
        return Err("scheduled spine setup/source mismatch".into());
    }
    Ok(f.rules)
}

pub(super) fn compile(
    eg: &EGraph,
    sink: CellActionSink,
    tables: Vec<usize>,
    root: &str,
) -> Result<Kernel> {
    if sink.atoms.iter().any(|a| {
        a.columns.iter().zip(&a.sorts).any(|(o, s)| {
            matches!(o, CellOperand::Constant { .. })
                && !["i64", "f64", "bool", "String", "Unit"].contains(&s.as_str())
        })
    }) {
        return Err(
            "boundary equality/container constants require recanonicalization support".into(),
        );
    }
    if !eg.get_function(root).is_some_and(|f| f.is_constructor()) {
        return Err("boundary root must be a constructor".into());
    }
    let roots = sink
        .atoms
        .iter()
        .enumerate()
        .filter(|(_, a)| a.function == root)
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    if roots.len() != 1 || sink.atoms.iter().any(|a| a.columns.len() > 64) {
        return Err(
            "boundary requires one unambiguous root and at most 64 columns per atom".into(),
        );
    }
    compile_seed(eg, sink, tables, roots[0])
}

fn compile_seed(
    eg: &EGraph,
    sink: CellActionSink,
    tables: Vec<usize>,
    seed: usize,
) -> Result<Kernel> {
    if seed >= sink.atoms.len()
        || !eg
            .get_function(&sink.atoms[seed].function)
            .is_some_and(|f| f.is_constructor())
    {
        return Err("invalid constructor seed".into());
    }
    if sink.atoms.iter().any(|a| a.columns.len() > 64) {
        return Err("boundary atom exceeds 64 columns".into());
    }
    let mut bound = vec![false; sink.slots().len()];
    for c in &sink.atoms[seed].columns {
        if let CellOperand::Slot(i) = c {
            bound[*i] = true;
        }
    }
    let root_slots = bound.clone();
    let mut pending = (0..sink.atoms.len())
        .filter(|i| *i != seed)
        .collect::<BTreeSet<_>>();
    let mut steps = vec![];
    while !pending.is_empty() {
        let next = pending
            .iter()
            .filter_map(|i| {
                let a = &sink.atoms[*i];
                let n = a.columns.len() - 1;
                let full = a.columns[..n].iter().all(|o| known(o, &bound));
                let output =
                    eg.cell_sort_is_equality(&a.sorts[n]).ok()? && known(&a.columns[n], &bound);
                let mask = a
                    .columns
                    .iter()
                    .enumerate()
                    .filter(|(j, o)| a.sorts[*j] != "Unit" && known(o, &bound))
                    .fold(0u64, |m, (j, _)| m | (1 << j));
                if !full && !output && mask == 0 {
                    return None;
                }
                let rooted = a
                    .columns
                    .iter()
                    .filter(|o| matches!(o,CellOperand::Slot(s) if root_slots[*s]))
                    .count();
                let missing = a.columns.iter().filter(|o| !known(o, &bound)).count();
                Some((
                    *i,
                    if full {
                        Access::Key
                    } else if output {
                        Access::Output
                    } else {
                        Access::Columns(mask)
                    },
                    (
                        full,
                        output,
                        rooted > 0,
                        mask.count_ones(),
                        std::cmp::Reverse(missing),
                        std::cmp::Reverse(*i),
                    ),
                ))
            })
            .max_by_key(|x| x.2);
        let Some((i, access, _)) = next else {
            return Err("unanchored boundary component; retain ordinary query".into());
        };
        for o in &sink.atoms[i].columns {
            if let CellOperand::Slot(s) = o {
                bound[*s] = true
            }
        }
        pending.remove(&i);
        steps.push((i, access));
    }
    if bound.iter().any(|b| !*b) {
        return Err("unbound boundary slot".into());
    }
    Ok(Kernel::from_wiring(sink, tables, seed, steps))
}

fn instantiate(eg: &EGraph, sink: CellActionSink, tables: Vec<usize>, p: &Plan) -> Result<Kernel> {
    if p.shape != artifact::Shape::of(&sink) {
        return Err("scheduled spine shape/guard mismatch".into());
    }
    let mut k = compile_seed(eg, sink, tables, p.seed)?;
    if p.steps != k.steps
        || p.prefix_len > k.steps.len()
        || p.carried_slots.iter().any(|s| *s >= k.slot_count)
        || p.carried_slots
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .len()
            != p.carried_slots.len()
    {
        return Err("invalid scheduled spine wiring or payload".into());
    }
    k.payload_layout = Some((p.prefix_len, p.carried_slots.clone()));
    Ok(k)
}

pub(super) fn install(
    eg: &EGraph,
    sink: CellActionSink,
    tables: Vec<usize>,
    spec: &Spec,
) -> Result<Kernel> {
    if let Some(p) = &spec.plan {
        if sink
            .atoms
            .get(p.seed)
            .is_none_or(|a| a.function != spec.root)
        {
            return Err("scheduled spine anchor mismatch".into());
        }
        instantiate(eg, sink, tables, p)
    } else {
        compile(eg, sink, tables, &spec.root)
    }
}

/// Export the full typed query and every constructor anchor. Unsupported queries
/// stay in the original matcher; they do not make the input program unsupported.
pub fn inspect(eg: &mut EGraph, sets: &[String], reserved: &BTreeSet<String>) -> Result<Json> {
    let mut records = vec![];
    for set in sets {
        for name in eg.cell_rule_names(set)? {
            let item = (|| -> Result<Json> {
                if reserved.contains(&name) {
                    return Err("already served by existing parameterized program".into());
                }
                let sink = eg.prepare_cell_action(set, &name)?;
                if !eg.cell_query_is_seminaive(&sink)
                    || !sink.primitives.is_empty()
                    || sink.native_residuals
                {
                    return Err("native numeric/candidate query retained".into());
                }
                if sink.atoms.iter().any(|a| {
                    a.columns.iter().zip(&a.sorts).any(|(o, s)| {
                        matches!(o, CellOperand::Constant { .. })
                            && !["i64", "f64", "bool", "String", "Unit"].contains(&s.as_str())
                    })
                }) {
                    return Err("nonportable constant retained".into());
                }
                let shape = artifact::Shape::of(&sink);
                let mut plans = vec![];
                for seed in 0..sink.atoms.len() {
                    if !eg
                        .get_function(&sink.atoms[seed].function)
                        .is_some_and(|f| f.is_constructor())
                    {
                        continue;
                    }
                    let current = eg.prepare_cell_action(set, &name)?;
                    let tables = (0..current.atoms.len()).collect();
                    if let Ok(k) = compile_seed(eg, current, tables, seed) {
                        plans.push(Plan {
                            shape: shape.clone(),
                            seed,
                            steps: k.steps,
                            prefix_len: 0,
                            carried_slots: vec![],
                        });
                    }
                }
                if plans.is_empty() {
                    return Err("no connected constructor-anchored query".into());
                }
                Ok(json!({"ruleset":set,"rule":name,"shape":shape,"plans":plans}))
            })();
            records.push(match item {
                Ok(v) => v,
                Err(e) => json!({"ruleset":set,"rule":name,"retained_native":e.to_string()}),
            });
        }
    }
    Ok(json!({"schema":"scheduled-spine-queries/v1","queries":records}))
}

impl Runtime {
    pub(super) fn query_boundary(
        &mut self,
        set: &str,
        sinks: &[&CellActionSink],
    ) -> Result<Vec<(usize, Vec<Vec<Value>>)>> {
        self.sync()?;
        let mut selected = vec![];
        for id in self.boundary.get(set).into_iter().flatten() {
            let source = &self.kernels[*id].sink;
            let Some(i) = sinks.iter().position(|s| s.name == source.name) else {
                continue;
            };
            if source.slots() != sinks[i].slots() {
                return Err("boundary slot interface changed".into());
            }
            selected.push((*id, i));
        }
        let mut out = vec![];
        for (id, i) in selected {
            let rows =
                self.kernels[id].matches(&self.eg, &self.rows, &mut self.stats.keyed_reads)?;
            out.push((i, rows));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_frontier_matches_after_missing_ports_union_visibility_and_reinsert() {
        let mut eg = EGraph::default();
        eg.parse_and_run_program(
            None,
            r#"
          (datatype E (N i64) (Loop E E) (Const E E E))
          (relation Ctx (E E)) (relation Ty (E E)) (relation Hit (E E E E))
          (ruleset s)
          (rule ((= root (Loop x y)) (Ctx x a) (Ctx y b) (= k (Const c t b)) (Ty x u))
                ((Hit root k a u)) :ruleset s :name "q")
          (Loop (N 1) (N 2)) (Ctx (N 1) (N 10))
          (Ty (N 1) (N 20)) (Const (N 3) (N 20) (N 11))
        "#,
        )
        .unwrap();
        let spec = Spec {
            ruleset: "s".into(),
            rule: "q".into(),
            root: "Loop".into(),
            plan: None,
        };
        let mut r =
            Runtime::new_with_boundaries(eg, &[], &BTreeMap::new(), vec![], None, &[spec]).unwrap();
        for input in [
            "",
            "(Ctx (N 2) (N 11))",
            "(Ctx (N 2) (N 12)) (Const (N 4) (N 20) (N 12))",
            "(union (N 11) (N 12))",
            "(delete (Ctx (N 2) (N 11)))",
            "(Ctx (N 2) (N 11))",
            "(subsume (Loop (N 1) (N 2)))",
            "(Loop (N 1) (N 2))",
        ] {
            if !input.is_empty() {
                r.input(input).unwrap();
            }
            let sink = r.eg.prepare_cell_action("s", "q").unwrap();
            let query = r.eg.prepare_cell_query(&sink).unwrap();
            let mut expected = r.eg.run_cell_queries(&[&query]).unwrap().pop().unwrap();
            expected.sort();
            expected.dedup();
            let answer = r.query_boundary("s", &[&sink]).unwrap();
            let got = &answer[0].1;
            assert_eq!(got, &expected, "after {input}");
            r.eg.apply_completed_cell_ruleset(&[(&sink, got.as_slice())])
                .unwrap();
        }
        assert!(!r.rows.projected.is_empty());
        assert!(r.kernels[0].rows_examined.get() > 0);
    }
    #[test]
    fn selected_payloads_match_native_through_function_updates_and_aliases() {
        for payload in [false, true] {
            let mut eg = EGraph::default();
            eg.parse_and_run_program(
                None,
                r#"
                (datatype E (N i64) (Loop E E) (C E E))
                (relation Ctx (E E)) (function Cost (E) i64 :merge (min old new))
                (relation Hit (E E i64)) (ruleset s)
                (rule ((= r (Loop x y)) (Ctx y ctx) (= cost (Cost x)) (< cost 10) (= c (C z ctx)))
                      ((Hit r c cost)) :ruleset s :name "q")
                (Loop (N 1) (N 2)) (Ctx (N 2) (N 3))
                (set (Cost (N 1)) 20) (C (N 4) (N 3))
            "#,
            )
            .unwrap();
            let q = inspect(&mut eg, &["s".into()], &BTreeSet::new()).unwrap();
            let mut p: Plan = serde_json::from_value(q["queries"][0]["plans"][0].clone()).unwrap();
            // Choose the actual Loop row, independent of native atom ordering.
            for plan in q["queries"][0]["plans"].as_array().unwrap() {
                let candidate: Plan = serde_json::from_value(plan.clone()).unwrap();
                if plan["shape"]["atoms"][candidate.seed]["table"] == "Loop" {
                    p = candidate;
                    break;
                }
            }
            p.prefix_len = p.steps.len();
            if payload {
                p.carried_slots = (0..p.shape.slots.len()).collect();
            }
            let spec = Spec {
                ruleset: "s".into(),
                rule: "q".into(),
                root: "Loop".into(),
                plan: Some(p),
            };
            let mut r =
                Runtime::new_with_boundaries(eg, &[], &BTreeMap::new(), vec![], None, &[spec])
                    .unwrap();
            for input in [
                "",
                "(set (Cost (N 1)) 8)",
                "(C (N 5) (N 3))",
                "(Ctx (N 2) (N 6)) (C (N 7) (N 6))",
                "(union (N 3) (N 6))",
                "(set (Cost (N 1)) 4)",
                "(delete (Ctx (N 2) (N 3)))",
                "(Ctx (N 2) (N 3))",
                "(subsume (Loop (N 1) (N 2)))",
            ] {
                if !input.is_empty() {
                    r.input(input).unwrap();
                }
                let sink = r.eg.prepare_cell_action("s", "q").unwrap();
                let native = r.eg.prepare_cell_query(&sink).unwrap();
                let mut expected = r.eg.run_cell_queries(&[&native]).unwrap().pop().unwrap();
                expected.sort();
                expected.dedup();
                let got = r.query_boundary("s", &[&sink]).unwrap();
                assert_eq!(got[0].1, expected, "payload={payload}, after {input}");
                r.eg.apply_completed_cell_ruleset(&[(&sink, &got[0].1)])
                    .unwrap();
            }
            if payload {
                assert!(r.kernels[0].payload_resumes.get() > 0);
            }
        }
    }

    #[test]
    fn portable_layout_rejects_omitted_atoms_and_changed_guards() {
        let source = "(datatype E (N i64) (Loop E E)) (relation Hit (E)) (ruleset s) (rule ((= r (Loop x y)) (= (N i) x) (< i 10)) ((Hit r)) :ruleset s :name \"q\")";
        let mut eg = EGraph::default();
        eg.parse_and_run_program(None, source).unwrap();
        let q = inspect(&mut eg, &["s".into()], &BTreeSet::new()).unwrap();
        let mut p: Plan = serde_json::from_value(q["queries"][0]["plans"][0].clone()).unwrap();
        let sink = eg.prepare_cell_action("s", "q").unwrap();
        let ts = (0..sink.atoms.len()).collect();
        p.steps.clear();
        assert!(instantiate(&eg, sink, ts, &p).is_err());
        let p: Plan = serde_json::from_value(q["queries"][0]["plans"][0].clone()).unwrap();
        let mut changed = EGraph::default();
        changed
            .parse_and_run_program(None, &source.replace("i 10", "i 11"))
            .unwrap();
        let sink = changed.prepare_cell_action("s", "q").unwrap();
        let ts = (0..sink.atoms.len()).collect();
        assert!(instantiate(&changed, sink, ts, &p).is_err());
    }
}
