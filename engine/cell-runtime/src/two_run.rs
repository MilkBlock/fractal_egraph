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
}
pub(crate) fn closed(p: &Program, t: usize, op: usize, rid: usize, pivot: usize) -> Option<Plan> {
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
    if inputs
        .iter()
        .any(|(_, a)| matches!(a,Arg::Slot(s) if producer.defs.contains_key(s)))
    {
        return None;
    }
    Some(Plan {
        producer_template: t,
        producer_op: op,
        consumer: rid,
        pivot,
        consumer_template: consumer.template,
        child_op: child_op as usize,
        inputs,
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
impl Program {
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
    pending: Option<(FactRef, usize, usize)>,
    plans: BTreeMap<Pair, Vec<(usize, usize)>>,
    used: BTreeSet<u32>,
    stats: Counts,
    pairs: BTreeMap<Pair, Work>,
}
fn normalize(p: &Program, t: usize, mut a: Arg) -> Option<Arg> {
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
    }
    pub(super) fn try_two_run(&mut self, id: u32, frame: Cell) -> std::result::Result<(), Cell> {
        if !self.two_run.execute {
            return Err(frame);
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
        json!({"scope":"actual Spine anchor-owner to consumer; disjoint survey reserves each producer at most once", "survey":self.two_run.enabled,"execute":self.two_run.execute,
            "stats":self.two_run.stats,"pairs":self.two_run.pairs.iter().map(|(k,w)|json!({"producer_template":k.0,"producer_op":k.1,"consumer":k.2,"pivot":k.3,"projection":self.two_run.plans[k],"work":w})).collect::<Vec<_>>()})
    }
}
