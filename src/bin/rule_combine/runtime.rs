use super::compose::{Candidate, Pat, Rule};
use egglog::{EGraph, RuleActionOutcome, TraceSession, Value};
use serde_json::json;
use std::collections::{BTreeMap, HashMap, HashSet};
type Env = BTreeMap<String, Value>;
#[derive(Clone)]
enum Arg {
    E(Value),
    Int(i64),
}
#[derive(Clone)]
struct Node {
    op: String,
    args: Vec<Arg>,
}
pub struct Snapshot {
    rows: HashMap<Value, Vec<Node>>,
}
const OPS: &[(&str, usize)] = &[
    ("Var", 1),
    ("Zero", 0),
    ("One", 0),
    ("Add", 2),
    ("Mul", 2),
    ("Neg", 1),
    ("S0", 1),
    ("S1", 1),
    ("S2", 1),
    ("S3", 1),
    ("S4", 1),
    ("S5", 1),
    ("S6", 1),
    ("S7", 1),
    ("S8", 1),
];
fn canon(eg: &EGraph, v: Value) -> Value {
    eg.get_canonical_value(v, eg.get_sort_by_name("E").unwrap())
}
impl Snapshot {
    pub fn new(eg: &EGraph) -> Self {
        let mut rows: HashMap<Value, Vec<Node>> = HashMap::new();
        for &(op, arity) in OPS {
            eg.function_for_each(op, |row| {
                let args = row.vals[..arity]
                    .iter()
                    .map(|v| {
                        if op == "Var" {
                            Arg::Int(eg.value_to_base::<i64>(*v))
                        } else {
                            Arg::E(canon(eg, *v))
                        }
                    })
                    .collect();
                rows.entry(canon(eg, row.vals[arity]))
                    .or_default()
                    .push(Node {
                        op: op.into(),
                        args,
                    });
            })
            .unwrap();
        }
        Self { rows }
    }
    fn matches(&self, pat: &Pat, root: Value, env: &Env, budget: &mut usize) -> Vec<Env> {
        if *budget == 0 {
            return vec![];
        }
        *budget -= 1;
        match pat {
            Pat::Var(v) => {
                if env.get(v).is_some_and(|x| *x != root) {
                    vec![]
                } else {
                    let mut env = env.clone();
                    env.insert(v.clone(), root);
                    vec![env]
                }
            }
            Pat::App(op, args) => {
                let mut result = Vec::new();
                if let Some(nodes) = self.rows.get(&root) {
                    for node in nodes {
                        if node.op != *op || node.args.len() != args.len() {
                            continue;
                        }
                        let mut partial = vec![env.clone()];
                        for (p, a) in args.iter().zip(&node.args) {
                            let Arg::E(v) = a else {
                                partial.clear();
                                break;
                            };
                            partial = partial
                                .iter()
                                .flat_map(|e| self.matches(p, *v, e, budget))
                                .take(16)
                                .collect();
                        }
                        result.extend(partial);
                        if result.len() >= 16 {
                            break;
                        }
                    }
                }
                result
            }
        }
    }
    pub fn closure(&self, roots: &[Value]) -> serde_json::Value {
        // Least-size ground representative names every class, including cyclic classes.
        let mut labels: HashMap<Value, (usize, String)> = HashMap::new();
        loop {
            let mut changed = false;
            for (id, nodes) in &self.rows {
                for node in nodes {
                    let mut cost = 1;
                    let mut parts = Vec::new();
                    let mut ready = true;
                    for a in &node.args {
                        match a {
                            Arg::Int(i) => parts.push(i.to_string()),
                            Arg::E(v) => {
                                if let Some((c, s)) = labels.get(v) {
                                    cost += c;
                                    parts.push(s.clone());
                                } else {
                                    ready = false;
                                    break;
                                }
                            }
                        }
                    }
                    if !ready {
                        continue;
                    }
                    let repr = if parts.is_empty() {
                        format!("({})", node.op)
                    } else {
                        format!("({} {})", node.op, parts.join(" "))
                    };
                    let candidate = (cost, repr);
                    if labels.get(id).is_none_or(|old| candidate < *old) {
                        labels.insert(*id, candidate);
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        assert_eq!(
            labels.len(),
            self.rows.len(),
            "all classes must have a ground witness"
        );
        let mut records = Vec::new();
        for (id, nodes) in &self.rows {
            for node in nodes {
                let args: Vec<_> = node
                    .args
                    .iter()
                    .map(|a| match a {
                        Arg::Int(i) => format!("literal:{i}"),
                        Arg::E(v) => labels[v].1.clone(),
                    })
                    .collect();
                records.push((labels[id].1.clone(), node.op.clone(), args));
            }
        }
        records.sort();
        records.dedup();
        json!({"nodes":records,"roots":roots.iter().map(|r|labels[r].1.clone()).collect::<Vec<_>>()})
    }
}
fn eval(eg: &EGraph, p: &Pat, env: &Env) -> Option<Value> {
    match p {
        Pat::Var(v) => env.get(v).copied().map(|v| canon(eg, v)),
        Pat::App(op, a) => {
            let args: Option<Vec<_>> = a.iter().map(|p| eval(eg, p, env)).collect();
            eg.lookup_function(op, &args?).map(|v| canon(eg, v))
        }
    }
}
pub fn setup(rules: &[Rule]) -> EGraph {
    let mut eg = EGraph::default();
    let mut text = "(datatype E (Var i64) (Zero) (One) (Add E E) (Mul E E) (Neg E)".to_string();
    for i in 0..9 {
        text += &format!(" (S{i} E)");
    }
    text += ")\n(ruleset base)\n(ruleset shortcuts)\n(unstable-combined-ruleset active base shortcuts)\n";
    for r in rules {
        text += &r.command("base");
        text.push('\n');
    }
    eg.parse_and_run_program(None, &text).unwrap();
    eg
}
pub fn insert(eg: &mut EGraph, case: &str, id: usize) -> Value {
    let v = format!("(Var {id})");
    let expr = match case {
        "chain" => format!("(S0 {v})"),
        "negative" => format!("(Add {v} (Var {}))", id + 10000000),
        _ => format!("(Mul (Neg (Neg {v})) (Add (One) (Zero)))"),
    };
    let expr = eg.parser.get_expr_from_string(None, &expr).unwrap();
    eg.eval_expr(&expr).unwrap().1
}
pub struct Run {
    pub graph: EGraph,
    pub roots: Vec<Value>,
    pub rounds: usize,
    pub events: usize,
    pub probes: usize,
    pub enabled: Vec<usize>,
    pub selected: Vec<usize>,
    pub activation: Vec<serde_json::Value>,
    pub preprocess_ns: u128,
    pub inference_ns: u128,
    pub discovery_ns: u128,
    pub install_ns: u128,
    pub candidates: Vec<Candidate>,
}
pub fn run(case: &str, mode: &str, count: usize, waves: usize) -> Run {
    use std::time::Instant;
    assert!(["original", "observe", "static", "dynamic"].contains(&mode));
    let t = Instant::now();
    let rules = super::compose::rules(case);
    let candidates = if mode == "original" {
        Vec::new()
    } else {
        super::compose::candidates(&rules)
    };
    let preprocess_ns = t.elapsed().as_nanos();
    assert!(candidates.len() <= 64, "candidate budget exceeded");
    let mut graph = setup(&rules);
    let mut roots = Vec::new();
    let mut enabled = Vec::new();
    let mut selected = Vec::new();
    let mut install_ns = 0;
    let mut discovery_ns = 0;
    let mut activation = Vec::new();
    if mode == "static" {
        let t = Instant::now();
        for (i, c) in candidates.iter().enumerate() {
            graph
                .parse_and_run_program(None, &c.rule.command("shortcuts"))
                .unwrap();
            enabled.push(i);
        }
        install_ns += t.elapsed().as_nanos();
    }
    let mut rounds = 0;
    let mut events = 0;
    let mut probes = 0;
    let mut support = vec![0usize; candidates.len()];
    let mut inference_ns = 0;
    for wave in 0..waves {
        for j in 0..count {
            roots.push(insert(&mut graph, case, wave * count + j));
        }
        let mut saturated = false;
        for round in 0..32 {
            let step = Instant::now();
            let trace = TraceSession::new();
            let discovering = (mode == "observe" || mode == "dynamic")
                && selected.len() < candidates.len().min(8);
            let report = if discovering {
                graph.step_rules_with_trace("active", &trace)
            } else {
                graph.step_rules("active")
            }
            .unwrap();
            rounds += 1;
            let outcomes: HashSet<_> = trace
                .drain_action_outcomes()
                .into_iter()
                .filter(|o| o.outcome == RuleActionOutcome::Survived)
                .map(|o| o.match_event_id)
                .collect();
            let matches = trace.drain_matches();
            events += matches.len();
            let mut newly = 0;
            if discovering && !matches.is_empty() {
                let t = Instant::now();
                let snapshot = Snapshot::new(&graph);
                let mut ready = Vec::new();
                let mut visited = HashSet::new();
                let mut round_probes = 0;
                'events: for event in matches
                    .iter()
                    .filter(|e| outcomes.contains(&e.event_id))
                    .take(4096)
                {
                    let Some(first) = rules.iter().position(|r| r.name == event.rule.as_ref())
                    else {
                        continue;
                    };
                    let vars = rules[first].lhs.vars();
                    let env: Env = event
                        .bindings
                        .iter()
                        .filter_map(|b| {
                            let name = b.name.as_deref()?;
                            vars.contains(name)
                                .then(|| (name.to_owned(), canon(&graph, b.value)))
                        })
                        .collect();
                    for (i, c) in candidates
                        .iter()
                        .enumerate()
                        .filter(|(i, c)| c.first == first && !selected.contains(i))
                    {
                        if ready.contains(&i) {
                            continue;
                        }
                        let Some(target) = eval(&graph, rules[first].rhs.at(&c.path), &env) else {
                            continue;
                        };
                        if !visited.insert((i, target)) {
                            continue;
                        }
                        if round_probes >= 128 {
                            break 'events;
                        }
                        round_probes += 1;
                        probes += 1;
                        let mut budget = 256;
                        if !snapshot
                            .matches(&rules[c.second].lhs, target, &Env::new(), &mut budget)
                            .is_empty()
                        {
                            support[i] += 1;
                            if support[i] >= 2 && !ready.contains(&i) {
                                ready.push(i);
                            }
                        }
                    }
                }
                discovery_ns += t.elapsed().as_nanos();
                {
                    let t = Instant::now();
                    ready.sort_by_key(|i| std::cmp::Reverse(support[*i]));
                    for i in ready.into_iter().take(8 - selected.len()) {
                        selected.push(i);
                        if mode == "dynamic" {
                            graph
                                .parse_and_run_program(
                                    None,
                                    &candidates[i].rule.command("shortcuts"),
                                )
                                .unwrap();
                            enabled.push(i);
                            newly += 1;
                        }
                        activation.push(json!({"wave":wave,"round":round,"candidate":i,"observations":support[i],"installed":mode=="dynamic"}));
                    }
                    install_ns += t.elapsed().as_nanos();
                }
            }
            inference_ns += step.elapsed().as_nanos();
            if !report.updated && newly == 0 {
                saturated = true;
                break;
            }
        }
        assert!(saturated, "failed to saturate");
    }
    let roots = roots.into_iter().map(|v| canon(&graph, v)).collect();
    Run {
        graph,
        roots,
        rounds,
        events,
        probes,
        enabled,
        selected,
        activation,
        preprocess_ns,
        inference_ns,
        discovery_ns,
        install_ns,
        candidates,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shortcuts_preserve_complete_saturated_graph() {
        for case in ["chain", "algebra", "negative"] {
            let base = run(case, "original", 3, 2);
            let expected = Snapshot::new(&base.graph).closure(&base.roots);
            for mode in ["observe", "static", "dynamic"] {
                let r = run(case, mode, 3, 2);
                assert_eq!(
                    Snapshot::new(&r.graph).closure(&r.roots),
                    expected,
                    "{case}/{mode}"
                );
                if case == "negative" && mode == "dynamic" {
                    assert!(r.enabled.is_empty());
                }
            }
        }
    }
    #[test]
    fn every_candidate_has_a_two_step_derivation() {
        for case in ["chain", "algebra"] {
            let rules = super::super::compose::rules(case);
            for c in super::super::compose::candidates(&rules) {
                let mut eg = setup(&rules);
                let mut env = Env::new();
                for (i, v) in c.rule.lhs.vars().iter().enumerate() {
                    let e = eg
                        .parser
                        .get_expr_from_string(None, &format!("(Var {})", i + 100))
                        .unwrap();
                    env.insert(v.clone(), eg.eval_expr(&e).unwrap().1);
                }
                fn ground(p: &Pat) -> String {
                    match p {
                        Pat::Var(v) => format!(
                            "(Var {})",
                            v.strip_prefix('v').unwrap().parse::<usize>().unwrap() + 100
                        ),
                        Pat::App(op, a) => format!(
                            "({}{})",
                            op,
                            a.iter()
                                .map(|p| format!(" {}", ground(p)))
                                .collect::<String>()
                        ),
                    }
                }
                let e = eg
                    .parser
                    .get_expr_from_string(None, &ground(&c.rule.lhs))
                    .unwrap();
                let root = eg.eval_expr(&e).unwrap().1;
                let a = &rules[c.first];
                let b = &rules[c.second];
                eg.parse_and_run_program(
                    None,
                    &format!(
                        "(ruleset first)\n(ruleset second)\n{}\n{}",
                        a.command("first"),
                        b.command("second")
                    ),
                )
                .unwrap();
                eg.step_rules("first").unwrap();
                let mid = eval(&eg, &c.middle, &env).expect("first RHS exists");
                assert_eq!(canon(&eg, root), mid);
                eg.step_rules("second").unwrap();
                let target = eval(&eg, &c.rule.rhs, &env).expect("composed RHS exists");
                assert_eq!(canon(&eg, root), target);
            }
        }
    }
}
