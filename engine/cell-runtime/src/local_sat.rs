//! Source-authorized local continuation inside one Saturate scope.
//! Reuse checked relative wiring; never infer a match from rule names.
use crate::{program::*, value::Value};
use serde::{Deserialize, Serialize};
use serde_json::{Value as J, json};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

#[derive(Clone)]
pub struct Transfer {
    pub consumer: usize,
    pub nonexpanding: bool,
    pub assignments: Vec<(usize, Arg)>,
    pub rows: Vec<(usize, usize, Vec<Arg>, Option<usize>)>,
}
pub struct Capture {
    pub slots: Vec<usize>,
    pub effects: BTreeSet<usize>,
}
#[derive(Default)]
pub struct Plan {
    pub sets: Vec<String>,
    pub outgoing: BTreeMap<usize, Vec<Transfer>>,
    pub targets: BTreeSet<usize>,
    pub captures: BTreeMap<usize, Rc<Capture>>,
    pub completion_keys: BTreeMap<usize, Vec<usize>>,
    pub rejected: BTreeMap<String, usize>,
}
fn stable(t: &Table) -> bool {
    t.virtual_fact
        || (!t.constructor
            && t.schema.last().is_some_and(|s| s["kind"] == "Unit")
            && (t.merge.is_null() || t.merge == json!({"var":"old"})))
}
fn scalar(sort: &J) -> bool {
    matches!(
        sort["kind"].as_str(),
        Some("i64" | "f64" | "bool" | "Unit" | "String" | "structural_key")
    )
}
fn sort_affected(sort: &J, cone: &BTreeSet<String>) -> bool {
    if sort["kind"] == "eq" {
        cone.contains(sort["name"].as_str().unwrap_or(""))
    } else if sort["kind"] == "set" {
        sort_affected(&sort["element"], cone)
    } else {
        !scalar(sort)
    }
}
fn argument_inputs(p: &Program, rid: usize, arg: &Arg) -> BTreeSet<usize> {
    fn visit(
        p: &Program,
        rid: usize,
        a: &Arg,
        seen: &mut BTreeSet<usize>,
        out: &mut BTreeSet<usize>,
    ) {
        if let Arg::Slot(i) = a {
            if !seen.insert(*i) {
                return;
            }
            let r = &p.rules[rid];
            let t = &p.templates[r.template];
            if r.inputs.contains(i) {
                out.insert(*i);
                return;
            }
            if let Some(op) = t.defs.get(i) {
                match &t.ops[*op] {
                    Op::Copy(_, a) => visit(p, rid, a, seen, out),
                    Op::Let(_, _, args) => {
                        for a in args {
                            visit(p, rid, a, seen, out)
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    let mut out = BTreeSet::new();
    visit(p, rid, arg, &mut BTreeSet::new(), &mut out);
    out
}
fn relevant_inputs(p: &Program, rid: usize) -> BTreeSet<usize> {
    let rule = &p.rules[rid];
    let template = &p.templates[rule.template];
    let mut result = BTreeSet::new();
    let mut visited = BTreeSet::new();
    fn visit(
        a: &Arg,
        r: &Rule,
        t: &Template,
        seen: &mut BTreeSet<usize>,
        out: &mut BTreeSet<usize>,
    ) {
        if let Arg::Slot(i) = a {
            if !seen.insert(*i) {
                return;
            }
            if r.inputs.contains(i) {
                out.insert(*i);
                return;
            }
            if let Some(op) = t.defs.get(i) {
                match &t.ops[*op] {
                    Op::Copy(_, a) => visit(a, r, t, seen, out),
                    Op::Let(_, _, args) => {
                        for a in args {
                            visit(a, r, t, seen, out)
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    for op in &*template.ops {
        match op {
            Op::Let(_, _, args) => {
                for a in args {
                    visit(a, rule, template, &mut visited, &mut result)
                }
            }
            Op::Set(_, args, v) => {
                for a in args {
                    visit(a, rule, template, &mut visited, &mut result)
                }
                visit(v, rule, template, &mut visited, &mut result)
            }
            Op::Union(a, b) => {
                visit(a, rule, template, &mut visited, &mut result);
                visit(b, rule, template, &mut visited, &mut result)
            }
            _ => {}
        }
    }
    result
}
fn publisher(p: &Program, rid: usize) -> bool {
    let ops = &p.templates[p.rules[rid].template].ops;
    ops.iter().any(|o|matches!(o,Op::Set(t,_,_) if p.views.markers.contains(t))) && ops.iter().all(|o| {
        matches!(o, Op::Copy(..))
            || matches!(o,Op::Set(t,_,Arg::Literal(Value::Int(0))) if p.views.markers.contains(t))
    })
}
/// Finite repeat / probes are not saturation permission. Nested saturated inert
/// publishers can implement the source marker prelude for a single ruleset.
fn collect_sets(body: &J) -> Option<Vec<String>> {
    fn collect(n: &J, out: &mut BTreeSet<String>) -> Option<()> {
        if let Some(s) = n["run"].as_str() {
            if n.as_object()?.len() != 1 {
                return None;
            }
            out.insert(s.into());
        } else if let Some(xs) = n["sequence"].as_array() {
            for x in xs {
                collect(x, out)?;
            }
        } else if let Some(s) = n.get("saturate") {
            collect(s, out)?;
        } else {
            return None;
        }
        Some(())
    }
    let mut sets = BTreeSet::new();
    collect(body, &mut sets)?;
    Some(sets.into_iter().collect())
}
pub fn scope_sets(p: &Program, body: &J) -> Option<Vec<String>> {
    let sets = collect_sets(body)?;
    let principal = sets
        .iter()
        .filter(|s| {
            p.sets
                .get(*s)
                .is_some_and(|rs| rs.iter().any(|r| !publisher(p, *r)))
        })
        .count();
    (principal <= 1 && sets.iter().all(|s| p.sets.contains_key(s))).then(|| sets)
}
type Cones = BTreeMap<String, BTreeSet<String>>;
fn union_cones(p: &Program) -> Cones {
    let mut cones: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for t in &p.tables {
        for s in &t.schema {
            if s["kind"] == "eq" {
                let name = s["name"].as_str().unwrap().to_string();
                cones.entry(name.clone()).or_default().insert(name);
            }
        }
    }
    loop {
        let old = cones.clone();
        for roots in cones.values_mut() {
            for t in &p.tables {
                if t.constructor
                    && t.schema.last().unwrap()["kind"] == "eq"
                    && t.schema[..t.schema.len() - 1]
                        .iter()
                        .any(|s| sort_affected(s, roots))
                {
                    roots.insert(t.schema.last().unwrap()["name"].as_str().unwrap().into());
                }
            }
        }
        if old == cones {
            break;
        }
    }
    cones
}
impl Plan {
    pub fn compile(p: &Program, sets: Vec<String>, cones: &Cones) -> Self {
        let rules = sets
            .iter()
            .flat_map(|s| p.sets[s].iter().copied())
            .collect::<BTreeSet<_>>();
        let mut result = Self {
            sets,
            ..Self::default()
        };
        let mut hidden = BTreeSet::new();
        for &r in &rules {
            for op in &*p.templates[p.rules[r].template].ops {
                if let Op::Hide(t, _, _) = op {
                    hidden.insert(*t);
                }
            }
        }
        let mut reads = BTreeMap::new();
        let mut writes = BTreeMap::new();
        let mut unsafe_rules = BTreeSet::new();
        for &r in &rules {
            let rule = &p.rules[r];
            let template = &p.templates[rule.template];
            let mut rs = BTreeSet::new();
            let mut ws = BTreeSet::new();
            let mut safe = true;
            let mut unions = BTreeSet::new();
            let pure_args = |args: &[Arg]| {
                args.iter().all(|a| match a {
                    Arg::Slot(i) => scalar(&template.sorts[*i]),
                    Arg::Literal(v) => !matches!(v, Value::Eq(_) | Value::Set(_)),
                })
            };
            for a in &*rule.query {
                match &a.call {
                    Call::Table(t) => {
                        rs.insert(*t);
                        safe &= stable(&p.tables[*t]) && !hidden.contains(t);
                    }
                    Call::Primitive(..) => safe &= pure_args(&a.args),
                }
            }
            for op in &*template.ops {
                match op {
                    Op::Copy(..) => {}
                    Op::Let(_, Call::Table(t), _) if p.tables[*t].virtual_fact => {
                        ws.insert(*t);
                    }
                    Op::Let(_, Call::Table(t), _) => {
                        rs.insert(*t);
                        safe = false;
                    }
                    Op::Let(_, Call::Primitive(..), args) => safe &= pure_args(args),
                    Op::Set(t, _, Arg::Literal(Value::Int(0)))
                        if stable(&p.tables[*t]) && !p.tables[*t].constructor =>
                    {
                        ws.insert(*t);
                    }
                    Op::Set(t, _, _) if p.tables[*t].constructor => {
                        ws.insert(*t);
                        unions.insert(
                            p.tables[*t].schema.last().unwrap()["name"]
                                .as_str()
                                .unwrap()
                                .into(),
                        );
                    }
                    Op::Set(t, _, _) => {
                        ws.insert(*t);
                        rs.insert(*t);
                        if let Some(names) =
                            p.artifact["merge_contracts"]["tables"][*t]["reads"].as_array()
                        {
                            for name in names {
                                if let Some(table) = p
                                    .tables
                                    .iter()
                                    .position(|t| Some(t.name.as_str()) == name.as_str())
                                {
                                    rs.insert(table);
                                }
                            }
                        }
                        safe = false;
                    }
                    Op::Union(a, _) => {
                        if let Arg::Slot(i) = a {
                            unions.insert(
                                template.sorts[*i]["name"]
                                    .as_str()
                                    .unwrap_or("")
                                    .to_string(),
                            );
                        } else {
                            safe = false
                        }
                    }
                    Op::Hide(t, _, _) => {
                        ws.insert(*t);
                        safe = false;
                    }
                    Op::Panic => safe = false,
                }
            }
            for sort in unions {
                let Some(cone) = cones.get(&sort) else {
                    safe = false;
                    continue;
                };
                for (t, table) in p.tables.iter().enumerate() {
                    let affected = table.schema.iter().any(|s| sort_affected(s, &cone));
                    if affected {
                        ws.insert(t);
                    }
                    let key_affected = table.schema[..table.schema.len() - 1]
                        .iter()
                        .any(|s| sort_affected(s, &cone));
                    let kind = p.artifact["merge_contracts"]["tables"][t]["kind"]
                        .as_str()
                        .unwrap_or("unknown");
                    if key_affected && !stable(table) && !matches!(kind, "lattice" | "key-min") {
                        safe = false;
                    }
                }
            }
            safe &= ws.is_disjoint(&hidden);
            if !safe {
                unsafe_rules.insert(r);
            }
            reads.insert(r, rs);
            writes.insert(r, ws);
        }
        // Preserve ordered consumers by excluding their producer cone, rather
        // than rejecting a mixed ruleset wholesale.
        loop {
            let sensitive = unsafe_rules
                .iter()
                .flat_map(|r| reads[r].iter().copied())
                .collect::<BTreeSet<_>>();
            let before = unsafe_rules.len();
            for &r in &rules {
                if !writes[&r].is_disjoint(&sensitive) {
                    unsafe_rules.insert(r);
                }
            }
            if before == unsafe_rules.len() {
                break;
            }
        }
        result.rejected.insert(
            "ordered_or_nonmonotone_dependency_rules".into(),
            unsafe_rules.len(),
        );
        let mut routes = p
            .multi_stage
            .routes
            .iter()
            .filter(|(_, t)| rules.contains(&t.producer) && rules.contains(&t.consumer))
            .collect::<Vec<_>>();
        routes.sort_by_key(|(key, t)| (t.producer, key.1, t.consumer, t.anchor));
        for (key, t) in routes {
            if unsafe_rules.contains(&t.consumer) {
                continue;
            }
            let source_ops = &p.templates[p.rules[t.producer].template].ops;
            let immediate = |i: usize| {
                matches!(&source_ops[i],Op::Let(_,Call::Table(table),_) if p.tables[*table].virtual_fact)
                    || matches!(&source_ops[i],Op::Set(table,_,_) if p.tables[*table].constructor)
                    || matches!(&source_ops[i],Op::Set(table,_,Arg::Literal(Value::Int(0))) if !p.tables[*table].constructor && stable(&p.tables[*table]))
            };
            if !immediate(key.1)
                || t.reads
                    .iter()
                    .any(|r| r.effect.is_some_and(|i| !immediate(i)))
            {
                *result
                    .rejected
                    .entry("source_effect_waits_for_merge".into())
                    .or_default() += 1;
                continue;
            }
            let q = &p.rules[t.consumer].query;
            let covered = t
                .reads
                .iter()
                .map(|r| r.atom)
                .chain([t.anchor])
                .collect::<BTreeSet<_>>();
            if q.iter()
                .enumerate()
                .any(|(i, a)| matches!(a.call, Call::Table(_)) && !covered.contains(&i))
            {
                *result
                    .rejected
                    .entry("routes_need_external_reads".into())
                    .or_default() += 1;
                continue;
            }
            let Ok((table, cols)) = crate::multistage::source(p, t.producer, "effect", key.1)
            else {
                continue;
            };
            let mut rows = vec![(t.anchor, table, cols, Some(key.1))];
            rows.extend(
                t.reads
                    .iter()
                    .map(|r| (r.atom, r.table, r.columns.clone(), r.effect)),
            );
            let nonexpanding = false;
            result
                .outgoing
                .entry(t.producer)
                .or_default()
                .push(Transfer {
                    consumer: t.consumer,
                    rows,
                    nonexpanding,
                    assignments: vec![],
                });
            result.targets.insert(t.consumer);
        }
        // Query facts may be forwarded even when their parameters are unused
        // by this producer's own RHS. Close relevance through every continuation
        // before certifying a finite-value cycle.
        let mut relevant = rules
            .iter()
            .map(|r| (*r, relevant_inputs(p, *r)))
            .collect::<BTreeMap<_, _>>();
        loop {
            let mut changed = false;
            for (&producer, transfers) in &result.outgoing {
                for transfer in transfers {
                    let needed = relevant[&transfer.consumer].clone();
                    let q = &p.rules[transfer.consumer].query;
                    for (atom, _, cols, _) in &transfer.rows {
                        for (to, from) in q[*atom].args.iter().zip(cols) {
                            if let Arg::Slot(i) = to
                                && needed.contains(i)
                            {
                                for dep in argument_inputs(p, producer, from) {
                                    changed |= relevant.get_mut(&producer).unwrap().insert(dep);
                                }
                            }
                        }
                    }
                }
            }
            if !changed {
                break;
            }
        }
        for (&producer, transfers) in &mut result.outgoing {
            for transfer in transfers {
                let q = &p.rules[transfer.consumer].query;
                let inputs = &p.rules[producer].inputs;
                transfer.nonexpanding = q
                    .iter()
                    .all(|a| !matches!(&a.call,Call::Primitive(_,out) if out!="Unit"))
                    && relevant[&transfer.consumer].iter().all(|slot| {
                        if p.templates[p.rules[transfer.consumer].template].sorts[*slot]["kind"]
                            == "Unit"
                        {
                            return true;
                        }
                        transfer.rows.iter().any(|(atom, _, cols, _)| {
                            q[*atom].args.iter().zip(cols).any(|(to, from)| {
                                *to == Arg::Slot(*slot)
                                    && match from {
                                        Arg::Literal(_) => true,
                                        Arg::Slot(i) => inputs.contains(i),
                                    }
                            })
                        })
                    });
            }
        }
        for (&producer, transfers) in &mut result.outgoing {
            let slots = transfers
                .iter()
                .flat_map(|t| t.rows.iter().flat_map(|(_, _, args, _)| args))
                .filter_map(|a| if let Arg::Slot(i) = a { Some(*i) } else { None })
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let effects = transfers
                .iter()
                .flat_map(|t| t.rows.iter().filter_map(|(_, _, _, op)| *op))
                .collect();
            for t in transfers {
                let mut mapping = BTreeMap::new();
                for (atom, _, columns, _) in &t.rows {
                    for (to, from) in p.rules[t.consumer].query[*atom].args.iter().zip(columns) {
                        if let Arg::Slot(i) = to {
                            let from = match from {
                                Arg::Slot(s) => Arg::Slot(slots.binary_search(s).unwrap()),
                                a => a.clone(),
                            };
                            if let Some(old) = mapping.insert(*i, from.clone()) {
                                assert_eq!(old, from, "checked local wiring changed");
                            }
                        }
                    }
                }
                t.assignments = mapping.into_iter().collect();
            }
            result
                .captures
                .insert(producer, Rc::new(Capture { slots, effects }));
        }
        for &rid in &result.targets {
            let mut key = relevant_inputs(p, rid);
            for a in &*p.rules[rid].query {
                if let Call::Primitive(name, _) = &a.call {
                    if !matches!(name.as_str(), "<" | "<=" | ">" | ">=" | "==" | "!=") {
                        for arg in &a.args[..a.args.len() - 1] {
                            if let Arg::Slot(i) = arg {
                                key.insert(*i);
                            }
                        }
                    }
                }
            }
            result
                .completion_keys
                .insert(rid, key.into_iter().collect());
        }
        result
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ScopeKey {
    pub path: Vec<usize>,
    pub iterations: Vec<u64>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Session {
    pub key: ScopeKey,
    pub sets: Vec<String>,
    pub seen: BTreeSet<(usize, Vec<Value>)>,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Stats {
    pub scopes: u64,
    pub source_runs: u64,
    pub idle_runs: u64,
    pub captures: u64,
    pub transfer_attempts: u64,
    pub local_bindings: u64,
    pub duplicate_bindings: u64,
    pub seed_receipts: u64,
    pub cycle_deferrals: u64,
    pub finite_reentries: u64,
    pub guard_rejections: u64,
    pub incomplete_contexts: u64,
    pub audit_bindings: u64,
    pub max_chain: usize,
    pub peak_seen: usize,
    pub max_pending: usize,
    pub by_rule: BTreeMap<usize, u64>,
    pub by_set: BTreeMap<String, u64>,
    pub dispatch_sets: BTreeMap<String, u64>,
}
pub struct LocalSat {
    pub enabled: bool,
    pub check: bool,
    pub plans: BTreeMap<Vec<String>, Rc<Plan>>,
    pub sessions: Vec<Session>,
    pub stats: Stats,
}
impl LocalSat {
    pub fn new(p: &Program) -> Self {
        let enabled = std::env::var("CELL_CPU_LOCAL_SAT").as_deref() == Ok("1");
        let mut s = Self {
            enabled,
            check: std::env::var("CELL_CPU_LOCAL_SAT_AUDIT").as_deref() == Ok("1"),
            plans: BTreeMap::new(),
            sessions: vec![],
            stats: Stats::default(),
        };
        fn visit(n: &J, p: &Program, s: &mut LocalSat, cones: &Cones) {
            if let Some(body) = n.get("saturate") {
                if let Some(sets) = scope_sets(p, body) {
                    s.plans
                        .entry(sets.clone())
                        .or_insert_with(|| Rc::new(Plan::compile(p, sets, cones)));
                }
                visit(body, p, s, cones);
            }
            if let Some(xs) = n["sequence"].as_array() {
                for x in xs {
                    visit(x, p, s, cones)
                }
            }
            if let Some(b) = n.get("body") {
                visit(b, p, s, cones)
            }
        }
        if enabled {
            let cones = union_cones(p);
            for n in &p.control {
                visit(n, p, &mut s, &cones)
            }
        }
        s
    }
    pub fn enter(&mut self, __p: &Program, body: &J, path: &[usize], iterations: &[u64]) {
        if !self.enabled {
            return;
        }
        let Some(sets) = collect_sets(body) else {
            return;
        };
        if self.plans.get(&sets).is_none_or(|p| p.outgoing.is_empty()) {
            return;
        }
        self.stats.scopes += 1;
        self.sessions.push(Session {
            key: ScopeKey {
                path: path.to_vec(),
                iterations: iterations.to_vec(),
            },
            sets,
            seen: BTreeSet::new(),
        });
    }
    pub fn exit(&mut self, path: &[usize], iterations: &[u64]) {
        self.sessions
            .retain(|s| s.key.path != path || s.key.iterations != iterations);
    }
    pub fn active(&self, set: &str) -> Option<(usize, Rc<Plan>)> {
        self.sessions
            .iter()
            .enumerate()
            .find(|(_, s)| s.sets.iter().any(|n| n == set))
            .map(|(i, s)| (i, self.plans[&s.sets].clone()))
    }
    pub fn report(&self) -> J {
        json!({"enabled":self.enabled,"audit":self.check,"stats":self.stats,
        "active_scopes":self.sessions.len(),"plans":self.plans.values().map(|p|json!({"sets":p.sets,"targets":p.targets,"producer_rules":p.outgoing.len(),"transfers":p.outgoing.values().map(|ts|ts.len()).sum::<usize>(),"rejected":p.rejected})).collect::<Vec<_>>(),
        "scope":"same Saturate scope; complete checked producer wiring; monotone noninterfering lanes; finite-value reentry closes locally; expanding cycles return to ordinary delta queue"})
    }
    pub fn image(&self) -> J {
        json!({"enabled":self.enabled,"sessions":self.sessions,"stats":self.stats})
    }
}
