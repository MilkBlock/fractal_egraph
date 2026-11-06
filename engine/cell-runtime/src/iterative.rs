//! Frozen, append-only binary continuations. Leaves refer to validated
//! spine-block entries. IDs are scoped to their seed catalog and recipe.
use crate::{program::*, spine_blocks::Block};
use serde::{Deserialize, Serialize};
use serde_json::{Value as J, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};
pub type Id = usize;
type Entry = (usize, usize);
impl Program {
    /// Ablation keeps the learned compositions and RHS programs unchanged.
    pub fn disable_iterative_lhs(&mut self) {
        if self.artifact["iterative_bake"].get("lhs_handoffs").is_none() {
            return;
        }
        let cat = &self.iterative;
        let switch = crate::result_handoff::Switch::build(
            self,
            cat.units.iter().map(|u| u.path.as_slice()),
            &cat.leaves,
            self.artifact["iterative_bake"]["rhs_handoffs"]["reuse_results"]
                .as_bool()
                .unwrap_or(true),
        );
        self.iterative.switch = switch;
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct Join {
    pub left: Id,
    pub output: usize,
    pub right: Id,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Limits {
    pub depth: usize,
    pub nodes: usize,
    pub total: usize,
    pub per_round: usize,
    pub candidates: usize,
    pub support: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            depth: 4,
            nodes: 8,
            total: 64,
            per_round: 8,
            candidates: 512,
            support: 2,
        }
    }
}
impl Limits {
    pub fn validate(self) -> Result<()> {
        if [
            self.depth,
            self.nodes,
            self.total,
            self.per_round,
            self.candidates,
        ]
        .contains(&0)
            || self.support == 0
        {
            return Err("Bake limits must be positive".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
struct Unit {
    first: Entry,
    last: Entry,
    path: Vec<(Option<usize>, Entry)>,
    terminal: Option<Rc<Block>>,
}
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub limits: Limits,
    units: Vec<Unit>,
    leaves: BTreeMap<Entry, Id>,
    jumps: BTreeMap<Join, Id>,
    paths: BTreeSet<Vec<(Option<usize>, Entry)>>,
    switch: crate::result_handoff::Switch,
}
#[derive(Clone, Copy, Debug)]
struct Segment {
    unit: Id,
    incoming: usize,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct State {
    stack: Vec<Segment>,
    pub output: usize,
    pending: Option<Join>,
    pub complete: bool,
    pub direct: bool,
    capped: bool,
    last_run: usize,
    witness: Option<crate::graph::Dependency>,
    pub(crate) history: crate::result_handoff::History,
    pc: usize,
    active: Option<Id>,
    pub(crate) collect_pairs: bool,
    samples: Option<Rc<Vec<(usize, usize, Option<crate::graph::Dependency>)>>>,
}
impl State {
    pub fn fused(&self, catalog: &Catalog) -> bool {
        self.active
            .or_else(|| self.stack.last().map(|s| s.unit))
            .is_some_and(|id| id >= catalog.leaves.len())
    }
}
impl Catalog {
    /// Transfer learned compositions, never workload identities or receipts.
    /// Every rule is mapped by its complete typed signature, and every new
    /// target join is revalidated before compiling its ports.
    pub fn transfer_lhs(source: &Program, target: &Program) -> Result<J> {
        let mut plan = target.artifact["iterative_bake"].clone();
        if plan.is_null() {
            plan = Self::seed(target, source.iterative.limits)?;
        }
        plan["limits"] = json!(source.iterative.limits);
        Self::expand_lhs_seed(target, &mut plan)?;
        Self::emit_handoffs(target, &mut plan)?;
        let mut cat = Self::parse(target, &plan)?;
        let mut rules = BTreeMap::<String, Vec<usize>>::new();
        for (i, r) in target.artifact["recipe"]["rules"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            rules
                .entry(crate::compiler::rule_signature(r))
                .or_default()
                .push(i);
        }
        let mapping = source.artifact["recipe"]["rules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                match rules
                    .get(&crate::compiler::rule_signature(r))
                    .map(Vec::as_slice)
                {
                    Some(&[r]) => Some(r),
                    _ => None,
                }
            })
            .collect::<Vec<_>>();
        let mut paths = cat
            .units
            .iter()
            .enumerate()
            .map(|(i, u)| (u.path.clone(), i))
            .collect::<BTreeMap<_, _>>();
        let joins = source
            .iterative
            .jumps
            .iter()
            .map(|(j, i)| (*i, *j))
            .collect::<BTreeMap<_, _>>();
        let mut ids = vec![None; source.iterative.units.len()];
        for (id, u) in source.iterative.units.iter().enumerate() {
            let Some(path) = u
                .path
                .iter()
                .map(|&(op, (r, pivot))| Some((op, (mapping[r]?, pivot))))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            if let Some(&old) = paths.get(&path) {
                ids[id] = Some(old);
                continue;
            }
            let Some(j) = joins.get(&id) else { continue };
            let (Some(left), Some(right)) = (ids[j.left], ids[j.right]) else {
                continue;
            };
            let j = Join {
                left,
                output: j.output,
                right,
            };
            let Some(unit) = cat.combine(target, j) else {
                continue;
            };
            if unit.path != path || cat.units.len() >= cat.limits.total {
                continue;
            }
            let next = cat.units.len();
            paths.insert(path.clone(), next);
            ids[id] = Some(next);
            cat.paths.insert(path);
            cat.jumps.insert(j, next);
            cat.units.push(unit);
            plan["joins"].as_array_mut().unwrap().push(json!(j));
        }
        Self::emit_handoffs(target, &mut plan)?;
        Self::emit_lhs(target, &mut plan)?;
        let mut raw = target.artifact.clone();
        raw["iterative_bake"] = plan;
        raw.as_object_mut().unwrap().remove("sha256");
        raw["sha256"] = crate::checksum::artifact_digest(&raw).into();
        Ok(Program::parse(raw)?.artifact)
    }
    pub fn seed(p: &Program, limits: Limits) -> Result<J> {
        limits.validate()?;
        let seed = &p.artifact["spine_blocks"];
        let entries = seed["entries"]
            .as_array()
            .ok_or("warm start requires validated spine blocks")?;
        if entries.is_empty() || entries.len() > limits.total {
            return Err("seed size outside catalog budget".into());
        }
        Ok(
            json!({"schema":"eggfuse-iterative/v1","recipe":p.artifact["recipe"]["sha256"],"seed":crate::checksum::artifact_digest(seed),"limits":limits,"joins":[]}),
        )
    }
    /// The RHS-selected seed omits many valid residual consumer entries. Make
    /// those typed scheduled entries available as leaves before learning joins.
    /// Existing compositions retain their paths after deterministic ID remapping.
    pub fn expand_lhs_seed(p: &Program, plan: &mut J) -> Result<()> {
        if plan.get("lhs_entries").is_some() {
            return Ok(());
        }
        let base = p.artifact["spine_blocks"]["entries"]
            .as_array()
            .ok_or("missing leaves")?;
        let old = base
            .iter()
            .map(|e| {
                (
                    e["consumer"].as_u64().unwrap() as usize,
                    e["pivot"].as_u64().unwrap() as usize,
                )
            })
            .collect::<BTreeSet<_>>();
        let entries = p
            .multi_stage
            .stage_entries
            .iter()
            .map(|(r, a, _)| (*r, *a))
            .filter(|e| !old.contains(e))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let limits: Limits =
            serde_json::from_value(plan["limits"].clone()).map_err(|e| e.to_string())?;
        if base.len() + entries.len() + plan["joins"].as_array().unwrap().len() > limits.total {
            return Err(
                "LHS seed entries exceed --max-units; increase the explicit catalog budget".into(),
            );
        }
        for j in plan["joins"].as_array_mut().unwrap() {
            for k in ["left", "right"] {
                let id = j[k].as_u64().unwrap() as usize;
                if id >= base.len() {
                    j[k] = json!(id + entries.len());
                }
            }
        }
        plan["lhs_entries"] = json!(entries);
        plan.as_object_mut().unwrap().remove("rhs_handoffs");
        plan.as_object_mut().unwrap().remove("lhs_handoffs");
        Ok(())
    }
    pub(crate) fn parse(p: &Program, raw: &J) -> Result<Self> {
        if raw.is_null() {
            return Ok(Self::default());
        }
        let limits: Limits =
            serde_json::from_value(raw["limits"].clone()).map_err(|e| e.to_string())?;
        let seed = Self::seed(p, limits)?;
        for k in ["schema", "recipe", "seed"] {
            if raw[k] != seed[k] {
                return Err("iterative catalog source mismatch".into());
            }
        }
        let mut s = Self {
            limits,
            ..Default::default()
        };
        for e in array(&p.artifact["spine_blocks"]["entries"])? {
            let entry = (
                e["consumer"].as_u64().unwrap() as usize,
                e["pivot"].as_u64().unwrap() as usize,
            );
            let terminal = p
                .spine_blocks
                .get(entry.0, entry.1)
                .ok_or("invalid leaf entry")?;
            let path = vec![(None, entry)];
            let id = s.units.len();
            s.leaves.insert(entry, id);
            s.paths.insert(path.clone());
            s.units.push(Unit {
                first: entry,
                last: entry,
                path,
                terminal: Some(terminal),
            });
        }
        if let Some(raw) = raw.get("lhs_entries") {
            for e in array(raw)? {
                let entry: (usize, usize) =
                    serde_json::from_value(e.clone()).map_err(|e| e.to_string())?;
                if s.leaves.contains_key(&entry)
                    || !p
                        .multi_stage
                        .stage_entries
                        .iter()
                        .any(|(r, a, _)| (*r, *a) == entry)
                    || s.units.len() >= limits.total
                {
                    return Err("invalid LHS seed entry".into());
                }
                let id = s.units.len();
                let path = vec![(None, entry)];
                s.leaves.insert(entry, id);
                s.paths.insert(path.clone());
                s.units.push(Unit {
                    first: entry,
                    last: entry,
                    path,
                    terminal: Block::lower(p, entry.0, entry.1).map(Rc::new),
                });
            }
        }
        for j in array(&raw["joins"])? {
            let join: Join = serde_json::from_value(j.clone()).map_err(|e| e.to_string())?;
            let u = s
                .combine(p, join)
                .ok_or("invalid or oversized binary continuation")?;
            if s.units.len() >= limits.total || !s.paths.insert(u.path.clone()) {
                return Err("duplicate continuation or catalog full".into());
            }
            s.jumps.insert(join, s.units.len());
            s.units.push(u);
        }
        if let Some(raw) = raw.get("rhs_handoffs") {
            let reuse = raw["reuse_results"]
                .as_bool()
                .ok_or("missing result switch policy")?;
            s.switch = crate::result_handoff::Switch::build(
                p,
                s.units.iter().map(|u| u.path.as_slice()),
                &s.leaves,
                reuse,
            );
            if *raw != s.switch.artifact() {
                return Err("invalid cross-Run RHS plan".into());
            }
        }
        if let Some(claimed) = raw.get("lhs_handoffs") {
            s.switch = crate::result_handoff::Switch::build_lhs(
                p,
                s.units.iter().map(|u| u.path.as_slice()),
                &s.leaves,
                raw["rhs_handoffs"]["reuse_results"]
                    .as_bool()
                    .unwrap_or(true),
            );
            if *claimed != s.switch.artifact() {
                return Err("invalid cross-Run LHS plan".into());
            }
        }
        Ok(s)
    }
    pub fn emit_handoffs(p: &Program, plan: &mut J) -> Result<()> {
        Self::emit_switch(p, plan, true)
    }
    pub fn emit_lhs(p: &Program, plan: &mut J) -> Result<()> {
        plan.as_object_mut()
            .ok_or("missing iterative catalog")?
            .remove("lhs_handoffs");
        let cat = Self::parse(p, plan)?;
        plan["lhs_handoffs"] = crate::result_handoff::Switch::build_lhs(
            p,
            cat.units.iter().map(|u| u.path.as_slice()),
            &cat.leaves,
            plan["rhs_handoffs"]["reuse_results"]
                .as_bool()
                .unwrap_or(true),
        )
        .artifact();
        Ok(())
    }
    pub fn emit_switch(p: &Program, plan: &mut J, reuse: bool) -> Result<()> {
        plan.as_object_mut()
            .ok_or("missing iterative catalog")?
            .remove("lhs_handoffs");
        plan.as_object_mut()
            .ok_or("missing iterative catalog")?
            .remove("rhs_handoffs");
        let cat = Self::parse(p, plan)?;
        plan["rhs_handoffs"] = crate::result_handoff::Switch::build(
            p,
            cat.units.iter().map(|u| u.path.as_slice()),
            &cat.leaves,
            reuse,
        )
        .artifact();
        Ok(())
    }
    pub(crate) fn exports(&self, rid: usize) -> &[crate::result_handoff::Donor] {
        self.switch.exports.get(rid).map_or(&[], Vec::as_slice)
    }
    pub(crate) fn capture_sources(&self, s: &State) -> &[crate::result_handoff::Donor] {
        self.switch.supply(s.pc)
    }
    pub(crate) fn history_depth(&self) -> usize {
        self.switch.depth
    }
    pub(crate) fn program(&self, s: &State) -> &[crate::result_handoff::Reuse] {
        self.switch
            .program(s.pc)
            .map_or(&[], |t| t.reuse.as_slice())
    }
    pub(crate) fn lhs_program(&self, s: &State) -> &[crate::result_handoff::Reuse] {
        self.switch.program(s.pc).map_or(&[], |t| t.lhs.as_slice())
    }
    pub(crate) fn active_unit(&self, s: &State) -> usize {
        self.switch.active(s.pc)
    }
    pub(crate) fn closed(&self, s: &State) -> bool {
        self.switch.program(s.pc).is_some_and(|p| p.complete)
    }
    fn combine(&self, p: &Program, j: Join) -> Option<Unit> {
        let (l, r) = (self.units.get(j.left)?, self.units.get(j.right)?);
        let size = l.path.len().checked_add(r.path.len())?;
        if size > self.limits.depth.min(self.limits.nodes) {
            return None;
        }
        let table = match p.templates[p.rules[l.last.0].template].ops.get(j.output)? {
            Op::Let(_, Call::Table(t), _) | Op::Set(t, _, _) => *t,
            _ => return None,
        };
        if !matches!(p.rules[r.first.0].query[r.first.1].call,Call::Table(t) if t==table) {
            return None;
        }
        let mut path = l.path.clone();
        let mut tail = r.path.clone();
        tail[0].0 = Some(j.output);
        path.extend(tail);
        Some(Unit {
            first: l.first,
            last: r.last,
            path,
            terminal: r.terminal.clone(),
        })
    }
    pub(crate) fn enabled(&self) -> bool {
        !self.units.is_empty()
    }
    // Executed only on an originally scheduled and checked anchor. Reductions
    // consume adjacent, non-overlapping segments of this particular cursor.
    pub(crate) fn dispatch(
        &self,
        state: &mut State,
        rule: usize,
        pivot: usize,
        complete: bool,
        witness: crate::graph::Dependency,
    ) -> Option<Rc<Block>> {
        if self.switch.enabled() {
            let Some(leaf) = self.switch.leaf(rule, pivot) else {
                *state = State::default();
                return None;
            };
            state.pc = self.switch.advance(state.pc, state.output, leaf);
            let unit = self.switch.active(state.pc);
            state.active = Some(unit);
            state.direct = complete;
            state.complete = complete;
            state.pending = None;
            state.capped = false;
            state.witness = Some(witness);
            if state.collect_pairs {
                let size = self.units[unit].path.len();
                let mut overlap = size - 1;
                let mut incoming = state.output;
                while overlap != 0 {
                    let Some(segment) = state.stack.pop() else {
                        break;
                    };
                    let span = self.units[segment.unit].path.len();
                    incoming = segment.incoming;
                    if span > overlap {
                        state.stack.clear();
                        break;
                    }
                    overlap -= span;
                }
                if let Some(left) = state.stack.last() {
                    let j = Join {
                        left: left.unit,
                        output: incoming,
                        right: unit,
                    };
                    if self.units[left.unit].path.len() + size
                        <= self.limits.depth.min(self.limits.nodes)
                    {
                        state.pending = Some(j);
                    } else {
                        state.capped = true;
                    }
                }
                state.stack.push(Segment { unit, incoming });
                let mut expanded = 0;
                let keep = state
                    .stack
                    .iter()
                    .rev()
                    .take_while(|s| {
                        expanded += self.units[s.unit].path.len();
                        expanded <= self.limits.depth.min(self.limits.nodes)
                    })
                    .count();
                state.stack.drain(..state.stack.len() - keep);
            }
            return self.units[unit].terminal.clone();
        }
        let Some(&id) = self.leaves.get(&(rule, pivot)) else {
            *state = State::default();
            return None;
        };
        let mut right = Segment {
            unit: id,
            incoming: state.output,
        };
        state.pending = None;
        state.capped = false;
        state.witness = Some(witness);
        state.complete = complete;
        while let Some(left) = state.stack.last() {
            let join = Join {
                left: left.unit,
                output: right.incoming,
                right: right.unit,
            };
            if let Some(&unit) = self.jumps.get(&join) {
                right = Segment {
                    unit,
                    incoming: left.incoming,
                };
                state.stack.pop();
            } else {
                if self.units[left.unit].path.len() + self.units[right.unit].path.len()
                    <= self.limits.depth.min(self.limits.nodes)
                {
                    state.pending = Some(join);
                } else {
                    state.capped = true;
                }
                break;
            }
        }
        state.direct = complete && right.unit >= self.leaves.len();
        state.stack.push(right);
        // A future reduction cannot reach past D primitive applications.
        let mut size = 0;
        let keep = state
            .stack
            .iter()
            .rev()
            .take_while(|s| {
                size += self.units[s.unit].path.len();
                size <= self.limits.depth.min(self.limits.nodes)
            })
            .count();
        state.stack.drain(..state.stack.len() - keep);
        self.units[right.unit].terminal.clone()
    }
    pub fn entries(&self) -> Vec<J> {
        self.units
            .iter()
            .enumerate()
            .map(|(id, u)| {
                let mut entry =
                    json!({"id":id,"expanded":u.path,"depth":u.path.len(),"nodes":u.path.len()});
                if self.switch.enabled() {
                    entry["terminal"] = json!(self.switch.program(self.switch.replay(&u.path)));
                }
                entry
            })
            .collect()
    }
    pub fn select(&self, p: &Program, profile: &Training) -> Vec<(Join, Evidence)> {
        self.select_mode(p, profile, false)
    }
    pub fn select_lhs(&self, p: &Program, profile: &Training) -> Vec<(Join, Evidence)> {
        self.select_mode(p, profile, true)
    }
    fn select_mode(&self, p: &Program, profile: &Training, lhs: bool) -> Vec<(Join, Evidence)> {
        let mut choices = vec![];
        for (&join, e) in &profile.pairs {
            if e.visits < self.limits.support {
                continue;
            }
            let Some(u) = self.combine(p, join) else {
                continue;
            };
            if self.paths.contains(&u.path) {
                continue;
            }
            let mut e = e.clone();
            e.measured_lhs_probes=profile.reads.get(&join).map_or(0,|rs|rs.values().sum());
            if self.switch.enabled() {
                let old_program = self.switch.program(self.switch.replay(&u.path)).unwrap();
                let old = if lhs {
                    &old_program.lhs
                } else {
                    &old_program.reuse
                };
                let proof = if lhs {
                    crate::result_handoff::derive_lhs(p, &u.path)
                } else {
                    crate::result_handoff::derive(p, &u.path)
                };
                let additional = proof
                    .iter()
                    .filter(|r| !old.iter().any(|s| s.op == r.op))
                    .map(|r|if lhs {profile.probes(join,r.op).min(e.visits)}else{e.visits}).sum();
                e.predicted_key_reads = additional;
            }
            // A + B can be useful even when B itself has nothing to reuse.
            // Propose a bounded bridge only if observed B + C and the typed
            // three-unit path expose a donor in A. This is a ranking estimate;
            // the next round must witness the joined AB actually followed by C.
            if self.switch.enabled() && e.predicted_key_reads == 0 {
                for (next, w) in profile.pairs.range(
                    Join {
                        left: join.right,
                        output: 0,
                        right: 0,
                    }..,
                ) {
                    if next.left != join.right {
                        break;
                    }
                    if w.visits < self.limits.support {
                        continue;
                    }
                    let tail = &self.units[next.right];
                    if u.path.len() + tail.path.len() > self.limits.depth.min(self.limits.nodes) {
                        continue;
                    }
                    let Some(_) = self.combine(p, *next) else {
                        continue;
                    };
                    let mut path = u.path.clone();
                    let mut suffix = tail.path.clone();
                    suffix[0].0 = Some(next.output);
                    path.extend(suffix);
                    let old_program = self.switch.program(self.switch.replay(&path)).unwrap();
                    let old = if lhs {
                        &old_program.lhs
                    } else {
                        &old_program.reuse
                    };
                    let start_of_b = self.units[join.left].path.len();
                    let proof = if lhs {
                        crate::result_handoff::derive_lhs(p, &path)
                    } else {
                        crate::result_handoff::derive(p, &path)
                    };
                    if proof.iter().any(|r| {
                        path.len() - 1 - r.back < start_of_b && !old.iter().any(|x| x.op == r.op)
                            && (!lhs || profile.probes(*next,r.op)>0)
                    }) {
                        e.bridge_support = e.bridge_support.max(w.visits.min(e.visits));
                    }
                }
            }
            if e.avoidable_origin_slots != 0 || e.predicted_key_reads != 0 || e.bridge_support != 0
            {
                choices.push((join, e, u.path));
            }
        }
        choices.sort_by_key(|(j, e, _)| {
            (
                std::cmp::Reverse(e.predicted_key_reads),
                std::cmp::Reverse(e.bridge_support),
                std::cmp::Reverse(e.avoidable_origin_slots),
                std::cmp::Reverse(e.visits),
                *j,
            )
        });
        let n = self
            .limits
            .per_round
            .min(self.limits.total.saturating_sub(self.units.len()));
        let mut paths = BTreeSet::new();
        choices
            .into_iter()
            .filter_map(|(j, e, path)| paths.insert(path).then_some((j, e)))
            .take(n)
            .collect()
    }
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct Evidence {
    pub visits: u64,
    pub measured_lhs_probes:u64,
    pub predicted_key_reads: u64,
    pub bridge_support: u64,
    pub avoidable_origin_slots: u64,
    pub producer_run: usize,
    pub consumer_run: usize,
    pub witness: Option<crate::graph::Dependency>,
    width: u64,
}
#[derive(Default)]
pub struct Training {
    pub(crate) collect_pairs: bool,
    pairs: BTreeMap<Join, Evidence>,
    reads:BTreeMap<Join,BTreeMap<usize,u64>>,
    hits: BTreeMap<Id, u64>,
    evictions: u64,
    capped_events: u64,
    pub(crate) run: usize,
    pub direct_origins: u64,
    pub saved_key_slots: u64,
    pub(crate) seal_moves: u64,
}
impl Training {
    fn probes(&self,j:Join,atom:usize)->u64 {self.reads.get(&j).and_then(|r|r.get(&atom)).copied().unwrap_or(0)}
    pub(crate) fn positive_read(&mut self,c:&Catalog,s:&State,atom:usize) {
        if !self.collect_pairs {return}
        let Some(j)=s.pending else{return};
        if !self.reads.contains_key(&j) && self.reads.len()>=c.limits.candidates {return}
        *self.reads.entry(j).or_default().entry(atom).or_default()+=1;
    }
    pub(crate) fn applied(&mut self, c: &Catalog, s: &mut State, inputs: usize) {
        let parent_run = std::mem::replace(&mut s.last_run, self.run);
        self.capped_events += u64::from(s.capped);
        let Some(id) = s.active.or_else(|| s.stack.last().map(|x| x.unit)) else {
            return;
        };
        *self.hits.entry(id).or_default() += 1;
        if s.direct {
            self.direct_origins += 1;
            self.saved_key_slots += inputs as u64;
        }
        // In training only, retain a bounded witness window. With a multi-step
        // right operand its *first* Run identifies the sampled join boundary.
        if s.collect_pairs && s.active.is_some() {
            let window = Rc::make_mut(s.samples.get_or_insert_with(Default::default));
            let limit = c.limits.depth.min(c.limits.nodes);
            if window.len() == limit {
                window.remove(0);
            }
            window.push((parent_run, self.run, s.witness));
        }
        let Some(j) = s.pending else { return };
        // Once a right operand already takes the direct origin, extending its
        // left boundary earns no second credit for the same operation.
        if parent_run == 0 || parent_run >= self.run {
            return;
        }
        let sample = if s.collect_pairs && s.active.is_some() {
            let window = s.samples.as_deref().unwrap();
            let Some(i) = window.len().checked_sub(c.units[j.right].path.len()) else {
                return;
            };
            window[i]
        } else {
            (parent_run, self.run, s.witness)
        };
        let saves_key = s.complete && !s.fused(c) && !c.switch.enabled();
        if self.pairs.len() == c.limits.candidates && !self.pairs.contains_key(&j) {
            self.pairs.retain(|_, e| {
                e.visits -= 1;
                e.avoidable_origin_slots = e.avoidable_origin_slots.saturating_sub(e.width);
                e.visits > 0
            });
            self.evictions += 1;
            return;
        }
        let e = self.pairs.entry(j).or_insert(Evidence {
            producer_run: sample.0,
            consumer_run: sample.1,
            witness: sample.2,
            width: inputs as u64,
            ..Default::default()
        });
        e.visits += 1;
        if saves_key {
            if e.avoidable_origin_slots == 0 {
                e.producer_run = sample.0;
                e.consumer_run = sample.1;
                e.witness = sample.2;
            }
            e.avoidable_origin_slots += inputs as u64;
        }
    }
    pub fn report(&self) -> J {
        json!({"applied_units":self.hits,"candidate_shapes":self.pairs.len(),"positive_lhs_probe_shapes":self.reads.len(),"positive_lhs_probes":self.reads.values().flat_map(|r|r.values()).sum::<u64>(),"counter_prunes":self.evictions,"oversized_applied_pairs":self.capped_events,"direct_origins":self.direct_origins,"saved_origin_key_slots":self.saved_key_slots,"saved_answer_slot_copies":self.seal_moves})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Dependency;
    #[test]
    fn two_reduced_operands_can_join_without_repeating_the_prefix() {
        let terminal = Rc::new(Block {
            port: None,
            inputs: vec![],
        });
        let mut c = Catalog::default();
        for i in 0..4 {
            c.leaves.insert((i, 0), i);
            c.units.push(Unit {
                first: (i, 0),
                last: (i, 0),
                terminal: Some(terminal.clone()),
                path: vec![(None, (i, 0))],
            });
        }
        for (l, r) in [(0, 1), (2, 3), (4, 5)] {
            let mut path = c.units[l].path.clone();
            let mut right = c.units[r].path.clone();
            right[0].0 = Some(0);
            path.extend(right);
            let unit = Unit {
                first: c.units[l].first,
                last: c.units[r].last,
                path,
                terminal: Some(terminal.clone()),
            };
            c.jumps.insert(
                Join {
                    left: l,
                    right: r,
                    output: 0,
                },
                c.units.len(),
            );
            c.units.push(unit);
        }
        let mut state = State::default();
        for i in 0..4 {
            assert!(
                c.dispatch(&mut state, i, 0, true, Dependency::Function(0))
                    .is_some()
            );
        }
        assert_eq!(state.stack.len(), 1);
        assert_eq!(state.stack[0].unit, 6);
        assert!(state.direct);
        assert!(
            c.dispatch(&mut state, 99, 0, true, Dependency::Function(0))
                .is_none()
        );
        assert!(state.stack.is_empty() && !state.direct);
    }
    #[test]
    fn tail_counters_remain_bounded_and_credit_only_complete_entries() {
        let mut c = Catalog::default();
        c.limits.candidates = 1;
        c.leaves.insert((0, 0), 0);
        let mut s = State {
            stack: vec![Segment {
                unit: 0,
                incoming: 0,
            }],
            last_run: 1,
            ..Default::default()
        };
        let mut training = Training {
            run: 2,
            ..Default::default()
        };
        s.pending = Some(Join {
            left: 0,
            output: 1,
            right: 0,
        });
        training.applied(&c, &mut s, 3);
        assert_eq!(
            training.pairs[&s.pending.unwrap()].avoidable_origin_slots,
            0
        );
        training.run = 3;
        s.complete = true;
        training.applied(&c, &mut s, 3);
        let e = &training.pairs[&s.pending.unwrap()];
        assert_eq!(
            (
                e.visits,
                e.avoidable_origin_slots,
                e.producer_run,
                e.consumer_run
            ),
            (2, 3, 2, 3)
        );
        s.pending = Some(Join {
            left: 0,
            output: 2,
            right: 0,
        });
        training.run = 4;
        training.applied(&c, &mut s, 5);
        assert_eq!(training.evictions, 1);
        assert_eq!(training.pairs.len(), 1);
        assert_eq!(
            training
                .pairs
                .values()
                .next()
                .unwrap()
                .avoidable_origin_slots,
            0
        );
    }
}
