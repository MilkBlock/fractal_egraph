//! Actual committed anchor owner to scheduled consumer, with typed projections.
use super::*;
type Pair = (usize, usize, usize, usize);
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Plan {
    pub producer_template: usize,
    pub producer_op: usize,
    pub consumer: usize,
    pub pivot: usize,
    pub consumer_template: usize,
    pub child_op: usize,
    pub inputs: Vec<(usize, Arg)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub results: Vec<usize>,
}
pub(crate) fn closed(p: &Program, t: usize, op: usize, rid: usize, pivot: usize) -> Option<Plan> {
    closed_mode(p, t, op, rid, pivot, false)
}
fn closed_mode(
    p: &Program,
    t: usize,
    op: usize,
    rid: usize,
    pivot: usize,
    result_ports: bool,
) -> Option<Plan> {
    let producer = p.templates.get(t)?;
    let consumer = p.rules.get(rid)?;
    p.port_frames.get(t)?.as_ref()?;
    let &[child_op] = p.port_frames.get(consumer.template)?.as_deref()? else {
        return None;
    };
    if !matches!(
        p.templates[consumer.template].ops[child_op as usize],
        Op::Let(_, Call::Table(_), _)
    ) {
        return None;
    }
    let (table, args, value) = match producer.ops.get(op)? {
        Op::Let(dst, Call::Table(t), args) => (*t, args, Arg::Slot(*dst)),
        Op::Set(t, args, value) => (*t, args, value.clone()),
        _ => return None,
    };
    let atom = consumer.query.get(pivot)?;
    if !matches!(atom.call,Call::Table(t) if t==table) {
        return None;
    }
    let mut map = BTreeMap::new();
    for (a, b) in args.iter().chain(std::iter::once(&value)).zip(&atom.args) {
        if let Arg::Slot(b) = b {
            map.entry(*b).or_insert(normalize(p, t, a.clone())?);
        }
    }
    let mut inputs = consumer
        .inputs
        .iter()
        .map(|s| Some((*s, map.get(s)?.clone())))
        .collect::<Option<Vec<_>>>()?;
    inputs.sort_by_key(|x| x.0);
    let mut results = BTreeSet::new();
    for (_, a) in &inputs {
        if let Arg::Slot(s) = a
            && let Some(&def) = producer.defs.get(s)
        {
            if !result_ports
                || !matches!(&producer.ops[def],Op::Let(_,Call::Table(table),_) if p.tables[*table].constructor && p.tables[*table].virtual_fact)
            {
                return None;
            }
            results.insert(*s);
        }
    }
    Some(Plan {
        producer_template: t,
        producer_op: op,
        consumer: rid,
        pivot,
        consumer_template: consumer.template,
        child_op: child_op as usize,
        inputs,
        results: results.into_iter().collect(),
    })
}
pub(crate) fn load(p: &Program) -> Result<Vec<Option<std::rc::Rc<Plan>>>> {
    let mut out = vec![None; p.rules.len()];
    let Some(raw) = p.artifact.get("two_run_blocks") else {
        return Ok(out);
    };
    if raw["schema"] != "eggfuse-two-run-blocks/v1" {
        return Err("invalid two-run catalog schema".into());
    }
    for item in raw["entries"].as_array().ok_or("missing two-run entries")? {
        let claimed: Plan = serde_json::from_value(item.clone()).map_err(|e| e.to_string())?;
        let expected = closed(
            p,
            claimed.producer_template,
            claimed.producer_op,
            claimed.consumer,
            claimed.pivot,
        )
        .ok_or("unproved two-run projection")?;
        if claimed != expected || out[claimed.consumer].is_some() {
            return Err("invalid two-run certificate".into());
        }
        let rid = claimed.consumer;
        out[rid] = Some(std::rc::Rc::new(claimed));
    }
    Ok(out)
}
pub(crate) fn load_multi(p: &Program) -> Result<Vec<Vec<std::rc::Rc<Plan>>>> {
    let mut out = vec![vec![]; p.rules.len()];
    let Some(raw) = p.artifact.get("multi_run_blocks") else {
        return Ok(out);
    };
    if raw["schema"] != "eggfuse-multi-run-ports/v1" {
        return Err("invalid multi-run catalog schema".into());
    }
    let mut seen = BTreeSet::new();
    for item in raw["entries"]
        .as_array()
        .ok_or("missing multi-run entries")?
    {
        let claimed: Plan = serde_json::from_value(item.clone()).map_err(|e| e.to_string())?;
        let expected = closed_mode(
            p,
            claimed.producer_template,
            claimed.producer_op,
            claimed.consumer,
            claimed.pivot,
            true,
        )
        .ok_or("unproved multi-run projection")?;
        if claimed != expected
            || !seen.insert((
                claimed.producer_template,
                claimed.producer_op,
                claimed.consumer,
                claimed.pivot,
            ))
        {
            return Err("invalid multi-run certificate".into());
        }
        out[claimed.consumer].push(std::rc::Rc::new(claimed));
    }
    for plans in &mut out {
        plans.sort_by_key(|p| (p.pivot, p.producer_template, p.producer_op));
    }
    Ok(out)
}
impl Program {
    pub fn compile_multi_run(&self) -> J {
        let mut readers = vec![vec![]; self.tables.len()];
        for (rid, r) in self.rules.iter().enumerate() {
            for (pivot, a) in r.query.iter().enumerate() {
                if let Call::Table(t) = a.call {
                    readers[t].push((rid, pivot));
                }
            }
        }
        let mut entries = vec![];
        for (t, ops) in self.port_frames.iter().enumerate() {
            for &op in ops.iter().flatten() {
                let table = match self.templates[t].ops[op as usize] {
                    Op::Let(_, Call::Table(t), _) | Op::Set(t, _, _) => t,
                    _ => continue,
                };
                for &(r, pivot) in &readers[table] {
                    if let Some(plan) = closed_mode(self, t, op as usize, r, pivot, true) {
                        entries.push(plan);
                    }
                }
            }
        }
        json!({"schema":"eggfuse-multi-run-ports/v1","entries":entries})
    }
    pub fn bake_two_run(&self, profile: &J, target: &Program) -> Result<J> {
        if profile["status"] != "complete" || profile["program_sha256"] != self.artifact["sha256"] {
            return Err("two-run profile input mismatch".into());
        }
        let mut ranked = vec![];
        for row in profile["two_run_blocks"]["pairs"]
            .as_array()
            .ok_or("missing two-run profile")?
        {
            let t = row["producer_template"].as_u64().ok_or("producer")? as usize;
            let op = row["producer_op"].as_u64().ok_or("op")? as usize;
            let r = row["consumer"].as_u64().ok_or("consumer")? as usize;
            let pivot = row["pivot"].as_u64().ok_or("pivot")? as usize;
            if let Some(plan) = closed(self, t, op, r, pivot) {
                let count = row["work"]["calls"].as_u64().ok_or("calls")?;
                ranked.push((count.saturating_mul(plan.inputs.len() as u64), plan));
            }
        }
        ranked.sort_by_key(|(score, p)| {
            (
                std::cmp::Reverse(*score),
                p.producer_template,
                p.producer_op,
                p.consumer,
                p.pivot,
            )
        });
        let mut ids = BTreeMap::<String, Vec<usize>>::new();
        for (i, r) in target.artifact["recipe"]["rules"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            ids.entry(crate::compiler::rule_signature(r))
                .or_default()
                .push(i);
        }
        let mut entries = vec![];
        let mut selected_score = 0;
        for (score, p) in ranked {
            let signature =
                |r| crate::compiler::rule_signature(&self.artifact["recipe"]["rules"][r]);
            let (Some(&[a]), Some(&[b])) = (
                ids.get(&signature(p.producer_template)).map(Vec::as_slice),
                ids.get(&signature(p.consumer)).map(Vec::as_slice),
            ) else {
                continue;
            };
            if let Some(plan) = closed(target, target.rules[a].template, p.producer_op, b, p.pivot)
            {
                entries.push(plan);
                selected_score = score;
                break;
            }
        }
        Ok(
            json!({"schema":"eggfuse-two-run-blocks/v1","training_program":self.artifact["sha256"],"ranked_saved_parameters":selected_score,"entries":entries}),
        )
    }
}
#[derive(Default, Serialize)]
struct Counts {
    early_by_phase: BTreeMap<usize, u64>,
    reuse_by_phase: BTreeMap<usize, [u64; 2]>,
    completed_by_phase: BTreeMap<usize, u64>,
    early_attempts: u64,
    early_bindings: u64,
    early_parameters: u64,
    early_result_inputs: u64,
    restored_frames: u64,
    owned_roots: u64,
    root_parameters_copied: u64,
    resumed: u64,
    function_anchor: u64,
    unsupported_consumer: u64,
    no_source_port: u64,
    prepared: u64,
    new_port_consumers: u64,
    same_run: u64,
    parent_link: u64,
    guard_failure: u64,
    small_projection: u64,
    disjoint_pairs: u64,
    overlap: u64,
    shared_values: u64,
}
#[derive(Default, Serialize)]
struct Work {
    calls: u64,
    shared_values: u64,
    producer_values: u64,
    consumer_values: u64,
}
#[derive(Default)]
pub(super) struct Survey {
    pub enabled: bool,
    pub execute: bool,
    multi: bool,
    limit: usize,
    selected: Option<(FactRef, std::rc::Rc<Plan>)>,
    loan: Option<Loan>,
    reads: std::cell::Cell<u64>,
    pending: Option<(FactRef, usize, usize)>,
    plans: BTreeMap<Pair, Vec<(usize, usize)>>,
    used: BTreeSet<u32>,
    stats: Counts,
    pairs: BTreeMap<Pair, Work>,
}
struct Loan {
    id: u32,
    prepared: port_frames::phases::Prepared,
    plan: std::rc::Rc<Plan>,
    parent: Option<FactRef>,
}
pub(crate) fn normalize(p: &Program, t: usize, mut a: Arg) -> Option<Arg> {
    let mut seen = BTreeSet::new();
    while let Arg::Slot(s) = a {
        if !seen.insert(s) {
            return None;
        }
        if let Some(next) = p
            .known_results
            .static_slots
            .get(t)
            .and_then(|xs| xs.get(s))
            .and_then(Option::as_ref)
        {
            a = next.clone();
            continue;
        }
        if let Some(&op) = p.templates[t].defs.get(&s)
            && let Op::Copy(_, next) = &p.templates[t].ops[op]
        {
            a = next.clone();
            continue;
        }
        break;
    }
    Some(a)
}
pub(crate) fn derive(
    p: &Program,
    t: usize,
    op: usize,
    rid: usize,
    pivot: usize,
) -> Vec<(usize, usize)> {
    let (table, args, value) = match &p.templates[t].ops[op] {
        Op::Let(dst, Call::Table(t), args) => (*t, args, Arg::Slot(*dst)),
        Op::Set(t, args, value) => (*t, args, value.clone()),
        _ => return vec![],
    };
    let atom = &p.rules[rid].query[pivot];
    if !matches!(atom.call,Call::Table(t) if t==table) {
        return vec![];
    }
    let mut mapping = BTreeMap::new();
    for (a, b) in args.iter().chain(std::iter::once(&value)).zip(&atom.args) {
        if let (Some(Arg::Slot(a)), Arg::Slot(b)) = (normalize(p, t, a.clone()), b) {
            mapping.entry(*b).or_insert(a);
        }
    }
    mapping.into_iter().collect()
}
impl Graph {
    pub fn configure_two_run_survey(&mut self, on: bool) {
        self.two_run.enabled = on;
    }
    pub fn configure_two_run(&mut self, on: bool) {
        self.two_run.execute = on;
        self.two_run.multi = false;
        self.two_run.limit = 2;
    }
    pub fn configure_multi_run(&mut self, depth: usize) -> Result<()> {
        if !(2..=port_frames::phases::MAX_PHASES).contains(&depth) {
            return Err("port block depth must be 2..8".into());
        }
        self.two_run.execute = true;
        self.two_run.multi = true;
        self.two_run.limit = depth;
        Ok(())
    }
    pub(super) fn borrow_two_run_inputs(
        &mut self,
        id: u32,
        template: usize,
        values: crate::slots::Slots,
        parent: Option<FactRef>,
    ) -> (crate::slots::Slots, Option<FactRef>) {
        if !self.two_run.multi {
            return (values, parent);
        }
        let Some((source, plan)) = self.two_run.selected.clone() else {
            return (values, parent);
        };
        if template != plan.consumer_template {
            return (values, parent);
        }
        self.two_run.stats.early_attempts += 1;
        if self.port_frames.parent(source.cell).is_some() {
            // A root may itself borrow an ordinary constructor parent. Flatten
            // that one snapshot once, so all later phases share owned ports.
            let guarded = self
                .port_frames
                .born(source.cell)
                .is_some_and(|e| e < self.epoch)
                && self.fact_state(source).is_some_and(|s| !s.hidden)
                && values.len() == plan.inputs.len()
                && plan.inputs.iter().all(|(s, a)| {
                    values.get(s).is_some_and(|v| {
                        self.argument(source.cell, a)
                            .is_ok_and(|actual| actual == canon(v, &self.parents))
                    })
                });
            if !guarded {
                return (values, parent);
            }
            let mut frame = self.port_frames.root_frame(source.cell).unwrap();
            let old_parent = frame.parent.take().unwrap();
            let columns = self.program.templates[frame.template]
                .parent_columns
                .keys()
                .copied()
                .collect::<Vec<_>>();
            for slot in &columns {
                let Ok(value) = self.slot(source.cell, *slot) else {
                    return (values, parent);
                };
                frame.values.insert(*slot, value);
            }
            if !self.port_frames.own_root(source.cell, frame) {
                return (values, parent);
            }
            self.release_reference(old_parent);
            self.two_run.stats.owned_roots += 1;
            self.two_run.stats.root_parameters_copied += columns.len() as u64;
        }
        let Some(prepared) = self.port_frames.prepare_program(
            source,
            plan.clone(),
            &values,
            &self.parents,
            self.epoch,
            self.two_run.limit,
            Some(&self.program),
        ) else {
            return (values, parent);
        };
        self.two_run.stats.early_bindings += 1;
        *self
            .two_run
            .stats
            .early_by_phase
            .entry(prepared.phase)
            .or_default() += 1;
        self.two_run.stats.early_parameters += values.len() as u64;
        self.two_run.stats.early_result_inputs +=
            self.port_frames.prepared_results(&prepared) as u64;
        self.two_run.loan = Some(Loan {
            id,
            prepared,
            plan,
            parent,
        });
        (Default::default(), None)
    }
    pub(super) fn borrowed_two_run_slot(&self, id: u32, slot: usize) -> Option<Value> {
        let loan = self.two_run.loan.as_ref().filter(|l| l.id == id)?;
        let value = self.port_frames.prepared_value(&loan.prepared, slot)?;
        self.two_run.reads.set(self.two_run.reads.get() + 1);
        Some(canon(&value, &self.parents))
    }
    pub(super) fn clear_two_run_loan(&mut self, id: u32) {
        if self.two_run.loan.as_ref().is_some_and(|l| l.id == id) {
            self.two_run.loan = None;
        }
    }
    pub(super) fn record_two_run_reuse(&mut self, id: u32, w: Dependency) {
        let Some(loan) = self.two_run.loan.as_ref().filter(|l| l.id == id) else {
            return;
        };
        let same = match w {
            Dependency::Fact(f) => {
                self.port_frames.owner(f.cell) == self.port_frames.owner(loan.prepared.source.cell)
            }
            _ => false,
        };
        self.two_run
            .stats
            .reuse_by_phase
            .entry(loan.prepared.phase)
            .or_default()[usize::from(!same)] += 1;
    }
    pub(super) fn complete_port_constructor(
        &mut self,
        id: u32,
        op: usize,
        table: usize,
    ) -> Result<Option<(FactRef, Value)>> {
        let Some(loan) = self.two_run.loan.as_ref().filter(|l| l.id == id) else {
            return Ok(None);
        };
        let phase = loan.prepared.phase;
        let Some(fact) = self.port_frames.prepared_reuse(&loan.prepared, op, table) else {
            return Ok(None);
        };
        // Consume the actual committed result, never a replayed proposal.
        let value = if let Some((actual, values)) = &self.fact_state(fact).unwrap().decoded {
            if *actual != table {
                return Ok(None);
            }
            values.last().unwrap().clone()
        } else {
            let Some(row) = self.fact(fact)? else {
                return Ok(None);
            };
            if row.table != table || row.hidden {
                return Ok(None);
            }
            row.values.last().unwrap().clone()
        };
        *self
            .two_run
            .stats
            .completed_by_phase
            .entry(phase)
            .or_default() += 1;
        #[cfg(feature = "rhs-work-audit")]
        self.rhs_births.read(Dependency::Fact(fact));
        Ok(Some((fact, canon(&value, &self.parents))))
    }
    pub(super) fn restore_two_run_frame(&mut self, id: u32, mut frame: Cell) -> Cell {
        if !self.two_run.loan.as_ref().is_some_and(|l| l.id == id) {
            return frame;
        }
        let loan = self.two_run.loan.take().unwrap();
        for (slot, _) in &loan.plan.inputs {
            let value = self
                .port_frames
                .prepared_value(&loan.prepared, *slot)
                .expect("checked live input port");
            frame.values.insert(*slot, canon(&value, &self.parents));
        }
        frame.parent = loan.parent.filter(|_| {
            !self.program.templates[frame.template]
                .parent_columns
                .is_empty()
        });
        if frame.parent.is_some() {
            for slot in self.program.templates[frame.template].parent_columns.keys() {
                frame.values.remove(slot);
            }
        }
        self.two_run.stats.restored_frames += 1;
        frame
    }
    pub(super) fn try_two_run(&mut self, id: u32, frame: Cell) -> std::result::Result<(), Cell> {
        if !self.two_run.execute {
            return Err(frame);
        }
        if self.two_run.multi {
            let Some(loan) = self.two_run.loan.as_ref().filter(|l| l.id == id) else {
                return Err(frame);
            };
            let prepared = loan.prepared.clone();
            return match self.port_frames.attach_prepared(id, frame, prepared) {
                Ok(()) => {
                    self.two_run.loan = None;
                    Ok(())
                }
                Err(frame) => Err(self.restore_two_run_frame(id, frame)),
            };
        }
        let Some((source, rid, _)) = self.two_run.pending.take() else {
            return Err(frame);
        };
        let Some(plan) = self.program.two_run[rid].clone() else {
            return Err(frame);
        };
        self.port_frames
            .attach(source, id, frame, plan, &self.parents, self.epoch)
    }
    pub(crate) fn prepare_two_run_execution(
        &mut self,
        rid: usize,
        known: &crate::known_results::QueryFacts,
        resumed: bool,
    ) {
        if !self.two_run.execute {
            return;
        }
        self.two_run.pending = None;
        self.two_run.selected = None;
        self.two_run.loan = None;
        if self.two_run.multi {
            if resumed {
                for group in self.program.multi_run[rid].chunk_by(|a, b| a.pivot == b.pivot) {
                    let Some(f) = known.get(group[0].pivot) else {
                        continue;
                    };
                    let Some(template) = self.port_frames.template(f.cell) else {
                        continue;
                    };
                    let Ok(at) = group.binary_search_by_key(&(template, f.op as usize), |p| {
                        (p.producer_template, p.producer_op)
                    }) else {
                        continue;
                    };
                    if self.port_frames.successor(f.cell).is_none()
                        && self
                            .port_frames
                            .phase(f.cell)
                            .is_some_and(|p| p < self.two_run.limit)
                    {
                        self.two_run.selected = Some((f, group[at].clone()));
                        break;
                    }
                }
            }
            return;
        }
        if resumed
            && let Some(p) = &self.program.two_run[rid]
            && let Some(f) = known.get(p.pivot)
        {
            self.two_run.pending = Some((f, rid, p.pivot));
        }
    }
    #[cfg(feature = "two-run-survey")]
    pub(crate) fn prepare_two_run(&mut self, rid: usize, anchor: Option<(Dependency, usize)>) {
        self.two_run.pending = None;
        if !self.two_run.enabled {
            return;
        }
        let Some((w, pivot)) = anchor else { return };
        self.two_run.stats.resumed += 1;
        let Dependency::Fact(f) = w else {
            self.two_run.stats.function_anchor += 1;
            return;
        };
        if !self.port_eligible(self.program.rules[rid].template) {
            self.two_run.stats.unsupported_consumer += 1;
            return;
        }
        if self.port_frames.template(f.cell).is_none() {
            self.two_run.stats.no_source_port += 1;
            return;
        }
        self.two_run.stats.prepared += 1;
        self.two_run.pending = Some((f, rid, pivot));
    }
    pub(super) fn survey_two_run(&mut self, child: u32) {
        let Some((source, rid, pivot)) = self.two_run.pending.take() else {
            return;
        };
        self.two_run.stats.new_port_consumers += 1;
        let Some(t) = self.port_frames.template(source.cell) else {
            return;
        };
        if self
            .port_frames
            .born(source.cell)
            .is_none_or(|e| e >= self.epoch)
        {
            self.two_run.stats.same_run += 1;
            return;
        }
        if self.port_frames.parent(source.cell).is_some()
            || self.port_frames.parent(child).is_some()
        {
            self.two_run.stats.parent_link += 1;
            return;
        }
        if self.fact_state(source).is_none_or(|s| s.hidden) {
            self.two_run.stats.guard_failure += 1;
            return;
        }
        let pair = (t, source.op as usize, rid, pivot);
        let plan = self
            .two_run
            .plans
            .entry(pair)
            .or_insert_with(|| derive(&self.program, t, source.op as usize, rid, pivot));
        let mut shared = 0;
        for &(b, a) in plan.iter() {
            let av = self
                .port_frames
                .read_slot(source.cell, a)
                .and_then(|(_, _, v)| v);
            let bv = self.port_frames.read_slot(child, b).and_then(|(_, _, v)| v);
            if let (Some(av), Some(bv)) = (av, bv) {
                if canon(av, &self.parents) != canon(bv, &self.parents) {
                    self.two_run.stats.guard_failure += 1;
                    return;
                }
                shared += 1;
            }
        }
        if shared < 2 {
            self.two_run.stats.small_projection += 1;
            return;
        }
        if self.two_run.used.contains(&source.cell) || self.two_run.used.contains(&child) {
            self.two_run.stats.overlap += 1;
            return;
        }
        self.two_run.used.extend([source.cell, child]);
        self.two_run.stats.disjoint_pairs += 1;
        self.two_run.stats.shared_values += shared;
        let w = self.two_run.pairs.entry(pair).or_default();
        w.calls += 1;
        w.shared_values += shared;
        w.producer_values += self.port_frames.parameter_count(source.cell).unwrap() as u64;
        w.consumer_values += self.port_frames.parameter_count(child).unwrap() as u64;
    }
    pub fn two_run_report(&self) -> J {
        json!({"scope":"actual Spine anchor-owner to consumer; disjoint survey reserves each producer at most once", "survey":self.two_run.enabled,"execute":self.two_run.execute,"multi":self.two_run.multi,"phase_limit":self.two_run.limit,"early_slot_reads":self.two_run.reads.get(),"multi_catalog_edges":self.program.multi_run.iter().map(Vec::len).sum::<usize>(),
            "stats":self.two_run.stats,"pairs":self.two_run.pairs.iter().map(|(k,w)|json!({"producer_template":k.0,"producer_op":k.1,"consumer":k.2,"pivot":k.3,"projection":self.two_run.plans[k],"work":w})).collect::<Vec<_>>()})
    }
}
