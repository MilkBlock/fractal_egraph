//! Immutable producer parameters and mutable fact ports in shared-layout slabs.
//! Logical Cell/FactRef IDs are unchanged; no ordinary Cell record is installed.
use super::*;
use std::cell::Cell as Counter;
const ROW_BITS: u32 = 22;
const ROW_LIMIT: usize = 1 << ROW_BITS;
const SLAB_LIMIT: usize = 1 << (31 - ROW_BITS);
fn address(slab: usize, row: usize) -> u32 {
    arena::PORT_TAG | ((slab as u32) << ROW_BITS) | row as u32
}

pub(crate) fn derive(p: &Program, known: &crate::known_results::Plans) -> Vec<Option<Vec<u32>>> {
    let mut result = vec![None; p.templates.len()];
    let mut seen = vec![false; p.templates.len()];
    for (rid, r) in p.rules.iter().enumerate() {
        let t = &p.templates[r.template];
        let mut ops = vec![];
        let mut valid = true;
        for (i, op) in t.ops.iter().enumerate() {
            if known.static_skip.get(rid).is_some_and(|v| v[i]) {
                continue;
            }
            match op {
                Op::Copy(..) | Op::Union(..) => (),
                Op::Let(_, Call::Table(id), _) | Op::Set(id, _, _)
                    if p.tables[*id].constructor && p.tables[*id].virtual_fact =>
                {
                    ops.push(i as u32)
                }
                _ => valid = false,
            }
        }
        let plan = (valid && !ops.is_empty() && ops.len() <= 4).then_some(ops);
        if !seen[r.template] {
            result[r.template] = plan;
            seen[r.template] = true;
        } else if result[r.template] != plan {
            result[r.template] = None;
        }
    }
    result
}

