//! Native LHS/RHS coverage probes over a frozen final e-graph.
use egglog::{
    ast::{Action, Command, Expr, Fact, Rule},
    EGraph, Value,
};
use serde_json::{json, Value as Json};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    error::Error,
    io::Write,
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
#[allow(dead_code)]
#[path = "tier0_probe/breadth.rs"]
mod breadth;
#[path = "tier0_probe/fractal.rs"]
mod fractal;
#[path = "tier0_probe/intervention.rs"]
mod intervention;
pub use intervention::run as r23_intervention;
pub use breadth::run as coverage_breadth;
#[derive(Clone)]
struct Table {
    name: String,
    input: Vec<String>,
    output: String,
    eq: Vec<bool>,
}
struct Snapshot {
    tables: Vec<Table>,
    rows: Vec<(usize, Vec<Value>)>,
    ids: HashMap<(usize, Vec<Value>), usize>,
    uses: HashMap<(String, Value), usize>,
    alternatives: HashMap<(String, Value), usize>,
}
impl Snapshot {
    fn new(eg: &EGraph) -> Self {
        let mut names = eg.get_function_names();
        names.sort();
        let tables: Vec<_> = names
            .into_iter()
            .filter_map(|name| {
                let f = eg.get_function(&name).unwrap();
                let s = f.schema();
                s.output.is_eq_sort().then(|| Table {
                    name,
                    input: s.input.iter().map(|s| s.name().to_string()).collect(),
                    output: s.output.name().into(),
                    eq: s.input.iter().map(|s| s.is_eq_sort()).collect(),
                })
            })
            .collect();
        let mut rows = vec![];
        let mut ids = HashMap::new();
        let mut uses = HashMap::new();
        let mut alternatives = HashMap::new();
        for (i, t) in tables.iter().enumerate() {
            eg.function_for_each(&t.name, |r| {
                if r.subsumed {
                    return;
                }
                let row = r.vals.to_vec();
                let id = rows.len();
                assert!(ids.insert((i, row.clone()), id).is_none());
                for (j, &eq) in t.eq.iter().enumerate() {
                    if eq {
                        *uses.entry((t.input[j].clone(), row[j])).or_insert(0) += 1;
                    }
                }
                *alternatives
                    .entry((t.output.clone(), *row.last().unwrap()))
                    .or_insert(0) += 1;
                rows.push((i, row));
            })
            .unwrap();
        }
        Self {
            tables,
            rows,
            ids,
            uses,
            alternatives,
        }
    }
    fn unchanged(&self, eg: &EGraph) -> bool {
        self.tables.iter().enumerate().all(|(i, t)| {
            let mut count = 0;
            let mut same = true;
            eg.function_for_each(&t.name, |r| {
                if !r.subsumed {
                    count += 1;
                    same &= self.ids.contains_key(&(i, r.vals.to_vec()));
                }
            })
            .unwrap();
            same && count == self.rows.iter().filter(|(j, _)| *j == i).count()
        })
    }
}
#[derive(Clone)]
enum Ref {
    Var(String),
    Lit(Value),
}
#[derive(Clone)]
struct Atom {
    table: usize,
    inputs: Vec<Ref>,
    output: Ref,
    covered: bool,
}
struct Query<'a> {
    eg: &'a mut EGraph,
    snapshot: &'a Snapshot,
    types: BTreeMap<String, String>,
    used: BTreeSet<String>,
    original: BTreeSet<String>,
    clauses: Vec<String>,
    atoms: Vec<Atom>,
    roots: Vec<Ref>,
    counter: usize,
    equalities: Vec<(Ref, Ref)>,
}
impl<'a> Query<'a> {
    fn infer(&self, e: &Expr) -> Option<String> {
        match e {
            Expr::Call(_, op, _) => self
                .eg
                .get_function(op)
                .map(|f| f.schema().output.name().into()),
            Expr::Var(_, v) => self.types.get(v).cloned(),
            _ => None,
        }
    }
    fn expr(&mut self, e: &Expr, expected: Option<String>, covered: bool) -> Result<(String, Ref)> {
        match e {
            Expr::Var(_, v) => {
                if v.starts_with("__probe_") {
                    return Err("reserved probe variable prefix".into());
                }
                if let Some(s) = expected {
                    if let Some(old) = self.types.insert(v.clone(), s.clone()) {
                        if old != s {
                            return Err("variable sort mismatch".into());
                        }
                    }
                }
                self.used.insert(v.clone());
                self.original.insert(v.clone());
                Ok((v.clone(), Ref::Var(v.clone())))
            }
            Expr::Lit(..) => {
                let (_, v) = self.eg.eval_expr(e)?;
                Ok((e.to_string(), Ref::Lit(v)))
            }
            Expr::Call(_, op, args) => {
                let table = self
                    .snapshot
                    .tables
                    .iter()
                    .position(|t| t.name == *op)
                    .ok_or("only constructor calls supported in probes")?;
                let schema = self.snapshot.tables[table].clone();
                if args.len() != schema.input.len() {
                    return Err("constructor arity mismatch".into());
                }
                let mut values = vec![];
                let mut texts = vec![];
                for (arg, sort) in args.iter().zip(&schema.input) {
                    let (s, v) = self.expr(arg, Some(sort.clone()), covered)?;
                    texts.push(s);
                    values.push(v);
                }
                let name = format!("__probe_v{}", self.counter);
                self.counter += 1;
                self.used.insert(name.clone());
                self.types.insert(name.clone(), schema.output);
                self.clauses
                    .push(format!("(= {name} ({op} {}))", texts.join(" ")));
                let output = Ref::Var(name.clone());
                self.atoms.push(Atom {
                    table,
                    inputs: values,
                    output: output.clone(),
                    covered,
                });
                Ok((name, output))
            }
        }
    }
    fn fact(&mut self, f: &Fact, covered: bool) -> Result<()> {
        match f {
            Fact::Eq(_, a, b) => {
                let sort = self.infer(a).or_else(|| self.infer(b));
                let (a_text, a_ref) = self.expr(a, sort.clone(), covered)?;
                let (b_text, b_ref) = self.expr(b, sort, covered)?;
                self.equalities.push((a_ref.clone(), b_ref.clone()));
                self.clauses.push(format!("(= {a_text} {b_text})"));
                if covered {
                    if matches!(a, Expr::Call(..)) {
                        self.roots.push(a_ref);
                    }
                    if matches!(b, Expr::Call(..)) {
                        self.roots.push(b_ref);
                    }
                }
            }
            Fact::Fact(e) => {
                let (_, v) = self.expr(e, None, covered)?;
                if covered {
                    self.roots.push(v);
                }
            }
        }
        Ok(())
    }
}
#[derive(Clone)]
struct Instance {
    all: Vec<usize>,
    interior: Vec<usize>,
    bindings: Option<BTreeMap<String, Value>>,
}
fn pack(instances: &[(usize, &Instance)], template_costs: &[usize], interior: bool) -> Json {
    let mut sorted = instances.to_vec();
    sorted.sort_by_key(|(family, i)| {
        (
            std::cmp::Reverse(if interior {
                i.interior.len()
            } else {
                i.all.len()
            }),
            i.all.clone(),
            *family,
        )
    });
    let mut removed = HashSet::<usize>::new();
    let mut retained = HashSet::<usize>::new();
    let mut families = BTreeSet::new();
    let mut selected = 0;
    for (family, i) in sorted {
        let candidate = if interior { &i.interior } else { &i.all };
        if candidate.len() <= 1 {
            continue;
        }
        let set: HashSet<_> = candidate.iter().copied().collect();
        if candidate
            .iter()
            .any(|n| removed.contains(n) || retained.contains(n))
            || i.all
                .iter()
                .filter(|n| !set.contains(n))
                .any(|n| removed.contains(n))
        {
            continue;
        }
        removed.extend(candidate.iter().copied());
        retained.extend(i.all.iter().filter(|n| !set.contains(n)).copied());
        families.insert(family);
        selected += 1;
    }
    let overhead: usize = families.iter().map(|&i| template_costs[i]).sum();
    let net = removed.len() as i64 - selected as i64 - overhead as i64;
    json!({"selected_instances":selected,"covered_or_interior_nodes":removed.len(),"new_instance_nodes":selected,"template_node_cost":overhead,"net_node_estimate":net,"best_of_this_packing_or_noop":net.max(0),"packing":"greedy by immediate node gain, conflicts include removals against retained interfaces; not optimal"})
}
fn probe(
    base: &EGraph,
    snapshot: &Snapshot,
    rule: &Rule,
    side: &str,
    guards: &[Fact],
) -> Result<(Json, Vec<Instance>, usize)> {
    probe_impl(base, snapshot, rule, side, guards, false)
}
fn probe_impl(
    base: &EGraph,
    snapshot: &Snapshot,
    rule: &Rule,
    side: &str,
    guards: &[Fact],
    capture: bool,
) -> Result<(Json, Vec<Instance>, usize)> {
    let mut eg = base.clone();
    let mut q = Query {
        eg: &mut eg,
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
        for f in &rule.body {
            q.fact(f, true)?;
        }
    } else {
        for a in &rule.head.0 {
            match a {
                Action::Union(span, a, b) => {
                    q.fact(&Fact::Eq(span.clone(), a.clone(), b.clone()), true)?
                }
                Action::Expr(_, e) => {
                    q.fact(&Fact::Fact(e.clone()), true)?;
                }
                _ => {
                    return Err(
                        "RHS action not representable as a positive constructor/equality pattern"
                            .into(),
                    );
                }
            }
        }
        for f in guards {
            q.fact(f, false)?;
        }
    }
    let pattern_nodes = q.atoms.iter().filter(|a| a.covered).count();
    if pattern_nodes == 0 {
        return Ok((
            json!({"rule":rule.name,"side":side,"status":"no_explicit_constructor_nodes","instances":null,"unique_covered_nodes":0}),
            vec![],
            0,
        ));
    }
    let names: Vec<_> = q.used.iter().cloned().collect();
    let sorts = names
        .iter()
        .map(|n| {
            q.types
                .get(n)
                .cloned()
                .ok_or("unbound/untyped interface variable")
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let atoms = q.atoms.clone();
    let original = q.original.clone();
    let types = q.types.clone();
    let roots = q.roots.clone();
    let source = format!(
        "(sort __ProbeInstance) (constructor __Probe ({}) __ProbeInstance) (ruleset __probe_only) (rule ({}) ((__Probe {})) :ruleset __probe_only) (run __probe_only 1)",
        sorts.join(" "),
        q.clauses.join(" "),
        names.join(" ")
    );
    drop(q);
    eg.parse_and_run_program(None, &source)?;
    let index: BTreeMap<_, _> = names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.clone(), i))
        .collect();
    let mut instances = vec![];
    let mut unique = HashSet::<usize>::new();
    let mut gross = 0;
    let mut intra_duplicates = 0;
    eg.function_for_each("__Probe", |row| {
        let resolve = |r: &Ref| match r {
            Ref::Var(n) => row.vals[index[n]],
            Ref::Lit(v) => *v,
        };
        let mut footprint = BTreeSet::new();
        let mut protected = HashSet::new();
        for n in &original {
            if eg.get_sort_by_name(&types[n]).unwrap().is_eq_sort() {
                protected.insert((types[n].clone(), row.vals[index[n]]));
            }
        }
        for r in &roots {
            if let Ref::Var(n) = r {
                protected.insert((types[n].clone(), resolve(r)));
            }
        }
        for a in &atoms {
            let mut key: Vec<_> = a.inputs.iter().map(resolve).collect();
            key.push(resolve(&a.output));
            let id = snapshot.ids[&(a.table, key)];
            if a.covered {
                footprint.insert(id);
            } else {
                let (table, r) = &snapshot.rows[id];
                let t = &snapshot.tables[*table];
                protected.insert((t.output.clone(), *r.last().unwrap()));
            }
        }
        gross += footprint.len();
        intra_duplicates += pattern_nodes - footprint.len();
        unique.extend(footprint.iter().copied());
        let mut inside_uses = HashMap::new();
        let mut inside_alts = HashMap::new();
        for &id in &footprint {
            let (table, r) = &snapshot.rows[id];
            let t = &snapshot.tables[*table];
            for (j, &eq) in t.eq.iter().enumerate() {
                if eq {
                    *inside_uses.entry((t.input[j].clone(), r[j])).or_insert(0) += 1;
                }
            }
            *inside_alts
                .entry((t.output.clone(), *r.last().unwrap()))
                .or_insert(0) += 1;
        }
        let interior = footprint
            .iter()
            .filter(|&&id| {
                let (table, r) = &snapshot.rows[id];
                let key = (snapshot.tables[*table].output.clone(), *r.last().unwrap());
                !protected.contains(&key)
                    && snapshot.uses.get(&key).copied().unwrap_or(0)
                        == inside_uses.get(&key).copied().unwrap_or(0)
                    && snapshot.alternatives[&key] == inside_alts[&key]
            })
            .copied()
            .collect();
        instances.push(Instance {
            all: footprint.into_iter().collect(),
            interior,
            bindings: capture.then(|| {
                original
                    .iter()
                    .map(|n| (n.clone(), row.vals[index[n]]))
                    .collect()
            }),
        });
    })?;
    assert!(
        snapshot.unchanged(&eg),
        "probe modified original constructor rows"
    );
    let count = instances.len();
    let costs = [pattern_nodes + 1];
    let refs: Vec<_> = instances.iter().map(|i| (0, i)).collect();
    let mut ideal = pack(&refs, &costs, false);
    let mut conservative = pack(&refs, &costs, true);
    for r in [&mut ideal, &mut conservative] {
        r["ratio_of_original_nodes"] = json!(
            r["best_of_this_packing_or_noop"].as_i64().unwrap() as f64 / snapshot.rows.len() as f64
        );
    }
    let report = json!({"rule":rule.name,"side":side,"status":"complete","pattern_node_occurrences":pattern_nodes,"marker_type":"separate __ProbeInstance, no union into Math","instances":count,"projected_columns":names,"naive_instances_times_pattern_size":count*pattern_nodes,"within_instance_duplicate_occurrences":intra_duplicates,"gross_distinct_per_instance":gross,"unique_covered_nodes":unique.len(),"unique_coverage_ratio":unique.len() as f64/snapshot.rows.len() as f64,"cross_instance_repeated_coverage":gross-unique.len(),"whole_footprint_disjoint_model":ideal,"external_reference_filtered_model":conservative,"frozen_original_rows_unchanged":true,"probe_source":source});
    Ok((report, instances, pattern_nodes + 1))
}
struct FamilyState {
    binding: [Value; 3],
    lhs: Vec<usize>,
    rhs: Vec<usize>,
    next: Option<usize>,
    blocked: &'static str,
}
fn family_walk(states: &[FamilyState], seeds: &[usize], denominator: usize) -> Json {
    let mut frontier: BTreeSet<_> = seeds.iter().copied().collect();
    let mut visited = BTreeSet::new();
    let mut discovered = frontier.clone();
    let mut lhs = BTreeSet::new();
    let mut rhs = BTreeSet::new();
    let mut blocked = BTreeMap::new();
    let mut revisits = 0;
    let mut checkpoints = vec![];
    let mut previous = 0;
    let mut complete = 0;
    let trigger: BTreeSet<_> = seeds
        .iter()
        .flat_map(|&i| states[i].lhs.iter().copied())
        .collect();
    let mut sample = vec![];
    for depth in 1..=8 {
        let mut next = BTreeSet::new();
        for id in frontier {
            if !visited.insert(id) {
                continue;
            }
            let s = &states[id];
            lhs.extend(s.lhs.iter().copied());
            if let Some(target) = s.next {
                complete += 1;
                rhs.extend(s.rhs.iter().copied());
                if sample.len() < 8 {
                    sample.push(json!({"binding":s.binding.iter().map(|v|format!("{v:?}")).collect::<Vec<_>>(),"next_binding":states[target].binding.iter().map(|v|format!("{v:?}")).collect::<Vec<_>>(),"lhs_nodes":s.lhs,"rhs_nodes":s.rhs}));
                }
                if discovered.insert(target) {
                    next.insert(target);
                } else {
                    revisits += 1;
                }
            } else {
                *blocked.entry(s.blocked).or_insert(0usize) += 1;
            }
        }
        frontier = next;
        if [1, 2, 4, 8].contains(&depth) {
            let union: BTreeSet<_> = lhs.union(&rhs).copied().collect();
            checkpoints.push(json!({"depth":depth,"visited_bindings":visited.len(),"complete_rule_states":complete,"lhs_unique":lhs.len(),"rhs_unique":rhs.len(),"joint_unique":union.len(),"new_since_previous_checkpoint":union.len()-previous,"beyond_trigger_lhs":union.difference(&trigger).count(),"coverage_ratio":union.len() as f64/denominator as f64,"blocked_bindings":blocked,"remaining_frontier":frontier.len(),"revisited_transitions":revisits}));
            previous = union.len();
        }
    }
    json!({"seed_bindings":seeds.len(),"trigger_lhs_nodes":trigger.len(),"checkpoints":checkpoints,"sample_complete_transitions":sample})
}
fn lookup_readonly(eg: &mut EGraph, e: &Expr) -> Option<Value> {
    match e {
        Expr::Lit(..) => eg.eval_expr(e).ok().map(|(_, v)| v),
        Expr::Call(_, op, args) => {
            let values = args
                .iter()
                .map(|a| lookup_readonly(eg, a))
                .collect::<Option<Vec<_>>>()?;
            eg.lookup_function(op, &values)
        }
        _ => None,
    }
}
/// Explicit candidate fixture: the direct residual-integral continuation of R23.
/// It does not discover or certify an infinite family.
pub fn integration_family(source: &str, seed: &str) -> Result<Json> {
    let text = std::fs::read_to_string(source)?;
    let mut eg = EGraph::default();
    let commands = eg.parse_program(Some(source.into()), &text)?;
    let rule = commands
        .iter()
        .find_map(|c| {
            if let Command::Rule { rule } = c {
                (rule.name == "R23").then(|| rule.clone())
            } else {
                None
            }
        })
        .ok_or("expected named normalized R23")?;
    let [Fact::Eq(_, Expr::Var(_, root_name), lhs_expr)] = rule.body.as_slice() else {
        return Err("unexpected R23 LHS contract".into());
    };
    let [Action::Union(_, Expr::Var(_, rhs_root), rhs_expr)] = rule.head.0.as_slice() else {
        return Err("unexpected R23 RHS contract".into());
    };
    if root_name != rhs_root
        || lhs_expr.to_string() != "(Integral (Mul a b) x)"
        || rhs_expr.to_string()
            != "(Sub (Mul a (Integral b x)) (Integral (Mul (Diff x a) (Integral b x)) x))"
    {
        return Err("R23 does not match the selected integration-by-parts candidate".into());
    }
    eg.run_program(commands)?;
    let snapshot = Snapshot::new(&eg);
    let mut seed_commands = eg.parse_program(None, seed)?;
    if seed_commands.len() != 1 {
        return Err("one seed expression required".into());
    }
    let Command::Action(Action::Expr(_, seed_expr)) = seed_commands.remove(0) else {
        return Err("seed must be a constructor expression".into());
    };
    let Expr::Call(_, op, integral_args) = &seed_expr else {
        return Err("expected product integral seed".into());
    };
    if op != "Integral" || integral_args.len() != 2 {
        return Err("expected Integral with two arguments".into());
    }
    let Expr::Call(_, op, factors) = &integral_args[0] else {
        return Err("expected Mul seed integrand".into());
    };
    if op != "Mul" || factors.len() != 2 {
        return Err("expected Mul seed integrand".into());
    }
    let initial = [
        lookup_readonly(&mut eg, &factors[0]).ok_or("seed a not present")?,
        lookup_readonly(&mut eg, &factors[1]).ok_or("seed b not present")?,
        lookup_readonly(&mut eg, &integral_args[1]).ok_or("seed x not present")?,
    ];
    let seed_class = lookup_readonly(&mut eg, &seed_expr).ok_or("seed expression not present")?;
    let (lhs_report, lhs_instances, _) = probe_impl(&eg, &snapshot, &rule, "lhs", &[], true)?;
    let (rhs_report, rhs_instances, _) = probe_impl(&eg, &snapshot, &rule, "rhs", &[], true)?;
    let key = |i: &Instance| {
        let b = i.bindings.as_ref().unwrap();
        [b["a"], b["b"], b["x"]]
    };
    let mut index = HashMap::new();
    let mut states = vec![];
    let mut entry_seeds = vec![];
    for i in lhs_instances {
        let binding = key(&i);
        let id = states.len();
        assert!(
            index.insert(binding, id).is_none(),
            "duplicate binding state"
        );
        if i.bindings.as_ref().unwrap()[root_name] == seed_class {
            entry_seeds.push(id);
        }
        states.push(FamilyState {
            binding,
            lhs: i.all,
            rhs: vec![],
            next: None,
            blocked: "rhs-not-present",
        });
    }
    let mut rhs_without_lhs = 0;
    let mut union_missing = 0;
    for instance in rhs_instances {
        let binding = key(&instance);
        let Some(&id) = index.get(&binding) else {
            rhs_without_lhs += 1;
            continue;
        };
        let [a, b, x] = binding;
        let mul = eg.lookup_function("Mul", &[a, b]).unwrap();
        let lhs_root = eg.lookup_function("Integral", &[mul, x]).unwrap();
        if lhs_root != instance.bindings.as_ref().unwrap()[root_name] {
            states[id].blocked = "union-not-present";
            union_missing += 1;
            continue;
        }
        let next = [
            eg.lookup_function("Diff", &[x, a])
                .ok_or("matched RHS missing Diff")?,
            eg.lookup_function("Integral", &[b, x])
                .ok_or("matched RHS missing Integral")?,
            x,
        ];
        let target = *index
            .get(&next)
            .ok_or("RHS residual integral has no matching LHS state")?;
        assert!(states[id].next.is_none());
        states[id].next = Some(target);
        states[id].rhs = instance.all;
        states[id].blocked = "";
    }
    let fixed = *index
        .get(&initial)
        .ok_or("fixed seed binding is not an LHS instance")?;
    let all: Vec<_> = (0..states.len()).collect();
    let fixed_result = family_walk(&states, &[fixed], snapshot.rows.len());
    let rooted_result = family_walk(&states, &entry_seeds, snapshot.rows.len());
    let global_result = family_walk(&states, &all, snapshot.rows.len());
    assert_eq!(
        global_result["checkpoints"][0]["lhs_unique"],
        lhs_report["unique_covered_nodes"]
    );
    if rhs_without_lhs == 0 && union_missing == 0 {
        assert_eq!(
            global_result["checkpoints"][0]["rhs_unique"],
            rhs_report["unique_covered_nodes"]
        );
    }
    let global_counts: Vec<_> = global_result["checkpoints"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["joint_unique"].clone())
        .collect();
    assert!(
        global_counts.iter().all(|n| n == &global_counts[0]),
        "all-starts coverage must already include every complete constituent instance"
    );
    assert!(snapshot.unchanged(&eg));
    Ok(
        json!({"source":source,"seed_expression":seed,"original_enodes":snapshot.rows.len(),"lhs_states":states.len(),"complete_states":states.iter().filter(|s|s.next.is_some()).count(),"entry_class_seed_count":entry_seeds.len(),"rhs_instances_without_lhs":rhs_without_lhs,"rhs_instances_without_union":union_missing,"transition":"(a,b,x) -> (Diff(x,a), Integral(b,x), x)","resumable_frontier":{"fixed_binding":fractal::observe(&states,&[fixed],8),"entry_eclass":fractal::observe(&states,&entry_seeds,16)},"fixed_binding":fixed_result,"fixed_entry_eclass":rooted_result,"all_lhs_starts_control":global_result,"primitive_lhs_probe":lhs_report,"primitive_rhs_probe":rhs_report,"original_rows_unchanged":true,"scope":"read-only finite family coverage in the native final graph; complete states require LHS, RHS and union equality already present; not historical execution counts or an infinite-rule proof","limitations":["only direct residual-integral continuation; no intervening commutation or coarse rules","entry-eclass roots may contain alternatives learned during the original run","missing RHS or union stops expansion; no missing nodes are manufactured","a repeated canonical binding is visited once, even if multiple roots reach it"]}),
    )
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 {
        return Err("tier0_probe source.egg comma-separated-rule-names output.json".into());
    }
    let text = std::fs::read_to_string(&args[1])?;
    let selected: BTreeSet<_> = args[2].split(',').collect();
    let mut eg = EGraph::default();
    let parsed = eg.parse_program(Some(args[1].clone()), &text)?;
    let commands: Vec<_> = parsed
        .into_iter()
        .enumerate()
        .map(|(i, c)| egg_layout::visual_rule::normalize(c, i))
        .collect();
    let rules: Vec<_> = commands
        .iter()
        .filter_map(|c| {
            if let Command::Rule { rule } = c {
                selected.contains(rule.name.as_str()).then(|| rule.clone())
            } else {
                None
            }
        })
        .collect();
    if rules.len() != selected.len() {
        return Err("some selected rules were not found; use normalized combined.egg names".into());
    }
    // Metadata identifies binding connection constraints for combined RHS probes.
    let mut constraints = BTreeMap::<String, BTreeSet<usize>>::new();
    for line in text.lines() {
        if let Some(s) = line.strip_prefix("; @egg-viz-json ") {
            let m: Json = serde_json::from_str(s)?;
            if m["kind"] == "combined_witness_bundle" {
                let mut indices = BTreeSet::new();
                for c in m["connections"].as_array().ok_or("missing connections")? {
                    let start = c["constraint_start"]
                        .as_u64()
                        .ok_or("missing constraint start")?
                        as usize;
                    let count = c["constraint_count"]
                        .as_u64()
                        .ok_or("missing constraint count")?
                        as usize;
                    indices.extend(start..start + count);
                }
                constraints.insert(
                    m["id"].as_str().ok_or("missing combined ID")?.into(),
                    indices,
                );
            }
        }
    }
    eg.run_program(commands)?;
    let snapshot = Snapshot::new(&eg);
    let denominator = snapshot.rows.len();
    let mut reports = vec![];
    let mut families = vec![];
    let mut costs = vec![];
    let mut incidence = std::io::BufWriter::new(std::fs::File::create(format!(
        "{}.coverage.jsonl",
        args[3]
    ))?);
    for rule in &rules {
        for side in ["lhs", "rhs"] {
            eprintln!("probing {} {side}", rule.name);
            let guards: Vec<_> = constraints
                .get(&rule.name)
                .into_iter()
                .flatten()
                .map(|&i| rule.body[i].clone())
                .collect();
            let (report, instances, cost) = probe(&eg, &snapshot, rule, side, &guards)?;
            for (i, instance) in instances.iter().enumerate() {
                writeln!(
                    incidence,
                    "{}",
                    json!({"rule":rule.name,"side":side,"instance":i,"nodes":instance.all,"interior":instance.interior})
                )?;
            }
            reports.push(report);
            families.push(instances);
            costs.push(cost);
            let partial =
                json!({"original_enodes":denominator,"probes":reports,"status":"running"});
            std::fs::write(&args[3], serde_json::to_string_pretty(&partial)?)?;
        }
    }
    let all: Vec<_> = families
        .iter()
        .enumerate()
        .flat_map(|(f, v)| v.iter().map(move |i| (f, i)))
        .collect();
    let union: HashSet<_> = all
        .iter()
        .flat_map(|(_, i)| i.all.iter().copied())
        .collect();
    incidence.flush()?;
    let mut node_file =
        std::io::BufWriter::new(std::fs::File::create(format!("{}.nodes.jsonl", args[3]))?);
    let mut covered: Vec<_> = union.iter().copied().collect();
    covered.sort_unstable();
    for id in covered {
        let (table, row) = &snapshot.rows[id];
        let t = &snapshot.tables[*table];
        writeln!(
            node_file,
            "{}",
            json!({"node":id,"operator":t.name,"input_sorts":t.input,"inputs":row[..row.len()-1].iter().map(|v|format!("{v:?}")).collect::<Vec<_>>(),"output_sort":t.output,"output":format!("{:?}",row.last().unwrap())})
        )?;
    }
    node_file.flush()?;
    let side_totals:BTreeMap<_,_>=["lhs","rhs"].into_iter().enumerate().map(|(side,name)|{
        let subset:Vec<_>=all.iter().filter(|(family,_)|family%2==side).copied().collect();
        let unique:HashSet<_>=subset.iter().flat_map(|(_,i)|i.all.iter().copied()).collect();
        (name,json!({"unique_nodes":unique.len(),"coverage_ratio":unique.len() as f64/denominator as f64,"whole_footprint_disjoint_model":pack(&subset,&costs,false),"external_reference_filtered_model":pack(&subset,&costs,true)}))
    }).collect();
    let result = json!({"source":args[1],"status":"complete","original_enodes":denominator,"probes":reports,"side_totals":side_totals,"lhs_rhs_joint_unique_coverage":union.len(),"joint_whole_footprint_disjoint_model":pack(&all,&costs,false),"joint_external_reference_filtered_model":pack(&all,&costs,true),"scope":"actual native final e-graph probes; marker-only rules in clones, original schedules preserved; no actual compression or enode deletion","limitations":["greedy packing is not optimal","node-unit template/instance model, not byte accounting","filtered model retains roots, variable interfaces, externally used classes and external class alternatives","virtual matching/rebuild support would still be required for a real compressed representation","combined RHS uses exported binding-connection constraints; guards are not counted as RHS coverage"]});
    std::fs::write(&args[3], serde_json::to_string_pretty(&result)?)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn base() -> EGraph {
        let mut e = EGraph::default();
        e.parse_and_run_program(None, "(datatype E (Leaf i64) (A E) (Pair E E) (Wrap E))")
            .unwrap();
        e
    }
    fn rule(e: &mut EGraph, s: &str) -> Rule {
        let Command::Rule { rule } = e.parse_program(None, s).unwrap().remove(0) else {
            panic!()
        };
        rule
    }
    #[test]
    fn counts_intra_match_and_cross_match_overlap_separately() {
        let mut e = base();
        e.parse_and_run_program(
            None,
            "(Pair (A (Leaf 0)) (A (Leaf 0))) (Pair (A (Leaf 0)) (A (Leaf 1)))",
        )
        .unwrap();
        let r = rule(
            &mut e,
            "(rule ((= root (Pair (A x) (A y)))) ((union root (Pair (A y) (A x)))))",
        );
        let s = Snapshot::new(&e);
        let (report, _, _) = probe(&e, &s, &r, "lhs", &[]).unwrap();
        assert_eq!(report["instances"], 2);
        assert_eq!(report["naive_instances_times_pattern_size"], 6);
        assert_eq!(report["within_instance_duplicate_occurrences"], 1);
        assert_eq!(report["unique_covered_nodes"], 4);
        assert_eq!(report["cross_instance_repeated_coverage"], 1);
    }
    #[test]
    fn different_enodes_in_one_class_are_not_one_coverage_item() {
        let mut e = base();
        e.parse_and_run_program(None, "(union (A (Leaf 0)) (A (Leaf 1)))")
            .unwrap();
        let r = rule(&mut e, "(rule ((= root (A x))) ((union root (A x))))");
        let (report, _, _) = probe(&e, &Snapshot::new(&e), &r, "lhs", &[]).unwrap();
        assert_eq!(report["instances"], 2);
        assert_eq!(report["unique_covered_nodes"], 2);
    }
    #[test]
    fn rhs_binding_constraints_prevent_unrelated_cartesian_products() {
        let mut e = base();
        e.parse_and_run_program(None, "(Wrap (Leaf 0)) (Wrap (Leaf 1))")
            .unwrap();
        let r = rule(
            &mut e,
            "(rule ((= p (A x)) (= q (A y)) (= x y)) ((union p (Wrap x)) (union q (Wrap y))))",
        );
        let s = Snapshot::new(&e);
        let (loose, _, _) = probe(&e, &s, &r, "rhs", &[]).unwrap();
        let (bound, _, _) = probe(&e, &s, &r, "rhs", &[r.body[2].clone()]).unwrap();
        assert_eq!(loose["instances"], 4);
        assert_eq!(bound["instances"], 2);
    }
    #[test]
    fn packing_never_counts_the_same_removed_node_twice() {
        let a = Instance {
            all: vec![0, 1, 2],
            interior: vec![1, 2],
            bindings: None,
        };
        let b = Instance {
            all: vec![2, 3, 4],
            interior: vec![3, 4],
            bindings: None,
        };
        let p = pack(&[(0, &a), (0, &b)], &[0], true);
        assert_eq!(p["selected_instances"], 1);
    }
    #[test]
    fn outside_uses_remove_nodes_from_the_interior_estimate() {
        let mut e = base();
        e.parse_and_run_program(None, "(Wrap (A (A (Leaf 0))))")
            .unwrap();
        let r = rule(
            &mut e,
            "(rule ((= root (Wrap (A (A x))))) ((union root x)))",
        );
        let (_, before, _) = probe(&e, &Snapshot::new(&e), &r, "lhs", &[]).unwrap();
        assert_eq!(before[0].interior.len(), 2);
        e.parse_and_run_program(None, "(Pair (A (A (Leaf 0))) (Leaf 1))")
            .unwrap();
        let (_, after, _) = probe(&e, &Snapshot::new(&e), &r, "lhs", &[]).unwrap();
        assert_eq!(after[0].interior.len(), 1);
        let (variable, _, _) = probe(&e, &Snapshot::new(&e), &r, "rhs", &[]).unwrap();
        assert_eq!(variable["status"], "no_explicit_constructor_nodes");
        assert!(variable["instances"].is_null());
    }
    #[test]
    fn family_walk_deduplicates_cycles_and_shared_starts() {
        let e = EGraph::default();
        let v = e.base_to_value(0i64);
        let states = vec![
            FamilyState {
                binding: [v; 3],
                lhs: vec![0],
                rhs: vec![1],
                next: Some(1),
                blocked: "",
            },
            FamilyState {
                binding: [v; 3],
                lhs: vec![1],
                rhs: vec![2],
                next: Some(0),
                blocked: "",
            },
        ];
        let r = family_walk(&states, &[0], 10);
        assert_eq!(r["checkpoints"][0]["joint_unique"], 2);
        assert_eq!(r["checkpoints"][3]["joint_unique"], 3);
        assert_eq!(r["checkpoints"][3]["visited_bindings"], 2);
        let all = family_walk(&states, &[0, 1], 10);
        assert_eq!(all["checkpoints"][0]["joint_unique"], 3);
        assert_eq!(all["checkpoints"][0]["remaining_frontier"], 0);
    }
    #[test]
    fn native_family_stops_at_missing_rhs_or_missing_union() {
        let seed = "(Integral (Mul (Cos (Var \"x\")) (Var \"x\")) (Var \"x\"))";
        let rhs = "(Sub (Mul (Cos (Var \"x\")) (Integral (Var \"x\") (Var \"x\"))) (Integral (Mul (Diff (Var \"x\") (Cos (Var \"x\"))) (Integral (Var \"x\") (Var \"x\"))) (Var \"x\")))";
        for do_union in [true, false] {
            let mut source = String::from(
                "(datatype Math (Var String) (Cos Math) (Mul Math Math) (Integral Math Math) (Diff Math Math) (Sub Math Math))\n(rule ((= root (Integral (Mul a b) x))) ((union root (Sub (Mul a (Integral b x)) (Integral (Mul (Diff x a) (Integral b x)) x)))) :name \"R23\")\n",
            );
            source.push_str(seed);
            source.push('\n');
            source.push_str(if do_union { "(run 1)" } else { rhs });
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "integration-family-{}-{nonce}.egg",
                std::process::id()
            ));
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .unwrap()
                .write_all(source.as_bytes())
                .unwrap();
            let result = integration_family(path.to_str().unwrap(), seed);
            std::fs::remove_file(path).unwrap();
            let r = result.unwrap();
            assert_eq!(r["original_rows_unchanged"], true);
            let last = &r["fixed_binding"]["checkpoints"][3];
            if do_union {
                assert_eq!(last["complete_rule_states"], 1);
                assert_eq!(last["joint_unique"], 8);
                assert_eq!(last["blocked_bindings"]["rhs-not-present"], 1);
            } else {
                assert_eq!(last["complete_rule_states"], 0);
                assert_eq!(last["joint_unique"], 2);
                assert_eq!(last["blocked_bindings"]["union-not-present"], 1);
            }
        }
    }
}
