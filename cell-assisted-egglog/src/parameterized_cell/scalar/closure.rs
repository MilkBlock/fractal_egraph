//! A guarded synchronous min-plus dataflow. Current row stamps provide activation
//! tokens; inactive constraints are not replayed after an external deletion.
use super::*;
use egglog::ast::{GenericSchedule, Schedule};

#[derive(Default, Serialize)]
pub(in crate::parameterized_cell) struct Stats {
    pub builds: usize,
    pub closed: usize,
    pub deferred_hidden: usize,
    pub deferred_range: usize,
    pub updates: usize,
    pub waves: usize,
    pub evaluated: usize,
    pub lhs_filtered: usize,
    pub seconds: f64,
}
#[derive(Default)]
pub(in crate::parameterized_cell) struct Scope {
    pub allowed: bool,
    pub helpers: BTreeSet<String>,
    pub active: bool,
    pub model: Option<Model>,
    pub emitters: BTreeMap<usize, CellActionSink>,
    pub stats: Stats,
}
fn calls(e: &Expr, out: &mut BTreeSet<String>) {
    if let Expr::Call(_, f, args) = e {
        out.insert(f.clone());
        for a in args {
            calls(a, out)
        }
    }
}
fn rule_calls(rule: &Rule) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for f in &rule.body {
        match f {
            Fact::Eq(_, a, b) => {
                calls(a, &mut out);
                calls(b, &mut out)
            }
            Fact::Fact(e) => calls(e, &mut out),
        }
    }
    for a in &rule.head.0 {
        match a {
            Action::Set(_, f, args, e) => {
                out.insert(f.clone());
                for a in args {
                    calls(a, &mut out)
                }
                calls(e, &mut out)
            }
            Action::Change(_, _, f, args) => {
                out.insert(f.clone());
                for a in args {
                    calls(a, &mut out)
                }
            }
            Action::Union(_, a, b) => {
                calls(a, &mut out);
                calls(b, &mut out)
            }
            Action::Expr(_, e) | Action::Let(_, _, e) => calls(e, &mut out),
            Action::Panic(..) => (),
        }
    }
    out
}
pub(in crate::parameterized_cell) fn configure(
    r: &mut Runtime,
    commands: &[Command],
) -> Result<()> {
    let source = commands
        .iter()
        .filter_map(|c| {
            if let Command::Rule { rule } = c {
                Some(((rule.ruleset.clone(), rule.name.clone()), rule_calls(rule)))
            } else {
                None
            }
        })
        .collect::<BTreeMap<_, _>>();
    let mut serial = 0;
    let merges = r.eg.cell_merge_read_dependencies();
    for (set, group) in &mut r.scalar {
        let fields = group
            .plans
            .iter()
            .map(|(_, p)| p.field.clone())
            .collect::<BTreeSet<_>>();
        let mut affected = fields.clone();
        loop {
            let before = affected.len();
            for (f, reads) in &merges {
                if reads.iter().any(|n| affected.contains(n)) {
                    affected.insert(f.clone());
                }
            }
            if affected.len() == before {
                break;
            }
        }
        let all_known =
            r.eg.cell_rule_names(set)?
                .iter()
                .all(|n| source.contains_key(&(set.clone(), n.clone())));
        group.certify_min = all_known;
        group.scope.allowed = all_known
            && r.eg.cell_ruleset_is_seminaive(set)
            && group
                .others
                .iter()
                .all(|n| source[&(set.clone(), n.clone())].is_disjoint(&affected));
        if !group.scope.allowed {
            continue;
        }
        for (name, _) in r.eg.cell_rule_environment_identity() {
            if name == *set {
                continue;
            }
            if !r.eg.cell_ruleset_is_seminaive(&name) {
                continue;
            }
            if let Ok(rules) = r.eg.cell_rule_names(&name) {
                if rules.iter().all(|n| {
                    source
                        .get(&(name.clone(), n.clone()))
                        .is_some_and(|c| c.is_disjoint(&affected))
                }) {
                    group.scope.helpers.insert(name);
                }
            }
        }
        for field in fields {
            let table = r
                .names
                .iter()
                .position(|n| *n == field)
                .ok_or("unmirrored scalar closure output")?;
            let sort = r.eg.get_function(&field).unwrap().schema().input[0]
                .name()
                .to_string();
            let set = format!("scalar_close_{serial}");
            let gate = format!("scalar_close_gate_{serial}");
            serial += 1;
            r.eg.parse_and_run_program(None,&format!("(ruleset {set}) (function {gate} ({sort} i64) i64 :merge old) (rule ((= 0 ({gate} root result))) ((set ({field} root) result)) :ruleset {set} :name \"publish\")"))?;
            group
                .scope
                .emitters
                .insert(table, r.eg.prepare_cell_action(&set, "publish")?);
        }
    }
    r.identity = r.eg.cell_rule_environment_stamp();
    Ok(())
}
fn flatten<'a>(s: &'a Schedule, out: &mut Vec<&'a Schedule>) {
    match s {
        GenericSchedule::Sequence(_, xs) => {
            for x in xs {
                flatten(x, out)
            }
        }
        _ => out.push(s),
    }
}
fn independent(s: &Schedule, sets: &BTreeSet<String>) -> bool {
    match s {
        GenericSchedule::Run(_, r) => r.until.is_none() && sets.contains(&r.ruleset),
        GenericSchedule::Sequence(_, xs) => xs.iter().all(|s| independent(s, sets)),
        GenericSchedule::Saturate(_, s) => independent(s, sets),
        _ => false,
    }
}
impl Scope {
    pub fn accepts(&self, set: &str, body: &Schedule) -> bool {
        if !self.allowed {
            return false;
        }
        let mut xs = vec![];
        flatten(body, &mut xs);
        let Some(GenericSchedule::Run(_, r)) = xs.last().copied() else {
            return false;
        };
        r.ruleset == set
            && r.until.is_none()
            && xs[..xs.len() - 1]
                .iter()
                .all(|s| matches!(s,GenericSchedule::Saturate(_,x) if independent(x,&self.helpers)))
    }
}

