//! Rooted scalar Cell transfers. Queries use cached ports; native residuals and
//! all other rules share one original commit. No-op min proposals are certified
//! only after their checked LHS and RHS arithmetic succeeds.
use super::*;
use egglog::ast::{Action, Expr, Fact, Literal, Rule};
mod closure;

pub(super) struct Spec {
    pub set: String,
    pub name: String,
    pub seed: String,
    pub field: String,
    root_name: Option<String>,
    args: Vec<String>,
    rule: Rule,
    fields: BTreeSet<String>,
}
fn v(e: &Expr) -> Option<&str> {
    if let Expr::Var(_, v) = e {
        Some(v)
    } else {
        None
    }
}
pub(super) fn discover(eg: &EGraph, commands: &[Command]) -> Vec<Spec> {
    let mut fields = commands
        .iter()
        .filter_map(|c| match c {
            Command::Function {
                name,
                schema,
                merge: Some(m),
                ..
            } if schema.input.len() == 1
                && schema.output == "i64"
                && m.to_string() == "(min old new)" =>
            {
                Some(name.clone())
            }
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    // A min proposal can be omitted across union/other min writes, but not
    // across a concurrent explicit deletion of the destination function.
    for c in commands {
        if let Command::Rule { rule } = c {
            for a in &rule.head.0 {
                if let Action::Change(_, _, name, _) = a {
                    fields.remove(name);
                }
            }
        }
    }
    let mut specs = vec![];
    for c in commands {
        let Command::Rule { rule } = c else { continue };
        if joint::SETS.contains(&rule.ruleset.as_str()) {
            continue;
        }
        let [Action::Set(_, field, keys, _)] = rule.head.0.as_slice() else {
            continue;
        };
        if !fields.contains(field) || keys.len() != 1 {
            continue;
        }
        let Some(first) = rule.body.first() else {
            continue;
        };
        let (root_name, root) = match first {
            Fact::Eq(_, a, b) if v(a).is_some() => (Some(v(a).unwrap().to_string()), b),
            Fact::Fact(e) => (None, e),
            _ => continue,
        };
        let Expr::Call(_, seed, args) = root else {
            continue;
        };
        if !eg.get_function(seed).is_some_and(|f| f.is_constructor()) {
            continue;
        }
        if !(keys[0].to_string() == root.to_string()
            || v(&keys[0]) == root_name.as_deref() && root_name.is_some())
        {
            continue;
        }
        let Some(args) = args
            .iter()
            .map(|a| v(a).map(str::to_string))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        if args.iter().collect::<BTreeSet<_>>().len() != args.len() {
            continue;
        }
        let spec = Spec {
            set: rule.ruleset.clone(),
            name: rule.name.clone(),
            seed: seed.clone(),
            field: field.clone(),
            root_name,
            args,
            rule: rule.clone(),
            fields: fields.clone(),
        };
        // Syntactic eligibility is checked without installing a speculative
        // native action sink. Unsupported rules stay in the original ruleset.
        if symbolic(&spec, None).is_ok() {
            specs.push(spec)
        }
    }
    specs
}
#[derive(Clone)]
enum Number {
    Slot(usize),
    Const(i64),
    Add(Box<Number>, Box<Number>),
}
impl Number {
    fn eval(&self, eg: &EGraph, b: &[Value]) -> Option<i64> {
        match self {
            Self::Slot(i) => Some(eg.value_to_base::<i64>(b[*i])),
            Self::Const(v) => Some(*v),
            Self::Add(a, bx) => a.eval(eg, b)?.checked_add(bx.eval(eg, b)?),
        }
    }
}
fn number(
    e: &Expr,
    vars: &BTreeMap<String, Number>,
    nodes: &BTreeMap<String, usize>,
    fields: &BTreeSet<String>,
    sink: Option<&CellActionSink>,
    read: bool,
) -> Result<Number> {
    Ok(match e {
        Expr::Lit(_, Literal::Int(n)) => Number::Const(*n),
        Expr::Var(_, name) => vars.get(name).cloned().ok_or("unbound scalar value")?,
        Expr::Call(_, op, xs) if op == "+" && xs.len() == 2 => Number::Add(
            Box::new(number(&xs[0], vars, nodes, fields, sink, read)?),
            Box::new(number(&xs[1], vars, nodes, fields, sink, read)?),
        ),
        Expr::Call(_, op, xs) if read && fields.contains(op) && xs.len() == 1 => {
            let key = *nodes
                .get(v(&xs[0]).ok_or("scalar port needs an existing variable")?)
                .ok_or("foreign scalar port")?;
            let output = if let Some(sink) = sink {
                let atom = sink
                    .atoms
                    .iter()
                    .find(|a| {
                        a.function == *op && matches!(&a.columns[0],CellOperand::Slot(i) if *i==key)
                    })
                    .ok_or("missing typed scalar read")?;
                let CellOperand::Slot(i) = atom.columns[1] else {
                    return Err("scalar port has a constrained result".into());
                };
                i
            } else {
                0
            };
            Number::Slot(output)
        }
        _ => return Err("unsupported scalar expression".into()),
    })
}
fn symbolic(spec: &Spec, sink: Option<&CellActionSink>) -> Result<(usize, Vec<Number>, Number)> {
    let mut nodes = BTreeMap::new();
    let mut vars = BTreeMap::new();
    let root = if let Some(sink) = sink {
        let atom = sink
            .atoms
            .iter()
            .find(|a| a.function == spec.seed)
            .ok_or("missing scalar root")?;
        for (name, column) in spec.args.iter().zip(&atom.columns) {
            let CellOperand::Slot(i) = column else {
                return Err("literal scalar root slot".into());
            };
            nodes.insert(name.clone(), *i);
            if sink.slots()[*i].1 == "i64" {
                vars.insert(name.clone(), Number::Slot(*i));
            }
        }
        let CellOperand::Slot(root) = atom.columns.last().unwrap() else {
            return Err("missing scalar output slot".into());
        };
        *root
    } else {
        for (i, name) in spec.args.iter().enumerate() {
            nodes.insert(name.clone(), i);
            vars.insert(name.clone(), Number::Slot(i));
        }
        spec.args.len()
    };
    if let Some(name) = &spec.root_name {
        nodes.insert(name.clone(), root);
    }
    let mut lhs = vec![];
    for fact in &spec.rule.body[1..] {
        let Fact::Eq(_, a, b) = fact else {
            return Err("scalar rule has another predicate".into());
        };
        let name = v(a).ok_or("scalar binding must be a variable")?;
        if nodes.contains_key(name) || vars.contains_key(name) {
            return Err("scalar equality constrains an existing variable".into());
        }
        let value = number(b, &vars, &nodes, &spec.fields, sink, true)?;
        lhs.push(value.clone());
        vars.insert(name.into(), value);
    }
    let Action::Set(_, _, _, rhs) = &spec.rule.head.0[0] else {
        unreachable!()
    };
    Ok((
        root,
        lhs,
        number(rhs, &vars, &nodes, &spec.fields, sink, false)?,
    ))
}
pub(super) struct Plan {
    field: String,
    root: usize,
    lhs: Vec<Number>,
    rhs: Number,
}
impl Plan {
    pub(super) fn compile(spec: &Spec, sink: &CellActionSink) -> Result<Self> {
        let (root, lhs, rhs) = symbolic(spec, Some(sink))?;
        Ok(Self {
            field: spec.field.clone(),
            root,
            lhs,
            rhs,
        })
    }
}
#[derive(Default, Serialize)]
pub(super) struct Counters {
    runs: usize,
    structural_candidates: usize,
    lhs_filtered: usize,
    min_noops: usize,
    native_candidates: usize,
    batch_dominated: usize,
    other_quiet_runs: usize,
    quiet_structural_candidates: usize,
    prepare_seconds: f64,
    commit_seconds: f64,
}
#[derive(Default)]
pub(super) struct Group {
    pub plans: Vec<(usize, Plan)>,
    pub stats: Counters,
    pub others: Vec<String>,
    pub scope: closure::Scope,
    pub certify_min: bool,
}
impl Runtime {
    pub(super) fn run_scalar(&mut self, set: &str) -> Result<egglog_reports::RunReport> {
        self.sync()?;
        let mut group = self.scalar.remove(set).ok_or("missing scalar group")?;
        let result = (|| {
            let started = std::time::Instant::now();
            let count_before = group.stats.structural_candidates;
            let mut candidates: Vec<Vec<Option<Vec<Value>>>> = vec![vec![]; group.plans.len()];
            let mut best = std::collections::HashMap::<(usize, Value), (i64, usize, usize)>::new();
            for (index, (id, plan)) in group.plans.iter().enumerate() {
                let raw =
                    self.kernels[*id].matches(&self.eg, &self.rows, &mut self.stats.keyed_reads)?;
                group.stats.structural_candidates += raw.len();
                let table = self
                    .names
                    .iter()
                    .position(|n| *n == plan.field)
                    .ok_or("missing scalar destination table")?;
                for b in raw {
                    if plan.lhs.iter().any(|e| e.eval(&self.eg, &b).is_none()) {
                        group.stats.lhs_filtered += 1;
                        continue;
                    }
                    let cost = plan.rhs.eval(&self.eg, &b);
                    let no_op = group.certify_min
                        && cost.is_some_and(|c| {
                            self.rows
                                .keys
                                .get(&(table, vec![b[plan.root]]))
                                .is_some_and(|row| {
                                    self.eg.value_to_base::<i64>(*row.values.last().unwrap()) <= c
                                })
                        });
                    if no_op {
                        group.stats.min_noops += 1;
                    } else {
                        if let Some(c) = cost.filter(|_| group.certify_min) {
                            let key = (table, b[plan.root]);
                            if let Some((old, old_i, old_j)) = best.get(&key).copied() {
                                group.stats.batch_dominated += 1;
                                if old <= c {
                                    continue;
                                }
                                candidates[old_i][old_j] = None;
                            }
                            best.insert(key, (c, index, candidates[index].len()));
                        }
                        candidates[index].push(Some(b));
                    }
                }
            }
            let candidates = candidates
                .into_iter()
                .map(|v| v.into_iter().flatten().collect::<Vec<_>>())
                .collect::<Vec<_>>();
            group.stats.native_candidates += candidates.iter().map(Vec::len).sum::<usize>();
            let batches = group
                .plans
                .iter()
                .zip(&candidates)
                .map(|((id, _), b)| (&self.kernels[*id].sink, b.as_slice()))
                .collect::<Vec<_>>();
            group.stats.prepare_seconds += started.elapsed().as_secs_f64();
            let started = std::time::Instant::now();
            let mut report = self.eg.step_cell_mixed_candidates(set, &batches)?;
            group.stats.commit_seconds += started.elapsed().as_secs_f64();
            let quiet = group.others.iter().all(|n| {
                report
                    .num_matches_per_rule
                    .get(n.as_str())
                    .copied()
                    .unwrap_or(0)
                    == 0
            });
            if quiet {
                group.stats.other_quiet_runs += 1;
                group.stats.quiet_structural_candidates +=
                    group.stats.structural_candidates - count_before;
            }
            self.sync()?;
            if quiet {
                if report.updated {
                    report.union(closure::finish(self, &mut group)?);
                }
            } else {
                group.scope.model = None;
            }
            group.stats.runs += 1;
            Ok(report)
        })();
        self.scalar.insert(set.into(), group);
        result
    }
}
