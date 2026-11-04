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
    terminal: Rc<Block>,
}
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub limits: Limits,
    units: Vec<Unit>,
    leaves: BTreeMap<Entry, Id>,
    jumps: BTreeMap<Join, Id>,
    paths: BTreeSet<Vec<(Option<usize>, Entry)>>,
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
}
impl State {
    pub fn fused(&self, catalog: &Catalog) -> bool {
        self.stack
            .last()
            .is_some_and(|u| u.unit >= catalog.leaves.len())
    }
}
impl Catalog {
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
                terminal,
            });
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
        Ok(s)
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
        Some(self.units[right.unit].terminal.clone())
    }
    pub fn entries(&self) -> Vec<J> {
        self.units.iter().enumerate().map(|(id,u)|json!({"id":id,"expanded":u.path,"depth":u.path.len(),"nodes":u.path.len()})).collect()
    }
    pub fn select(&self, p: &Program, profile: &Training) -> Vec<(Join, Evidence)> {
        let mut choices = profile
            .pairs
            .iter()
            .filter(|(_, e)| e.visits >= self.limits.support && e.avoidable_origin_slots > 0)
            .collect::<Vec<_>>();
        choices.sort_by_key(|(j, e)| {
            (
                std::cmp::Reverse(e.avoidable_origin_slots),
                std::cmp::Reverse(e.visits),
                **j,
            )
        });
        let mut paths = self.paths.clone();
        let n = self
            .limits
            .per_round
            .min(self.limits.total.saturating_sub(self.units.len()));
        choices
            .into_iter()
            .filter_map(|(&j, e)| {
                let u = self.combine(p, j)?;
                paths.insert(u.path).then_some((j, e.clone()))
            })
            .take(n)
            .collect()
    }
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct Evidence {
    pub visits: u64,
    pub avoidable_origin_slots: u64,
    pub producer_run: usize,
    pub consumer_run: usize,
    pub witness: Option<crate::graph::Dependency>,
    width: u64,
}
#[derive(Default)]
pub struct Training {
    pairs: BTreeMap<Join, Evidence>,
    hits: BTreeMap<Id, u64>,
    evictions: u64,
    capped_events: u64,
    pub(crate) run: usize,
    pub direct_origins: u64,
    pub saved_key_slots: u64,
    pub(crate) seal_moves: u64,
}
impl Training {
    pub(crate) fn applied(&mut self, c: &Catalog, s: &mut State, inputs: usize) {
        let parent_run = std::mem::replace(&mut s.last_run, self.run);
        self.capped_events += u64::from(s.capped);
        let Some(last) = s.stack.last() else { return };
        *self.hits.entry(last.unit).or_default() += 1;
        if s.fused(c) && s.complete {
            self.direct_origins += 1;
            self.saved_key_slots += inputs as u64;
        }
        let Some(j) = s.pending else { return };
        // Once a right operand already takes the direct origin, extending its
        // left boundary earns no second credit for the same operation.
        if parent_run == 0 || parent_run >= self.run {
            return;
        }
        let saves_key = s.complete && !s.fused(c);
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
            producer_run: parent_run,
            consumer_run: self.run,
            witness: s.witness,
            width: inputs as u64,
            ..Default::default()
        });
        e.visits += 1;
        if saves_key {
            if e.avoidable_origin_slots == 0 {
                e.producer_run = parent_run;
                e.consumer_run = self.run;
                e.witness = s.witness;
            }
            e.avoidable_origin_slots += inputs as u64;
        }
    }
    pub fn report(&self) -> J {
        json!({"applied_units":self.hits,"candidate_shapes":self.pairs.len(),"counter_prunes":self.evictions,"oversized_applied_pairs":self.capped_events,"direct_origins":self.direct_origins,"saved_origin_key_slots":self.saved_key_slots,"saved_answer_slot_copies":self.seal_moves})
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
                terminal: terminal.clone(),
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
                terminal: terminal.clone(),
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