#[derive(Default, Serialize)]
pub(super) struct Counts {
    pub installed: u64,
    pub removed: u64,
    pub fallback: u64,
}
struct Record {
    id: u32,
    depth: u32,
    born: u64,
}
struct Slab {
    template: usize,
    slots: Vec<usize>,
    ops: Vec<u32>,
    records: Vec<Record>,
    values: Vec<Value>,
    facts: Vec<Option<FactState>>,
    parents: Option<Vec<FactRef>>,
}
#[derive(Default)]
pub(super) struct Store {
    pub enabled: bool,
    slabs: Vec<Slab>,
    by_template: Vec<Vec<usize>>,
    directory: arena::SharedDirectory,
    live: usize,
    pub stats: Counts,
    contexts: Counter<u64>,
    cross_run_contexts: Counter<u64>,
}
impl Store {
    pub fn len(&self) -> usize {
        self.live
    }
    pub fn new(directory: arena::SharedDirectory) -> Self {
        Self {
            directory,
            ..Default::default()
        }
    }
    fn location(&self, id: u32) -> Option<(usize, usize)> {
        let word = self.directory.borrow().get(id);
        (word & arena::PORT_TAG != 0).then_some((
            ((word & !arena::PORT_TAG) >> ROW_BITS) as usize,
            (word as usize) & (ROW_LIMIT - 1),
        ))
    }
    fn locate(&self, id: u32) -> Option<(&Slab, usize)> {
        let (b, r) = self.location(id)?;
        Some((&self.slabs[b], r))
    }
    pub fn template(&self, id: u32) -> Option<usize> {
        Some(self.locate(id)?.0.template)
    }
    #[cfg(test)]
    pub fn value(&self, id: u32, slot: usize) -> Option<&Value> {
        let (b, r) = self.locate(id)?;
        Some(&b.values[r * b.slots.len() + b.slots.binary_search(&slot).ok()?])
    }
    pub fn parent(&self, id: u32) -> Option<FactRef> {
        let (b, r) = self.locate(id)?;
        Some(b.parents.as_ref()?[r])
    }
    pub fn read_slot(
        &self,
        id: u32,
        slot: usize,
    ) -> Option<(usize, Option<FactRef>, Option<&Value>)> {
        let (b, r) = self.locate(id)?;
        Some((
            b.template,
            b.parents.as_ref().map(|p| p[r]),
            b.slots
                .binary_search(&slot)
                .ok()
                .map(|col| &b.values[r * b.slots.len() + col]),
        ))
    }
    pub fn state(&self, f: FactRef) -> Option<&FactState> {
        let (b, r) = self.locate(f.cell)?;
        b.facts[r * b.ops.len() + b.ops.binary_search(&f.op).ok()?].as_ref()
    }
    pub fn state_mut(&mut self, f: FactRef) -> Option<&mut FactState> {
        let (b, r) = self.location(f.cell)?;
        let b = &mut self.slabs[b];
        let i = r * b.ops.len() + b.ops.binary_search(&f.op).ok()?;
        b.facts[i].as_mut()
    }
    pub fn take_state(&mut self, f: FactRef) -> Option<FactState> {
        let (b, r) = self.location(f.cell)?;
        let b = &mut self.slabs[b];
        let i = r * b.ops.len() + b.ops.binary_search(&f.op).ok()?;
        b.facts[i].take()
    }
    pub fn empty(&self, id: u32) -> bool {
        self.locate(id).is_some_and(|(b, r)| {
            b.facts[r * b.ops.len()..(r + 1) * b.ops.len()]
                .iter()
                .all(Option::is_none)
        })
    }
    pub fn insert(&mut self, id: u32, c: Cell) -> std::result::Result<(), Cell> {
        let t = c.template;
        if self.by_template.len() <= t {
            self.by_template.resize_with(t + 1, Vec::new);
        }
        let found = self.by_template[t].iter().copied().find(|&i| {
            let b = &self.slabs[i];
            b.parents.is_some() == c.parent.is_some()
                && b.slots.iter().eq(c.values.keys())
                && b.ops.iter().eq(c.facts.keys())
        });
        if found.map_or(self.slabs.len() >= SLAB_LIMIT, |i| {
            self.slabs[i].records.len() >= ROW_LIMIT
        }) {
            return Err(c);
        }
        assert_eq!(self.directory.borrow().get(id), 0);
        let i = found.unwrap_or_else(|| {
            let i = self.slabs.len();
            self.slabs.push(Slab {
                template: t,
                slots: c.values.keys().copied().collect(),
                ops: c.facts.keys().copied().collect(),
                records: vec![],
                values: vec![],
                facts: vec![],
                parents: c.parent.map(|_| vec![]),
            });
            self.by_template[t].push(i);
            i
        });
        let b = &mut self.slabs[i];
        let r = b.records.len();
        if let Some(parents) = &mut b.parents {
            parents.push(c.parent.unwrap());
        }
        b.records.push(Record {
            id,
            depth: c.recipe_depth,
            born: c.started_epoch,
        });
        b.values.extend(c.values.into_values());
        b.facts.extend(c.facts.into_values().map(Some));
        self.directory.borrow_mut().set(id, address(i, r));
        self.live += 1;
        self.stats.installed += 1;
        Ok(())
    }
    pub fn remove(&mut self, id: u32) -> bool {
        let Some((i, r)) = self.location(id) else {
            return false;
        };
        self.directory.borrow_mut().set(id, 0);
        self.live -= 1;
        let b = &mut self.slabs[i];
        let last = b.records.len() - 1;
        for col in 0..b.slots.len() {
            b.values
                .swap(r * b.slots.len() + col, last * b.slots.len() + col);
        }
        for col in 0..b.ops.len() {
            b.facts
                .swap(r * b.ops.len() + col, last * b.ops.len() + col);
        }
        b.values.truncate(last * b.slots.len());
        b.facts.truncate(last * b.ops.len());
        b.records.swap_remove(r);
        if let Some(parents) = &mut b.parents {
            parents.swap_remove(r);
        }
        if r < last {
            self.directory
                .borrow_mut()
                .set(b.records[r].id, address(i, r));
        }
        self.stats.removed += 1;
        true
    }
    pub fn context(&self, f: FactRef, epoch: u64) -> Option<producer::Context> {
        let (b, r) = self.locate(f.cell)?;
        b.facts[r * b.ops.len() + b.ops.binary_search(&f.op).ok()?].as_ref()?;
        let row = &b.records[r];
        self.contexts.set(self.contexts.get() + 1);
        self.cross_run_contexts
            .set(self.cross_run_contexts.get() + u64::from(row.born < epoch));
        Some(producer::Context {
            completed: true,
            cell: f.cell,
            template: b.template,
            depth: row.depth,
            op: f.op as usize,
        })
    }
    pub fn report(&self) -> J {
        json!({"enabled":self.enabled,"live_records":self.live,"slabs":self.slabs.len(),
            "stats":self.stats,"context_requests":self.contexts.get(),"cross_run_context_requests":self.cross_run_contexts.get(),
            "parameter_values":self.slabs.iter().map(|b|b.values.len()).sum::<usize>(),
            "fact_positions":self.slabs.iter().map(|b|b.facts.iter().filter(|s|s.is_some()).count()).sum::<usize>(),
            "slab_capacity_bytes":self.slabs.iter().map(|b|b.values.capacity()*std::mem::size_of::<Value>()+b.facts.capacity()*std::mem::size_of::<Option<FactState>>()+b.records.capacity()*std::mem::size_of::<Record>()+b.parents.as_ref().map_or(0,|v|v.capacity()*std::mem::size_of::<FactRef>())).sum::<usize>(),
            "borrowed_parent_records":self.slabs.iter().map(|b|b.parents.as_ref().map_or(0,Vec::len)).sum::<usize>(),
            "directory_kind":"shared-tagged-cell-directory","max_slabs":SLAB_LIMIT,"max_rows_per_slab":ROW_LIMIT,
            "directory_capacity":self.directory.borrow().capacity()})
    }
}

