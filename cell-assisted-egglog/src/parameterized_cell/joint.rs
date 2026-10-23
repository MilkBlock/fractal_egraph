//! A guarded joint-closure transfer, not a cache of cost-specific snapshots.
//! The six helpers have an explicit checked contract. Producer constructors,
//! operand wiring, Term expressions and cost expressions come from source AST.
use super::*;
use egglog::ast::{Action, Expr, Fact, Literal, Rule};

pub const SETS: &[&str] = &["terms", "terms-helpers", "terms-helpers-helpers"];
pub const SCHEDULE: &str = "(run-schedule (saturate (seq terms (saturate (seq terms-helpers (saturate terms-helpers-helpers))))))";

struct Producer {
    name: String,
    constructor: String,
    args: Vec<String>,
    ports: Vec<(String, String)>, // Expr variable, cost variable
    cost: Expr,
    term_calls: BTreeSet<String>,
    original: CellActionSink,
    macros: [CellActionSink; 2],
    code: String,
}
#[derive(Default, Serialize)]
pub struct Counters {
    pub boundaries: usize,
    pub skipped_joint_saturations: usize,
    pub quiescent_skips: usize,
    pub macro_instances: usize,
    pub macro_waves: usize,
    pub fallback_boundaries: usize,
    pub dataflow_boundaries: usize,
    pub dataflow_ruleset_runs: usize,
    pub port_reusing_boundaries: usize,
    pub uniform_cost_boundaries: usize,
    pub reasons: BTreeMap<String, usize>,
}
pub(super) struct Joint {
    proofs_enabled: bool,
    producers: Vec<Producer>,
    improve_rule: String,
    cost_universe_closed: bool,
    noop_unions: BTreeMap<String, usize>,
    body: String,
    stable: BTreeSet<Value>,
    extendable: bool,
    closed_epoch: Option<u64>,
    sentinel: i64,
    counters: Counters,
    comparison: Option<comparison::Scope>,
}
fn call<'a>(e: &'a Expr, name: &str) -> Result<&'a [Expr]> {
    if let Expr::Call(_, op, args) = e {
        if op == name {
            return Ok(args);
        }
    }
    Err(format!("joint contract expected {name}").into())
}
fn var(e: &Expr) -> Result<String> {
    if let Expr::Var(_, v) = e {
        Ok(v.clone())
    } else {
        Err("joint contract requires a named port".into())
    }
}
fn signature(r: &Rule) -> (String, String, String) {
    (
        r.ruleset.clone(),
        r.body
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" "),
        r.head
            .0
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" "),
    )
}
fn schedule_key(s: &Schedule) -> String {
    match s {
        GenericSchedule::Sequence(_, xs) => {
            xs.iter().map(schedule_key).collect::<Vec<_>>().join(";")
        }
        GenericSchedule::Saturate(_, x) => format!("sat[{}]", schedule_key(x)),
        GenericSchedule::Repeat(_, n, x) => format!("repeat{n}[{}]", schedule_key(x)),
        GenericSchedule::Run(_, r) if r.until.is_none() => format!("run:{}", r.ruleset),
        _ => s.to_string(),
    }
}
fn cost_expr(e: &Expr, ports: &BTreeSet<String>) -> bool {
    match e {
        Expr::Var(_, v) => ports.contains(v),
        Expr::Lit(_, Literal::Int(_)) => true,
        Expr::Call(_, op, xs) => {
            op == "+" && xs.len() == 2 && xs.iter().all(|x| cost_expr(x, ports))
        }
        _ => false,
    }
}
fn cost(e: &Expr, values: &BTreeMap<String, i64>) -> Option<i64> {
    match e {
        Expr::Var(_, v) => values.get(v).copied(),
        Expr::Lit(_, Literal::Int(v)) => Some(*v),
        Expr::Call(_, op, xs) if op == "+" && xs.len() == 2 => {
            cost(&xs[0], values)?.checked_add(cost(&xs[1], values)?)
        }
        _ => None,
    }
}
fn subst(e: &Expr, vars: &BTreeMap<String, String>) -> Result<String> {
    Ok(match e {
        Expr::Var(_, v) => vars
            .get(v)
            .cloned()
            .ok_or_else(|| format!("unbound producer RHS variable {v}; slots {vars:?}"))?,
        Expr::Lit(_, l) => l.to_string(),
        Expr::Call(_, op, xs) => format!(
            "({op} {})",
            xs.iter()
                .map(|x| subst(x, vars))
                .collect::<Result<Vec<_>>>()?
                .join(" ")
        ),
    })
}
impl Joint {
    pub(super) fn compile(eg: &mut EGraph, commands: &[Command]) -> Result<Self> {
        for name in ["TCPair", "Smaller", "PotentialExtractedExpr", "NoTerm"] {
            if !eg.get_function(name).is_some_and(|f| f.is_constructor()) {
                return Err(format!("joint helper symbol {name} must be a constructor").into());
            }
        }
        let tc_schema = eg.get_function("TCPair").unwrap().schema();
        let expr_schema = eg
            .get_function("ExtractedExpr")
            .ok_or("missing ExtractedExpr")?
            .schema();
        if tc_schema.input.len() != 2
            || expr_schema.input.len() != 1
            || tc_schema.input[1].name() != "i64"
            || [
                tc_schema.input[0].name(),
                tc_schema.output.name(),
                expr_schema.input[0].name(),
            ]
            .into_iter()
            .collect::<BTreeSet<_>>()
            .len()
                != 3
        {
            return Err("joint contract requires distinct Expr, Term and TermAndCost sorts".into());
        }
        let cost_sort = tc_schema.output.name().to_string();
        let comparison_safe = comparison::Scope::safe_schema(eg, &cost_sort);
        let expected = eg.parse_program(None, include_str!("joint_helpers.egg"))?;
        let want = expected
            .iter()
            .filter_map(|c| {
                if let Command::Rule { rule } = c {
                    Some(signature(rule))
                } else {
                    None
                }
            })
            .collect::<BTreeSet<_>>();
        let have = commands
            .iter()
            .filter_map(|c| {
                if let Command::Rule { rule } = c {
                    if SETS[1..].contains(&rule.ruleset.as_str()) {
                        Some(signature(rule))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect::<BTreeSet<_>>();
        if want != have
            || SETS[1..]
                .iter()
                .map(|s| eg.cell_rule_names(s).map(|r| r.len()))
                .collect::<std::result::Result<Vec<_>, _>>()?
                .iter()
                .sum::<usize>()
                != want.len()
        {
            return Err("joint helper contract does not match installed source".into());
        }
        if !commands.iter().any(|c|matches!(c,Command::Function{name,merge:Some(m),..} if name=="ExtractedExpr" && m.to_string()=="(Smaller old new)")){
            return Err("joint contract requires the original Smaller merge".into())
        }
        let mut noop_unions = BTreeMap::new();
        for command in commands {
            let Command::Rule { rule } = command else {
                continue;
            };
            if rule.ruleset != SETS[2] {
                continue;
            }
            let [Action::Union(_, root, target)] = rule.head.0.as_slice() else {
                continue;
            };
            let Some(Fact::Eq(_, lhs, Expr::Call(_, op, args))) = rule.body.first() else {
                continue;
            };
            if op == "Smaller" && root.to_string() == lhs.to_string() {
                if let Some(column) = args
                    .iter()
                    .position(|x| x.to_string() == target.to_string())
                {
                    noop_unions.insert(rule.name.clone(), column);
                }
            }
        }
        let improve_signature = expected
            .iter()
            .find_map(|c| {
                if let Command::Rule { rule } = c {
                    (rule.name == "improve").then(|| signature(rule))
                } else {
                    None
                }
            })
            .unwrap();
        let improve_rule = commands
            .iter()
            .find_map(|c| {
                if let Command::Rule { rule } = c {
                    (signature(rule) == improve_signature).then(|| rule.name.clone())
                } else {
                    None
                }
            })
            .unwrap();
        let init = expected
            .iter()
            .find_map(|c| {
                if let Command::Rule { rule } = c {
                    (rule.name == "init").then_some(rule)
                } else {
                    None
                }
            })
            .unwrap();
        let Action::Set(_, _, _, sentinel_expr) = &init.head.0[0] else {
            unreachable!()
        };
        let sentinel_args = call(sentinel_expr, "TCPair")?;
        let Expr::Lit(_, Literal::Int(sentinel)) = sentinel_args[1] else {
            unreachable!()
        };
        let cost_universe_closed = eg.get_function_names().iter().all(|name| {
            eg.get_function(name).unwrap().schema().output.name() != cost_sort
                || matches!(name.as_str(), "TCPair" | "Smaller" | "ExtractedExpr")
        });
        let mut producers = vec![];
        for c in commands {
            let Command::Rule { rule } = c else { continue };
            if rule.ruleset != "terms" {
                continue;
            }
            let Some(Fact::Eq(_, root, Expr::Call(_, ctor, args))) = rule.body.first() else {
                return Err("joint producer needs a constructor root".into());
            };
            let root = var(root)?;
            if !eg.get_function(ctor).is_some_and(|f| f.is_constructor()) {
                return Err("joint root is not a constructor".into());
            }
            let args = args.iter().map(var).collect::<Result<Vec<_>>>()?;
            if args.iter().collect::<BTreeSet<_>>().len() != args.len() {
                return Err("repeated root slots need another transfer contract".into());
            }
            let mut ports = vec![];
            let mut bound = args
                .iter()
                .cloned()
                .chain([root.clone()])
                .collect::<BTreeSet<_>>();
            for fact in &rule.body[1..] {
                let Fact::Eq(_, pair, read) = fact else {
                    return Err("unsupported producer guard".into());
                };
                let p = call(pair, "TCPair")?;
                let read = call(read, "ExtractedExpr")?;
                if p.len() != 2 || read.len() != 1 {
                    return Err("invalid producer port".into());
                }
                let expr = var(&read[0])?;
                if !args.contains(&expr) {
                    return Err("port is outside root".into());
                }
                if !bound.insert(var(&p[0])?) || !bound.insert(var(&p[1])?) {
                    return Err("shared output variables need another transfer contract".into());
                }
                ports.push((expr, var(&p[1])?));
            }
            let [Action::Expr(_, rhs)] = rule.head.0.as_slice() else {
                return Err("joint producer needs one candidate effect".into());
            };
            let candidate = call(rhs, "PotentialExtractedExpr")?;
            if candidate.len() != 2 || var(&candidate[0])? != root {
                return Err("candidate has different root".into());
            }
            let pair = call(&candidate[1], "TCPair")?;
            if pair.len() != 2 || !cost_expr(&pair[1], &ports.iter().map(|p| p.1.clone()).collect())
            {
                return Err("unsupported checked cost expression".into());
            }
            // Every RHS call except integer + must be a constructor. No hidden
            // function lookup, effect or arbitrary primitive in the macro.
            fn constructors(e: &Expr, eg: &EGraph) -> bool {
                match e {
                    Expr::Call(_, op, xs) => {
                        eg.get_function(op).is_some_and(|f| f.is_constructor())
                            && xs.iter().all(|x| constructors(x, eg))
                    }
                    _ => true,
                }
            }
            if !constructors(&pair[0], eg) {
                return Err("Term expression contains a non-constructor".into());
            }
            fn calls(e: &Expr, out: &mut BTreeSet<String>) {
                if let Expr::Call(_, op, xs) = e {
                    out.insert(op.clone());
                    for x in xs {
                        calls(x, out)
                    }
                }
            }
            let mut term_calls = BTreeSet::new();
            calls(&pair[0], &mut term_calls);
            let original = eg.prepare_cell_action("terms", &rule.name)?;
            let slots = original.slots();
            let mut vars = slots
                .iter()
                .enumerate()
                .map(|(i, (v, _))| (v.to_string(), format!("p{i}")))
                .collect::<BTreeMap<_, _>>();
            let root_atom = original
                .atoms
                .iter()
                .find(|a| a.function == *ctor)
                .ok_or("missing typed root atom")?;
            let CellOperand::Slot(root_slot) = root_atom.columns.last().unwrap() else {
                return Err("root is not a bound slot".into());
            };
            vars.insert(root.clone(), format!("p{root_slot}"));
            let rhs = subst(rhs, &vars)?;
            let root = vars.get(&root).ok_or("missing root slot")?;
            let candidate = subst(&candidate[1], &vars)?;
            let gate = format!("joint_gate_{}", producers.len());
            let set = format!("joint_macro_{}", producers.len());
            let mut code = format!(
                "(ruleset {set})\n(function {gate} ({}) i64 :merge old)\n",
                slots.iter().map(|s| s.1).collect::<Vec<_>>().join(" ")
            );
            for low in [false, true] {
                let name = if low { "low" } else { "high" };
                let actions = if low {
                    format!(
                        "(set (ExtractedExpr {root}) {candidate}) (union (Smaller {sentinel_expr} {candidate}) {candidate})"
                    )
                } else {
                    String::new()
                };
                code.push_str(&format!("(rule ((= 0 ({gate} {}))) ({rhs} (set (ExtractedExpr {root}) {sentinel_expr}) {actions}) :ruleset {set} :name \"{name}\")\n",(0..slots.len()).map(|i|format!("p{i}")).collect::<Vec<_>>().join(" ")));
            }
            eg.parse_and_run_program(None, &code)?;
            let macros = [
                eg.prepare_cell_action(&set, "high")?,
                eg.prepare_cell_action(&set, "low")?,
            ];
            producers.push(Producer {
                name: rule.name.clone(),
                constructor: ctor.clone(),
                args,
                ports,
                cost: pair[1].clone(),
                term_calls,
                original,
                macros,
                code,
            });
        }
        if producers.is_empty() || producers.len() != eg.cell_rule_names("terms")?.len() {
            return Err("joint certificate must cover every producer rule".into());
        }
        let roots = producers
            .iter()
            .map(|p| p.constructor.clone())
            .collect::<BTreeSet<_>>();
        if producers.iter().any(|p| !p.term_calls.is_disjoint(&roots)) {
            return Err("producer RHS constructs another producer root".into());
        }
        let commands = eg.parse_program(None, SCHEDULE)?;
        let Command::RunSchedule(schedule) = &commands[0] else {
            unreachable!()
        };
        fn body(s: &Schedule) -> &Schedule {
            match s {
                GenericSchedule::Saturate(_, b) => b,
                GenericSchedule::Sequence(_, xs) if xs.len() == 1 => body(&xs[0]),
                _ => panic!("invalid built-in joint schedule"),
            }
        }
        let comparison = if comparison_safe { Some(comparison::Scope::compile(eg, &cost_sort)?) } else { None };
        Ok(Self {
            proofs_enabled: std::env::var("CELL_JOINT_PROOFS").as_deref()!=Ok("0"),
            producers,
            improve_rule,
            cost_universe_closed,
            noop_unions,
            body: schedule_key(body(schedule)),
            stable: BTreeSet::new(),
            extendable: true,
            closed_epoch: None,
            sentinel,
            counters: Counters::default(),
            comparison,
        })
    }
    pub(super) fn observes(&mut self, r: &Runtime, batch: &CellCommitBatch) {
        if !self.proofs_enabled {return}
        if !batch.unions.is_empty() || !batch.clears.is_empty() {
            self.extendable = false;
        }
        for d in &batch.rows {
            let name = &r.names[d.table];
            if !d.visible
                || d.row.is_none()
                || r.rows.keys.contains_key(&(d.table, d.key.clone().into()))
                || name == "PotentialExtractedExpr"
                || name == "Smaller"
            {
                self.extendable = false;
            }
        }
    }
    pub(super) fn accepts(&self, body: &Schedule) -> bool {
        schedule_key(body) == self.body
    }
    fn table(r: &Runtime, name: &str) -> usize {
        r.names.iter().position(|s| s == name).unwrap()
    }
    fn selected_cost(&self, r: &Runtime, root: Value) -> Option<(i64, u32)> {
        let f = Self::table(r, "ExtractedExpr");
        let tc = Self::table(r, "TCPair");
        let row = r.rows.keys.get(&(f, smallvec![root]))?;
        let value = *row.values.last()?;
        let keys = r.rows.outputs.get(&(tc, value))?;
        if keys.len() != 1 {
            return None;
        }
        let key = keys.first()?;
        Some((
            r.eg.value_to_base::<i64>(key[1]),
            row.stamp.max(r.rows.keys.get(&(tc, key.clone()))?.stamp),
        ))
    }
    fn decline(&mut self, reason: &str) -> Option<bool> {
        *self.counters.reasons.entry(reason.into()).or_default() += 1;
        self.counters.fallback_boundaries += 1;
        None
    }
    pub(super) fn try_close(&mut self, r: &mut Runtime) -> Result<Option<bool>> {
        self.counters.boundaries += 1;
        if !self.proofs_enabled {return Ok(self.decline("joint proofs disabled; generic dataflow retained"));}
        if self.closed_epoch == Some(r.epoch) {
            self.counters.skipped_joint_saturations += 1;
            self.counters.quiescent_skips += 1;
            return Ok(Some(false));
        }
        // An obsolete key after rebuild is not an invisible fact. Retain the
        // exact current hidden keys, which hashcons must not silently revive.
        if r.rows.hidden.iter().any(|(t, _)| {
            matches!(
                r.names[*t].as_str(),
                "TCPair" | "PotentialExtractedExpr" | "Smaller"
            )
        }) {
            return Ok(self.decline("currently subsumed helper row"));
        }
        if !self.extendable && self.closed_epoch.is_some() {
            return Ok(self.decline("changed existing state or external helper work"));
        }
        let tc = Self::table(r, "TCPair");
        if r.rows
            .outputs
            .iter()
            .any(|((t, _), keys)| *t == tc && keys.len() != 1)
        {
            return Ok(self.decline("multiple TCPair alternatives"));
        }
        if self.closed_epoch.is_none()
            && ["PotentialExtractedExpr", "Smaller"]
                .iter()
                .any(|n| r.rows.keys.keys().any(|(t, _)| *t == Self::table(r, n)))
        {
            return Ok(self.decline("uncertified helper state"));
        }
        let mut pending = BTreeMap::new();
        let mut roots = BTreeSet::new();
        let f = Self::table(r, "ExtractedExpr");
        for (i, p) in self.producers.iter().enumerate() {
            let table = Self::table(r, &p.constructor);
            for ((_, key), row) in r.rows.table(table) {
                let root = *row.values.last().unwrap();
                if !roots.insert(root) {
                    return Ok(self.decline("multiple producers for one Expr"));
                }
                if r.rows.keys.contains_key(&(f, smallvec![root])) {
                    if !self.stable.contains(&root) {
                        return Ok(self.decline("existing result needs incremental update"));
                    }
                } else {
                    pending.insert(root, (i, key.clone()));
                }
            }
        }
        // Pure preflight: prove every numeric branch before publishing anything.
        // Missing inputs/cycles without initial costs remain blocked, just as the
        // native producer queries. No guessed seed or iteration budget.
        let mut costs = BTreeMap::new();
        let mut waves = vec![];
        loop {
            let mut wave = vec![];
            for (root, (i, key)) in &pending {
                let p = &self.producers[*i];
                let mut values = BTreeMap::new();
                let mut ready = true;
                let mut fresh = false;
                let mut stamp = r.rows.keys[&(Self::table(r, &p.constructor), key.clone())].stamp;
                for (arg, costvar) in &p.ports {
                    let child = key[p.args.iter().position(|a| a == arg).unwrap()];
                    let c = if let Some(c) = costs.get(&child) {
                        fresh = true;
                        Some(*c)
                    } else {
                        self.selected_cost(r, child).map(|(c, s)| {
                            stamp = stamp.max(s);
                            c
                        })
                    };
                    if let Some(c) = c {
                        values.insert(costvar.clone(), c);
                    } else {
                        ready = false;
                        break;
                    }
                }
                if ready && (fresh || stamp >= r.eg.cell_query_watermark(&p.original)) {
                    let Some(c) = cost(&p.cost, &values) else {
                        return Ok(self.decline("checked integer overflow"));
                    };
                    if c < 0 {
                        return Ok(self.decline("negative candidate cost"));
                    }
                    wave.push((*root, *i, key.clone(), c));
                }
            }
            if wave.is_empty() {
                break;
            }
            for (root, _, _, c) in &wave {
                pending.remove(root);
                costs.insert(*root, (*c).min(self.sentinel));
            }
            waves.push(wave);
        }
        let mut updated = false;
        for wave in waves {
            let mut owned = BTreeMap::<(usize, usize), Vec<Vec<Value>>>::new();
            for (_, i, key, c) in &wave {
                let p = &self.producers[*i];
                let kernel = r
                    .kernels
                    .iter()
                    .find(|k| {
                        k.sink.name == p.name && k.sink.atoms[k.seed].function == p.constructor
                    })
                    .ok_or("missing producer kernel")?;
                let row = &r.rows.keys[&(kernel.tables[kernel.seed], key.clone())];
                let mut b = smallvec![None; kernel.sink.slots().len()];
                if !unify(&kernel.sink.atoms[kernel.seed].columns, &row.values, &mut b) {
                    return Err("joint preflight disagrees with seed".into());
                }
                let mut bindings = vec![];
                kernel.read_program(
                    &r.eg,
                    &r.rows,
                    0,
                    b,
                    0,
                    0,
                    &mut Reads::new(),
                    &mut bindings,
                    &mut r.stats.keyed_reads,
                    &mut 0,
                )?;
                if bindings.len() != 1 {
                    return Err("joint proof lost unique producer binding".into());
                }
                let by_name = kernel
                    .sink
                    .slots()
                    .into_iter()
                    .zip(bindings.pop().unwrap())
                    .map(|((n, _), v)| (n.to_string(), v))
                    .collect::<BTreeMap<_, _>>();
                let original = p
                    .original
                    .slots()
                    .iter()
                    .map(|(n, _)| by_name[*n])
                    .collect::<Vec<_>>();
                let branch = usize::from(*c < self.sentinel);
                let sink = &p.macros[branch];
                let args = sink
                    .slots()
                    .iter()
                    .map(|(name, _)| {
                        original[name.strip_prefix('p').unwrap().parse::<usize>().unwrap()]
                    })
                    .collect::<Vec<_>>();
                owned.entry((*i, branch)).or_default().push(args);
            }
            let batches = owned
                .iter()
                .map(|((i, b), args)| (&self.producers[*i].macros[*b], args.as_slice()))
                .collect::<Vec<_>>();
            updated |= r.eg.apply_cell_binding_batches(&batches)?.updated;
            r.sync()?; // Joint is temporarily taken out of Runtime: internal effects.
            self.counters.macro_instances += wave.len();
            self.counters.macro_waves += 1;
        }
        self.complete(r)?;
        self.counters.skipped_joint_saturations += 1;
        Ok(Some(updated))
    }
    // Under the checked helper contract, a resolved TCPair class keeps its
    // unique cost: comparison only joins a Smaller result with its minimum
    // argument; equal-cost ties and term congruence cannot mix distinct costs.
    // Therefore resolved ExtractedExpr costs can only decrease in this scope.
    fn uniform_costs(&self, r: &Runtime) -> bool {
        if !self.cost_universe_closed {
            return false;
        }
        let t = Self::table(r, "TCPair");
        let smaller = Self::table(r, "Smaller");
        if r.rows
            .hidden
            .iter()
            .any(|(table, _)| *table == t || *table == smaller)
        {
            return false;
        }
        let mut costs = FxHashMap::default();
        for (_, row) in r.rows.table(t) {
            let c = r.eg.value_to_base::<i64>(row.values[1]);
            let root = *row.values.last().unwrap();
            if costs.insert(root, c).is_some_and(|old| old != c) {
                return false;
            }
        }
        // Existing unresolved Smaller aliases are obligations too. Do not
        // assume that today's uniform TCPair rows alone make future costs
        // immutable: an old union could conceal a conflicting pending minimum.
        let edges = r
            .rows
            .table(Self::table(r, "Smaller"))
            .into_iter()
            .map(|(_, row)| (row.values[0], row.values[1], row.values[2]))
            .collect::<Vec<_>>();
        let mut waiting = FxHashMap::<Value, Vec<usize>>::default();
        let mut queue = std::collections::VecDeque::new();
        let mut done = vec![false; edges.len()];
        for (i, (a, b, _)) in edges.iter().enumerate() {
            waiting.entry(*a).or_default().push(i);
            if a != b {
                waiting.entry(*b).or_default().push(i)
            }
        }
        let mut ready = (0..edges.len()).collect::<std::collections::VecDeque<_>>();
        while let Some(i) = ready.pop_front() {
            if done[i] {
                continue;
            }
            let (a, b, out) = edges[i];
            let (Some(a), Some(b)) = (costs.get(&a).copied(), costs.get(&b).copied()) else {
                continue;
            };
            let c = a.min(b);
            done[i] = true;
            if let Some(old) = costs.get(&out) {
                if *old != c {
                    return false;
                }
            } else {
                costs.insert(out, c);
                queue.push_back(out);
            }
            while let Some(v) = queue.pop_front() {
                if let Some(users) = waiting.get(&v) {
                    ready.extend(users)
                }
            }
        }
        // Unseeded cyclic components cannot acquire a TCPair using these rules.
        // New constructors have new classes (or an already checked TCPair key).
        true
    }
    pub(super) fn run_dataflow(&mut self, r: &mut Runtime, s: &Schedule) -> Result<bool> {
        // Producer/helper Runs keep their frozen waves, including multiple
        // producers, missing ports, aliases and mutable function outputs.
        // The inner comparison Saturate may use its certified fixed point.
        // No original relational query or GPU round-trip is required.
        for ids in r.sets.values() {
            for id in ids {
                let k = &mut r.kernels[*id];
                if !k.watch {
                    k.watch = true;
                    let table = k.tables[k.seed];
                    k.dirty.extend(
                        r.rows
                            .table(table)
                            .into_iter()
                            .map(|((_, key), _)| key.clone()),
                    );
                }
            }
        }
        let improve = r.sets["terms-helpers"]
            .iter()
            .copied()
            .find(|id| r.kernels[*id].sink.name == self.improve_rule)
            .unwrap();
        let uniform = self.proofs_enabled && self.uniform_costs(r);
        let mut guarded = r.sets[SETS[2]].clone();
        guarded.push(improve);
        for id in &guarded {
            r.kernels[*id].retire_guard_failures = uniform;
        }
        self.counters.uniform_cost_boundaries += usize::from(uniform);
        if uniform && std::env::var("CELL_COMPARISON_CLOSURE").as_deref() != Ok("0") {
            r.comparison = self.comparison.take();
        }
        let before = r.stats.ruleset_runs;
        let before_reuse = r.kernels.iter().map(|k| k.reused_reads).sum::<usize>();
        let run = (|| {
            let mut updated = false;
            loop {
                let changed = r.step(s)?;
                updated |= changed;
                if !changed {
                    break;
                }
            }
            Ok::<_, Box<dyn std::error::Error>>(updated)
        })();
        if r.comparison.is_some() { self.comparison = r.comparison.take(); }
        for id in guarded {
            let k = &mut r.kernels[id];
            k.retire_guard_failures = false;
            k.dirty.append(&mut k.retired);
        }
        let updated = run?;
        self.complete(r)?;
        self.counters.dataflow_boundaries += 1;
        self.counters.dataflow_ruleset_runs += r.stats.ruleset_runs - before;
        self.counters.port_reusing_boundaries +=
            usize::from(r.kernels.iter().map(|k| k.reused_reads).sum::<usize>() > before_reuse);
        Ok(updated)
    }
    pub(super) fn complete(&mut self, r: &mut Runtime) -> Result<()> {
        for set in SETS {
            r.eg.certify_cell_saturation(set)?;
        }
        if !self.proofs_enabled {return Ok(())}
        let f = Self::table(r, "ExtractedExpr");
        self.stable.clear();
        for p in &self.producers {
            let table = Self::table(r, &p.constructor);
            for (_, row) in r.rows.table(table) {
                if r.rows
                    .keys
                    .contains_key(&(f, smallvec![*row.values.last().unwrap()]))
                {
                    self.stable.insert(*row.values.last().unwrap());
                }
            }
        }
        self.extendable = true;
        self.closed_epoch = Some(r.epoch);
        Ok(())
    }
    pub(super) fn report(&self) -> Json {
        json!({"credit_contract":"same-dataflow-optional-joint-proofs/v1","proofs_enabled":self.proofs_enabled,"contract":"parameterized terms with certified comparison closure/v3","closed_cost_producer_universe":self.cost_universe_closed,"stats":self.counters,"comparison_closure":self.comparison.as_ref().map(|s|&s.counters),"producer_templates":self.producers.len(),"macro_source":self.producers.iter().map(|p|p.code.clone()).collect::<Vec<_>>(),"concrete_closure_entries":0})
    }
}

impl Runtime {
    /// Ablate only this proof group. Keep rooted query plans, subscriptions,
    /// port caching, native effects and the original schedule in both lanes.
    pub fn set_joint_proofs(&mut self, enabled: bool) -> Result<()> {
        let proof=self.joint.as_mut().ok_or("no joint proof group installed")?;
        if proof.proofs_enabled!=enabled {
            proof.proofs_enabled=enabled;
            proof.closed_epoch=None;proof.stable.clear();proof.extendable=false;
        }
        Ok(())
    }
    /// Install the checked terms transfer and original rules in the same native
    /// engine. Unsupported contracts fail compilation, never erase a rule.
    pub fn new_joint(source: &str) -> Result<Self> {
        let mut eg = EGraph::default();
        let commands = eg.parse_program(None, source)?;
        if commands
            .iter()
            .any(|c| matches!(c, Command::RunSchedule(_)))
        {
            return Err("use explicit new_joint_commands for scheduled setup".into());
        }
        Self::new_joint_commands(commands)
    }
    /// Execute explicit setup (including initialization), then compile the
    /// certificate against that engine. Main inference is not part of setup.
    pub fn new_joint_commands(commands: Vec<Command>) -> Result<Self> {
        Self::setup_joint(commands, false)
    }
    /// The full eggcc benchmark's shared stable-choice setup. Only to-hoist is
    /// changed by that setup; the three certified terms rulesets stay original.
    pub fn new_joint_stable_commands(commands: Vec<Command>) -> Result<Self> {
        Self::setup_joint(commands, true)
    }
    fn setup_joint(commands: Vec<Command>, stable: bool) -> Result<Self> {
        let mut eg = EGraph::default();
        if stable {
            crate::stable_choice::setup(&mut eg, &commands)?;
        } else {
            eg.run_program(commands.clone())?;
        }
        let joint = Joint::compile(&mut eg, &commands)?;
        // The producer's source constructor is specified by the checked
        // transfer contract, not picked by a cost heuristic among seed plans.
        let seeds = joint
            .producers
            .iter()
            .map(|p| (p.name.clone(), p.constructor.clone()))
            .collect();
        let scalar = super::scalar::discover(&eg, &commands);
        let boundaries=super::boundary::load(&commands)?;
        let mut r = Self::new_with_boundaries(eg, SETS, &seeds, scalar, None, &boundaries)?;
        for id in &r.sets[SETS[2]] {
            let k = &mut r.kernels[*id];
            if k.sink.atoms[k.seed].function == "Smaller" {
                k.noop_union_column = joint.noop_unions.get(&k.sink.name).copied();
            }
        }
        r.joint = Some(joint);
        r.configure_scalar_closure(&commands)?;
        Ok(r)
    }
    pub fn detach_joint(mut self) -> Result<(EGraph, Observer)> {
        if self.joint.is_none() {
            return Err("no joint proof installed".into());
        }
        self.forwarded = Some(vec![]);
        for ids in self.sets.values() {
            for id in ids {
                self.kernels[*id].watch = false;
                self.kernels[*id].dirty.clear();
                self.kernels[*id].paths.clear();
            }
        }
        let eg = std::mem::take(&mut self.eg);
        Ok((eg, Observer(self)))
    }
}

/// The same certificate in the shared CPU/GPU event loop. The observer never
/// owns a second live graph; it borrows the original engine at each boundary.
pub struct Observer(Runtime);
impl Observer {
    fn with<T>(&mut self, eg: &mut EGraph, f: impl FnOnce(&mut Runtime) -> Result<T>) -> Result<T> {
        std::mem::swap(&mut self.0.eg, eg);
        let result = f(&mut self.0);
        std::mem::swap(&mut self.0.eg, eg);
        result
    }
}
impl crate::table_live::SaturationHook for Observer {
    fn binding_connections(&self)->crate::cell_connection::Library {
        let mut out=crate::cell_connection::Library::default();
        for k in &self.0.kernels {out.add_program(&k.sink);}
        out
    }
    fn query_cells(&mut self,eg:&mut EGraph,set:&str,sinks:&[&CellActionSink])->Result<Vec<(usize,Vec<Vec<Value>>) >> {
        if !self.0.scalar.contains_key(set) && !self.0.boundary.contains_key(set){return Ok(vec![])}
        self.with(eg,|r|{
            if r.eg.cell_rule_environment_stamp()!=r.identity{return Ok(vec![])}
            let mut out=if r.scalar.contains_key(set){r.query_scalar(set,sinks)?}else{vec![]};
            if r.boundary.contains_key(set){out.extend(r.query_boundary(set,sinks)?)}
            Ok(out)
        })
    }

    fn try_run(&mut self, eg: &mut EGraph, set: &str) -> Result<Option<egglog_reports::RunReport>> {
        if !self.0.scalar.contains_key(set) {
            return Ok(None);
        }
        self.with(eg, |r| {
            if r.eg.cell_rule_environment_stamp() != r.identity {
                return Ok(None);
            }
            r.run_scalar(set).map(Some)
        })
    }
    fn handles(&self, s: &Schedule) -> bool {
        self.0.joint.as_ref().is_some_and(|j| j.accepts(s)) || self.0.has_scalar_scope(s)
    }
    fn diagnostics(&self) -> Json {
        self.0.report()
    }
    fn try_boundary(&mut self, eg: &mut EGraph, s: &Schedule) -> Result<Option<Json>> {
        self.with(eg,|r|{
            if r.eg.cell_rule_environment_stamp()!=r.identity{return Ok(None)}
            r.sync()?;
            if r.enter_scalar_scope(s){return Ok(None)}
            if !r.joint.as_ref().is_some_and(|j|j.accepts(s)){return Ok(None)}
            let mut j=r.joint.take().unwrap();
            let result=(||{
                if let Some(updated)=j.try_close(r)? {
                    return Ok(json!({"complete":true,"updated":updated,"rulesets":SETS,"proof":"fresh parameterized terms joint closure"}));
                }
                let updated=j.run_dataflow(r,s)?;
                Ok(json!({"complete":true,"updated":updated,"rulesets":SETS,"proof":"parameterized producer/helper waves; certified comparison closure when available"}))
            })();
            r.joint=Some(j);result.map(Some)
        })
    }
    fn after_boundary(&mut self, eg: &mut EGraph, s: &Schedule) -> Result<()> {
        self.with(eg, |r| {
            if r.eg.cell_rule_environment_stamp() != r.identity {
                return Ok(());
            }
            r.sync()?;
            r.leave_scalar_scope(s);
            if r.joint.as_ref().is_some_and(|j| j.accepts(s)) {
                let mut j = r.joint.take().unwrap();
                let result = j.complete(r);
                r.joint = Some(j);
                result?;
            }
            Ok(())
        })
    }
    fn take_commits(&mut self, eg: &mut EGraph) -> Result<(Vec<String>, Vec<CellCommitBatch>)> {
        self.with(eg, |r| {
            r.sync()?;
            Ok((
                r.names.clone(),
                std::mem::take(r.forwarded.as_mut().unwrap()),
            ))
        })
    }
}
