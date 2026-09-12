//! Paired native interventions on an explicit, selected R23 recurrence.
use super::*;
#[derive(Clone)]
struct Binding {
    a: String,
    b: String,
    x: String,
}
impl Binding {
    fn lhs(&self) -> String {
        format!("(Integral (Mul {} {}) {})", self.a, self.b, self.x)
    }
    fn rhs(&self) -> String {
        format!(
            "(Sub (Mul {} (Integral {} {})) (Integral (Mul (Diff {} {}) (Integral {} {})) {}))",
            self.a, self.b, self.x, self.x, self.a, self.b, self.x, self.x
        )
    }
    fn next(&self) -> Self {
        Self {
            a: format!("(Diff {} {})", self.x, self.a),
            b: format!("(Integral {} {})", self.b, self.x),
            x: self.x.clone(),
        }
    }
    fn json(&self) -> Json {
        json!({"a":self.a,"b":self.b,"x":self.x})
    }
}
fn expression(eg: &mut EGraph, s: &str) -> Result<Expr> {
    let c = eg.parse_program(None, s)?.remove(0);
    let Command::Action(Action::Expr(_, e)) = c else {
        return Err("expected expression".into());
    };
    Ok(e)
}
fn lookup(eg: &mut EGraph, s: &str) -> Result<Option<Value>> {
    let e = expression(eg, s)?;
    Ok(lookup_readonly(eg, &e))
}
fn status(eg: &mut EGraph, b: &Binding) -> Result<&'static str> {
    let Some(lhs) = lookup(eg, &b.lhs())? else {
        return Ok("lhs_missing");
    };
    match lookup(eg, &b.rhs())? {
        None => Ok("enabled_rhs_missing"),
        Some(rhs) if rhs != lhs => Ok("enabled_union_missing"),
        _ => Ok("complete"),
    }
}
fn walk(eg: &mut EGraph, start: &Binding, cap: usize) -> Result<(usize, Binding, String)> {
    let mut b = start.clone();
    let mut seen = HashSet::new();
    for depth in 0..cap {
        let s = status(eg, &b)?;
        if s != "complete" {
            return Ok((depth, b, s.into()));
        }
        let k = [
            lookup(eg, &b.a)?.unwrap(),
            lookup(eg, &b.b)?.unwrap(),
            lookup(eg, &b.x)?.unwrap(),
        ];
        if !seen.insert(k) {
            return Ok((depth, b, "repeated_binding".into()));
        }
        b = b.next();
    }
    Ok((cap, b, "depth_cap_unknown".into()))
}
fn count(eg: &EGraph, names: &[String]) -> usize {
    let mut n = 0;
    for name in names {
        eg.function_for_each(name, |r| {
            if !r.subsumed {
                n += 1
            }
        })
        .unwrap();
    }
    n
}
/// Restrict the unmodified source rule body to one canonical binding. Globals
/// refer only to materialized expressions; their creation must not add Math rows.
fn apply(eg: &mut EGraph, rule: &Rule, b: &Binding, id: usize, names: &[String]) -> Result<Json> {
    for s in [&b.a, &b.b, &b.x] {
        if lookup(eg, s)?.is_none() {
            return Err("cannot anchor absent binding".into());
        }
    }
    let before = count(eg, names);
    let prefix = format!("__intervention_{id}");
    eg.parse_and_run_program(
        None,
        &format!(
            "(let ${prefix}_a {}) (let ${prefix}_b {}) (let ${prefix}_x {})",
            b.a, b.b, b.x
        ),
    )?;
    assert_eq!(count(eg, names), before, "anchoring created original nodes");
    let mut local = rule.clone();
    local.ruleset = prefix.clone();
    local.name = prefix.clone();
    let cmd = eg
        .parse_program(
            None,
            &format!(
                "(rule ((= a ${prefix}_a) (= b ${prefix}_b) {}) ())",
                if rule.name == "R23" {
                    format!("(= x ${prefix}_x)")
                } else {
                    String::new()
                }
            ),
        )?
        .remove(0);
    let Command::Rule { rule: guards } = cmd else {
        unreachable!()
    };
    local.body.extend(guards.body);
    eg.parse_and_run_program(None, &format!("(ruleset {prefix})"))?;
    eg.run_program(vec![Command::Rule { rule: local }])?;
    eg.parse_and_run_program(None, &format!("(run {prefix} 1)"))?;
    Ok(
        json!({"source_rule":rule.name,"binding":b.json(),"before_enodes":before,"after_enodes":count(eg,names),"net_enode_change":count(eg,names) as i64-before as i64,"scope":"completed native anchored rule step; net row count, not inserted-event count"}),
    )
}
struct Sample {
    round: usize,
    orientation: String,
    binding: Binding,
    snapshot: EGraph,
    baseline_nodes: usize,
    original: Vec<Json>,
}
fn reference_footprint(
    reference: &mut EGraph,
    snapshot: &Snapshot,
    start: &Binding,
    depth: usize,
) -> Result<Json> {
    fn visit(
        eg: &mut EGraph,
        snapshot: &Snapshot,
        e: &Expr,
        covered: &mut HashSet<usize>,
        unresolved: &mut usize,
    ) -> Option<Value> {
        match e {
            Expr::Lit(..) => eg.eval_expr(e).ok().map(|(_, v)| v),
            Expr::Call(_, op, args) => {
                let vals: Option<Vec<_>> = args
                    .iter()
                    .map(|e| visit(eg, snapshot, e, covered, unresolved))
                    .collect();
                let Some(mut vals) = vals else {
                    *unresolved += 1;
                    return None;
                };
                let Some(v) = eg.lookup_function(op, &vals) else {
                    *unresolved += 1;
                    return None;
                };
                let table = snapshot.tables.iter().position(|t| &t.name == op)?;
                vals.push(v);
                if let Some(id) = snapshot.ids.get(&(table, vals)) {
                    covered.insert(*id);
                }
                Some(v)
            }
            _ => None,
        }
    }
    // Interfaces are holes: count just the 2 LHS / 7 RHS constructor occurrences.
    fn explicit(
        eg: &mut EGraph,
        snapshot: &Snapshot,
        b: &Binding,
        covered: &mut HashSet<usize>,
        unresolved: &mut usize,
    ) -> Result<()> {
        let mut scratch = HashSet::new();
        let mut env = BTreeMap::new();
        for (n, s) in [("a", &b.a), ("b", &b.b), ("x", &b.x)] {
            let e = expression(eg, s)?;
            let mut ignored = 0;
            env.insert(n, visit(eg, snapshot, &e, &mut scratch, &mut ignored));
        }
        fn emit(
            eg: &EGraph,
            snapshot: &Snapshot,
            op: &str,
            args: &[Option<Value>],
            covered: &mut HashSet<usize>,
            unresolved: &mut usize,
        ) -> Option<Value> {
            let v: Option<Vec<_>> = args.iter().copied().collect();
            let Some(mut v) = v else {
                *unresolved += 1;
                return None;
            };
            let Some(out) = eg.lookup_function(op, &v) else {
                *unresolved += 1;
                return None;
            };
            let table = snapshot.tables.iter().position(|t| t.name == op).unwrap();
            v.push(out);
            covered.insert(snapshot.ids[&(table, v)]);
            Some(out)
        }
        let (a, b, x) = (env["a"], env["b"], env["x"]);
        let m = emit(eg, snapshot, "Mul", &[a, b], covered, unresolved);
        emit(eg, snapshot, "Integral", &[m, x], covered, unresolved);
        let i = emit(eg, snapshot, "Integral", &[b, x], covered, unresolved);
        let d = emit(eg, snapshot, "Diff", &[x, a], covered, unresolved);
        let left = emit(eg, snapshot, "Mul", &[a, i], covered, unresolved);
        let m = emit(eg, snapshot, "Mul", &[d, i], covered, unresolved);
        let right = emit(eg, snapshot, "Integral", &[m, x], covered, unresolved);
        emit(eg, snapshot, "Sub", &[left, right], covered, unresolved);
        Ok(())
    }
    let mut b = start.clone();
    let mut covered = HashSet::new();
    let mut unresolved = 0;
    for _ in 0..depth {
        explicit(reference, snapshot, &b, &mut covered, &mut unresolved)?;
        b = b.next();
    }
    Ok(
        json!({"reference_covered_enodes":covered.len(),"reference_coverage_ratio":covered.len() as f64/snapshot.rows.len() as f64,"unresolved_reference_constructor_occurrences":unresolved,"scope":"explicit LHS/RHS occurrences mapped by lookup in fixed reference; interface interiors excluded; unresolved does not prove semantic novelty"}),
    )
}
pub fn run(source: &str, output: &str) -> Result<()> {
    let mut original = EGraph::default();
    let text = std::fs::read_to_string(source)?;
    let commands = original.parse_program(Some(source.into()), &text)?;
    let schedule = commands
        .iter()
        .position(|c| c.to_string().starts_with("(run-schedule"))
        .ok_or("missing original schedule")?;
    if commands[schedule].to_string()
        != original.parse_program(None, "(run-schedule (repeat 11 (run)))")?[0].to_string()
    {
        return Err(format!(
            "expected original 11-round schedule, got {}",
            commands[schedule]
        )
        .into());
    }
    let rules: Vec<_> = commands[..schedule]
        .iter()
        .filter_map(|c| {
            if let Command::Rule { rule } = c {
                Some(rule.clone())
            } else {
                None
            }
        })
        .collect();
    let r23 = rules
        .iter()
        .find(|r| r.name == "R23")
        .ok_or("missing R23")?;
    let r1 = rules.iter().find(|r| r.name == "R1").ok_or("missing R1")?;
    if !r23
        .to_string()
        .contains("(Integral (Mul (Diff x a) (Integral b x)) x)")
    {
        return Err("unexpected R23 recurrence".into());
    }
    original.run_program(commands[..schedule].to_vec())?;
    let names: Vec<_> = Snapshot::new(&original)
        .tables
        .into_iter()
        .map(|t| t.name)
        .collect();
    let x = "(Var \"x\")".to_string();
    let cos = format!("(Cos {x})");
    let roots = [
        (
            "original",
            Binding {
                a: cos.clone(),
                b: x.clone(),
                x: x.clone(),
            },
        ),
        (
            "commuted",
            Binding {
                a: x.clone(),
                b: cos,
                x,
            },
        ),
    ];
    let mut samples = vec![];
    let mut trajectory = vec![];
    for round in 0..=11 {
        if round > 0 {
            original.parse_and_run_program(None, "(run 1)")?;
        }
        let n = count(&original, &names);
        eprintln!("original round {round}, {n} nodes");
        trajectory.push(json!({"round":round,"enodes":n}));
        for s in &mut samples {
            let s: &mut Sample = s;
            if round > s.round && round <= s.round + 4 {
                let (d, _, stop) = walk(&mut original, &s.binding, 16)?;
                s.original.push(json!({"opportunities":round-s.round,"completed_depth":d,"stop":stop,"net_enode_change":n as i64-s.baseline_nodes as i64}));
            }
        }
        if [0, 2, 4, 6].contains(&round) {
            for (orientation, root) in &roots {
                if lookup(&mut original, &root.lhs())?.is_none() {
                    continue;
                }
                let (depth, b, stop) = walk(&mut original, root, 16)?;
                if stop != "enabled_rhs_missing" && stop != "enabled_union_missing" {
                    return Err(format!("expected enabled frontier, got {stop}").into());
                }
                eprintln!("sample {round}/{orientation}: prior depth {depth}, {stop}");
                samples.push(Sample {
                    round,
                    orientation: orientation.to_string(),
                    binding: b,
                    snapshot: original.clone(),
                    baseline_nodes: n,
                    original: vec![],
                });
            }
        }
    }
    let reference = Snapshot::new(&original);
    let mut rows = vec![];
    for (sample_id, s) in samples.into_iter().enumerate() {
        let mut branches = vec![];
        for coarse in [false, true] {
            let mut eg = s.snapshot.clone();
            let mut b = s.binding.clone();
            let mut events = vec![];
            let mut checkpoints = vec![];
            if coarse {
                events.push(apply(&mut eg, r1, &b, 10000 + sample_id, &names)?);
            }
            for step in 1..=4 {
                let before = status(&mut eg, &b)?;
                if before == "lhs_missing" {
                    return Err("R23 lost its recursive trigger".into());
                }
                events.push(apply(&mut eg, r23, &b, step, &names)?);
                if status(&mut eg, &b)? != "complete" {
                    return Err("anchored R23 did not complete".into());
                }
                b = b.next();
                if lookup(&mut eg, &b.lhs())?.is_none() {
                    return Err("RHS did not supply next LHS".into());
                }
                let (d, _, stop) = walk(&mut eg, &s.binding, 16)?;
                checkpoints.push(json!({"opportunities":step,"completed_depth":d,"stop":stop,"next_trigger_present":true,"net_enode_change":count(&eg,&names) as i64-s.baseline_nodes as i64,"reference":reference_footprint(&mut original,&reference,&s.binding,d)?}));
            }
            branches.push(json!({"treatment":if coarse{"local_R1_then_focused_R23"}else{"focused_R23"},"extra_coarse_steps":usize::from(coarse),"checkpoints":checkpoints,"native_steps":events}));
        }
        let mut baseline = s.original;
        for c in &mut baseline {
            c["reference"] = reference_footprint(
                &mut original,
                &reference,
                &s.binding,
                c["completed_depth"].as_u64().unwrap() as usize,
            )?;
        }
        rows.push(json!({"snapshot_round":s.round,"orientation":s.orientation,"binding":s.binding.json(),"initial_status":status(&mut s.snapshot.clone(),&s.binding)?,"original_schedule":baseline,"interventions":branches}));
    }
    assert!(reference.unchanged(&original));
    std::fs::write(
        output,
        serde_json::to_string_pretty(
            &json!({"status":"complete","source":source,"reference_enodes":reference.rows.len(),"trajectory":trajectory,"samples":rows,"budget":"four R23 opportunities: four original all-rule rounds versus four anchored local steps; coarse arm has one extra legal R1 step. Not equal total work.","scope":"native finite paired interventions on two orientations of one seed lineage, sampled at rounds 0/2/4/6; not independent random samples; original branch reused from unmodified trajectory","reference_unchanged":true,"coarse_scope":"R1 at the same concrete factors is an extra-effect control, not an inferred necessary trigger repair"}),
        )?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (EGraph, Rule, Vec<String>, Binding) {
        let mut eg = EGraph::default();
        eg.parse_and_run_program(None,"(datatype Math (Var String) (Cos Math) (Mul Math Math) (Sub Math Math) (Diff Math Math) (Integral Math Math)) (Integral (Mul (Cos (Var \"x\")) (Var \"x\")) (Var \"x\")) (Integral (Mul (Cos (Var \"y\")) (Var \"y\")) (Var \"y\"))").unwrap();
        let c=eg.parse_program(None,"(rule ((= root (Integral (Mul a b) x))) ((union root (Sub (Mul a (Integral b x)) (Integral (Mul (Diff x a) (Integral b x)) x)))) :name \"R23\")").unwrap().remove(0);
        let Command::Rule { rule } = c else {
            unreachable!()
        };
        let names = Snapshot::new(&eg)
            .tables
            .into_iter()
            .map(|t| t.name)
            .collect();
        (
            eg,
            rule,
            names,
            Binding {
                a: "(Cos (Var \"x\"))".into(),
                b: "(Var \"x\")".into(),
                x: "(Var \"x\")".into(),
            },
        )
    }
    #[test]
    fn anchored_steps_preserve_other_binding_and_supply_the_next_trigger() {
        let (mut eg, r, names, start) = setup();
        let mut b = start.clone();
        let y = Binding {
            a: "(Cos (Var \"y\"))".into(),
            b: "(Var \"y\")".into(),
            x: "(Var \"y\")".into(),
        };
        for k in 1..=4 {
            assert_eq!(status(&mut eg, &b).unwrap(), "enabled_rhs_missing");
            apply(&mut eg, &r, &b, k, &names).unwrap();
            assert_eq!(status(&mut eg, &b).unwrap(), "complete");
            b = b.next();
            assert!(lookup(&mut eg, &b.lhs()).unwrap().is_some());
        }
        assert_eq!(status(&mut eg, &y).unwrap(), "enabled_rhs_missing");
        assert_eq!(walk(&mut eg, &start, 16).unwrap().0, 4);
        let snapshot = Snapshot::new(&eg);
        let reference = reference_footprint(&mut eg, &snapshot, &start, 4).unwrap();
        assert_eq!(reference["reference_covered_enodes"], 26);
        assert_eq!(reference["unresolved_reference_constructor_occurrences"], 0);
        assert!(snapshot.unchanged(&eg));
    }
    #[test]
    fn missing_union_is_not_a_missing_lhs_or_completed_step() {
        let (mut eg, r, names, b) = setup();
        eg.parse_and_run_program(None, &b.rhs()).unwrap();
        assert_eq!(status(&mut eg, &b).unwrap(), "enabled_union_missing");
        apply(&mut eg, &r, &b, 1, &names).unwrap();
        assert_eq!(status(&mut eg, &b).unwrap(), "complete");
    }
}
