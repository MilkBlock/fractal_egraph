//! Read-only temporal coverage audit. The native engine executes its real rules;
//! diagnostic pattern joins run over copied rows, never over the live engine.
#[path = "support/cell_query_anatomy.rs"]
mod cell_query_anatomy;
use egg_layout::{
    pipeline::Result, saturated_rule_composition::SaturatedRuleComposition as State, visual_rule,
};
use egglog::{
    EGraph, TraceSession, Value, WriteOutcome,
    ast::{Command, GenericSchedule},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::PathBuf,
    time::Instant,
};
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct RowKey {
    op: String,
    values: Vec<Value>,
}
#[derive(Clone)]
struct DataRow {
    key: RowKey,
    hidden: bool,
}
#[derive(Default)]
struct Table {
    rows: Vec<DataRow>,
    indices: Vec<HashMap<Value, Vec<usize>>>,
}
#[derive(Default)]
struct Snapshot {
    tables: BTreeMap<String, Table>,
    literals: BTreeMap<(String, String), Value>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Atom {
    op: String,
    cols: Vec<usize>,
    hidden: bool,
    credit: bool,
}
#[derive(Clone, Serialize, Deserialize)]
struct Pattern {
    id: usize,
    vars: Vec<(String, Option<String>)>,
    atoms: Vec<Atom>,
}
#[derive(Serialize, Deserialize)]
struct Library {
    version: usize,
    scope: String,
    patterns: Vec<Pattern>,
    unsupported: Vec<Json>,
    signatures: BTreeSet<String>,
}
fn background(op: &str) -> bool {
    matches!(op, "TNil" | "PureTypeList" | "TypeList-length")
}
fn literal(eg: &EGraph, sort: &str, v: Value) -> Option<String> {
    Some(match sort {
        "i64" => eg.value_to_base::<i64>(v).to_string(),
        "String" => {
            serde_json::to_string(&eg.value_to_base::<egglog::sort::S>(v).to_string()).ok()?
        }
        "bool" => eg.value_to_base::<bool>(v).to_string(),
        "Unit" => "()".into(),
        "f64" => {
            let x = eg.value_to_base::<egglog::sort::F>(v).0.0;
            if x.is_nan() {
                "NaN".into()
            } else if x == 0. {
                "0".into()
            } else {
                x.to_string()
            }
        }
        _ => return None,
    })
}
fn normalize(sort: &str, s: &str) -> Result<String> {
    Ok(match sort {
        "String" => serde_json::to_string(&serde_json::from_str::<String>(s)?)?,
        "i64" => s.parse::<i64>()?.to_string(),
        "bool" => s.parse::<bool>()?.to_string(),
        "Unit" if s == "()" => s.into(),
        "f64" => {
            let x = s.parse::<f64>()?;
            if x.is_nan() {
                "NaN".into()
            } else if x == 0. {
                "0".into()
            } else {
                x.to_string()
            }
        }
        _ => return Err(format!("unsupported literal {sort}:{s}").into()),
    })
}
fn patterns(dir: &PathBuf) -> Result<(Vec<Pattern>, Vec<Json>)> {
    let mut paths = std::fs::read_dir(dir)?
        .map(|p| p.map(|p| p.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.sort();
    let mut patterns = vec![];
    let mut unsupported = vec![];
    for path in paths {
        let name = path.file_name().unwrap().to_string_lossy();
        if !name.starts_with("group-") || path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let s: State = serde_json::from_slice(&std::fs::read(&path)?)?;
        if s.rows
            .iter()
            .any(|r| r.op.starts_with('@') && r.op != "@binding")
        {
            unsupported.push(json!({"file":name,"reason":"container incidence requires a container-aware row matcher"}));
            continue;
        }
        let selected: Vec<_> = s
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| !r.op.starts_with('@'))
            .collect();
        if !selected.iter().any(|(_, r)| !background(&r.op)) {
            unsupported.push(json!({"file":name,"reason":"background-only state"}));
            continue;
        }
        let used: BTreeSet<_> = selected
            .iter()
            .flat_map(|(_, r)| r.args.iter().copied().chain([r.result]))
            .collect();
        if s.rows
            .iter()
            .filter(|r| r.op == "@binding")
            .any(|r| !used.contains(&r.args[0]))
        {
            unsupported.push(json!({"file":name,"reason":"unconstrained boundary slot"}));
            continue;
        }
        let map: BTreeMap<_, _> = used.iter().enumerate().map(|(i, x)| (*x, i)).collect();
        let vars = used
            .iter()
            .map(|v| {
                let x = &s.values[*v];
                Ok((
                    x.sort.clone(),
                    x.literal
                        .as_deref()
                        .map(|s| normalize(&x.sort, s))
                        .transpose()?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let atoms = selected
            .into_iter()
            .map(|(i, r)| Atom {
                op: r.op.clone(),
                cols: r.args.iter().chain([&r.result]).map(|v| map[v]).collect(),
                hidden: s.subsumed_rows.contains(&i),
                credit: !background(&r.op),
            })
            .collect();
        let id = name
            .trim_start_matches("group-")
            .trim_end_matches(".json")
            .parse()?;
        patterns.push(Pattern { id, vars, atoms });
    }
    Ok((patterns, unsupported))
}
fn snapshot(eg: &EGraph, patterns: &[Pattern]) -> Result<Snapshot> {
    let ops: BTreeSet<_> = patterns
        .iter()
        .flat_map(|p| p.atoms.iter().map(|a| a.op.clone()))
        .collect();
    let mut snap = Snapshot::default();
    for op in ops {
        let f = eg
            .get_function(&op)
            .ok_or_else(|| format!("missing template operator {op}"))?;
        let sorts: Vec<_> = f
            .schema()
            .input
            .iter()
            .chain([&f.schema().output])
            .map(|s| s.name().to_string())
            .collect();
        for p in patterns {
            for a in &p.atoms {
                if a.op == op
                    && (a.cols.len() != sorts.len()
                        || a.cols
                            .iter()
                            .zip(&sorts)
                            .any(|(v, s)| p.vars.get(*v).is_none_or(|(ty, _)| ty != s)))
                {
                    return Err(format!(
                        "frozen pattern {} has incompatible schema for {op}",
                        p.id
                    )
                    .into());
                }
            }
        }
        let mut table = Table {
            rows: vec![],
            indices: (0..sorts.len()).map(|_| HashMap::new()).collect(),
        };
        eg.function_for_each(&op, |r| {
            let id = table.rows.len();
            let values = r.vals[..sorts.len()].to_vec();
            for (i, v) in values.iter().enumerate() {
                table.indices[i].entry(*v).or_default().push(id);
                if let Some(lit) = literal(eg, &sorts[i], *v) {
                    snap.literals.insert((sorts[i].clone(), lit), *v);
                }
            }
            table.rows.push(DataRow {
                key: RowKey {
                    op: op.clone(),
                    values,
                },
                hidden: r.subsumed,
            });
        })?;
        snap.tables.insert(op, table);
    }
    Ok(snap)
}
#[derive(Clone)]
struct Instance {
    pattern: usize,
    values: Vec<Value>,
}
#[derive(Default)]
struct Cover {
    instances: Vec<Instance>,
    rows: BTreeMap<RowKey, Vec<usize>>,
}
fn join(
    p: &Pattern,
    s: &Snapshot,
    assigned: &mut [Option<Value>],
    remaining: &mut Vec<usize>,
    emit: &mut impl FnMut(Vec<Value>),
) {
    if remaining.is_empty() {
        emit(assigned.iter().map(|x| x.unwrap()).collect());
        return;
    }
    let mut choice = None;
    for (pos, &a) in remaining.iter().enumerate() {
        let atom = &p.atoms[a];
        let table = &s.tables[&atom.op];
        let mut ids = None;
        for (col, &v) in atom.cols.iter().enumerate() {
            if let Some(value) = assigned[v] {
                let Some(rows) = table.indices[col].get(&value) else {
                    return;
                };
                if ids.is_none_or(|old: &Vec<usize>| rows.len() < old.len()) {
                    ids = Some(rows);
                }
            }
        }
        let len = ids.map_or(table.rows.len(), Vec::len);
        if len == 0 {
            return;
        }
        if choice
            .as_ref()
            .is_none_or(|(_, _, n): &(usize, Option<&Vec<usize>>, usize)| len < *n)
        {
            choice = Some((pos, ids, len));
        }
    }
    let (pos, ids, _) = choice.unwrap();
    let atom_index = remaining.remove(pos);
    let atom = &p.atoms[atom_index];
    let table = &s.tables[&atom.op];
    let all;
    let ids = if let Some(ids) = ids {
        ids
    } else {
        all = (0..table.rows.len()).collect::<Vec<_>>();
        &all
    };
    for &id in ids {
        let row = &table.rows[id];
        if row.hidden != atom.hidden {
            continue;
        }
        let mut changed = vec![];
        let mut valid = true;
        for (&v, &value) in atom.cols.iter().zip(&row.key.values) {
            if let Some(old) = assigned[v] {
                if old != value {
                    valid = false;
                    break;
                }
            } else {
                assigned[v] = Some(value);
                changed.push(v);
            }
        }
        if valid {
            join(p, s, assigned, remaining, emit);
        }
        for v in changed {
            assigned[v] = None;
        }
    }
    remaining.insert(pos, atom_index);
}
fn cover(patterns: &[Pattern], s: &Snapshot) -> Cover {
    let mut out = Cover::default();
    for (pi, p) in patterns.iter().enumerate() {
        let mut assigned = vec![None; p.vars.len()];
        let mut possible = true;
        for (i, (sort, lit)) in p.vars.iter().enumerate() {
            if let Some(lit) = lit {
                if let Some(v) = s.literals.get(&(sort.clone(), lit.clone())) {
                    assigned[i] = Some(*v);
                } else {
                    possible = false;
                    break;
                }
            }
        }
        if !possible {
            continue;
        }
        join(
            p,
            s,
            &mut assigned,
            &mut (0..p.atoms.len()).collect(),
            &mut |values| {
                let id = out.instances.len();
                for a in &p.atoms {
                    if a.credit && !a.hidden {
                        let key = RowKey {
                            op: a.op.clone(),
                            values: a.cols.iter().map(|v| values[*v]).collect(),
                        };
                        let xs = out.rows.entry(key).or_default();
                        if xs.last() != Some(&id) {
                            xs.push(id);
                        }
                    }
                }
                out.instances.push(Instance {
                    pattern: pi,
                    values,
                });
            },
        );
    }
    out
}
#[derive(Default, Clone, Serialize)]
struct Counts {
    logical: usize,
    effective_insert_update_union: usize,
    complete_witness: usize,
    read_atoms: usize,
    covered_read_atoms: usize,
    any_cell: usize,
    all_reads_one_cell: usize,
    multiple_internal_atoms: usize,
    one_marker_atom_contraction: usize,
    known_effect_signature: usize,
    signature_candidates: usize,
    signature_unsupported: usize,
}
struct Audit {
    anatomy: cell_query_anatomy::Anatomy,
    library: Library,
    train: bool,
    stats: BTreeMap<String, Counts>,
    effective_stats: BTreeMap<String, Counts>,
    rulesets: BTreeMap<String, Counts>,
    steps: usize,
    audit_seconds: f64,
    native_seconds: f64,
    rule_seconds: BTreeMap<String, f64>,
    profile: BTreeMap<String, [usize; 3]>,
}
fn role(
    eg: &EGraph,
    sort: &str,
    value: Value,
    p: &Pattern,
    instance: &Instance,
    external: &mut BTreeMap<(String, Value), usize>,
) -> Json {
    if let Some(lit) = literal(eg, sort, value) {
        return json!(["literal", sort, lit]);
    }
    let slots: Vec<_> = p
        .vars
        .iter()
        .enumerate()
        .filter_map(|(i, (s, _))| (s == sort && instance.values[i] == value).then_some(i))
        .collect();
    if !slots.is_empty() {
        return json!(["cell", slots]);
    }
    let next = external.len();
    let id = *external.entry((sort.into(), value)).or_insert(next);
    json!(["external_or_residual", sort, id])
}
impl Audit {
    fn step(
        &mut self,
        eg: &mut EGraph,
        set: &str,
        paths: &BTreeMap<String, BTreeMap<String, String>>,
    ) -> Result<(bool, bool)> {
        let tick = Instant::now();
        let snap = snapshot(eg, &self.library.patterns)?;
        let coverage = cover(&self.library.patterns, &snap);
        self.audit_seconds += tick.elapsed().as_secs_f64();
        #[cfg(feature = "native-read-profile")]
        eg.reset_join_read_profile(true);
        let trace = TraceSession::with_dependencies();
        let tick = Instant::now();
        let result = eg.step_rules_with_trace(set, &trace)?;
        self.native_seconds += tick.elapsed().as_secs_f64();
        for (rule, time) in &result.search_and_apply_time_per_rule {
            *self.rule_seconds.entry(rule.to_string()).or_default() += time.as_secs_f64();
        }

        #[cfg(feature = "native-read-profile")]
        for (name, p) in eg.join_read_profile() {
            if let (Some(name), Some(p)) = (name, p) {
                let x = self.profile.entry(name).or_default();
                x[0] += p.scan_rows;
                x[1] += p.refine_rows;
                x[2] += p.key_lookups;
            }
        }
        let batch = trace.drain_completed();
        let schemas: BTreeMap<_, _> = trace
            .table_names()
            .into_iter()
            .filter_map(|(t, n)| {
                eg.get_function(&n).map(|f| {
                    (
                        t,
                        (
                            n.to_string(),
                            f.schema()
                                .input
                                .iter()
                                .chain([&f.schema().output])
                                .map(|s| s.name().to_string())
                                .collect::<Vec<_>>(),
                        ),
                    )
                })
            })
            .collect();
        let mut reads = BTreeMap::<u64, Vec<_>>::new();
        for r in &batch.reads {
            if !r.planner_summary {
                reads.entry(r.match_event_id).or_default().push(r);
            }
        }
        let mut writes = BTreeMap::<u64, Vec<_>>::new();
        for w in &batch.writes {
            if w.rebuild_of.is_none() {
                writes.entry(w.match_event_id).or_default().push(w);
            }
        }
        let mut unions = BTreeMap::<u64, Vec<_>>::new();
        for u in &batch.unions {
            if u.displaced.is_some() {
                unions.entry(u.match_event_id).or_default().push(u);
            }
        }
        let tick = Instant::now();
        for m in &batch.matches {
            let mut c = Counts {
                logical: 1,
                ..Default::default()
            };
            let mut ws = writes.get(&m.event_id).cloned().unwrap_or_default();
            let us = unions.get(&m.event_id).cloned().unwrap_or_default();
            let effective = ws
                .iter()
                .any(|w| matches!(w.outcome, WriteOutcome::Inserted | WriteOutcome::Updated))
                || !us.is_empty();
            if effective {
                c.effective_insert_update_union = 1;
            }
            {
                if m.physical_witness_complete {
                    c.complete_witness = 1;
                    let mut rs = reads.get(&m.event_id).cloned().unwrap_or_default();
                    let path = |span: &Option<std::sync::Arc<str>>| -> String {
                        span.as_ref()
                            .and_then(|s| {
                                paths.get(m.rule.as_ref()).and_then(|p| p.get(s.as_ref()))
                            })
                            .cloned()
                            .unwrap_or_else(|| "unmapped".into())
                    };
                    rs.sort_by_key(|r| (path(&r.source_span), r.table_name.clone()));
                    ws.sort_by_key(|w| (path(&w.source_span), w.table, w.event_id));
                    let mut hits = BTreeMap::<usize, usize>::new();
                    for r in &rs {
                        c.read_atoms += 1;
                        if let Some((name, sorts)) = schemas.get(&r.table) {
                            let key = RowKey {
                                op: name.clone(),
                                values: r.row.iter().take(sorts.len()).copied().collect(),
                            };
                            if let Some(instances) = coverage.rows.get(&key) {
                                c.covered_read_atoms += 1;
                                for &id in instances {
                                    *hits.entry(id).or_default() += 1;
                                }
                            }
                        }
                    }
                    let observations: Option<Vec<_>> = rs
                        .iter()
                        .map(|r| {
                            let (op, sorts) = schemas.get(&r.table)?;
                            if r.row.len() < sorts.len() {
                                return None;
                            }
                            let key = RowKey {
                                op: op.clone(),
                                values: r.row.iter().take(sorts.len()).copied().collect(),
                            };
                            Some(cell_query_anatomy::Read {
                                op: op.clone(),
                                cols: sorts
                                    .iter()
                                    .zip(&r.row)
                                    .map(|(s, v)| (s.clone(), format!("{v:?}"), literal(eg, s, *v)))
                                    .collect(),
                                cells: coverage
                                    .rows
                                    .get(&key)
                                    .into_iter()
                                    .flatten()
                                    .copied()
                                    .collect(),
                            })
                        })
                        .collect();
                    if let Some(observations) = observations {
                        self.anatomy.record(
                            m.rule.as_ref(),
                            &observations,
                            effective,
                            !us.is_empty(),
                        );
                    } else {
                        self.anatomy.unsupported_matches += 1;
                    }
                    let maximum = hits.values().copied().max().unwrap_or(0);
                    c.any_cell = usize::from(maximum > 0);
                    c.all_reads_one_cell = usize::from(!rs.is_empty() && maximum == rs.len());
                    c.multiple_internal_atoms = usize::from(maximum >= 2);
                    c.one_marker_atom_contraction = maximum.saturating_sub(1);
                    let mut known = false;
                    for &id in hits.keys().filter(|_| effective) {
                        let inst = &coverage.instances[id];
                        let p = &self.library.patterns[inst.pattern];
                        let mut ext = BTreeMap::new();
                        let mut typed = BTreeMap::<Value, BTreeSet<String>>::new();
                        let mut input = vec![];
                        let mut output = vec![];
                        let mut valid = true;
                        for r in &rs {
                            let Some((op, sorts)) = schemas.get(&r.table) else {
                                valid = false;
                                break;
                            };
                            let pos = path(&r.source_span);
                            if pos == "unmapped" {
                                valid = false;
                                break;
                            }
                            let cols: Vec<_> = sorts
                                .iter()
                                .zip(&r.row)
                                .map(|(s, v)| {
                                    if eg.get_sort_by_name(s).is_some_and(|s| s.is_eq_sort()) {
                                        typed.entry(*v).or_default().insert(s.clone());
                                    }
                                    role(eg, s, *v, p, inst, &mut ext)
                                })
                                .collect();
                            input.push(json!([pos, op, cols]));
                        }
                        for w in &ws {
                            if w.outcome == WriteOutcome::Unsupported {
                                continue;
                            }
                            let Some((op, sorts)) = schemas.get(&w.table) else {
                                valid = false;
                                break;
                            };
                            let mut encode = |row: &[Value]| -> Json {
                                json!(
                                    sorts
                                        .iter()
                                        .zip(row)
                                        .map(|(s, v)| {
                                            if eg
                                                .get_sort_by_name(s)
                                                .is_some_and(|s| s.is_eq_sort())
                                            {
                                                typed.entry(*v).or_default().insert(s.clone());
                                            }
                                            role(eg, s, *v, p, inst, &mut ext)
                                        })
                                        .collect::<Vec<_>>()
                                )
                            };
                            let before = match &w.before {
                                None => json!("unknown"),
                                Some(None) => json!("absent"),
                                Some(Some(r)) => encode(r),
                            };
                            output.push(json!([
                                path(&w.source_span),
                                op,
                                format!("{:?}", w.outcome),
                                before,
                                encode(&w.proposed),
                                encode(&w.actual)
                            ]));
                        }
                        let mut equalities = vec![];
                        for u in &us {
                            let types: BTreeSet<_> = typed
                                .get(&u.lhs)
                                .into_iter()
                                .flatten()
                                .chain(typed.get(&u.rhs).into_iter().flatten())
                                .cloned()
                                .collect();
                            if types.len() != 1 {
                                valid = false;
                                break;
                            }
                            let ty = types.iter().next().unwrap();
                            equalities.push(json!([
                                role(eg, ty, u.lhs, p, inst, &mut ext),
                                role(eg, ty, u.rhs, p, inst, &mut ext)
                            ]));
                        }
                        if !valid {
                            c.signature_unsupported += 1;
                            continue;
                        }
                        c.signature_candidates += 1;
                        let key = serde_json::to_string(&json!([
                            p.id, m.rule, input, output, equalities
                        ]))?;
                        if self.train {
                            self.library.signatures.insert(key);
                        } else {
                            known |= self.library.signatures.contains(&key);
                        }
                    }
                    c.known_effect_signature = usize::from(known);
                }
            }
            if effective {
                add(
                    self.effective_stats.entry(m.rule.to_string()).or_default(),
                    &c,
                );
            }
            add(self.stats.entry(m.rule.to_string()).or_default(), &c);
            add(self.rulesets.entry(set.into()).or_default(), &c);
        }
        self.audit_seconds += tick.elapsed().as_secs_f64();
        self.anatomy.finish_step();
        self.steps += 1;
        if self.steps % 100 == 0 {
            eprintln!(
                "[coverage] {} native steps, {} frozen patterns",
                self.steps,
                self.library.patterns.len()
            );
        }
        Ok((result.updated, result.can_stop))
    }
    fn schedule(
        &mut self,
        eg: &mut EGraph,
        s: &egglog::ast::Schedule,
        paths: &BTreeMap<String, BTreeMap<String, String>>,
    ) -> Result<(bool, bool)> {
        use GenericSchedule::*;
        match s {
            Run(_, r) => {
                if r.until.is_some() {
                    return Err("until schedule unsupported in diagnostic walker".into());
                }
                self.step(eg, &r.ruleset, paths)
            }
            Sequence(_, ss) => {
                let (mut updated, mut stop) = (false, true);
                for s in ss {
                    let (u, c) = self.schedule(eg, s, paths)?;
                    updated |= u;
                    stop &= c;
                }
                Ok((updated, stop))
            }
            Repeat(_, n, s) => {
                let (mut updated, mut stop) = (false, true);
                for _ in 0..*n {
                    let (u, c) = self.schedule(eg, s, paths)?;
                    updated |= u;
                    stop &= c;
                    if c {
                        break;
                    }
                }
                Ok((updated, stop))
            }
            Saturate(_, s) => {
                let (mut updated, mut stop) = (false, true);
                loop {
                    let (u, c) = self.schedule(eg, s, paths)?;
                    updated |= u;
                    stop &= c;
                    if !u {
                        break;
                    }
                }
                Ok((updated, stop))
            }
        }
    }
}
fn add(a: &mut Counts, b: &Counts) {
    a.logical += b.logical;
    a.effective_insert_update_union += b.effective_insert_update_union;
    a.complete_witness += b.complete_witness;
    a.read_atoms += b.read_atoms;
    a.covered_read_atoms += b.covered_read_atoms;
    a.any_cell += b.any_cell;
    a.all_reads_one_cell += b.all_reads_one_cell;
    a.multiple_internal_atoms += b.multiple_internal_atoms;
    a.one_marker_atom_contraction += b.one_marker_atom_contraction;
    a.known_effect_signature += b.known_effect_signature;
    a.signature_candidates += b.signature_candidates;
    a.signature_unsupported += b.signature_unsupported;
}
fn totals(eg: &EGraph, c: &Cover) -> Result<Json> {
    let (mut all, mut visible, mut covered, mut expr, mut covered_expr) = (0, 0, 0, 0, 0);
    let mut classes = BTreeSet::new();
    let mut hit_classes = BTreeSet::new();
    for name in eg.get_function_names() {
        let f = eg.get_function(&name).unwrap();
        if !f.is_constructor() || f.is_let_binding() {
            continue;
        }
        let is_expr = f.schema().output.name() == "Expr";
        eg.function_for_each(&name, |r| {
            all += 1;
            if !r.subsumed {
                visible += 1;
            }
            let key = RowKey {
                op: name.to_string(),
                values: r.vals.to_vec(),
            };
            let hit = c.rows.contains_key(&key);
            covered += usize::from(hit);
            if is_expr {
                expr += 1;
                classes.insert(*r.vals.last().unwrap());
                if hit {
                    covered_expr += 1;
                    hit_classes.insert(*r.vals.last().unwrap());
                }
            }
        })?;
    }
    Ok(
        json!({"stored_constructor_rows":all,"visible_constructor_rows":visible,"covered_constructor_rows":covered,"expr_enodes":expr,"covered_expr_enodes":covered_expr,"expr_eclasses":classes.len(),"covered_expr_eclasses":hit_classes.len(),"positive_embeddings":c.instances.len(),"matched_patterns":c.instances.iter().map(|i|i.pattern).collect::<BTreeSet<_>>().len()}),
    )
}
#[path = "../experiments/eggcc_cell_probe/snapshot.rs"]
mod native_snapshot;
fn main() -> Result {
    let args: Vec<_> = std::env::args().collect();
    let mode = &args[1];
    let source = std::fs::read_to_string(&args[2])?;
    let library_path = PathBuf::from(&args[3]);
    let out = PathBuf::from(&args[4]);
    std::fs::create_dir(&out)?;
    let library = if mode == "train" && !library_path.is_file() {
        let (p, u) = patterns(&library_path)?;
        Library{version:1,scope:"Frozen empirical positive cell patterns and observed read/effect role signatures; not certified state transitions".into(),patterns:p,unsupported:u,signatures:BTreeSet::new()}
    } else {
        serde_json::from_slice(&std::fs::read(&library_path)?)?
    };
    if library.version != 1 {
        return Err("unsupported frozen library version".into());
    }
    if mode == "train" && !library.signatures.is_empty() {
        return Err("train requires a pattern-only library, not pre-learned signatures".into());
    }
    if mode != "train" && mode != "eval" {
        return Err("mode must be train or eval".into());
    }
    let frozen_count = library.signatures.len();
    let mut eg = EGraph::default();
    let program = visual_rule::surface_program(eg.parse_program(None, &source)?);
    let mut paths = BTreeMap::new();
    for c in &program {
        if let Command::Rule { rule } = c {
            paths.insert(
                rule.name.clone(),
                visual_rule::positions(rule)
                    .into_iter()
                    .map(|(s, p, _)| (s, p))
                    .collect(),
            );
        }
    }
    let mut audit = Audit {
        anatomy: Default::default(),
        library,
        train: mode == "train",
        stats: BTreeMap::new(),
        effective_stats: BTreeMap::new(),
        rulesets: BTreeMap::new(),
        steps: 0,
        audit_seconds: 0.,
        native_seconds: 0.,
        rule_seconds: BTreeMap::new(),
        profile: BTreeMap::new(),
    };
    for c in &program {
        if let Command::RunSchedule(s) = c {
            audit.schedule(&mut eg, s, &paths)?;
        } else {
            eg.run_program(vec![c.clone()])?;
        }
    }
    let tick = Instant::now();
    let c = cover(
        &audit.library.patterns,
        &snapshot(&eg, &audit.library.patterns)?,
    );
    let structure = totals(&eg, &c)?;
    audit.audit_seconds += tick.elapsed().as_secs_f64();
    let mut total = Counts::default();
    for counts in audit.stats.values() {
        add(&mut total, counts);
    }
    let mut effective_total = Counts::default();
    for counts in audit.effective_stats.values() {
        add(&mut effective_total, counts);
    }
    let mut control = EGraph::default();
    control.run_program_with_trace(program, &TraceSession::with_dependencies())?;
    let same = matches!(
        egg_layout::saturated_rule_composition::compare(
            &native_snapshot::snapshot_filtered(
                &eg,
                &BTreeSet::new(),
                "control",
                &BTreeSet::new()
            )?,
            &native_snapshot::snapshot_filtered(
                &control,
                &BTreeSet::new(),
                "control",
                &BTreeSet::new()
            )?
        )?,
        egg_layout::saturated_rule_composition::Comparison::Equivalent { .. }
    );
    if !audit.train && audit.library.signatures.len() != frozen_count {
        return Err("held-out evaluation mutated frozen library".into());
    }
    let report = json!({"mode":mode,"source":args[2],"same_final_state_as_native_control":same,"library_patterns":audit.library.patterns.len(),"unsupported_patterns":audit.library.unsupported,"signature_count_before":frozen_count,"signature_count_after":audit.library.signatures.len(),"structure":structure,"matches":total,"effective_matches":effective_total,"rulesets":audit.rulesets,"rules":audit.stats,"effective_rules":audit.effective_stats,"rule_search_and_apply_seconds":audit.rule_seconds,"native_steps":audit.steps,"audit_seconds":audit.audit_seconds,"native_traced_seconds":audit.native_seconds,"native_table_profile":audit.profile,"certified_transition_coverage":null,"certified_transition_status":"unavailable: these are observed closure snapshots; no certified eggcc state-to-state transition adapters are loaded","actual_join_savings":null,"limits":"No candidate or embedding cap. Positive containment and successful-read coverage are optimistic diagnostics, not closed-instance certificates or saved Join work. Mutation denominator includes inserted/updated writes and successful union displacement; subsume-only effects are not counted."});
    std::fs::write(
        out.join("query_anatomy.json"),
        serde_json::to_vec_pretty(&audit.anatomy.report())?,
    )?;
    std::fs::write(out.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
    if audit.train {
        std::fs::write(
            out.join("library.json"),
            serde_json::to_vec(&audit.library)?,
        )?;
    }
    println!(
        "{}",
        json!({"structure":structure,"matches":total,"effective_matches":effective_total,"control_equal":same})
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn p(id: usize) -> Pattern {
        Pattern {
            id,
            vars: vec![
                ("i64".into(), Some("1".into())),
                ("E".into(), None),
                ("E".into(), None),
            ],
            atoms: vec![
                Atom {
                    op: "A".into(),
                    cols: vec![0, 1],
                    hidden: false,
                    credit: true,
                },
                Atom {
                    op: "B".into(),
                    cols: vec![1, 2],
                    hidden: false,
                    credit: true,
                },
            ],
        }
    }
    fn engine() -> EGraph {
        let mut eg = EGraph::default();
        eg.parse_and_run_program(None, "(datatype E (A i64) (B E))")
            .unwrap();
        eg
    }
    #[test]
    fn overlap_is_deduplicated_and_literals_are_kept() {
        let mut eg = engine();
        eg.parse_and_run_program(None, "(B (A 1)) (B (A 2))")
            .unwrap();
        let ps = vec![p(0), p(1)];
        let c = cover(&ps, &snapshot(&eg, &ps).unwrap());
        assert_eq!(c.instances.len(), 2);
        assert_eq!(c.rows.len(), 2);
    }
    #[test]
    fn later_effect_cannot_cover_earlier_match() {
        let mut eg = engine();
        eg.parse_and_run_program(None, "(A 1) (rule ((A x)) ((B (A x))))")
            .unwrap();
        let ps = vec![p(0)];
        let before = cover(&ps, &snapshot(&eg, &ps).unwrap());
        assert!(before.rows.is_empty());
        eg.step_rules("").unwrap();
        assert!(before.rows.is_empty());
        assert_eq!(cover(&ps, &snapshot(&eg, &ps).unwrap()).rows.len(), 2);
    }
    #[test]
    fn hidden_rows_do_not_satisfy_visible_cell() {
        let mut eg = engine();
        eg.parse_and_run_program(None, "(B (A 1)) (subsume (B (A 1)))")
            .unwrap();
        let ps = vec![p(0)];
        assert!(
            cover(&ps, &snapshot(&eg, &ps).unwrap())
                .instances
                .is_empty()
        );
    }
}