type Coord = (usize, Value);
#[derive(Clone)]
struct Affine {
    constant: i64,
    terms: BTreeMap<usize, i64>,
}
impl Affine {
    fn compile(
        n: &Number,
        ports: &BTreeMap<usize, usize>,
        b: &Binding,
        eg: &EGraph,
    ) -> Option<Self> {
        match n {
            Number::Const(c) if *c >= 0 => Some(Self {
                constant: *c,
                terms: BTreeMap::new(),
            }),
            Number::Slot(s) => {
                if let Some(node) = ports.get(s) {
                    Some(Self {
                        constant: 0,
                        terms: [(*node, 1)].into_iter().collect(),
                    })
                } else {
                    let c = eg.value_to_base::<i64>(b[*s]?);
                    (c >= 0).then_some(Self {
                        constant: c,
                        terms: BTreeMap::new(),
                    })
                }
            }
            Number::Add(a, bx) => {
                let mut a = Self::compile(a, ports, b, eg)?;
                let b2 = Self::compile(bx, ports, b, eg)?;
                a.constant = a.constant.checked_add(b2.constant)?;
                for (i, c) in b2.terms {
                    let v = a.terms.entry(i).or_default();
                    *v = v.checked_add(c)?;
                }
                Some(a)
            }
            _ => None,
        }
    }
    fn eval(&self, values: &[Option<i64>]) -> Option<i64> {
        self.terms.iter().try_fold(self.constant, |sum, (i, c)| {
            sum.checked_add(values[*i]?.checked_mul(*c)?)
        })
    }
}
struct Edge {
    output: usize,
    ports: Vec<usize>,
    lhs: Vec<Option<Affine>>,
    rhs: Option<Affine>,
    kernel: usize,
    stamp: u32,
}
pub(in crate::parameterized_cell) struct Model {
    nodes: Vec<Coord>,
    edges: Vec<Edge>,
    users: Vec<Vec<usize>>,
}
impl Model {
    fn values(&self, r: &Runtime) -> Vec<Option<i64>> {
        self.nodes
            .iter()
            .map(|(t, v)| {
                r.rows
                    .keys
                    .get(&(*t, vec![*v]))
                    .map(|row| r.eg.value_to_base::<i64>(*row.values.last().unwrap()))
            })
            .collect()
    }
    pub(in crate::parameterized_cell) fn build(
        r: &Runtime,
        plans: &[(usize, Plan)],
    ) -> Result<Self> {
        let mut nodes = vec![];
        let mut ids = BTreeMap::<Coord, usize>::new();
        let mut edges = vec![];
        fn node(ids: &mut BTreeMap<Coord, usize>, nodes: &mut Vec<Coord>, key: Coord) -> usize {
            *ids.entry(key).or_insert_with(|| {
                let n = nodes.len();
                nodes.push(key);
                n
            })
        }
        for (kernel, plan) in plans {
            let k = &r.kernels[*kernel];
            let table = k.tables[k.seed];
            let dest = r.names.iter().position(|n| *n == plan.field).unwrap();
            for (_, row) in r.rows.keys.range((table, vec![])..(table + 1, vec![])) {
                let mut b = vec![None; k.slot_count];
                if !unify(&k.sink.atoms[k.seed].columns, &row.values, &mut b) {
                    continue;
                }
                let out = node(
                    &mut ids,
                    &mut nodes,
                    (dest, b[plan.root].ok_or("missing scalar output root")?),
                );
                let mut slots = BTreeMap::new();
                for (i, atom) in k.sink.atoms.iter().enumerate() {
                    if i == k.seed {
                        continue;
                    }
                    if atom.columns.len() != 2 {
                        return Err("scalar closure port is not unary".into());
                    }
                    let key = value(&atom.columns[0], &b);
                    let port = node(&mut ids, &mut nodes, (k.tables[i], key));
                    let CellOperand::Slot(s) = atom.columns[1] else {
                        return Err("constrained scalar output port".into());
                    };
                    if slots.insert(s, port).is_some_and(|old| old != port) {
                        return Err("scalar closure has equality-constrained outputs".into());
                    }
                }
                let ports = slots
                    .values()
                    .copied()
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>();
                let rhs = Affine::compile(&plan.rhs, &slots, &b, &r.eg);
                let lhs = plan
                    .lhs
                    .iter()
                    .map(|e| Affine::compile(e, &slots, &b, &r.eg))
                    .collect();
                edges.push(Edge {
                    output: out,
                    ports,
                    lhs,
                    rhs,
                    kernel: *kernel,
                    stamp: row.stamp,
                });
            }
        }
        let mut users = vec![vec![]; nodes.len()];
        for (i, e) in edges.iter().enumerate() {
            for p in &e.ports {
                users[*p].push(i)
            }
        }
        Ok(Self {
            nodes,
            edges,
            users,
        })
    }
    // Simulate the ORIGINAL synchronous min waves, without native table joins.
    // No logical observer can read intervening values in an admitted scope.
    // Publication still preserves waves: later native merge record shapes may
    // depend on the allocation/commit history. If any
    // RHS would fail, discard this dry run and retain ordinary native execution.
    pub(in crate::parameterized_cell) fn solve(
        &self,
        r: &Runtime,
        stats: &mut Stats,
    ) -> Result<Vec<Vec<(Coord, i64)>>> {
        if self
            .nodes
            .iter()
            .any(|(t, k)| r.rows.hidden.contains(&(*t, vec![*k])))
        {
            stats.deferred_hidden += 1;
            return Ok(vec![]);
        }
        let mut values = self.values(r);
        let initial = values.clone();
        if values.iter().flatten().any(|v| *v < 0)
            || self
                .edges
                .iter()
                .any(|e| e.rhs.is_none() || e.lhs.iter().any(Option::is_none))
        {
            stats.deferred_range += 1;
            return Ok(vec![]);
        }
        let mut work = Vec::new();
        for (i, e) in self.edges.iter().enumerate() {
            let mut stamp = e.stamp;
            for p in &e.ports {
                let (t, k) = self.nodes[*p];
                if let Some(row) = r.rows.keys.get(&(t, vec![k])) {
                    stamp = stamp.max(row.stamp)
                }
            }
            if stamp >= r.eg.cell_query_watermark(&r.kernels[e.kernel].sink) {
                work.push(i)
            }
        }
        let mut waves = vec![];
        let mut proposals = vec![None; values.len()];
        let mut touched = Vec::new();
        let mut marked = vec![false; self.edges.len()];
        while !work.is_empty() {
            stats.waves += 1;
            for i in work.drain(..) {
                let e = &self.edges[i];
                stats.evaluated += 1;
                if e.ports.iter().any(|p| values[*p].is_none()) {
                    continue;
                }
                if e.lhs
                    .iter()
                    .any(|f| f.as_ref().unwrap().eval(&values).is_none())
                {
                    stats.lhs_filtered += 1;
                    continue;
                }
                let Some(v) = e.rhs.as_ref().unwrap().eval(&values) else {
                    stats.deferred_range += 1;
                    return Ok(vec![]);
                };
                if values[e.output].is_none_or(|old| v < old) {
                    if let Some(old) = proposals[e.output] {
                        proposals[e.output] = Some(v.min(old))
                    } else {
                        proposals[e.output] = Some(v);
                        touched.push(e.output)
                    }
                }
            }
            // All equations in a wave read the same immutable value vector.
            // Only real min changes wake readers, exactly like row timestamps.
            let mut changes = vec![];
            for n in touched.drain(..) {
                values[n] = proposals[n].take();
                changes.push((self.nodes[n], values[n].unwrap()));
                for i in &self.users[n] {
                    if !marked[*i] {
                        marked[*i] = true;
                        work.push(*i)
                    }
                }
            }
            for i in &work {
                marked[*i] = false
            }
            if !changes.is_empty() {
                waves.push(changes)
            }
        }
        stats.updates += values
            .iter()
            .zip(initial)
            .filter(|(a, b)| **a != *b)
            .count();
        stats.closed += 1;
        Ok(waves)
    }
}

