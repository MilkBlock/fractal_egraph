//! Bounded, observed interface grammars over native coarse/smooth layers.
//! Finite witnesses are never promoted to arbitrary-depth proofs or semantic unions.
use crate::coarse_smooth::{Effect, LayerStore, RelativeBinding};
use crate::dag_embedding::{self, Dag, Edge, Node, Outcome};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Port {
    Internal { member: usize, output: usize },
    Boundary { port: usize },
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum BindingExpr {
    Input(usize),
    Int(i64),
    Text(String),
    Call(String, Vec<BindingExpr>),
    Projection(String),
    Unresolved(String),
    OpaqueLiteral(String),
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Condition {
    Equal(BindingExpr, BindingExpr),
    Fact(BindingExpr),
}
#[derive(Clone)]
struct SourceContract {
    outputs: Vec<BindingExpr>,
    conditions: Option<Vec<Condition>>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Member {
    pub rule: String,
    pub kind: String,
    pub input_roles: Vec<String>,
    pub output_roles: Vec<String>,
    pub output_terms: Vec<BindingExpr>,
    pub conditions: Option<Vec<Condition>>,
    pub binding: Vec<Port>,
    pub parents: Vec<Port>,
    pub values: Vec<usize>,
    pub required: Vec<Effect>,
    pub produced: Vec<Effect>,
    pub external_facts: Vec<Effect>,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Interface {
    pub members: Vec<Member>,
    /// Concrete histories outside the fragment are replaced by local ports.
    pub boundary_roles: Vec<String>,
    pub returns: Vec<Vec<Port>>,
    pub return_values: Vec<Vec<usize>>,
    pub return_requirements: Vec<Vec<Effect>>,
}
#[derive(Clone, Serialize)]
pub struct Template {
    pub interface: Interface,
    pub witnesses: Vec<Vec<usize>>,
}
#[derive(Clone, Serialize)]
pub struct Unit {
    pub template: usize,
    pub entry: usize,
    pub members: Vec<usize>,
    pub returns: Vec<usize>,
}
#[derive(Clone, Serialize)]
pub struct FractalComb {
    pub template: usize,
    pub units: Vec<usize>,
    pub recursive_edges: Vec<(usize, usize, usize)>,
    pub triggers: Vec<usize>,
    pub max_observed_depth: usize,
    pub external_each_unit: bool,
    pub status: String,
}
#[derive(Default, Serialize)]
pub struct Analysis {
    pub use_fractals: crate::use_fractals::Analysis,
    pub templates: Vec<Template>,
    pub units: Vec<Unit>,
    pub fractals: Vec<FractalComb>,
    pub coverage: Vec<Value>,
    pub truncated_candidates: usize,
    pub coverage_queries: usize,
    pub coverage_cache_hits: usize,
    pub coverage_skipped: usize,
    pub scope: String,
}
/// Reuse structural comparisons across completed rounds. Occurrence coverage
/// is still recomputed from the current prefix, so cached proofs never invent witnesses.
#[derive(Default)]
pub struct Analyzer {
    interfaces: BTreeMap<Interface, usize>,
    comparisons: BTreeMap<(usize, usize), Result<Outcome, String>>,
}
impl Analyzer {
    pub fn analyze(&mut self, store: &LayerStore) -> Analysis {
        analyze_cached(store, self)
    }
}

fn alias(id: usize, values: &mut BTreeMap<usize, usize>) -> usize {
    let n = values.len();
    *values.entry(id).or_insert(n)
}
fn effect(e: &Effect, values: &mut BTreeMap<usize, usize>) -> Effect {
    match e {
        Effect::RowFact(v) => Effect::RowFact(alias(*v, values)),
        Effect::Equal(a, b) => Effect::Equal(alias(*a, values), alias(*b, values)),
    }
}
/// A rooted ordered dependency slice. Parent traversal follows binding roles,
/// so independent event interleaving does not determine member order.
fn ordered(store: &LayerStore, selected: &BTreeSet<usize>) -> Vec<usize> {
    fn visit(
        s: &LayerStore,
        i: usize,
        set: &BTreeSet<usize>,
        seen: &mut BTreeSet<usize>,
        out: &mut Vec<usize>,
    ) {
        if !set.contains(&i) || !seen.insert(i) {
            return;
        }
        let a = &s.occurrences[i].apply;
        let mut ps = vec![];
        for b in &a.binding {
            if let RelativeBinding::ParentPort { parent, .. } = b {
                let p = a.parents[*parent];
                if !ps.contains(&p) {
                    ps.push(p);
                }
            }
        }
        let mut extra: Vec<_> = a
            .parents
            .iter()
            .copied()
            .filter(|p| !ps.contains(p))
            .collect();
        extra.sort_by_key(|p| (&s.occurrences[*p].apply.rule, s.occurrences[*p].apply.event));
        ps.extend(extra);
        for p in ps {
            visit(s, p, set, seen, out);
        }
        out.push(i);
    }
    use std::hash::{Hash, Hasher};
    // Hashes order candidates only; full interfaces still determine equality.
    let mut fingerprints = BTreeMap::new();
    for i in selected {
        let a = &store.occurrences[*i].apply;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        a.rule.hash(&mut h);
        a.input_roles.hash(&mut h);
        a.output_roles.hash(&mut h);
        for b in &a.binding {
            match b {
                RelativeBinding::ParentPort { parent, output } => {
                    fingerprints
                        .get(&a.parents[*parent])
                        .copied()
                        .unwrap_or(0u64)
                        .hash(&mut h);
                    output.hash(&mut h);
                }
                RelativeBinding::External { slot } => {
                    usize::MAX.hash(&mut h);
                    slot.hash(&mut h);
                }
            }
        }
        fingerprints.insert(*i, h.finish());
    }
    let used: BTreeSet<_> = selected
        .iter()
        .flat_map(|i| store.occurrences[*i].apply.parents.iter().copied())
        .collect();
    let mut roots: Vec<_> = selected
        .iter()
        .copied()
        .filter(|i| !used.contains(i))
        .collect();
    roots.sort_by_key(|i| (fingerprints[i], store.occurrences[*i].apply.event));
    let mut out = vec![];
    let mut seen = BTreeSet::new();
    for i in roots {
        visit(store, i, selected, &mut seen, &mut out);
    }
    out
}
fn interface(
    store: &LayerStore,
    members: &[usize],
    returns: &[usize],
    terms: &[SourceContract],
) -> Interface {
    let local: BTreeMap<_, _> = members.iter().enumerate().map(|(k, i)| (*i, k)).collect();
    let mut boundary = BTreeMap::<(usize, usize), usize>::new();
    let mut boundary_roles = vec![];
    let mut values = BTreeMap::new();
    let port = |owner: usize,
                b: &RelativeBinding,
                input: usize,
                boundary: &mut BTreeMap<(usize, usize), usize>,
                boundary_roles: &mut Vec<String>| {
        let a = &store.occurrences[owner].apply;
        let (identity, role, internal) = match b {
            RelativeBinding::ParentPort { parent, output } => {
                let p = a.parents[*parent];
                (
                    (p, *output),
                    // Caller placement belongs to the occurrence, not Γ.
                    format!("input:{}", a.input_roles[input]),
                    local.get(&p).map(|m| Port::Internal {
                        member: *m,
                        output: *output,
                    }),
                )
            }
            RelativeBinding::External { slot } => (
                (usize::MAX, a.external[*slot]),
                format!("input:{}", a.input_roles[input]),
                None,
            ),
        };
        internal.unwrap_or_else(|| {
            let n = boundary.len();
            let k = *boundary.entry(identity).or_insert_with(|| {
                boundary_roles.push(role);
                n
            });
            Port::Boundary { port: k }
        })
    };
    let mut ms = vec![];
    for i in members {
        let o = &store.occurrences[*i];
        let a = &o.apply;
        let binding = a
            .binding
            .iter()
            .enumerate()
            .map(|(input, b)| port(*i, b, input, &mut boundary, &mut boundary_roles))
            .collect();
        // Proof-only parent edges are preserved even without a bound value port.
        let mut parent_order = vec![];
        for b in &a.binding {
            if let RelativeBinding::ParentPort { parent, .. } = b {
                if !parent_order.contains(parent) {
                    parent_order.push(*parent);
                }
            }
        }
        parent_order.extend((0..a.parents.len()).filter(|p| {
            !a.binding
                .iter()
                .any(|b| matches!(b,RelativeBinding::ParentPort{parent,..} if parent==p))
        }));
        let mut parents: Vec<_> = parent_order
            .iter()
            .map(|p| &a.parents[*p])
            .map(|p| {
                if let Some(m) = local.get(p) {
                    Port::Internal {
                        member: *m,
                        output: usize::MAX,
                    }
                } else {
                    let key = (*p, usize::MAX);
                    let n = boundary.len();
                    let k = *boundary.entry(key).or_insert_with(|| {
                        boundary_roles.push("parent-context".into());
                        n
                    });
                    Port::Boundary { port: k }
                }
            })
            .collect();
        parents.sort();
        ms.push(Member {
            rule: a.rule.clone(),
            // A layout restart is not a semantic turning point. This fragment
            // explicitly exposes the supplying contexts as boundary ports.
            kind: if o.boundary_restart {
                "SmoothComb".into()
            } else {
                format!("{:?}", store.combs[o.comb].kind)
            },
            input_roles: a.input_roles.clone(),
            output_roles: a.output_roles.clone(),
            output_terms: terms[*i].outputs.clone(),
            conditions: terms[*i].conditions.clone(),
            binding,
            parents,
            values: a
                .wanted
                .iter()
                .chain(&a.outputs)
                .chain(&a.external)
                .map(|v| alias(*v, &mut values))
                .collect(),
            required: a.required.iter().map(|e| effect(e, &mut values)).collect(),
            produced: a.produced.iter().map(|e| effect(e, &mut values)).collect(),
            external_facts: a
                .external_facts
                .iter()
                .map(|e| effect(e, &mut values))
                .collect(),
        });
    }
    let mut ret = vec![];
    let mut rv = vec![];
    let mut req = vec![];
    for i in returns {
        let a = &store.occurrences[*i].apply;
        ret.push(
            a.binding
                .iter()
                .enumerate()
                .map(|(input, b)| port(*i, b, input, &mut boundary, &mut boundary_roles))
                .collect(),
        );
        rv.push(a.wanted.iter().map(|v| alias(*v, &mut values)).collect());
        req.push(a.required.iter().map(|e| effect(e, &mut values)).collect());
    }
    Interface {
        members: ms,
        boundary_roles,
        returns: ret,
        return_values: rv,
        return_requirements: req,
    }
}
fn intern(
    a: &mut Analysis,
    keys: &mut BTreeMap<Interface, usize>,
    key: Interface,
    members: Vec<usize>,
) -> usize {
    if let Some(i) = keys.get(&key) {
        if !a.templates[*i].witnesses.contains(&members) {
            a.templates[*i].witnesses.push(members);
        }
        *i
    } else {
        let i = a.templates.len();
        keys.insert(key.clone(), i);
        a.templates.push(Template {
            interface: key,
            witnesses: vec![members],
        });
        i
    }
}
/// Fixed candidate budget: one full layer slice (up to 32 members), one local
/// step and one forward return unit per occurrence. No all-subgraph enumeration.
pub fn analyze(store: &LayerStore) -> Analysis {
    Analyzer::default().analyze(store)
}
fn analyze_cached(store: &LayerStore, cache: &mut Analyzer) -> Analysis {
    let mut a=Analysis{scope:"Finite interface templates and witnessed return bindings. Coverage is structural with exact alias checks; unbounded FractalDominance and guard implication remain unknown. No tier0 effects are removed.".into(),..Default::default()};
    a.use_fractals = crate::use_fractals::analyze(store);
    let terms = output_terms(store);
    let mut keys = BTreeMap::new();
    let mut children = vec![vec![]; store.occurrences.len()];
    for (i, o) in store.occurrences.iter().enumerate() {
        for p in &o.apply.parents {
            children[*p].push(i);
        }
    }
    for (i, o) in store.occurrences.iter().enumerate() {
        intern(
            &mut a,
            &mut keys,
            interface(store, &[i], &[], &terms),
            vec![i],
        );
        // Stop at coarse introductions; their earlier histories become inputs.
        let mut set = BTreeSet::new();
        let mut todo = vec![i];
        while let Some(p) = todo.pop() {
            if !set.insert(p) {
                continue;
            }
            if set.len() > 32 {
                break;
            }
            if store.occurrences[p].smooth_layer.is_some() {
                todo.extend(&store.occurrences[p].apply.parents);
            }
        }
        if set.len() <= 32 {
            let members = ordered(store, &set);
            intern(
                &mut a,
                &mut keys,
                interface(store, &members, &[], &terms),
                members,
            );
        } else {
            a.truncated_candidates += 1;
        }
        // Each actual same-rule edge is also a bounded return witness. Other
        // consumers of the entry remain open, rather than polluting this body.
        let direct: Vec<_> = children[i]
            .iter()
            .copied()
            .filter(|p| {
                let n = &store.occurrences[*p].apply;
                n.rule == o.apply.rule
                    && n.input_roles == o.apply.input_roles
                    && n.output_roles == o.apply.output_roles
            })
            .collect();
        for next in direct.iter().take(64) {
            let t = intern(
                &mut a,
                &mut keys,
                interface(store, &[i], &[*next], &terms),
                vec![i],
            );
            a.units.push(Unit {
                template: t,
                entry: i,
                members: vec![i],
                returns: vec![*next],
            });
        }
        a.truncated_candidates += direct.len().saturating_sub(64);
        // Return to the entry rule/interface, including several apply points.
        let mut body = BTreeSet::from([i]);
        let mut returns = BTreeSet::new();
        let mut queue: VecDeque<_> = children[i].iter().map(|p| (*p, 1)).collect();
        let mut seen = BTreeSet::new();
        let mut truncated = false;
        while let Some((p, d)) = queue.pop_front() {
            if !seen.insert(p) {
                continue;
            }
            if seen.len() > 64 {
                truncated = true;
                break;
            }
            let next = &store.occurrences[p].apply;
            if next.rule == o.apply.rule
                && next.input_roles == o.apply.input_roles
                && next.output_roles == o.apply.output_roles
            {
                returns.insert(p);
                continue;
            }
            if d >= 3 || body.len() >= 16 || seen.len() > 64 {
                truncated = true;
                break;
            }
            body.insert(p);
            queue.extend(children[p].iter().map(|c| (*c, d + 1)));
        }
        if truncated {
            a.truncated_candidates += 1;
            continue;
        }
        if returns.is_empty() {
            continue;
        }
        let members = ordered(store, &body);
        let mut returns: Vec<_> = returns.into_iter().collect();
        // Binding routing, rather than application time, names sibling returns.
        returns.sort_by_key(|p| {
            let one = interface(store, &members, &[*p], &terms);
            serde_json::to_string(&one.returns).unwrap()
        });
        let t = intern(
            &mut a,
            &mut keys,
            interface(store, &members, &returns, &terms),
            members.clone(),
        );
        a.units.push(Unit {
            template: t,
            entry: i,
            members,
            returns,
        });
    }
    // Remove duplicate direct/full-unit witnesses before following returns.
    let mut unit_keys = BTreeSet::new();
    a.units
        .retain(|u| unit_keys.insert((u.template, u.entry, u.returns.clone())));
    let mut by_entry = BTreeMap::<(usize, usize), Vec<usize>>::new();
    for (i, u) in a.units.iter().enumerate() {
        by_entry.entry((u.entry, u.template)).or_default().push(i);
    }
    let mut groups = BTreeMap::<usize, Vec<usize>>::new();
    for (i, u) in a.units.iter().enumerate() {
        groups.entry(u.template).or_default().push(i);
    }
    let mut recurrence_edges = 0usize;
    for (template, units) in groups {
        let mut edges = vec![];
        let mut incoming = BTreeSet::new();
        for u in &units {
            for (slot, r) in a.units[*u].returns.iter().enumerate() {
                if let Some(nexts) = by_entry.get(&(*r, template)) {
                    for next in nexts {
                        if recurrence_edges >= 16384 {
                            a.truncated_candidates += 1;
                            continue;
                        }
                        recurrence_edges += 1;
                        edges.push((*u, slot, *next));
                        incoming.insert(*next);
                    }
                }
            }
        }
        if edges.is_empty() {
            continue;
        }
        let mut depth = BTreeMap::new();
        for u in units.iter().rev() {
            let d = 1 + edges
                .iter()
                .filter(|(from, _, _)| from == u)
                .map(|(_, _, to)| depth.get(to).copied().unwrap_or(1))
                .max()
                .unwrap_or(0);
            depth.insert(*u, d);
        }
        a.fractals.push(FractalComb {
            template,
            triggers: units
                .iter()
                .filter(|u| !incoming.contains(*u))
                .map(|u| a.units[*u].entry)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            max_observed_depth: depth.values().copied().max().unwrap_or(1),
            units,
            recursive_edges: edges,
            external_each_unit: external_demand(&a.templates[template].interface),
            status: "observed_finite_recursion; unbounded induction unknown".into(),
        });
    }
    coverage(&mut a, cache);
    a
}
fn external_demand(t: &Interface) -> bool {
    if t.members
        .iter()
        .any(|m| !m.external_facts.is_empty() || m.kind == "CoarseComb")
    {
        return true;
    }
    let entry: BTreeSet<_> = t.members[0]
        .binding
        .iter()
        .chain(&t.members[0].parents)
        .filter_map(|p| {
            if let Port::Boundary { port } = p {
                Some(*port)
            } else {
                None
            }
        })
        .collect();
    t.members
        .iter()
        .skip(1)
        .flat_map(|m| m.binding.iter().chain(&m.parents))
        .chain(t.returns.iter().flatten())
        .any(|p| matches!(p,Port::Boundary{port} if !entry.contains(port)))
}
fn graph(t: &Interface) -> Dag {
    let nodes = t
        .members
        .iter()
        .map(|m| Node {
            label: json!([m.rule, m.kind, m.input_roles, m.output_roles]),
            constraints: Value::Null,
            ports: m.output_roles.clone(),
        })
        .collect();
    let mut edges = vec![];
    for (to, m) in t.members.iter().enumerate() {
        for (input, p) in m.binding.iter().enumerate() {
            if let Port::Internal { member, output } = p {
                edges.push(Edge {
                    from: *member,
                    to,
                    label: json!(["binding", output, input]),
                });
            }
        }
        for p in &m.parents {
            if let Port::Internal { member, .. } = p {
                edges.push(Edge {
                    from: *member,
                    to,
                    label: json!("dependency"),
                });
            }
        }
    }
    Dag {
        root: t.members.len() - 1,
        nodes,
        edges,
        interface: vec![],
    }
}
fn compatible_aliases(s: &Interface, b: &Interface, map: &[usize]) -> bool {
    let mut forward = BTreeMap::new();
    let mut inverse = BTreeMap::new();
    let mut boundary = BTreeMap::<usize, Port>::new();
    for (i, j) in map.iter().enumerate() {
        let x = &s.members[i];
        let y = &b.members[*j];
        if x.values.len() != y.values.len()
            || x.binding.len() != y.binding.len()
            || x.parents.len() != y.parents.len()
        {
            return false;
        }
        for (p, q) in x
            .binding
            .iter()
            .chain(&x.parents)
            .zip(y.binding.iter().chain(&y.parents))
        {
            match p {
                Port::Internal { member, output } => {
                    if q != &(Port::Internal {
                        member: map[*member],
                        output: *output,
                    }) {
                        return false;
                    }
                }
                Port::Boundary { port } => {
                    if boundary.get(port).is_some_and(|old| old != q) {
                        return false;
                    }
                    boundary.insert(*port, q.clone());
                }
            }
        }
        let pairs = x.values.iter().zip(&y.values).map(|(x, y)| (*x, *y)).chain(
            x.required
                .iter()
                .chain(&x.produced)
                .chain(&x.external_facts)
                .zip(
                    y.required
                        .iter()
                        .chain(&y.produced)
                        .chain(&y.external_facts),
                )
                .flat_map(|(x, y)| match (x, y) {
                    (Effect::RowFact(a), Effect::RowFact(b)) => vec![(*a, *b)],
                    (Effect::Equal(a, c), Effect::Equal(b, d)) => vec![(*a, *b), (*c, *d)],
                    _ => vec![(usize::MAX, 0), (usize::MAX, 1)],
                }),
        );
        if x.required.len() != y.required.len()
            || x.produced.len() != y.produced.len()
            || x.external_facts.len() != y.external_facts.len()
        {
            return false;
        }
        for (x, y) in pairs {
            if forward.get(&x).is_some_and(|v| *v != y) || inverse.get(&y).is_some_and(|v| *v != x)
            {
                return false;
            }
            forward.insert(x, y);
            inverse.insert(y, x);
        }
    }
    true
}
fn coverage(a: &mut Analysis, cache: &mut Analyzer) {
    let ids: Vec<_> = a
        .templates
        .iter()
        .map(|t| {
            let n = cache.interfaces.len();
            *cache.interfaces.entry(t.interface.clone()).or_insert(n)
        })
        .collect();
    let graphs: Vec<_> = a.templates.iter().map(|t| graph(&t.interface)).collect();
    // A node-label inverted index avoids comparing unrelated rule families.
    let mut postings = BTreeMap::<String, BTreeSet<usize>>::new();
    for (i, g) in graphs.iter().enumerate() {
        for n in &g.nodes {
            postings.entry(n.label.to_string()).or_default().insert(i);
        }
    }
    let family_templates: BTreeSet<_> = a.fractals.iter().map(|f| f.template).collect();
    let mut order: Vec<_> = (0..graphs.len()).collect();
    order.sort_by_key(|i| {
        (
            !family_templates.contains(i),
            std::cmp::Reverse(a.templates[*i].witnesses.len()),
            graphs[*i].nodes.len(),
        )
    });
    for s in order {
        let g = &graphs[s];
        let mut candidates: Vec<_> = postings
            .get(&g.nodes[g.root].label.to_string())
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect();
        candidates.sort_by_key(|i| {
            (
                !family_templates.contains(i),
                std::cmp::Reverse(a.templates[*i].witnesses.len()),
                graphs[*i].nodes.len(),
            )
        });
        for b in candidates {
            if graphs[b].nodes.len() <= g.nodes.len() {
                continue;
            }
            if a.coverage_queries >= 512 {
                a.coverage_skipped += 1;
                continue;
            }
            a.coverage_queries += 1;
            let key = (ids[s], ids[b]);
            let result = if let Some(outcome) = cache.comparisons.get(&key) {
                a.coverage_cache_hits += 1;
                outcome.clone()
            } else {
                let result = dag_embedding::embed(g, &graphs[b], 2000);
                cache.comparisons.insert(key, result.clone());
                result
            };
            match result {
                Ok(Outcome::Found { node_map, .. }) => {
                    let valid = compatible_aliases(
                        &a.templates[s].interface,
                        &a.templates[b].interface,
                        &node_map,
                    );
                    let observed = a.templates[s]
                        .witnesses
                        .iter()
                        .filter(|sw| {
                            a.templates[b].witnesses.iter().any(|bw| {
                                sw.iter()
                                    .enumerate()
                                    .all(|(i, event)| bw.get(node_map[i]) == Some(event))
                            })
                        })
                        .count();
                    a.coverage.push(json!({"small":s,"big":b,"node_map":node_map,"observed_covered_instances":observed,
                        "scope":"body graph coverage; return contracts are not asserted equivalent",
                        "small_fractals":a.fractals.iter().enumerate().filter(|(_,f)|f.template==s).map(|(i,_)|i).collect::<Vec<_>>(),
                        "big_fractals":a.fractals.iter().enumerate().filter(|(_,f)|f.template==b).map(|(i,_)|i).collect::<Vec<_>>(),
                        "status":if valid{"TemplateCoverage"}else{"UnknownBindingMapping"},
                        "semantic_dominance":"unknown; extra boundary requirements and exported outputs must be checked",
                        "fractal_dominance":"unknown"}));
                }
                Ok(Outcome::UnknownBudget { .. }) => a
                    .coverage
                    .push(json!({"small":s,"big":b,"status":"UnknownBudget"})),
                _ => {}
            }
        }
    }
}

// Source expressions remain symbolic. No arithmetic fitting, constructor
// injectivity, guard implication or primitive totality is assumed here.
fn output_terms(store: &LayerStore) -> Vec<SourceContract> {
    use egglog::ast::{Command, Expr, Fact, Literal};
    fn term(e: &Expr, inputs: &[String]) -> BindingExpr {
        match e {
            Expr::Var(_, n) => inputs
                .iter()
                .position(|r| r == &format!("var:{n}"))
                .map(BindingExpr::Input)
                .unwrap_or_else(|| BindingExpr::Unresolved(n.clone())),
            Expr::Lit(_, Literal::Int(n)) => BindingExpr::Int(*n),
            Expr::Lit(_, Literal::String(s)) => BindingExpr::Text(s.clone()),
            Expr::Lit(..) => BindingExpr::OpaqueLiteral(e.to_string()),
            Expr::Call(_, op, args) => {
                BindingExpr::Call(op.clone(), args.iter().map(|e| term(e, inputs)).collect())
            }
        }
    }
    let mut parser = egglog::EGraph::default();
    let mut cache = BTreeMap::new();
    store
        .occurrences
        .iter()
        .map(|o| {
            let a = &o.apply;
            let key = (
                a.rule.clone(),
                a.input_roles.clone(),
                a.output_roles.clone(),
            );
            cache
                .entry(key)
                .or_insert_with(|| {
                    let parsed = parser.parse_program(None, &a.rule).ok();
                    let rule = parsed.as_ref().and_then(|cs| cs.first()).and_then(|c| {
                        if let Command::Rule { rule } = c {
                            Some(rule)
                        } else {
                            None
                        }
                    });
                    let conditions = rule.map(|r| {
                        r.body
                            .iter()
                            .map(|f| match f {
                                Fact::Eq(_, a, b) => Condition::Equal(
                                    term(a, &o.apply.input_roles),
                                    term(b, &o.apply.input_roles),
                                ),
                                Fact::Fact(e) => Condition::Fact(term(e, &a.input_roles)),
                            })
                            .collect()
                    });
                    let outputs = a
                        .output_roles
                        .iter()
                        .map(|role| {
                            if let Some(n) = role.strip_prefix("var:") {
                                return a
                                    .input_roles
                                    .iter()
                                    .position(|r| r == &format!("var:{n}"))
                                    .map(BindingExpr::Input)
                                    .unwrap_or_else(|| BindingExpr::Projection(role.clone()));
                            }
                            let Some((path, col)) = role
                                .strip_prefix("column:")
                                .and_then(|r| r.rsplit_once(':'))
                            else {
                                return BindingExpr::Projection(role.clone());
                            };
                            let Some(e) =
                                rule.and_then(|r| crate::visual_rule::expression_at(r, path))
                            else {
                                return BindingExpr::Projection(role.clone());
                            };
                            let Ok(col) = col.parse::<usize>() else {
                                return BindingExpr::Projection(role.clone());
                            };
                            if let Expr::Call(_, _, args) = e {
                                term(args.get(col).unwrap_or(e), &a.input_roles)
                            } else {
                                BindingExpr::Projection(role.clone())
                            }
                        })
                        .collect();
                    SourceContract {
                        outputs,
                        conditions,
                    }
                })
                .clone()
        })
        .collect()
}
