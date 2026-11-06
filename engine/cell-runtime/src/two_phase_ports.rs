//! Bounded chains share one root parameter row. Projections are flattened at
//! layout construction; parameter reads do not chase parent frames.
use super::*;
use std::{num::NonZeroU32, rc::Rc};
pub(crate) const MAX_PHASES: usize = 8;
#[derive(Clone, Copy)]
pub(super) struct Child {
    pub id: NonZeroU32,
    pub depth: u32,
    pub born: u64,
    pub owner: u32,
    pub source: u32,
    pub successor: u32,
    pub layout: usize,
    pub phase: usize,
}
#[derive(Clone, PartialEq, Eq)]
pub(super) enum Source {
    Column(usize),
    Literal(Value),
    Result { phase: usize, slot: usize },
}
pub(super) struct Layout {
    pub plan: Rc<two_run::Plan>,
    pub projection: Vec<Source>,
    exports: Vec<PortFact>,
    reuse: Option<PortFact>,
}
#[derive(Clone)]
struct PortFact {
    phase: usize,
    op: usize,
    table: usize,
    key: Vec<Source>,
}
fn fact_expression(
    p: &Program,
    t: usize,
    op: usize,
    phase: usize,
    input: impl Fn(usize) -> Option<Source>,
) -> Option<PortFact> {
    let fact=p.block_postconditions[t].iter().find(|f|f.op==op)?;
    let table=fact.table;let key=&fact.fields[..fact.fields.len()-1];
    let arg = |a: &Arg| -> Option<Source> {
        match two_run::normalize(p,t,a.clone())? {
        Arg::Literal(v)=>Some(Source::Literal(v)),Arg::Slot(s)=>input(s).or_else(||{
            let def=*p.templates[t].defs.get(&s)?;
            matches!(p.templates[t].ops[def],Op::Let(_,Call::Table(k),_) if p.tables[k].constructor)
                .then_some(Source::Result{phase,slot:s})
        }),
    }
    };
    Some(PortFact {
        phase,
        op,
        table,
        key: key.iter().map(&arg).collect::<Option<Vec<_>>>()?,
    })
}
#[derive(Clone)]
pub(crate) struct Prepared {
    pub source: FactRef,
    pub slab: usize,
    pub owner: usize,
    pub layout: usize,
    pub phase: usize,
}
pub(super) struct Extension {
    pub layouts: Vec<Layout>,
    layout_keys: BTreeMap<(usize, Rc<two_run::Plan>), usize>,
    pub children: Vec<Child>,
    pub facts: Vec<Option<FactState>>,
    pub links: Vec<u32>, // logical successor ID + 1, never a moving row offset
}
impl Store {
    pub fn query_input(&self,anchor:FactRef)->Option<block_lhs::Input> {
        self.state(anchor)?;
        let root=self.owner(anchor.cell)?;
        let (slab,_,_)=self.location(root)?;
        let mut ids=smallvec::smallvec![root];let mut next=self.successor(root);
        while let Some(id)=next {if ids.len()>=MAX_PHASES {return None}ids.push(id);next=self.successor(id);}
        let phase=ids.iter().position(|id|*id==anchor.cell)?;
        let key=if ids.len()==1 {block_lhs::ShapeKey::Root(self.slabs[slab].template)} else {
            let (_,r,_)=self.location(*ids.last()?)?;
            block_lhs::ShapeKey::Chain(slab,self.slabs[slab].extension.as_ref()?.children[r].layout)
        };
        Some(block_lhs::Input{key,ids,anchor_phase:phase})
    }
    pub fn query_shape(&self,input:&block_lhs::Input)->Option<block_lhs::Shape> {
        let (slab,_,_)=self.location(input.ids[0])?;let b=&self.slabs[slab];
        let mut phases=vec![block_lhs::Phase{template:b.template,inputs:vec![]}];
        for &id in &input.ids[1..] {
            let (_,r,_)=self.location(id)?;let e=b.extension.as_ref()?;let l=&e.layouts[e.children[r].layout];
            let inputs=l.plan.inputs.iter().zip(&l.projection).map(|((slot,_),src)|{
                let sym=match src {Source::Column(c)=>block_lhs::Symbol::Slot(0,b.slots[*c]),Source::Literal(v)=>block_lhs::Symbol::Literal(v.clone()),Source::Result{phase,slot}=>block_lhs::Symbol::Slot(phase-1,*slot)};
                (*slot,sym)
            }).collect();
            phases.push(block_lhs::Phase{template:l.plan.consumer_template,inputs});
        }
        Some(block_lhs::Shape{phases})
    }
    pub fn owner(&self, id: u32) -> Option<u32> {
        let (b, r, p) = self.locate(id)?;
        Some(if p == 0 {
            id
        } else {
            b.records[b.extension.as_ref()?.children[r].owner as usize].id
        })
    }
    pub fn root_frame(&self, id: u32) -> Option<Cell> {
        let (b, r, p) = self.locate(id)?;
        if p != 0 {
            return None;
        }
        Some(Cell {
            template: b.template,
            parent: b.parents.as_ref().map(|v| v[r]),
            values: b
                .slots
                .iter()
                .copied()
                .zip(
                    b.values[r * b.slots.len()..(r + 1) * b.slots.len()]
                        .iter()
                        .cloned(),
                )
                .collect(),
            facts: b
                .ops
                .iter()
                .copied()
                .zip(&b.facts[r * b.ops.len()..(r + 1) * b.ops.len()])
                .filter_map(|(op, s)| s.clone().map(|s| (op, s)))
                .collect(),
            completed: true,
            completed_prefix: 0,
            recipe_depth: b.records[r].depth,
            started_epoch: b.records[r].born,
        })
    }
    pub fn own_root(&mut self, id: u32, frame: Cell) -> bool {
        let old = self.root_frame(id).expect("root frame");
        assert!(self.successor(id).is_none());
        self.remove(id);
        self.stats.removed -= 1;
        match self.insert(id, frame) {
            Ok(()) => {
                self.stats.installed -= 1;
                true
            }
            Err(_) => {
                self.insert(id, old).expect("vacated source layout");
                self.stats.installed -= 1;
                false
            }
        }
    }
    pub fn phase(&self, id: u32) -> Option<usize> {
        let (b, r, p) = self.locate(id)?;
        Some(if p == 0 {
            1
        } else {
            b.extension.as_ref()?.children[r].phase
        })
    }
    pub fn successor(&self, id: u32) -> Option<u32> {
        let (b, r, p) = self.locate(id)?;
        let e = b.extension.as_ref()?;
        (if p == 0 {
            e.links[r]
        } else {
            e.children[r].successor
        })
        .checked_sub(1)
    }
    fn set_successor(&mut self, id: u32, next: u32) {
        let (i, r, p) = self.location(id).expect("live source block");
        let e = self.slabs[i].extension.as_mut().unwrap();
        if p == 0 {
            e.links[r] = next;
        } else {
            e.children[r].successor = next;
        }
    }
    fn source_value(
        &self,
        slab: usize,
        owner: usize,
        source: u32,
        projection: &Source,
    ) -> Option<Value> {
        let b = &self.slabs[slab];
        Some(match projection {
            Source::Column(c) => b.values[owner * b.slots.len() + c].clone(),
            Source::Literal(v) => v.clone(),
            Source::Result { phase, slot } => {
                let mut id = if *phase == 1 {
                    b.records[owner].id
                } else {
                    source
                };
                while self.phase(id)? > *phase {
                    let (b, r, _) = self.locate(id)?;
                    id = b.extension.as_ref()?.children[r].source;
                }
                if self.phase(id)? != *phase {
                    return None;
                }
                Value::Eq(((id as u64) << 32) | (*slot as u64 + 1))
            }
        })
    }
    pub fn prepared_value(&self, p: &Prepared, slot: usize) -> Option<Value> {
        let l = &self.slabs[p.slab].extension.as_ref()?.layouts[p.layout];
        let col = l.plan.inputs.binary_search_by_key(&slot, |x| x.0).ok()?;
        self.source_value(p.slab, p.owner, p.source.cell, &l.projection[col])
    }
    pub fn prepared_results(&self, p: &Prepared) -> usize {
        self.slabs[p.slab].extension.as_ref().unwrap().layouts[p.layout]
            .projection
            .iter()
            .filter(|v| matches!(v, Source::Result { .. }))
            .count()
    }
    pub fn prepared_reuse(&self, p: &Prepared, op: usize, table: usize) -> Option<FactRef> {
        let l = &self.slabs[p.slab].extension.as_ref()?.layouts[p.layout];
        if l.plan.child_op != op {
            return None;
        }
        let fact = l.reuse.as_ref()?;
        if fact.table != table {
            return None;
        }
        let mut id = if fact.phase == 1 {
            self.slabs[p.slab].records[p.owner].id
        } else {
            p.source.cell
        };
        while self.phase(id)? > fact.phase {
            let (b, r, _) = self.locate(id)?;
            id = b.extension.as_ref()?.children[r].source;
        }
        let f = FactRef {
            cell: id,
            op: fact.op as u32,
        };
        let state = self.state(f)?;
        if state.hidden {
            return None;
        }
        Some(f)
    }
    pub fn result_value(&self, id: u32, slot: usize) -> Option<Value> {
        let (b, r, p) = self.locate(id)?;
        if p == 0 {
            return None;
        }
        let e = b.extension.as_ref()?;
        let c = &e.children[r];
        let l = &e.layouts[c.layout];
        let col = l.plan.inputs.binary_search_by_key(&slot, |x| x.0).ok()?;
        if !matches!(l.projection[col], Source::Result { .. }) {
            return None;
        }
        let (slab, _, _) = self.location(id)?;
        self.source_value(slab, c.owner as usize, c.source, &l.projection[col])
    }
    pub fn prepare(
        &mut self,
        source: FactRef,
        plan: Rc<two_run::Plan>,
        values: &crate::slots::Slots,
        parents: &Parents,
        epoch: u64,
        limit: usize,
    ) -> Option<Prepared> {
        self.prepare_program(source, plan, values, parents, epoch, limit, None)
    }
    pub fn prepare_program(
        &mut self,
        source: FactRef,
        plan: Rc<two_run::Plan>,
        values: &crate::slots::Slots,
        parents: &Parents,
        epoch: u64,
        limit: usize,
        program: Option<&Program>,
    ) -> Option<Prepared> {
        let (i, r, p) = self.location(source.cell)?;
        let b = &self.slabs[i];
        let phase = self.phase(source.cell)? + 1;
        if phase > limit
            || self.template(source.cell)? != plan.producer_template
            || source.op as usize != plan.producer_op
            || self.born(source.cell)? >= epoch
            || self.parent(source.cell).is_some()
            || self.successor(source.cell).is_some()
            || self.state(source).is_none_or(|s| s.hidden)
            || values.len() != plan.inputs.len()
            || !values.keys().copied().eq(plan.inputs.iter().map(|x| x.0))
            || b.extension
                .as_ref()
                .is_some_and(|e| e.children.len() >= ROW_LIMIT)
        {
            return None;
        }
        let (owner, source_layout) = if p == 0 {
            (r, usize::MAX)
        } else {
            let c = &b.extension.as_ref()?.children[r];
            (c.owner as usize, c.layout)
        };
        let key = (source_layout, plan.clone());
        let existing = b
            .extension
            .as_ref()
            .and_then(|e| e.layout_keys.get(&key))
            .copied();
        let projection = if existing.is_none() {
            let mut projection = Vec::with_capacity(plan.inputs.len());
            for (_, arg) in &plan.inputs {
                let s = match arg {
                    Arg::Literal(v) => Source::Literal(v.clone()),
                    Arg::Slot(s) => {
                        if p == 0
                            && let Ok(col) = b.slots.binary_search(s)
                        {
                            Source::Column(col)
                        } else if p != 0
                            && let e = b.extension.as_ref()?
                            && let l = &e.layouts[source_layout]
                            && let Ok(col) = l.plan.inputs.binary_search_by_key(s, |x| x.0)
                        {
                            l.projection[col].clone()
                        } else if plan.results.contains(s) {
                            Source::Result {
                                phase: phase - 1,
                                slot: *s,
                            }
                        } else {
                            return None;
                        }
                    }
                };
                projection.push(s);
            }
            Some(projection)
        } else {
            None
        };
        let projected = if let Some(idx) = existing {
            &b.extension.as_ref()?.layouts[idx].projection
        } else {
            projection.as_ref()?
        };
        for ((_, actual), src) in values.iter().zip(projected) {
            if canon(actual, parents)
                != canon(&self.source_value(i, owner, source.cell, src)?, parents)
            {
                return None;
            }
        }
        let (exports, reuse) = if existing.is_none()
            && let Some(program) = program
        {
            let mut exports = if p == 0 {
                b.ops
                    .iter()
                    .filter_map(|&op| {
                        fact_expression(program, b.template, op as usize, 1, |s| {
                            b.slots.binary_search(&s).ok().map(Source::Column)
                        })
                    })
                    .collect::<Vec<_>>()
            } else {
                b.extension.as_ref()?.layouts[source_layout].exports.clone()
            };
            let current =
                fact_expression(program, plan.consumer_template, plan.child_op, phase, |s| {
                    plan.inputs
                        .binary_search_by_key(&s, |x| x.0)
                        .ok()
                        .map(|i| projected[i].clone())
                });
            let reuse = current
                .as_ref()
                .and_then(|current| {
                    exports
                        .iter()
                        .rev()
                        .find(|f| f.table == current.table && f.key == current.key)
                })
                .cloned();
            if let Some(current) = current {
                exports.push(current);
            }
            (exports, reuse)
        } else {
            (vec![], None)
        };
        let rows = b.records.len();
        let e = self.slabs[i].extension.get_or_insert_with(|| Extension {
            layouts: vec![],
            layout_keys: Default::default(),
            children: vec![],
            facts: vec![],
            links: vec![0; rows],
        });
        let layout = existing.unwrap_or_else(|| {
            let n = e.layouts.len();
            e.layouts.push(Layout {
                plan,
                projection: projection.unwrap(),
                exports,
                reuse,
            });
            e.layout_keys.insert(key, n);
            n
        });
        Some(Prepared {
            source,
            slab: i,
            owner,
            layout,
            phase,
        })
    }
    pub fn attach_prepared(
        &mut self,
        id: u32,
        mut c: Cell,
        p: Prepared,
    ) -> std::result::Result<(), Cell> {
        let plan = &self.slabs[p.slab].extension.as_ref().unwrap().layouts[p.layout].plan;
        if id == u32::MAX
            || self.successor(p.source.cell).is_some()
            || c.template != plan.consumer_template
            || c.parent.is_some()
            || c.facts.len() != 1
            || !c.facts.contains_key(&(plan.child_op as u32))
        {
            return Err(c);
        }
        assert_eq!(self.directory.borrow().get(id), 0);
        let fact = c.facts.remove(&(plan.child_op as u32));
        let n = self.slabs[p.slab]
            .extension
            .as_ref()
            .unwrap()
            .children
            .len();
        self.set_successor(p.source.cell, id + 1);
        let e = self.slabs[p.slab].extension.as_mut().unwrap();
        self.stats.saved_parameters += e.layouts[p.layout].plan.inputs.len() as u64;
        e.children.push(Child {
            id: NonZeroU32::new(id + 1).unwrap(),
            depth: c.recipe_depth,
            born: c.started_epoch,
            owner: p.owner as u32,
            source: p.source.cell,
            successor: 0,
            layout: p.layout,
            phase: p.phase,
        });
        e.facts.push(fact);
        self.extensions.insert(p.slab);
        self.directory.borrow_mut().set(id, address(p.slab, n) | 1);
        self.live += 1;
        self.stats.installed += 1;
        self.stats.fused_children += 1;
        self.stats.max_phase = self.stats.max_phase.max(p.phase);
        *self.stats.by_phase.entry(p.phase).or_default() += 1;
        Ok(())
    }
    pub fn attach(
        &mut self,
        source: FactRef,
        id: u32,
        c: Cell,
        plan: Rc<two_run::Plan>,
        parents: &Parents,
        epoch: u64,
    ) -> std::result::Result<(), Cell> {
        if c.parent.is_some() {
            return Err(c);
        }
        let Some(p) = self.prepare(source, plan, &c.values, parents, epoch, 2) else {
            return Err(c);
        };
        self.attach_prepared(id, c, p)
    }
    /// Snapshot every descendant before removing an ancestor, then detach from
    /// the tail so a surviving suffix never keeps a dead root alive.
    pub fn detach_chain(&mut self, owner: u32) -> Vec<(u32, Cell)> {
        let mut ids = vec![];
        let mut next = self.successor(owner);
        while let Some(id) = next {
            ids.push(id);
            next = self.successor(id);
        }
        let mut frames = Vec::with_capacity(ids.len());
        for &id in &ids {
            let (i, r, _) = self.location(id).unwrap();
            let e = self.slabs[i].extension.as_ref().unwrap();
            let c = e.children[r];
            let l = &e.layouts[c.layout];
            let values = l
                .plan
                .inputs
                .iter()
                .enumerate()
                .map(|(col, (slot, _))| {
                    (
                        *slot,
                        self.source_value(i, c.owner as usize, c.source, &l.projection[col])
                            .expect("retained projection"),
                    )
                })
                .collect();
            frames.push((
                id,
                Cell {
                    template: l.plan.consumer_template,
                    values,
                    parent: None,
                    facts: Default::default(),
                    completed: true,
                    completed_prefix: 0,
                    recipe_depth: c.depth,
                    started_epoch: c.born,
                },
            ));
        }
        for (id, frame) in frames.iter_mut().rev() {
            let (i, r, _) = self.location(*id).unwrap();
            let e = self.slabs[i].extension.as_mut().unwrap();
            let op = e.layouts[e.children[r].layout].plan.child_op as u32;
            if let Some(f) = e.facts[r].take() {
                frame.facts.insert(op, f);
            }
            self.remove(*id);
            self.stats.removed -= 1;
            self.stats.detached_children += 1;
        }
        frames
    }
    #[cfg(test)]
    pub fn detach_child(&mut self, owner: u32) -> Option<(u32, Cell)> {
        let mut frames = self.detach_chain(owner);
        assert!(frames.len() <= 1);
        frames.pop()
    }
    pub fn phase_rows(&self) -> (usize, usize, usize) {
        let mut children = 0;
        let mut bytes = 0;
        for b in &self.slabs {
            if let Some(e) = &b.extension {
                children += e.children.len();
                bytes += e.children.capacity() * std::mem::size_of::<Child>()
                    + e.facts.capacity() * std::mem::size_of::<Option<FactState>>()
                    + e.links.capacity() * 4
                    + e.layouts.capacity() * std::mem::size_of::<Layout>()
                    + e.layouts
                        .iter()
                        .map(|l| l.projection.capacity() * std::mem::size_of::<Source>())
                        .sum::<usize>();
                bytes += e
                    .layouts
                    .iter()
                    .map(|l| {
                        l.exports.capacity() * std::mem::size_of::<PortFact>()
                            + l.exports
                                .iter()
                                .map(|f| f.key.capacity() * std::mem::size_of::<Source>())
                                .sum::<usize>()
                            + l.reuse
                                .as_ref()
                                .map_or(0, |f| f.key.capacity() * std::mem::size_of::<Source>())
                    })
                    .sum::<usize>();
            }
        }
        (
            self.slabs.iter().map(|b| b.records.len()).sum(),
            children,
            bytes,
        )
    }
    pub fn extra_fact_positions(&self) -> usize {
        self.slabs
            .iter()
            .filter_map(|b| b.extension.as_ref())
            .map(|e| e.facts.iter().filter(|f| f.is_some()).count())
            .sum()
    }
    pub(super) fn remove_child(&mut self, slab: usize, index: usize) {
        let removed = self.slabs[slab].extension.as_ref().unwrap().children[index];
        assert_eq!(removed.successor, 0, "detach descendants first");
        self.set_successor(removed.source, 0);
        let e = self.slabs[slab].extension.as_mut().unwrap();
        e.children.swap_remove(index);
        e.facts.swap_remove(index);
        if let Some(moved) = e.children.get(index) {
            self.directory
                .borrow_mut()
                .set(moved.id.get() - 1, address(slab, index) | 1);
        }
        if e.children.is_empty() {
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