pub(in crate::parameterized_cell) fn finish(
    r: &mut Runtime,
    group: &mut Group,
) -> Result<egglog_reports::RunReport> {
    if !group.scope.active {
        return Ok(Default::default());
    }
    let start = std::time::Instant::now();
    if group.scope.model.is_none() {
        let m = Model::build(r, &group.plans)?;
        group.scope.model = Some(m);
        group.scope.stats.builds += 1;
    }
    let waves = group
        .scope
        .model
        .as_mut()
        .unwrap()
        .solve(r, &mut group.scope.stats)?;
    let mut report = egglog_reports::RunReport::default();
    for changes in waves {
        let mut batches = BTreeMap::<usize, Vec<Vec<Value>>>::new();
        for ((table, key), v) in changes {
            let sink = &group.scope.emitters[&table];
            let v = r.eg.base_to_value(v);
            let row = sink
                .slots()
                .iter()
                .map(|(name, _)| match *name {
                    "root" => key,
                    "result" => v,
                    _ => panic!("unexpected scalar emitter slot"),
                })
                .collect();
            batches.entry(table).or_default().push(row);
        }
        let args = batches
            .iter()
            .map(|(t, rows)| (&group.scope.emitters[t], rows.as_slice()))
            .collect::<Vec<_>>();
        report.union(r.eg.apply_cell_binding_batches(&args)?);
    }
    r.sync()?;
    group.scope.stats.seconds += start.elapsed().as_secs_f64();
    Ok(report)
}

impl Runtime {
    pub(in crate::parameterized_cell) fn configure_scalar_closure(
        &mut self,
        commands: &[Command],
    ) -> Result<()> {
        configure(self, commands)
    }
    pub(in crate::parameterized_cell) fn enter_scalar_scope(&mut self, body: &Schedule) -> bool {
        let mut entered = false;
        for (set, g) in &mut self.scalar {
            if g.scope.accepts(set, body) {
                g.scope.active = true;
                g.scope.model = None;
                entered = true;
            }
        }
        entered
    }
    pub(in crate::parameterized_cell) fn leave_scalar_scope(&mut self, body: &Schedule) {
        for (set, g) in &mut self.scalar {
            if g.scope.accepts(set, body) {
                g.scope.active = false;
                g.scope.model = None;
            }
        }
    }
    pub(in crate::parameterized_cell) fn has_scalar_scope(&self, body: &Schedule) -> bool {
        self.scalar
            .iter()
            .any(|(set, g)| g.scope.accepts(set, body))
    }
}
