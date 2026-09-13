//! Exact incremental union coverage; never expands all combined substitutions.
use super::*;
#[derive(Clone)]
struct Spec {
    name: String,
    side: String,
    atoms: Vec<Atom>,
    clauses: Vec<String>,
    eqs: Vec<(Ref, Ref)>,
    types: BTreeMap<String, String>,
    used: BTreeSet<String>,
    rhs_actions: Vec<String>,
}
// Alpha-equivalence of a complete action is a sufficient containment proof.
// Globals retain their identity; repeated variables retain their aliasing.
fn action_key(action: &Action) -> String {
    fn expr(e: &Expr, vars: &mut BTreeMap<String, usize>) -> Json {
        match e {
            Expr::Var(_, v) if v.starts_with('$') => json!(["global", v]),
            Expr::Var(_, v) => {
                let next = vars.len();
                let id = *vars.entry(v.clone()).or_insert(next);
                json!(["var", id])
            }
            Expr::Lit(..) => json!(["literal", e.to_string()]),
            Expr::Call(_, op, args) => json!([
                "call",
                op,
                args.iter().map(|e| expr(e, vars)).collect::<Vec<_>>()
            ]),
        }
    }
    let mut vars = BTreeMap::new();
    match action {
        Action::Union(_, a, b) => {
            json!(["union", expr(a, &mut vars), expr(b, &mut vars)]).to_string()
        }
        Action::Expr(_, e) => json!(["expr", expr(e, &mut vars)]).to_string(),
        _ => panic!("unsupported action must be rejected by prepare"),
    }
}
fn prepare(
    eg: &mut EGraph,
    snapshot: &Snapshot,
    r: &Rule,
    side: &str,
    guards: &[Fact],
) -> Result<Spec> {
    let mut q = Query {
        eg,
        snapshot,
        types: BTreeMap::new(),
        used: BTreeSet::new(),
        original: BTreeSet::new(),
        clauses: vec![],
        atoms: vec![],
        roots: vec![],
        counter: 0,
        equalities: vec![],
    };
    if side == "lhs" {
        for f in &r.body {
            q.fact(f, true)?;
        }
    } else {
        for a in &r.head.0 {
            match a {
                Action::Union(span, a, b) => {
                    q.fact(&Fact::Eq(span.clone(), a.clone(), b.clone()), true)?
                }
                Action::Expr(_, e) => q.fact(&Fact::Fact(e.clone()), true)?,
                _ => return Err("unsupported RHS action".into()),
            }
        }
        for g in guards {
            q.fact(g, false)?;
        }
    }
    Ok(Spec {
        name: r.name.clone(),
        side: side.into(),
        atoms: q.atoms,
        clauses: q.clauses,
        eqs: q.equalities,
        types: q.types,
        used: q.used,
        rhs_actions: if side == "rhs" {
            r.head.0.iter().map(action_key).collect()
        } else {
            vec![]
        },
    })
}
fn value(r: &Ref, env: &BTreeMap<String, Value>) -> Option<Value> {
    match r {
        Ref::Lit(v) => Some(*v),
        Ref::Var(v) => env.get(v).copied(),
    }
}
fn bind(r: &Ref, v: Value, env: &mut BTreeMap<String, Value>) -> bool {
    match r {
        Ref::Lit(old) => *old == v,
        Ref::Var(n) => match env.get(n) {
            Some(old) => *old == v,
            None => {
                env.insert(n.clone(), v);
                true
            }
        },
    }
}
fn row_env(a: &Atom, row: &[Value]) -> Option<BTreeMap<String, Value>> {
    let mut env = BTreeMap::new();
    for (r, &v) in a.inputs.iter().chain(std::iter::once(&a.output)).zip(row) {
        if !bind(r, v, &mut env) {
            return None;
        }
    }
    Some(env)
}
fn flat(spec: &Spec, a: &Atom, row: &[Value]) -> Option<bool> {
    let Some(mut env) = row_env(a, row) else {
        return Some(false);
    };
    loop {
        let before = env.len();
        for (a, b) in &spec.eqs {
            match (value(a, &env), value(b, &env)) {
                (Some(x), Some(y)) if x != y => return Some(false),
                (Some(x), None) => {
                    if !bind(b, x, &mut env) {
                        return Some(false);
                    }
                }
                (None, Some(y)) => {
                    if !bind(a, y, &mut env) {
                        return Some(false);
                    }
                }
                _ => {}
            }
        }
        if env.len() == before {
            break;
        }
    }
    spec.used
        .iter()
        .all(|n| env.contains_key(n))
        .then_some(true)
}
#[derive(Clone)]
struct FrozenConstant {
    name: String,
    sort: egglog::ArcSort,
    value: Value,
}
impl egglog::Primitive for FrozenConstant {
    fn name(&self) -> &str {
        &self.name
    }
    fn get_type_constraints(
        &self,
        span: &egglog::ast::Span,
    ) -> Box<dyn egglog::constraint::TypeConstraint> {
        Box::new(egglog::constraint::SimpleTypeConstraint::new(
            &self.name,
            vec![self.sort.clone()],
            span.clone(),
        ))
    }
}
impl egglog::PurePrim for FrozenConstant {
    fn apply<'a, 'db>(&self, _: egglog::PureState<'a, 'db>, _: &[Value]) -> Option<Value> {
        Some(self.value)
    }
}
fn anchored(
    eg: &mut EGraph,
    spec: &Spec,
    a: &Atom,
    row: &[Value],
    serial: &mut usize,
) -> Result<bool> {
    let Some(env) = row_env(a, row) else {
        return Ok(false);
    };
    let mut clauses = spec.clauses.clone();
    for (var, value) in env {
        let name = format!("__breadth_constant_{}", *serial);
        *serial += 1;
        let sort = eg
            .get_sort_by_name(&spec.types[&var])
            .ok_or("missing anchor sort")?;
        eg.add_pure_primitive(
            FrozenConstant {
                name: name.clone(),
                sort: sort.clone(),
                value,
            },
            None,
        );
        eg.parse_and_run_program(None, &format!("(let ${name} ({name}))"))?;
        clauses.push(format!("(= {var} ${name})"));
    }
    match eg.parse_and_run_program(None, &format!("(check {})", clauses.join(" "))) {
        Ok(_) => Ok(true),
        Err(egglog::Error::CheckError(..)) => Ok(false),
        Err(e) => Err(e.into()),
    }
}
fn projected(base: &EGraph, snapshot: &Snapshot, spec: &Spec, a: &Atom) -> Result<HashSet<usize>> {
    let mut eg = base.clone();
    let names: BTreeSet<_> = a
        .inputs
        .iter()
        .chain(std::iter::once(&a.output))
        .filter_map(|r| {
            if let Ref::Var(n) = r {
                Some(n.clone())
            } else {
                None
            }
        })
        .collect();
    let names: Vec<_> = names.into_iter().collect();
    let types: Vec<_> = names.iter().map(|n| spec.types[n].clone()).collect();
    let program=format!("(sort __BreadthMarker) (constructor __Cover ({}) __BreadthMarker) (ruleset __breadth_only) (rule ({}) ((__Cover {})) :ruleset __breadth_only) (run __breadth_only 1)",types.join(" "),spec.clauses.join(" "),names.join(" "));
    eg.parse_and_run_program(None, &program)?;
    let mut covered = HashSet::new();
    eg.function_for_each("__Cover", |r| {
        let env: BTreeMap<_, _> = names
            .iter()
            .enumerate()
            .map(|(i, n)| (n.clone(), r.vals[i]))
            .collect();
        let key: Vec<_> = a
            .inputs
            .iter()
            .chain(std::iter::once(&a.output))
            .map(|r| value(r, &env).unwrap())
            .collect();
        covered.insert(snapshot.ids[&(a.table, key)]);
    })?;
    assert!(snapshot.unchanged(&eg));
    Ok(covered)
}
// Count only original, non-subsumed rows, keyed by output sort and canonical value.
fn eclass_coverage(snapshot: &Snapshot, covered: &HashSet<usize>) -> Json {
    let mut counts = HashMap::new();
    for &id in covered {
        let (table, row) = &snapshot.rows[id];
        *counts
            .entry((snapshot.tables[*table].output.clone(), *row.last().unwrap()))
            .or_insert(0usize) += 1;
    }
    let total = snapshot.alternatives.len();
    let full = counts
        .iter()
        .filter(|(key, count)| snapshot.alternatives[*key] == **count)
        .count();
    let ratio = |n: usize| {
        if total == 0 {
            None
        } else {
            Some(n as f64 / total as f64)
        }
    };
    json!({"total_eclasses":total,"covered_eclasses":counts.len(),"coverage_ratio":ratio(counts.len()),
        "fully_covered_eclasses":full,"full_coverage_ratio":ratio(full),
        "partially_covered_eclasses":counts.len()-full,"uncovered_eclasses":total-counts.len()})
}
fn group(
    base: &EGraph,
    snapshot: &Snapshot,
    specs: &[Spec],
    initial: &HashSet<usize>,
    initial_rhs_proofs: &BTreeMap<String, String>,
) -> Result<(Json, HashSet<usize>)> {
    let mut covered = initial.clone();
    let mut remaining = vec![vec![]; snapshot.tables.len()];
    for (id, (table, _)) in snapshot.rows.iter().enumerate() {
        if !covered.contains(&id) {
            remaining[*table].push(id);
        }
    }
    let mut ordered: Vec<_> = specs.iter().collect();
    ordered.sort_by_key(|s| (s.atoms.len(), s.name.clone()));
    let mut logs = vec![];
    let mut checks = 0;
    let mut projections = 0;
    let mut serial = 0;
    let mut checker = base.clone();
    for spec in ordered {
        eprintln!("checking {} {}", spec.name, spec.side);
        if !spec.rhs_actions.is_empty()
            && spec
                .rhs_actions
                .iter()
                .all(|k| initial_rhs_proofs.contains_key(k))
        {
            logs.push(json!({"rule":spec.name,"side":spec.side,"added_nodes":0,
                "status":"no_additional_coverage_by_complete_action_containment",
                "covered_by_basic_rhs_rules":spec.rhs_actions.iter().map(|k|&initial_rhs_proofs[k]).collect::<Vec<_>>() }));
            continue;
        }
        let before = covered.len();
        let start_checks = checks;
        let start_projections = projections;
        let mut exact_rows = 0;
        let mut upper = 0;
        for atom in spec.atoms.iter().filter(|a| a.covered) {
            remaining[atom.table].retain(|id| !covered.contains(id));
            let todo: Vec<_> = remaining[atom.table]
                .iter()
                .copied()
                .filter(|&id| row_env(atom, &snapshot.rows[id].1).is_some())
                .collect();
            upper += todo.len();
            if todo.is_empty() {
                continue;
            }
            if spec.atoms.len() == 1 {
                for id in todo {
                    match flat(spec, atom, &snapshot.rows[id].1) {
                        Some(true) => {
                            covered.insert(id);
                            exact_rows += 1;
                        }
                        Some(false) => {}
                        None => {
                            checks += 1;
                            if anchored(
                                &mut checker,
                                spec,
                                atom,
                                &snapshot.rows[id].1,
                                &mut serial,
                            )? {
                                covered.insert(id);
                            }
                        }
                    }
                }
            } else if todo.len() <= 32 {
                for id in todo {
                    checks += 1;
                    if anchored(&mut checker, spec, atom, &snapshot.rows[id].1, &mut serial)? {
                        covered.insert(id);
                    }
                }
            } else {
                eprintln!(
                    "projecting {} {} {} ({} uncovered candidates)",
                    spec.name,
                    spec.side,
                    snapshot.tables[atom.table].name,
                    todo.len()
                );
                covered.extend(projected(base, snapshot, spec, atom)?);
                projections += 1;
            }
        }
        logs.push(json!({"rule":spec.name,"side":spec.side,"added_nodes":covered.len()-before,"candidate_upper_sum":upper,"exact_single_atom_rows":exact_rows,"native_existential_checks":checks-start_checks,"native_projected_queries":projections-start_projections,"status":if upper==0{"no_additional_coverage_by_atom_bound"}else{"exact"}}));
    }
    assert!(snapshot.unchanged(&checker));
    Ok((
        json!({"patterns":specs.len(),"initial_nodes":initial.len(),"final_nodes":covered.len(),"added_nodes":covered.len()-initial.len(),"coverage_ratio":covered.len() as f64/snapshot.rows.len() as f64,"native_existential_checks":checks,"native_projected_queries":projections,"pattern_results":logs,"eclasses":eclass_coverage(snapshot,&covered)}),
        covered,
    ))
}
pub fn run(source: &str, output: &str) -> Result<()> {
    let text = std::fs::read_to_string(source)?;
    let mut base = EGraph::default();
    let commands = base.parse_program(Some(source.into()), &text)?;
    let rules: Vec<_> = commands
        .iter()
        .filter_map(|c| {
            if let Command::Rule { rule } = c {
                Some(rule.clone())
            } else {
                None
            }
        })
        .collect();
    let mut constraints = BTreeMap::<String, BTreeSet<usize>>::new();
    for line in text.lines() {
        if let Some(s) = line.strip_prefix("; @egg-viz-json ") {
            let m: Json = serde_json::from_str(s)?;
            if m["kind"] == "combined_witness_bundle" {
                let mut indices = BTreeSet::new();
                for c in m["connections"].as_array().ok_or("missing connections")? {
                    let start = c["constraint_start"].as_u64().ok_or("missing start")? as usize;
                    let count = c["constraint_count"].as_u64().ok_or("missing count")? as usize;
                    indices.extend(start..start + count);
                }
                constraints.insert(m["id"].as_str().ok_or("missing id")?.into(), indices);
            }
        }
    }
    base.run_program(commands)?;
    let snapshot = Snapshot::new(&base);
    let mut work = base.clone();
    let mut basic = [vec![], vec![]];
    let mut combined = [vec![], vec![]];
    for r in &rules {
        for (side, name) in ["lhs", "rhs"].into_iter().enumerate() {
            let guards = constraints
                .get(&r.name)
                .into_iter()
                .flatten()
                .map(|&i| r.body.get(i).cloned().ok_or("invalid constraint index"))
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let s = prepare(&mut work, &snapshot, r, name, &guards)?;
            if constraints.contains_key(&r.name) {
                combined[side].push(s);
            } else {
                basic[side].push(s);
            }
        }
    }
    let rhs_proofs: BTreeMap<_, _> = basic[1]
        .iter()
        .filter(|s| s.rhs_actions.len() == 1)
        .map(|s| (s.rhs_actions[0].clone(), s.name.clone()))
        .collect();
    let mut report = json!({"status":"running","source":source,"original_enodes":snapshot.rows.len(),"basic_rules":basic[0].len(),"combined_rules":combined[0].len(),"groups":{}});
    let mut unions = vec![];
    let no_proofs = BTreeMap::new();
    for side in 0..2 {
        let name = if side == 0 { "lhs" } else { "rhs" };
        eprintln!("basic {name}");
        let (r, b) = group(
            &base,
            &snapshot,
            &basic[side],
            &HashSet::new(),
            &BTreeMap::new(),
        )?;
        report["groups"][format!("basic_{name}")] = r;
        std::fs::write(output, serde_json::to_string_pretty(&report)?)?;
        eprintln!("combined incremental {name}");
        let (r, c) = group(
            &base,
            &snapshot,
            &combined[side],
            &b,
            if side == 1 { &rhs_proofs } else { &no_proofs },
        )?;
        report["groups"][format!("basic_plus_combined_{name}")] = r;
        std::fs::write(output, serde_json::to_string_pretty(&report)?)?;
        unions.push((b, c));
    }
    let baseline: HashSet<_> = unions[0].0.union(&unions[1].0).copied().collect();
    let total: HashSet<_> = unions[0].1.union(&unions[1].1).copied().collect();
    let mut missing = BTreeMap::<String, usize>::new();
    for (id, (table, _)) in snapshot.rows.iter().enumerate() {
        if !total.contains(&id) {
            *missing
                .entry(snapshot.tables[*table].name.clone())
                .or_default() += 1;
        }
    }
    report["eclass_statistics"] = json!({"total_eclasses":snapshot.alternatives.len(),
        "mean_enodes_per_eclass":if snapshot.alternatives.is_empty(){None}else{Some(snapshot.rows.len() as f64/snapshot.alternatives.len() as f64)}});
    report["basic_joint_eclasses"] = eclass_coverage(&snapshot, &baseline);
    report["expanded_joint_eclasses"] = eclass_coverage(&snapshot, &total);
    report["status"] = json!("complete");
    report["basic_joint_nodes"] = json!(baseline.len());
    report["expanded_joint_nodes"] = json!(total.len());
    report["combined_added_joint_nodes"] = json!(total.difference(&baseline).count());
    report["uncovered_by_operator"] = json!(missing);
    report["original_rows_unchanged"] = json!(snapshot.unchanged(&base));
    report["scope"]=json!("exact union coverage on the final native graph; zero-marginal patterns proved using atom bounds; no instance count or compression packing measured");
    std::fs::write(output, serde_json::to_string_pretty(&report)?)?;
    println!(
        "{}",
        json!({"original":snapshot.rows.len(),"basic":baseline.len(),"expanded":total.len(),"added":total.difference(&baseline).count()})
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn eclass_coverage_distinguishes_partial_full_and_uncovered_after_union() {
        let mut eg = EGraph::default();
        eg.parse_and_run_program(
            None,
            "(datatype E (Leaf i64)) (union (Leaf 0) (Leaf 1)) (Leaf 2)",
        )
        .unwrap();
        let snapshot = Snapshot::new(&eg);
        assert_eq!(snapshot.rows.len(), 3);
        assert_eq!(snapshot.alternatives.len(), 2);
        let merged: Vec<_> = snapshot
            .rows
            .iter()
            .enumerate()
            .filter(|(_, (t, row))| {
                snapshot.alternatives[&(snapshot.tables[*t].output.clone(), *row.last().unwrap())]
                    == 2
            })
            .map(|(id, _)| id)
            .collect();
        let partial = eclass_coverage(&snapshot, &HashSet::from([merged[0]]));
        assert_eq!(partial["covered_eclasses"], 1);
        assert_eq!(partial["fully_covered_eclasses"], 0);
        assert_eq!(partial["coverage_ratio"], 0.5);
        assert_eq!(partial["uncovered_eclasses"], 1);
        let full = eclass_coverage(&snapshot, &merged.into_iter().collect());
        assert_eq!(full["covered_eclasses"], 1);
        assert_eq!(full["fully_covered_eclasses"], 1);
        assert_eq!(
            eclass_coverage(&snapshot, &(0..3).collect())["fully_covered_eclasses"],
            2
        );
        let empty = Snapshot::new(&EGraph::default());
        assert!(eclass_coverage(&empty, &HashSet::new())["coverage_ratio"].is_null());
    }
    #[test]
    fn projected_and_anchored_coverage_equal_full_instance_probe() {
        let mut eg = EGraph::default();
        eg.parse_and_run_program(None,"(datatype E (Leaf i64) (A E) (Pair E E)) (Pair (A (Leaf 0)) (A (Leaf 0))) (Pair (A (Leaf 0)) (A (Leaf 1)))").unwrap();
        let Command::Rule { rule } = eg
            .parse_program(
                None,
                "(rule ((= root (Pair (A x) (A x)))) ((union root (Pair x x))))",
            )
            .unwrap()
            .remove(0)
        else {
            panic!()
        };
        let snapshot = Snapshot::new(&eg);
        let (_, instances, _) = probe(&eg, &snapshot, &rule, "lhs", &[]).unwrap();
        let expected: HashSet<_> = instances
            .iter()
            .flat_map(|i| i.all.iter().copied())
            .collect();
        let mut work = eg.clone();
        let spec = prepare(&mut work, &snapshot, &rule, "lhs", &[]).unwrap();
        let mut projection = HashSet::new();
        let mut checked = HashSet::new();
        let mut serial = 0;
        for atom in &spec.atoms {
            projection.extend(projected(&eg, &snapshot, &spec, atom).unwrap());
            for (id, (table, row)) in snapshot.rows.iter().enumerate() {
                if *table == atom.table
                    && anchored(&mut work, &spec, atom, row, &mut serial).unwrap()
                {
                    checked.insert(id);
                }
            }
        }
        assert_eq!(projection, expected);
        assert_eq!(checked, expected);
        let (r, union) =
            group(&eg, &snapshot, &[spec.clone()], &expected, &BTreeMap::new()).unwrap();
        assert_eq!(union, expected);
        assert_eq!(r["added_nodes"], 0);
    }
    #[test]
    fn complete_action_containment_preserves_aliases_and_coverage() {
        let mut eg = EGraph::default();
        eg.parse_and_run_program(None,"(datatype E (Leaf i64) (A E) (Pair E E)) (Pair (A (Leaf 0)) (A (Leaf 0))) (Pair (A (Leaf 0)) (A (Leaf 1)))").unwrap();
        let texts = [
            "(rule () ((union r (Pair (A x) (A x)))))",
            "(rule ((= a b)) ((union root (Pair (A a) (A a)))))",
            "(rule () ((union root (Pair (A a) (A b)))))",
            "(rule () ((union root (Leaf 0))))",
            "(rule () ((union root (Leaf 1))))",
        ];
        let rules: Vec<_> = texts
            .iter()
            .map(|text| {
                let Command::Rule { rule } = eg.parse_program(None, text).unwrap().remove(0) else {
                    panic!()
                };
                rule
            })
            .collect();
        assert_eq!(
            action_key(&rules[0].head.0[0]),
            action_key(&rules[1].head.0[0])
        );
        assert_ne!(
            action_key(&rules[0].head.0[0]),
            action_key(&rules[2].head.0[0])
        );
        assert_ne!(
            action_key(&rules[3].head.0[0]),
            action_key(&rules[4].head.0[0])
        );
        let snapshot = Snapshot::new(&eg);
        let mut work = eg.clone();
        let base = prepare(&mut work, &snapshot, &rules[0], "rhs", &[]).unwrap();
        let constrained = prepare(&mut work, &snapshot, &rules[1], "rhs", &rules[1].body).unwrap();
        let (_, initial) = group(
            &eg,
            &snapshot,
            &[base.clone()],
            &HashSet::new(),
            &BTreeMap::new(),
        )
        .unwrap();
        let (_, instances, _) = probe(&eg, &snapshot, &rules[1], "rhs", &rules[1].body).unwrap();
        assert!(!instances.is_empty());
        assert!(instances
            .iter()
            .flat_map(|i| &i.all)
            .all(|id| initial.contains(id)));
        let proofs = BTreeMap::from([(base.rhs_actions[0].clone(), "base".into())]);
        let (report, covered) = group(&eg, &snapshot, &[constrained], &initial, &proofs).unwrap();
        assert_eq!(covered, initial);
        assert_eq!(
            report["pattern_results"][0]["status"],
            "no_additional_coverage_by_complete_action_containment"
        );
    }
    #[test]
    fn flat_projection_respects_variable_equalities() {
        let mut eg = EGraph::default();
        eg.parse_and_run_program(
            None,
            "(datatype E (Leaf i64) (Pair E E)) (Pair (Leaf 0) (Leaf 0)) (Pair (Leaf 0) (Leaf 1))",
        )
        .unwrap();
        let Command::Rule { rule } = eg
            .parse_program(
                None,
                "(rule ((= root (Pair x y)) (= x y)) ((union root x)))",
            )
            .unwrap()
            .remove(0)
        else {
            panic!()
        };
        let snapshot = Snapshot::new(&eg);
        let (_, instances, _) = probe(&eg, &snapshot, &rule, "lhs", &[]).unwrap();
        let expected: HashSet<_> = instances
            .iter()
            .flat_map(|i| i.all.iter().copied())
            .collect();
        let mut work = eg.clone();
        let spec = prepare(&mut work, &snapshot, &rule, "lhs", &[]).unwrap();
        let (_, covered) =
            group(&eg, &snapshot, &[spec], &HashSet::new(), &BTreeMap::new()).unwrap();
        assert_eq!(covered, expected);
    }
}
