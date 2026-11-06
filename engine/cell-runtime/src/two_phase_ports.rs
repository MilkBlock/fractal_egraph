//! A second scheduled producer appends one fact column to an existing block.
//! Its complete parameter view is a checked projection of the first phase.
use super::*;
use std::{num::NonZeroU32, rc::Rc};
#[derive(Clone, Copy)]
pub(super) struct Child {
    pub id: NonZeroU32,
    pub depth: u32,
    pub born: u64,
    pub owner: u32,
}
pub(super) enum Source {
    Column(usize),
    Literal(Value),
}
pub(super) struct Extension {
    pub plan: Rc<two_run::Plan>,
    pub projection: Vec<(usize, Source)>,
    pub children: Vec<Child>,
    pub facts: Vec<Option<FactState>>,
    pub links: Vec<u32>,
}
impl Store {
    pub fn attach(
        &mut self,
        source: FactRef,
        id: u32,
        mut c: Cell,
        plan: Rc<two_run::Plan>,
        parents: &Parents,
        epoch: u64,
    ) -> std::result::Result<(), Cell> {
        let Some((i, r, 0)) = self.location(source.cell) else {
            return Err(c);
        };
        let b = &self.slabs[i];
        if b.template != plan.producer_template
            || source.op as usize != plan.producer_op
            || c.template != plan.consumer_template
            || b.records[r].born >= epoch
            || b.parents.is_some()
            || c.parent.is_some()
            || c.facts.len() != 1
            || !c.facts.contains_key(&(plan.child_op as u32))
            || self.state(source).is_none_or(|s| s.hidden)
            || c.values.len() != plan.inputs.len()
            || !c.values.keys().copied().eq(plan.inputs.iter().map(|x| x.0))
            || id == u32::MAX
        {
            return Err(c);
        }
        if let Some(e) = &b.extension {
            if e.links[r] != 0 || *e.plan != *plan {
                return Err(c);
            }
        }
        // Every retained consumer parameter is guarded before changing storage.
        let guarded = plan
            .inputs
            .iter()
            .zip(c.values.iter())
            .all(|((_, a), (_, actual))| {
                let expected = match a {
                    Arg::Literal(v) => v,
                    Arg::Slot(slot) => {
                        let Ok(col) = b.slots.binary_search(slot) else {
                            return false;
                        };
                        &b.values[r * b.slots.len() + col]
                    }
                };
                canon(expected, parents) == canon(actual, parents)
            });
        if !guarded {
            return Err(c);
        }
        assert_eq!(self.directory.borrow().get(id), 0);
        let b = &mut self.slabs[i];
        let e = b.extension.get_or_insert_with(|| Extension {
            projection: plan
                .inputs
                .iter()
                .map(|(s, a)| {
                    (
                        *s,
                        match a {
                            Arg::Slot(slot) => Source::Column(b.slots.binary_search(slot).unwrap()),
                            Arg::Literal(v) => Source::Literal(v.clone()),
                        },
                    )
                })
                .collect(),
            plan: plan.clone(),
            children: vec![],
            facts: vec![],
            links: vec![0; b.records.len()],
        });
        let child_index = e.children.len();
        e.links[r] = child_index as u32 + 1;
        e.children.push(Child {
            id: NonZeroU32::new(id + 1).unwrap(),
            depth: c.recipe_depth,
            born: c.started_epoch,
            owner: r as u32,
        });
        e.facts.push(c.facts.remove(&(plan.child_op as u32)));
        self.extensions.insert(i);
        self.directory
            .borrow_mut()
            .set(id, address(i, child_index) | 1);
        self.live += 1;
        self.stats.installed += 1;
        self.stats.fused_children += 1;
        self.stats.saved_parameters += c.values.len() as u64;
        Ok(())
    }
    /// If phase A dies first, reconstruct B's ordinary parameter row before
    /// releasing A. This bounds lifetime coupling and keeps B's logical ID.
    pub fn detach_child(&mut self, owner: u32) -> Option<(u32, Cell)> {
        let (i, r, 0) = self.location(owner)? else {
            return None;
        };
        let b = &mut self.slabs[i];
        let e = b.extension.as_mut()?;
        let child_index = e.links[r].checked_sub(1)? as usize;
        let child = e.children[child_index];
        let id = child.id.get() - 1;
        let values = e
            .projection
            .iter()
            .map(|(slot, source)| {
                (
                    *slot,
                    match source {
                        Source::Column(col) => b.values[r * b.slots.len() + col].clone(),
                        Source::Literal(v) => v.clone(),
                    },
                )
            })
            .collect();
        let facts = e.facts[child_index]
            .take()
            .map(|f| (e.plan.child_op as u32, f))
            .into_iter()
            .collect();
        let frame = Cell {
            template: e.plan.consumer_template,
            values,
            facts,
            parent: None,
            started_epoch: child.born,
            completed: true,
            completed_prefix: 0,
            recipe_depth: child.depth,
        };
        self.remove_child(i, child_index);
        self.directory.borrow_mut().set(id, 0);
        self.live -= 1;
        self.stats.detached_children += 1;
        Some((id, frame))
    }
    pub fn phase_rows(&self) -> (usize, usize, usize) {
        let physical = self.slabs.iter().map(|b| b.records.len()).sum();
        let fused = self
            .slabs
            .iter()
            .filter_map(|b| b.extension.as_ref())
            .map(|e| e.children.len())
            .sum();
        let capacity = self
            .slabs
            .iter()
            .filter_map(|b| b.extension.as_ref())
            .map(|e| {
                e.children.capacity() * std::mem::size_of::<Child>()
                    + e.facts.capacity() * std::mem::size_of::<Option<FactState>>()
                    + e.links.capacity() * std::mem::size_of::<u32>()
            })
            .sum();
        (physical, fused, capacity)
    }
    pub fn extra_fact_positions(&self) -> usize {
        self.slabs
            .iter()
            .filter_map(|b| b.extension.as_ref())
            .map(|e| e.facts.iter().filter(|f| f.is_some()).count())
            .sum()
    }
    pub(super) fn remove_child(&mut self, slab: usize, index: usize) {
        let b = &mut self.slabs[slab];
        let e = b.extension.as_mut().unwrap();
        let removed = e.children.swap_remove(index);
        e.facts.swap_remove(index);
        e.links[removed.owner as usize] = 0;
        if let Some(moved) = e.children.get(index) {
            e.links[moved.owner as usize] = index as u32 + 1;
            self.directory
                .borrow_mut()
                .set(moved.id.get() - 1, address(slab, index) | 1);
        }
        if e.children.is_empty() {
            b.extension = None;
            self.extensions.remove(&slab);
        }
    }
    pub fn trim_child_capacity(&mut self) {
        for &i in &self.extensions {
            let e = self.slabs[i].extension.as_mut().unwrap();
            let n = e.children.len();
            if n.saturating_mul(2) < e.children.capacity() {
                let before = e.children.capacity() * std::mem::size_of::<Child>()
                    + e.facts.capacity() * std::mem::size_of::<Option<FactState>>();
                let target = n.next_power_of_two();
                e.children.shrink_to(target);
                e.facts.shrink_to(target);
                let after = e.children.capacity() * std::mem::size_of::<Child>()
                    + e.facts.capacity() * std::mem::size_of::<Option<FactState>>();
                self.stats.child_capacity_shrinks += 1;
                self.stats.child_capacity_bytes_released += (before - after) as u64;
            }
        }
    }
}
