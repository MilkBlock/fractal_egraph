//! Event-driven whole-Cell dispatch. Graft patterns are incremental queries over
//! committed rows, never original smooth-rule queries. Complete Cell receipts
//! provide source bindings. A conservative connected-component check decides
//! whether the original matcher can safely remain asleep.
use super::*;
use egglog::cell_events::{CellCommitBatch, CellCommitFeed};

type FactKey = (String, Vec<Value>);
type TypedValue = (String, Value);
fn direct_ruleset(body: &egglog::ast::Schedule) -> Option<&str> {
    use egglog::ast::GenericSchedule as S;
    match body {
        S::Run(_, c) if c.until.is_none() => Some(&c.ruleset),
        S::Sequence(_, xs) if xs.len() == 1 => direct_ruleset(&xs[0]),
        _ => None,
    }
}
// A connected positive LHS cannot draw its witnesses from different data
// components. Shared variables, ground terms and literals are actual value
// connections; merely sharing a constructor NAME is not. Wildcards are fresh.
fn connected_premises(body:&[Fact])->bool {
    fn walk(e:&Expr,out:&mut BTreeSet<String>)->bool {
        match e {
            Expr::Var(_,n)=>{if n!="_"{out.insert(format!("var:{n}"));}false},
            Expr::Lit(_,v)=>{out.insert(format!("literal:{v:?}"));true},
            Expr::Call(_,_,xs)=>{
                let mut ground=true;for x in xs {ground &= walk(x,out);}
                if ground {out.insert(format!("ground:{e}"));}ground
            }
        }
    }
    let mut components=body.iter().map(|f|{let mut vars=BTreeSet::new();match f{
        Fact::Eq(_,a,b)=>{walk(a,&mut vars);walk(b,&mut vars);},Fact::Fact(e)=>{walk(e,&mut vars);}
    }vars}).collect::<Vec<_>>();
    if components.is_empty(){return false}
    let mut joined=components.remove(0);
    loop {let before=components.len();let mut rest=vec![];
        for component in components {if joined.is_disjoint(&component){rest.push(component)}else{joined.extend(component)}}
        components=rest;if components.is_empty(){return true}if components.len()==before{return false}
    }
}
#[derive(Clone)]
enum Term {
    Var(usize),
    Literal(Value),
}
#[derive(Clone)]
struct Atom {
    op: String,
    terms: Vec<Term>,
}
struct Trigger {
    atoms: Vec<Atom>,
    equalities: Vec<(Term,Term)>,
    vars: usize,
    inputs: usize,
}
impl Trigger {
    fn compile(eg: &mut EGraph, lib: &Library, step: &Step) -> Result<Self> {
        let mut names = BTreeMap::new();
        let width = if let Some(s) = step.source {
            let body = lib.body(s);
            for (name, i) in &body.ports {
                if name == "root" {
                    names.insert("parent".to_string(), *i);
                }
                if let Some(n) = name.strip_prefix("input:") {
                    names.insert(n.to_string(), *i);
                }
            }
            body.values.len()
        } else {
            0
        };
        for (i, p) in step.parameters.iter().enumerate() {
            names.insert(p.name.clone(), width + i);
        }
        let mut out = Self {
            atoms: vec![],
            equalities: vec![],
            vars: step.input_types.len(),
            inputs: step.input_types.len(),
        };
        fn visit(
            t: &Expr,
            eg: &mut EGraph,
            names: &BTreeMap<String, usize>,
            out: &mut Trigger,
        ) -> Result<Term> {
            Ok(match t {
                Expr::Var(_, n) => Term::Var(*names.get(n).ok_or("unbound graft variable")?),
                Expr::Lit(..) => Term::Literal(eg.eval_expr(t)?.1),
                Expr::Call(_, op, args) => {
                    let mut terms = args
                        .iter()
                        .map(|a| visit(a, eg, names, out))
                        .collect::<Result<Vec<_>>>()?;
                    let result = out.vars;
                    out.vars += 1;
                    terms.push(Term::Var(result));
                    out.atoms.push(Atom {
                        op: op.clone(),
                        terms,
                    });
                    Term::Var(result)
                }
            })
        }
        let e = egglog::ast::Parser::default().get_expr_from_string(None, &step.graft)?;
        visit(&e, eg, &names, &mut out)?;
        for c in eg.parse_program(None,&step.premises.join("\n"))? {
            match c {
                Command::Action(Action::Expr(_,e))=>{visit(&e,eg,&names,&mut out)?;},
                Command::Action(Action::Union(_,a,b))=>{
                    let a=visit(&a,eg,&names,&mut out)?;let b=visit(&b,eg,&names,&mut out)?;out.equalities.push((a,b));
                },
                _=>return Err("unsupported positive seed premise".into()),
            }
        }

        if out.atoms.is_empty() {
            return Err("automatic graft requires a constructor event".into());
        }
        let used = out
            .atoms
            .iter()
            .flat_map(|a| &a.terms)
            .filter_map(|t| match t {
                Term::Var(v) => Some(*v),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        if (width..out.inputs).any(|i| !used.contains(&i))
            || (width > 0 && !(0..width).any(|i| used.contains(&i)))
        {
            return Err(
                "automatic graft must expose its external inputs and an anchor in its source Cell"
                    .into(),
            );
        }
        Ok(out)
    }
}
#[derive(Default)]
struct Rows {
    all: BTreeSet<FactKey>,
    keys: BTreeMap<(String, Vec<Value>), FactKey>,
    ops: BTreeMap<String, BTreeSet<FactKey>>,
    columns: BTreeMap<(String, usize, Value), BTreeSet<FactKey>>,
    incident: BTreeMap<TypedValue, BTreeSet<FactKey>>,
}
fn remove<K: Ord, V: Ord>(map: &mut BTreeMap<K, BTreeSet<V>>, k: &K, v: &V) {
    if let Some(s) = map.get_mut(k) {
        s.remove(v);
        if s.is_empty() {
            map.remove(k);
        }
    }
}
impl Rows {
    fn change(&mut self, key: FactKey, types: &[String], insert: bool) {
        if insert {
            if !self.all.insert(key.clone()) {
                return;
            }
            self.keys.insert(
                (key.0.clone(), key.1[..key.1.len() - 1].to_vec()),
                key.clone(),
            );
            self.ops
                .entry(key.0.clone())
                .or_default()
                .insert(key.clone());
            for (i, v) in key.1.iter().enumerate() {
                self.columns
                    .entry((key.0.clone(), i, *v))
                    .or_default()
                    .insert(key.clone());
                self.incident
                    .entry((types[i].clone(), *v))
                    .or_default()
                    .insert(key.clone());
            }
        } else {
            if !self.all.remove(&key) {
                return;
            }
            self.keys
                .remove(&(key.0.clone(), key.1[..key.1.len() - 1].to_vec()));
            remove(&mut self.ops, &key.0, &key);
            for (i, v) in key.1.iter().enumerate() {
                remove(&mut self.columns, &(key.0.clone(), i, *v), &key);
                remove(&mut self.incident, &(types[i].clone(), *v), &key);
            }
        }
    }
    fn candidates(&self, atom: &Atom, binding: &[Option<Value>]) -> Vec<FactKey> {
        let mut best = self.ops.get(&atom.op);
        for (i, t) in atom.terms.iter().enumerate() {
            let value = match t {
                Term::Var(v) => binding[*v],
                Term::Literal(v) => Some(*v),
            };
            if let Some(v) = value {
                let set = self.columns.get(&(atom.op.clone(), i, v));
                if set.is_none() {
                    return vec![];
                }
                if best.is_none_or(|b| set.unwrap().len() < b.len()) {
                    best = set;
                }
            }
        }
        best.map(|s| s.iter().cloned().collect())
            .unwrap_or_default()
    }
}
fn bind(atom: &Atom, row: &FactKey, values: &mut [Option<Value>]) -> bool {
    if atom.op != row.0 || atom.terms.len() != row.1.len() {
        return false;
    }
    for (term, v) in atom.terms.iter().zip(&row.1) {
        match term {
            Term::Literal(l) if l != v => return false,
            Term::Var(i) => match values[*i] {
                Some(old) if old != *v => return false,
                None => values[*i] = Some(*v),
                _ => (),
            },
            _ => (),
        }
    }
    true
}
fn solve(
    rows: &Rows,
    trigger: &Trigger,
    pending: &[usize],
    binding: Vec<Option<Value>>,
    out: &mut Vec<Vec<Option<Value>>>,
    visits: &mut usize,
) {
    let value=|t:&Term|match t{Term::Var(i)=>binding[*i],Term::Literal(v)=>Some(*v)};
    if trigger.equalities.iter().any(|(a,b)|matches!((value(a),value(b)),(Some(a),Some(b)) if a!=b)){return}
    if pending.is_empty() {
        out.push(binding);
        return;
    }
    let (atom, candidates) = pending
        .iter()
        .map(|i| (*i, rows.candidates(&trigger.atoms[*i], &binding)))
        .min_by_key(|(_, r)| r.len())
        .unwrap();
    let rest = pending
        .iter()
        .copied()
        .filter(|i| *i != atom)
        .collect::<Vec<_>>();
    for row in candidates {
        *visits += 1;
        let mut next = binding.clone();
        if bind(&trigger.atoms[atom], &row, &mut next) {
            solve(rows, trigger, &rest, next, out, visits);
        }
    }
}
#[derive(Clone, Serialize)]
pub struct LiveInstance {
    pub id: usize,
    pub state: usize,
    pub revision: usize,
    pub valid: bool,
    #[serde(skip)]
    frame: Vec<Value>,
}
impl LiveInstance {
    pub fn frame(&self) -> &[Value] {
        &self.frame
    }
}
#[derive(Default, Serialize)]
pub struct Counters {
    pub committed_rows: usize,
    pub union_events: usize,
    pub graft_candidate_rows: usize,
    pub macro_transitions: usize,
    pub macro_batches: usize,
    pub rejected_specializations: usize,
    pub invalidated_instances: usize,
    pub coverage_rows_checked: usize,
    pub native_fallbacks: usize,
    pub fallback_smooth_matches: usize,
    pub saturation_boundaries: usize,
    pub skipped_saturations: usize,
    pub stale_grafts: usize,
    pub graft_validation_seconds: f64,
    pub macro_apply_seconds: f64,
    pub publication_seconds: f64,
    pub coverage_seconds: f64,
}
/// Multiple overlapping whole-Cell certificates. An instance is a positive
/// subgraph guarantee, NOT a promise that foreign context cannot activate work.
pub struct AutomaticRuntime {
    core: Runtime,
    feed: CellCommitFeed,
    rows: Rows,
    schemas: BTreeMap<String, Vec<String>>,
    triggers: Vec<Trigger>,
    by_op: BTreeMap<String, Vec<(usize, usize)>>,
    by_source: BTreeMap<usize, Vec<usize>>,
    sources: BTreeMap<(usize, usize, Value), BTreeSet<usize>>,
    value_instances: BTreeMap<Value, BTreeSet<usize>>,
    identities: BTreeMap<(usize, Vec<Value>), usize>,
    owners: BTreeMap<FactKey, BTreeSet<usize>>,
    inputs: BTreeSet<TypedValue>,
    instances: Vec<LiveInstance>,
    attempted: BTreeSet<(usize, Vec<Value>)>,
    pending: BTreeSet<(usize, Vec<Value>)>,
    dirty: BTreeSet<TypedValue>,
    local_rules: bool,
    rule_identity: Vec<(String, String)>,
    scope_functions: BTreeSet<String>,
    graft_rulesets: BTreeSet<String>,
    failed: bool,
    forward_events: bool,
    forwarded: Vec<CellCommitBatch>,
    destructive_seen: bool,
    pub counters: Counters,
    pub transitions: Vec<Json>,
    pub batch_reports: Vec<Json>,
}

/// An observer paired with the SAME native engine returned by `detach`.
/// It borrows that engine during a boundary; the empty placeholder is reused,
/// so this does not clone a graph or create a fresh engine on every call.
pub struct SaturationObserver {
    run: AutomaticRuntime,
    audit: bool,
    declined: BTreeMap<String, usize>,
    rule_environment: Vec<(String,String)>,
}
impl SaturationObserver {
    pub fn diagnostics(&self) -> Json {
        json!({"certificates":self.run.core.lib.certificates.report(),"scope":"positive constructor/union; exact single-ruleset saturation","declined":self.declined,
            "destructive_seen":self.run.destructive_seen,"counters":self.run.counters})
    }
    fn decline(&mut self, reason: &str) -> Result<Option<Json>> {
        *self.declined.entry(reason.into()).or_default() += 1;
        Ok(None)
    }
    /// Expensive independent whole-state check, excluded from timing runs.
    pub fn set_audit(&mut self, audit: bool) {
        self.audit = audit;
    }
    /// Table journals are draining, not independent subscriptions. Forward every
    /// batch consumed by this observer so a second device mirror cannot miss it.
    pub fn take_commits(&mut self, eg: &mut EGraph) -> Result<(Vec<String>, Vec<CellCommitBatch>)> {
        std::mem::swap(eg, &mut self.run.core.eg);
        let result = self.run.drain();
        if result.is_err() {
            self.run.failed = true;
        }
        std::mem::swap(eg, &mut self.run.core.eg);
        result?;
        Ok((
            self.run
                .feed
                .tables
                .iter()
                .map(|t| t.name.clone())
                .collect(),
            std::mem::take(&mut self.run.forwarded),
        ))
    }
    pub fn try_boundary(
        &mut self,
        eg: &mut EGraph,
        body: &egglog::ast::Schedule,
    ) -> Result<Option<Json>> {
        if direct_ruleset(body) != Some(self.run.core.lib.ruleset.as_str()) {
            return self.decline("schedule body is not the certified single ruleset");
        }
        if self.run.failed {
            return Err("failed saturation observer cannot resume".into());
        }
        if eg.cell_rule_environment_identity()!=self.rule_environment {
            return self.decline("installed rule environment changed");
        }
        if self.run.destructive_seen {
            return self.decline("destructive visibility change needs a new proof");
        }
        if eg.get_function_names().iter().any(|name| {
            let f = eg.get_function(name).unwrap();
            !self.run.scope_functions.contains(name) && !f.is_let_binding()
        }) {
            return self.decline("foreign schema or merge semantics are not certified");
        }
        // The host runtime only accepts schedules/actions after installation;
        // unexpected additions to the certified ruleset must not use this proof.
        if eg.cell_ruleset_identity(&self.run.core.lib.ruleset)? != self.run.rule_identity {
            return self.decline("compiled ruleset identity changed");
        }
        std::mem::swap(eg, &mut self.run.core.eg);
        let result = (|| {
            self.run.drain()?;
            if self.run.destructive_seen {
                return self.decline("destructive visibility change needs a new proof");
            }
            let mut reference = self.audit.then(|| self.run.core.eg.clone());
            let (complete, updated) = self.run.complete_pending()?;
            let mut verified = false;
            if complete && let Some(reference) = &mut reference {
                let r = super::saturate(reference, &self.run.core.lib.ruleset)?;
                if r["updated"] != updated
                    || !matches!(
                        self.run.verify_against(reference)?,
                        Comparison::Equivalent { .. }
                    )
                {
                    return Err(
                        "certified saturation disagrees with independent native boundary".into(),
                    );
                }
                verified = true;
            }
            Ok(Some(
                json!({"complete":complete,"updated":updated,"boundary_verified":verified,"ruleset":self.run.core.lib.ruleset,"runtime":{"counters":self.run.counters,"instances":self.run.instances.len(),"pending_transitions":self.run.pending.len(),"native_smooth_matches":self.run.core.smooth_counts()}}),
            ))
        })();
        if result.is_err() {
            self.run.failed = true;
        }
        std::mem::swap(eg, &mut self.run.core.eg);
        result
    }
}
impl AutomaticRuntime {
    pub fn detach(mut self) -> (EGraph, SaturationObserver) {
        self.forward_events = true;
        let rule_environment=self.core.eg.cell_rule_environment_identity();
        let eg = std::mem::take(&mut self.core.eg);
        (
            eg,
            SaturationObserver {
                run: self,
                audit: false,
                declined: BTreeMap::new(),
                rule_environment,
            },
        )
    }
    pub fn new(lib: Library, inputs: &str) -> Result<Self> {
        let mut core = Runtime::new(lib, inputs)?;
        let mut triggers = vec![];
        let mut by_op = BTreeMap::<String, Vec<_>>::new();
        let mut by_source = BTreeMap::<usize, Vec<_>>::new();
        for (i, step) in core.lib.steps.iter().enumerate() {
            let t = Trigger::compile(&mut core.eg, &core.lib, step)?;
            for (a, atom) in t.atoms.iter().enumerate() {
                by_op.entry(atom.op.clone()).or_default().push((i, a));
            }
            if let Some(s) = step.source {
                by_source.entry(s).or_default().push(i);
            }
            triggers.push(t);
        }
        // Connected positive joins have the same component closure proof;
        // disconnected joins retain original execution.
        let local_rules = core
            .eg
            .parse_program(None, &core.lib.environment)?
            .iter()
            .all(|c| match c {
                Command::Rule { rule } => connected_premises(&rule.body),
                _ => true,
            });
        let schemas = core
            .lib
            .constructors
            .iter()
            .map(|n| {
                let f = core.eg.get_function(n).unwrap();
                (
                    n.clone(),
                    f.schema()
                        .input
                        .iter()
                        .chain([&f.schema().output])
                        .map(|s| s.name().to_string())
                        .collect(),
                )
            })
            .collect();
        let (feed, initial) = core.eg.subscribe_cell_commits(&core.lib.constructors)?;
        let rule_identity = core.eg.cell_ruleset_identity(&core.lib.ruleset)?;
        let scope_functions = core.eg.get_function_names().into_iter().collect();
        let mut run = Self {
            core,
            feed,
            rows: Rows::default(),
            schemas,
            triggers,
            by_op,
            by_source,
            sources: BTreeMap::new(),
            value_instances: BTreeMap::new(),
            identities: BTreeMap::new(),
            owners: BTreeMap::new(),
            inputs: BTreeSet::new(),
            instances: vec![],
            attempted: BTreeSet::new(),
            pending: BTreeSet::new(),
            dirty: BTreeSet::new(),
            local_rules,
            rule_identity,
            scope_functions,
            graft_rulesets: BTreeSet::new(),
            failed: false,
            forward_events: false,
            forwarded: vec![],
            destructive_seen: false,
            counters: Counters::default(),
            transitions: vec![],
            batch_reports: vec![],
        };
        run.ingest(initial)?;
        Ok(run)
    }
    pub fn engine(&self) -> &EGraph {
        &self.core.eg
    }
    pub fn instances(&self) -> &[LiveInstance] {
        &self.instances
    }
    pub fn library(&self) -> &Library {
        &self.core.lib
    }
    pub fn report(&self) -> Json {
        json!({"certificates":self.core.lib.certificates.report(),"counters":self.counters,"instances":self.instances,"transitions":self.transitions,"batch_reports":self.batch_reports,"pending_transitions":self.pending.len(),"native_smooth_matches":self.core.smooth_counts(),
        "original_query_watermarks":self.core.original.iter().map(|(s,w)|json!({"rule":s.name,"initial":w,"current":self.core.eg.cell_query_watermark(s)})).collect::<Vec<_>>()})
    }
    /// Commit ordinary constructor facts/union actions. No stage names, Cell IDs,
    /// or selected predecessor are accepted here. Native journals drive dispatch.
    pub fn commit(&mut self, code: &str) -> Result<Json> {
        if self.failed {
            return Err("failed automatic runtime cannot resume".into());
        }
        let ctors = self.core.lib.constructors.iter().cloned().collect();
        let commands = self.core.eg.parse_program(None, code)?;
        for c in &commands {
            let ok = match c {
                Command::Action(Action::Let(_, n, e)) => {
                    !n.starts_with("wc_") && positive(e, &ctors)
                }
                Command::Action(Action::Expr(_, e)) => positive(e, &ctors),
                Command::Action(Action::Union(_, a, b)) => {
                    positive(a, &ctors) && positive(b, &ctors)
                }
                _ => false,
            };
            if !ok {
                return Err(
                    "automatic input accepts only original constructor/union actions".into(),
                );
            }
        }
        let result = (|| {
            self.core.eg.run_program(commands)?;
            self.drain()?;
            Ok(self.report())
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    /// Optional native coarse-rule driver, separate from the saturated ruleset.
    /// Its matching cost is real and is never counted as skipped smooth work.
    pub fn install_grafts(&mut self, ruleset: &str, source: &str) -> Result<()> {
        symbol(ruleset)?;
        if ruleset == self.core.lib.ruleset || self.graft_rulesets.contains(ruleset) {
            return Err("distinct graft ruleset required".into());
        }
        let declarations = self
            .core
            .eg
            .parse_program(None, &self.core.lib.environment)?
            .into_iter()
            .filter(|c| {
                matches!(
                    c,
                    Command::Datatype { .. } | Command::Constructor { .. } | Command::Sort { .. }
                )
            })
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        environment(&format!("{declarations}\n{source}"), ruleset)?;
        let cmds = self.core.eg.parse_program(None, source)?;
        if cmds.iter().any(|c| !match c {
            Command::AddRuleset(_, n) => n == ruleset,
            Command::Rule { .. } | Command::Rewrite(..) | Command::BiRewrite(..) => true,
            _ => false,
        }) {
            return Err("graft program must only add positive rules".into());
        }
        self.core.eg.run_program(cmds)?;
        self.graft_rulesets.insert(ruleset.into());
        self.core.lib.certificates.bind_environment(&self.core.eg);
        Ok(())
    }
    pub fn step_grafts(&mut self, ruleset: &str) -> Result<Json> {
        if self.failed || !self.graft_rulesets.contains(ruleset) {
            return Err("unavailable graft ruleset".into());
        }
        let result = (|| {
            let r = self.core.eg.step_rules(ruleset)?;
            self.drain()?;
            Ok(json!({"coarse_matches":r.num_matches_per_rule,"runtime":self.report()}))
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    fn drain(&mut self) -> Result<()> {
        let b = self.core.eg.drain_cell_commits(&mut self.feed)?;
        if self.forward_events
            && (!b.rows.is_empty() || !b.clears.is_empty() || !b.unions.is_empty())
        {
            self.forwarded.push(b.clone());
        }
        self.ingest(b)
    }
    /// Observe commits now, publish derived closure effects only at this
    /// explicitly requested saturation boundary. Ordinary Run never calls it.
    pub fn saturate(&mut self) -> Result<Json> {
        if self.failed {
            return Err("failed automatic runtime cannot resume".into());
        }
        let result = (|| {
            self.drain()?;
            let (complete, mut updated) = self.complete_pending()?;
            if !complete {
                self.counters.native_fallbacks += 1;
                let r = super::saturate(&mut self.core.eg, &self.core.lib.ruleset)?;
                updated |= r["updated"] == true;
                self.counters.fallback_smooth_matches += r["smooth_matches"]
                    .as_object()
                    .unwrap()
                    .values()
                    .map(|n| n.as_u64().unwrap() as usize)
                    .sum::<usize>();
                self.drain()?;
            }
            self.dirty.clear();
            Ok(
                json!({"updated":updated,"can_stop":!updated,"complete":true,"skipped_native_saturation":complete,"runtime":self.report()}),
            )
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    /// Normal schedules preserve their native timing. Only an exact
    /// Saturate(Run(selected ruleset)) is eligible for a whole-Cell shortcut.
    pub fn run(&mut self, code: &str) -> Result<Json> {
        use egglog::ast::GenericSchedule as S;
        fn go(r: &mut AutomaticRuntime, s: &egglog::ast::Schedule) -> Result<(bool, bool)> {
            match s {
                S::Sequence(_, xs) => {
                    let (mut changed, mut stop) = (false, true);
                    for x in xs {
                        let (c, z) = go(r, x)?;
                        changed |= c;
                        stop &= z;
                    }
                    Ok((changed, stop))
                }
                S::Repeat(_, n, x) => {
                    let (mut changed, mut stop) = (false, true);
                    for _ in 0..*n {
                        let (c, z) = go(r, x)?;
                        changed |= c;
                        stop &= z;
                        if z {
                            break;
                        }
                    }
                    Ok((changed, stop))
                }
                S::Saturate(_, body)
                    if direct_ruleset(body) == Some(r.core.lib.ruleset.as_str()) =>
                {
                    let result = r.saturate()?;
                    Ok((result["updated"] == true, result["can_stop"] == true))
                }
                _ => {
                    let outputs = r
                        .core
                        .eg
                        .run_program(vec![Command::RunSchedule(s.clone())])?;
                    let (mut changed, mut stop) = (false, true);
                    for o in outputs {
                        if let egglog::CommandOutput::RunSchedule(p) = o {
                            changed |= p.updated;
                            stop &= p.can_stop;
                        }
                    }
                    r.drain()?;
                    Ok((changed, stop))
                }
            }
        }
        if self.failed {
            return Err("failed automatic runtime cannot resume".into());
        }
        let commands = self.core.eg.parse_program(None, code)?;
        if commands
            .iter()
            .any(|c| !matches!(c, Command::RunSchedule(_)))
        {
            return Err("schedule-only request required".into());
        }
        let result = (|| {
            let (mut updated, mut can_stop) = (false, true);
            for c in commands {
                if let Command::RunSchedule(s) = c {
                    let (a, b) = go(self, &s)?;
                    updated |= a;
                    can_stop &= b;
                }
            }
            Ok(json!({"updated":updated,"can_stop":can_stop,"runtime":self.report()}))
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    fn fact(&self, row: &Row, frame: &[Value]) -> FactKey {
        (
            row.op.clone(),
            row.args
                .iter()
                .chain([&row.result])
                .map(|i| frame[*i])
                .collect(),
        )
    }
    fn mark(&mut self, row: &FactKey) {
        if !self.core.lib.inert.contains(&row.0) {
            for (t, v) in self.schemas[&row.0].iter().zip(&row.1) {
                self.dirty.insert((t.clone(), *v));
            }
        }
    }
    fn ingest(&mut self, batch: CellCommitBatch) -> Result<()> {
        self.counters.committed_rows += batch.rows.len();
        self.counters.union_events += batch.unions.len();
        let union = !batch.unions.is_empty();
        let mut changed = BTreeSet::new();
        let mut affected = batch
            .unions
            .iter()
            .flat_map(|(a, b)| [a, b])
            .flat_map(|v| self.value_instances.get(v).into_iter().flatten().copied())
            .collect::<BTreeSet<_>>();
        for table in batch.clears {
            let name = self.feed.tables[table].name.clone();
            if !self.rows.ops.get(&name).is_none_or(|s| s.is_empty()) {
                self.destructive_seen = true;
            }
            for row in self.rows.ops.get(&name).cloned().unwrap_or_default() {
                affected.extend(self.owners.get(&row).into_iter().flatten().copied());
                self.mark(&row);
                self.rows.change(row, &self.schemas[&name], false);
            }
        }
        for delta in batch.rows {
            let op = self.feed.tables[delta.table].name.clone();
            // Rebuild removal has a visible canonical replacement. A genuine
            // deletion/subsumption breaks the positive-only proof contract;
            // fail closed for this observer rather than resurrecting an
            // invisible intermediate node with a multi-step macro.
            if !delta.visible && self.core.eg.cell_visible_row(&op, &delta.key).is_none() {
                self.destructive_seen = true;
            }
            // Native journal keys omit the result; rebuild can remove old keys.
            if let Some(row) = self.rows.keys.get(&(op.clone(), delta.key)).cloned() {
                affected.extend(self.owners.get(&row).into_iter().flatten().copied());
                self.mark(&row);
                self.rows.change(row, &self.schemas[&op], false);
            }
            if delta.visible {
                if let Some(vs) = delta.row {
                    let row = (op.clone(), vs);
                    if self.core.lib.inert.contains(&op) {
                        self.inputs.insert((
                            self.schemas[&op].last().unwrap().clone(),
                            *row.1.last().unwrap(),
                        ));
                    }
                    self.mark(&row);
                    self.rows.change(row.clone(), &self.schemas[&op], true);
                    changed.insert(row);
                }
            }
        }
        if union {
            self.inputs = self
                .inputs
                .iter()
                .map(|(t, v)| {
                    (
                        t.clone(),
                        self.core
                            .eg
                            .get_canonical_value(*v, self.core.eg.get_sort_by_name(t).unwrap()),
                    )
                })
                .collect();
            self.attempted = self
                .attempted
                .iter()
                .map(|(s, a)| {
                    Ok((
                        *s,
                        canonical_frame(&self.core.eg, &self.core.lib.steps[*s].input_types, a)?,
                    ))
                })
                .collect::<Result<_>>()?;
            self.pending = std::mem::take(&mut self.pending)
                .into_iter()
                .map(|(s, a)| {
                    Ok((
                        s,
                        canonical_frame(&self.core.eg, &self.core.lib.steps[s].input_types, &a)?,
                    ))
                })
                .collect::<Result<_>>()?;
        }
        self.refresh_instances(&affected)?;
        for row in changed {
            self.discover_row(&row)?;
        }
        if union {
            for i in affected {
                if self.instances[i].valid {
                    self.discover_instance(i)?;
                }
            }
        }
        Ok(())
    }
    fn refresh_instances(&mut self, affected: &BTreeSet<usize>) -> Result<()> {
        for &i in affected {
            self.unindex_instance(i);

            if !self.instances[i].valid {
                continue;
            }
            let state = self.instances[i].state;
            let body = self.core.lib.body(state);
            let frame = canonical_frame(&self.core.eg, &body.values, &self.instances[i].frame)?;
            let valid = partition(&body.values, &frame) == (0..frame.len()).collect::<Vec<_>>()
                && body
                    .rows
                    .iter()
                    .all(|r| self.rows.all.contains(&self.fact(r, &frame)));
            if !valid {
                self.instances[i].valid = false;
                self.counters.invalidated_instances += 1;
                continue;
            }
            if self.instances[i].frame != frame {
                self.instances[i].revision += 1;
                self.instances[i].frame = frame;
            }
            self.index_instance(i);
        }
        Ok(())
    }
    fn unindex_instance(&mut self, i: usize) {
        let instance = self.instances[i].clone();
        if !instance.valid {
            return;
        }
        self.identities
            .remove(&(instance.state, instance.frame.clone()));
        for (slot, v) in instance.frame.iter().enumerate() {
            remove(&mut self.sources, &(instance.state, slot, *v), &i);
            remove(&mut self.value_instances, v, &i);
        }
        for r in &self.core.lib.body(instance.state).rows {
            let fact = self.fact(r, &instance.frame);
            remove(&mut self.owners, &fact, &i);
        }
    }
    fn index_instance(&mut self, i: usize) {
        let instance = &self.instances[i];
        self.identities
            .insert((instance.state, instance.frame.clone()), i);
        for (slot, v) in instance.frame.iter().enumerate() {
            self.sources
                .entry((instance.state, slot, *v))
                .or_default()
                .insert(i);
            self.value_instances.entry(*v).or_default().insert(i);
        }
        for row in &self.core.lib.body(instance.state).rows {
            self.owners
                .entry(self.fact(row, &instance.frame))
                .or_default()
                .insert(i);
        }
    }
    fn accept_bindings(&mut self, step: usize, bindings: Vec<Vec<Option<Value>>>) -> Result<()> {
        let s = &self.core.lib.steps[step];
        let width = s.source.map_or(0, |id| self.core.lib.body(id).values.len());
        for b in bindings {
            let external = b[width..self.triggers[step].inputs]
                .iter()
                .copied()
                .collect::<Option<Vec<_>>>();
            let Some(ext) = external else { continue };
            if s.premises.is_empty() && !ext
                .iter()
                .zip(&s.parameters)
                .all(|(v, p)| self.inputs.contains(&(p.sort.clone(), *v)))
            {
                continue;
            }
            let prefixes = if let Some(state) = s.source {
                let mut candidates: Option<BTreeSet<usize>> = None;
                for (slot, v) in b[..width].iter().enumerate() {
                    if let Some(v) = v {
                        let found = self
                            .sources
                            .get(&(state, slot, *v))
                            .cloned()
                            .unwrap_or_default();
                        candidates = Some(match candidates {
                            None => found,
                            Some(c) => c.intersection(&found).copied().collect(),
                        });
                    }
                }
                candidates
                    .unwrap_or_default()
                    .into_iter()
                    .map(|i| self.instances[i].frame.clone())
                    .collect::<Vec<_>>()
            } else {
                vec![vec![]]
            };
            for mut a in prefixes {
                a.extend(&ext);
                if !self.attempted.contains(&(step, a.clone())) {
                    self.pending.insert((step, a));
                }
            }
        }
        Ok(())
    }
    fn discover_row(&mut self, row: &FactKey) -> Result<()> {
        for (step, anchor) in self.by_op.get(&row.0).cloned().unwrap_or_default() {
            let t = &self.triggers[step];
            let mut b = vec![None; t.vars];
            if !bind(&t.atoms[anchor], row, &mut b) {
                continue;
            }
            let mut answers = vec![];
            solve(
                &self.rows,
                t,
                &(0..t.atoms.len())
                    .filter(|i| *i != anchor)
                    .collect::<Vec<_>>(),
                b,
                &mut answers,
                &mut self.counters.graft_candidate_rows,
            );
            self.accept_bindings(step, answers)?;
        }
        Ok(())
    }
    fn discover_instance(&mut self, id: usize) -> Result<()> {
        let instance = self.instances[id].clone();
        for step in self
            .by_source
            .get(&instance.state)
            .cloned()
            .unwrap_or_default()
        {
            let t = &self.triggers[step];
            let mut b = vec![None; t.vars];
            for (i, v) in instance.frame.iter().enumerate() {
                b[i] = Some(*v);
            }
            let mut answers = vec![];
            solve(
                &self.rows,
                t,
                &(0..t.atoms.len()).collect::<Vec<_>>(),
                b,
                &mut answers,
                &mut self.counters.graft_candidate_rows,
            );
            self.accept_bindings(step, answers)?;
        }
        Ok(())
    }
    fn complete_pending(&mut self) -> Result<(bool, bool)> {
        self.counters.saturation_boundaries += 1;
        let mut updated = false;
        while let Some(id) = self.pending.first().map(|(id, _)| *id) {
            let prepare_start = Instant::now();
            let step = self.core.lib.steps[id].clone();
            let width = step
                .source
                .map_or(0, |s| self.core.lib.body(s).values.len());
            let mut prepared = BTreeMap::new();
            // One native effect batch for all CURRENT occurrences of this
            // transition. Positive constructor/union deltas are inflationary;
            // newly enabled successors are discovered after this commit.
            while self.pending.first().is_some_and(|(s, _)| *s == id) {
                let (_, args) = self.pending.pop_first().unwrap();
                if self.attempted.contains(&(id, args.clone())) {
                    continue;
                }
                let predecessor = step
                    .source
                    .and_then(|s| self.identities.get(&(s, args[..width].to_vec())).copied());
                if step.source.is_some() && predecessor.is_none() {
                    continue;
                }
                if partition(&step.input_types, &args) != step.input_partition {
                    self.counters.rejected_specializations += 1;
                    continue;
                }
                // Events only propose a graft. Its facts may have disappeared since
                // enqueueing; pin all input slots and recheck current visibility.
                let trigger = &self.triggers[id];
                let mut binding = vec![None; trigger.vars];
                for (i, v) in args.iter().enumerate() {
                    binding[i] = Some(*v);
                }
                let mut witnesses = vec![];
                solve(
                    &self.rows,
                    trigger,
                    &(0..trigger.atoms.len()).collect::<Vec<_>>(),
                    binding,
                    &mut witnesses,
                    &mut self.counters.graft_candidate_rows,
                );
                if witnesses.is_empty() {
                    self.counters.stale_grafts += 1;
                    continue;
                }
                self.attempted.insert((id, args.clone()));
                prepared.insert(args, predecessor);
            }
            self.counters.graft_validation_seconds += prepare_start.elapsed().as_secs_f64();
            if prepared.is_empty() {
                continue;
            }
            let before = self
                .core
                .original
                .iter()
                .map(|(s, _)| self.core.eg.cell_query_watermark(s))
                .collect::<Vec<_>>();
            if !self.core.lib.certificates.permits(step.target,crate::certificate::Strength::FiniteClosure,&self.core.eg) {
                return Err("missing or stale whole-Cell marker authority".into());
            }
            let apply_start = Instant::now();
            let (receipts, report) = self.core.emitters[id].apply_many(
                &mut self.core.eg,
                &prepared.keys().cloned().collect::<Vec<_>>(),
            )?;
            self.counters.macro_apply_seconds += apply_start.elapsed().as_secs_f64();
            let batch = self.batch_reports.len();
            self.counters.macro_batches += 1;
            self.batch_reports
                .push(json!({"transition":step.name,"native":report}));
            updated |= report["updated"] == true;
            if self
                .core
                .original
                .iter()
                .zip(before)
                .any(|((s, _), w)| self.core.eg.cell_query_watermark(s) != w)
            {
                return Err("smooth query unexpectedly ran during automatic macro".into());
            }
            let publication_start = Instant::now();
            self.drain()?;
            for (out, old) in receipts {
                let predecessor = prepared
                    .remove(&old)
                    .ok_or("native receipt changed its input binding before commit")?;
                let validated = validate_receipt(
                    &self.core.eg,
                    &step,
                    self.core.lib.body(step.target),
                    &out,
                    &old,
                );
                let frame = match validated {
                    Ok(f) => f,
                    Err(_) => {
                        self.counters.rejected_specializations += 1;
                        continue;
                    }
                };
                let target = if let Some(i) = self.identities.get(&(step.target, frame.clone())) {
                    *i
                } else {
                    let i = self.instances.len();
                    self.instances.push(LiveInstance {
                        id: i,
                        state: step.target,
                        revision: 0,
                        valid: true,
                        frame,
                    });
                    self.index_instance(i);
                    self.discover_instance(i)?;
                    i
                };
                self.counters.macro_transitions += 1;
                self.transitions.push(json!({"transition":step.name,"source_instance":predecessor,"target_instance":target,"source_state":step.source,"target_state":step.target,"native_batch":batch}));
            }
            if !prepared.is_empty() {
                return Err("missing native transition receipt".into());
            }
            self.counters.publication_seconds += publication_start.elapsed().as_secs_f64();
        }
        let coverage_start = Instant::now();
        let complete = self.covered();
        self.counters.coverage_seconds += coverage_start.elapsed().as_secs_f64();
        if complete {
            self.counters.skipped_saturations += 1;
            self.core
                .eg
                .certify_cell_saturation(&self.core.lib.ruleset)?;
            self.dirty.clear();
        }
        Ok((complete, updated))
    }
    /// All active rows in each affected connected component must belong to ONE
    /// certified complete body. Independent certificates alone are insufficient.
    fn covered(&mut self) -> bool {
        if self.dirty.is_empty() {
            return true;
        }
        if !self.local_rules {
            return false;
        }
        let mut seen = BTreeSet::new();
        for seed in &self.dirty {
            if seen.contains(seed) {
                continue;
            }
            let mut todo = vec![seed.clone()];
            let mut facts = BTreeSet::new();
            let mut common: Option<BTreeSet<usize>> = None;
            while let Some(v) = todo.pop() {
                if !seen.insert(v.clone()) {
                    continue;
                }
                for row in self.rows.incident.get(&v).into_iter().flatten() {
                    if self.core.lib.inert.contains(&row.0) || !facts.insert(row.clone()) {
                        continue;
                    }
                    self.counters.coverage_rows_checked += 1;
                    let owners = self.owners.get(row).cloned().unwrap_or_default();
                    common = Some(match common {
                        None => owners,
                        Some(c) => c.intersection(&owners).copied().collect(),
                    });
                    if common.as_ref().unwrap().is_empty() {
                        return false;
                    }
                    for (s, x) in self.schemas[&row.0].iter().zip(&row.1) {
                        todo.push((s.clone(), *x));
                    }
                }
            }
        }
        true
    }
    pub fn full_snapshot(&self) -> Result<State> {
        Ok(snapshot(
            &self.core.eg,
            &self.core.lib.constructors,
            scope(&self.core.lib.environment, &self.core.lib.ruleset),
            &BTreeMap::new(),
        )?
        .graph)
    }
    pub fn verify_against(&self, native: &EGraph) -> Result<Comparison> {
        let actual = self.full_snapshot()?;
        let expected = snapshot(
            native,
            &self.core.lib.constructors,
            actual.scope.clone(),
            &BTreeMap::new(),
        )?
        .graph;
        graph::compare(&expected, &actual)
    }
}

/// Reproducible native-driver acceptance run. Each action or coarse ruleset step
/// is followed by a saturation boundary. This is not a finite-run substitute.
pub fn run_checked(
    source: &str,
    plan: &Plan,
    inputs: &str,
    driver: &str,
    out: &std::path::Path,
) -> Result<Json> {
    use egglog::ast::GenericSchedule as Schedule;
    if out.exists() {
        return Err("automatic whole-Cell output directory already exists".into());
    }
    std::fs::create_dir_all(out)?;
    let lib = Library::bake(source, plan)?;
    let mut run = AutomaticRuntime::new(lib.clone(), inputs)?;
    let (mut native, _, _, _) = environment(source, &plan.ruleset)?;
    native.parse_and_run_program(None, inputs)?;
    let commands = native.parse_program(None, driver)?;
    let mut declarations = BTreeMap::<String, Vec<String>>::new();
    let mut operations = vec![];
    fn expand(s: egglog::ast::Schedule, out: &mut Vec<(Option<String>, String)>) -> Result<()> {
        match s {
            Schedule::Run(_, config) if config.until.is_none() => {
                out.push((
                    Some(config.ruleset.clone()),
                    format!("(run {} 1)", config.ruleset),
                ));
                Ok(())
            }
            Schedule::Repeat(_, n, s) => {
                for _ in 0..n {
                    expand((*s).clone(), out)?;
                }
                Ok(())
            }
            _ => {
                Err("automatic check driver supports fixed coarse runs, not saturate/until".into())
            }
        }
    }
    for c in commands {
        let set = match &c {
            Command::AddRuleset(_, s) => Some(s.clone()),
            Command::Rule { rule } => Some(rule.ruleset.clone()),
            Command::Rewrite(s, ..) | Command::BiRewrite(s, ..) => Some(s.clone()),
            _ => None,
        };
        if let Some(set) = set {
            if !operations.is_empty() {
                return Err("declare all coarse rules before driver actions".into());
            }
            declarations.entry(set).or_default().push(c.to_string());
        } else {
            match c {
                Command::RunSchedule(s) => expand(s, &mut operations)?,
                Command::Action(..) => operations.push((None, c.to_string())),
                _ => return Err("unsupported automatic check driver command".into()),
            }
        }
    }
    let mut original = format!("{source}\n{inputs}\n");
    for (set, cmds) in declarations {
        let code = cmds.join("\n");
        run.install_grafts(&set, &code)?;
        native.parse_and_run_program(None, &code)?;
        original += &format!("{code}\n");
    }
    let mut boundaries = vec![];
    for (i, (set, code)) in operations.iter().enumerate() {
        if let Some(set) = set {
            run.step_grafts(set)?
        } else {
            run.commit(code)?
        };
        let smooth = run.run(&format!("(run-schedule (saturate {}))", plan.ruleset))?;
        native.parse_and_run_program(None, code)?;
        let reference = saturate(&mut native, &plan.ruleset)?;
        original += &format!("{code}\n(run-schedule (saturate {}))\n", plan.ruleset);
        let comparison = run.verify_against(&native)?;
        if !matches!(comparison, Comparison::Equivalent { .. }) {
            return Err(format!("automatic boundary {i} differs: {comparison:?}").into());
        }
        let actual = run.full_snapshot()?;
        let expected = snapshot(
            &native,
            &lib.constructors,
            actual.scope.clone(),
            &BTreeMap::new(),
        )?
        .graph;
        crate::pipeline::write_json(&out.join(format!("{i}-cell.json")), &actual)?;
        crate::pipeline::write_json(&out.join(format!("{i}-native.json")), &expected)?;
        boundaries.push(json!({"boundary":i,"driver":code,"native":reference,"runtime":smooth,"comparison":comparison,"enodes":actual.rows.len(),"eclasses":actual.values.iter().filter(|v|v.literal.is_none()).count()}));
    }
    let report = json!({"schema":"automatic-whole-ripsat/v1","completed":true,"boundaries":boundaries,"final_runtime":run.report(),
        "scope":"Actual native committed-row dispatch plus whole-Cell effects. CPU selector, not a GPU benchmark. Full original facts and equivalence compared at every saturation boundary; unsupported boundaries use original saturation.",
        "no_manual_state_selection":true});
    std::fs::write(out.join("original.egg"), original)?;
    crate::pipeline::write_json(&out.join("library.json"), &lib)?;
    crate::pipeline::write_json(&out.join("report.json"), &report)?;
    Ok(report)
}
