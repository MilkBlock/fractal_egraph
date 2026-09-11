//! Native LHS/RHS coverage probes over a frozen final e-graph.
use egglog::{
    EGraph, Value,
    ast::{Action, Command, Expr, Fact, Rule},
};
use serde_json::{Value as Json, json};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    error::Error,
    io::Write,
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
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
        };
        let b = Instance {
            all: vec![2, 3, 4],
            interior: vec![3, 4],
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
}