impl Graph {
    pub fn configure_port_frames(&mut self, enabled: bool) {
        self.port_frames.enabled = enabled;
    }
    pub fn port_frames_report(&self) -> J {
        let mut report = self.port_frames.report();
        report["full_cells"] = json!(self.cells.len());
        report["embedded_writers"] = json!(self.writer_dir.live);
        report["all_producer_records"] =
            json!(self.cells.len() + self.writer_dir.live + self.port_frames.len());
        report
    }
    pub(super) fn port_eligible(&self, t: usize) -> bool {
        self.port_frames.enabled && self.program.port_frames.get(t).is_some_and(Option::is_some)
    }
    pub(super) fn port_birth(&self, f: FactRef) -> bool {
        self.active_rhs.as_ref().is_some_and(|(id, c)| {
            *id == f.cell
                && self.port_eligible(c.template)
                && self.program.port_frames[c.template]
                    .as_ref()
                    .unwrap()
                    .contains(&f.op)
        })
    }
    pub(super) fn seal_port_frame(&mut self, id: u32) -> bool {
        let Some((current, c)) = &self.active_rhs else {
            return false;
        };
        if *current != id || c.facts.is_empty() {
            return false;
        }
        let allowed = c.completed
            && c.completed_prefix == self.program.templates[c.template].ops.len()
            && self.port_eligible(c.template)
            && c.facts.iter().all(|(op, s)| {
                s.entry.is_none()
                    && self.program.port_frames[c.template]
                        .as_ref()
                        .unwrap()
                        .contains(op)
            });
        let (_, frame) = self.active_rhs.take().unwrap();
        if allowed {
            let parent = frame.parent;
            match self.port_frames.insert(id, frame) {
                Ok(()) => {
                    if let Some(parent) = parent {
                        self.retain_reference(parent);
                    }
                }
                Err(frame) => {
                    self.port_frames.stats.fallback += 1;
                    self.store_registered_cell(id, frame);
                }
            }
        } else {
            self.port_frames.stats.fallback += 1;
            self.store_registered_cell(id, frame);
        }
        true
    }
    pub(super) fn port_slot(
        &self,
        id: u32,
        t: usize,
        parent: Option<FactRef>,
        slot: usize,
    ) -> Result<Value> {
        if let Some(a) = self
            .program
            .known_results
            .static_slots
            .get(t)
            .and_then(|v| v.get(slot))
            .and_then(Option::as_ref)
        {
            return self.argument(id, a);
        }
        let template = &self.program.templates[t];
        if let (Some(parent), Some(&col)) = (parent, template.parent_columns.get(&slot)) {
            return self.fact_column(parent, col);
        }
        match &template.ops[*template.defs.get(&slot).ok_or("unready port slot")?] {
            Op::Copy(_, a) => self.argument(id, a),
            Op::Let(_, Call::Table(t), _) if self.program.tables[*t].constructor => Ok(Value::Eq(
                root(&self.parents, ((id as u64) << 32) | (slot as u64 + 1)),
            )),
            _ => Err("unready port slot".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(value: i64) -> Cell {
        Cell {
            template: 0,
            values: [(0, Value::Int(value))].into_iter().collect(),
            parent: None,
            facts: [(
                2,
                FactState {
                    #[cfg(feature = "receipt-slots")]
                    port_slot: 0,
                    decoded: None,
                    entry: None,
                    hidden: false,
                    epoch: 1,
                    born: 1,
                    hash: 0,
                },
            )]
            .into_iter()
            .collect(),
            started_epoch: 1,
            completed: true,
            completed_prefix: 3,
            recipe_depth: 2,
        }
    }
    #[test]
    fn slab_relocation_and_retained_parameters_preserve_logical_ids() {
        let mut s = Store::default();
        s.insert(10, frame(7)).unwrap();
        s.insert(20, frame(8)).unwrap();
        s.insert(30, frame(9)).unwrap();
        assert_eq!(s.value(20, 0), Some(&Value::Int(8)));
        assert!(s.remove(10));
        assert_eq!(s.value(30, 0), Some(&Value::Int(9)));
        let f = FactRef { cell: 20, op: 2 };
        s.state_mut(f).unwrap().hidden = true;
        assert!(s.state(f).unwrap().hidden);
        s.take_state(f);
        assert!(s.empty(20));
        assert_eq!(s.value(20, 0), Some(&Value::Int(8)));
        assert!(s.context(f, 2).is_none());
        assert_eq!(s.context(FactRef { cell: 30, op: 2 }, 2).unwrap().depth, 2);
    }
    #[test]
    fn tagged_directory_keeps_full_cells_and_ports_distinct() {
        let mut cells = arena::Cells::default();
        let mut ports = Store::new(cells.directory());
        cells.insert(1, frame(1));
        ports.insert(2, frame(2)).unwrap();
        cells.insert(3, frame(3));
        ports.insert(u32::MAX, frame(4)).unwrap();
        assert!(cells.get(&2).is_none());
        assert!(ports.template(1).is_none());
        cells.remove(&1);
        assert_eq!(cells.get(&3).unwrap().values[&0], Value::Int(3));
        assert_eq!(ports.value(2, 0), Some(&Value::Int(2)));
        ports.remove(2);
        assert_eq!(ports.value(u32::MAX, 0), Some(&Value::Int(4)));
        assert!(cells.get(&3).is_some());
    }
    #[test]
    fn layout_budget_rejects_without_installing_a_partial_record() {
        let mut s = Store::default();
        for i in 0..SLAB_LIMIT {
            let mut c = frame(i as i64);
            c.template = i;
            s.insert(i as u32, c).unwrap();
        }
        let mut c = frame(99);
        c.template = SLAB_LIMIT;
        let returned = s.insert(SLAB_LIMIT as u32, c).unwrap_err();
        assert_eq!(returned.values[&0], Value::Int(99));
        assert_eq!(s.live, SLAB_LIMIT);
        assert!(s.template(SLAB_LIMIT as u32).is_none());
    }
    #[test]
    fn a_retained_port_keeps_its_parameters_after_fact_retirement() {
        let p=serde_json::from_value(json!({"multi_stage":crate::multistage::MultiStage::default(),"entry_schedule":crate::entry_schedule::Plan::default(),"tables":[],"templates":[],"rules":[],"sets":{},"control":[],"initial":[],"table_templates":[],"artifact":{}})).unwrap();
        let mut g = Graph::new(p).unwrap();
        let f = FactRef { cell: 10, op: 2 };
        g.port_frames.insert(10, frame(7)).unwrap();
        g.retain_reference(f);
        g.port_frames.take_state(f);
        g.garbage.insert(10);
        g.collect_garbage();
        assert_eq!(g.slot(10, 0).unwrap(), Value::Int(7));
        g.release_reference(f);
        g.collect_garbage();
        assert!(g.slot(10, 0).is_err());
    }
    #[test]
    fn child_port_reads_and_releases_a_retired_parent_port() {
        let mut p:Program=serde_json::from_value(json!({"multi_stage":crate::multistage::MultiStage::default(),"entry_schedule":crate::entry_schedule::Plan::default(),"tables":[],"templates":[],"rules":[],"sets":{},"control":[],"initial":[],"table_templates":[],"artifact":{}})).unwrap();
        let template = |parent| Template {
            name: "port".into(),
            sorts: vec![],
            defs: [(1, 2)].into_iter().collect(),
            ops: std::rc::Rc::new(vec![
                Op::Copy(2, Arg::Literal(Value::Int(0))),
                Op::Copy(3, Arg::Literal(Value::Int(1))),
                Op::Let(1, Call::Table(0), vec![Arg::Slot(0)]),
            ]),
            parent_columns: if parent {
                [(0, 0)].into_iter().collect()
            } else {
                Default::default()
            },
        };
        p.templates = vec![template(false), template(true)];
        let mut g = Graph::new(p).unwrap();
        let parent = FactRef { cell: 10, op: 2 };
        let child = FactRef { cell: 20, op: 2 };
        g.port_frames.insert(10, frame(7)).unwrap();
        let mut c = frame(0);
        c.template = 1;
        c.parent = Some(parent);
        c.values = Default::default();
        g.retain_reference(parent);
        g.port_frames.insert(20, c).unwrap();
        assert_eq!(g.slot(20, 0).unwrap(), Value::Int(7));
        g.port_frames.take_state(parent);
        g.garbage.insert(10);
        g.collect_garbage();
        assert_eq!(g.slot(20, 0).unwrap(), Value::Int(7));
        g.port_frames.take_state(child);
        g.garbage.insert(20);
        g.collect_garbage();
        assert!(g.slot(20, 0).is_err());
        assert!(g.slot(10, 0).is_err());
    }
}
