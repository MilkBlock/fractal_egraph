//! A single functional write has no constructor positions. Its committed
//! function row owns the immutable producer slots until it is superseded.
use super::*;
use crate::slots::Slots;
#[derive(Clone, Debug)]
pub(super) struct Writer {
    pub template: usize,
    pub parent: Option<FactRef>,
    #[cfg(not(feature = "function-blocks"))]
    pub values: Slots,
    #[cfg(feature = "function-blocks")]
    pub values: crate::function_blocks::Parameters,
    pub(super) epoch: u64,
    pub completed: bool,
    pub depth: u32,
    prefix: usize,
}
#[derive(Default)]
pub(super) struct Directory {
    #[cfg(feature = "function-blocks")]
    pub blocks_enabled: bool,
    #[cfg(feature = "function-blocks")]
    pub blocks_installed: u64,
    #[cfg(feature = "function-blocks")]
    pub blocks_fallback: u64,
    #[cfg(feature = "function-blocks")]
    pub blocks_promoted: u64,
    #[cfg(feature = "function-blocks")]
    pub blocks_slots: u64,
    #[cfg(feature = "function-blocks")]
    pub blocks_omitted: u64,
    #[cfg(feature = "function-blocks")]
    pub parent_links_avoided: u64,
    #[cfg(feature = "function-blocks")]
    pub wide_reuses: u64,
    #[cfg(feature = "function-blocks")]
    pub wide_allocations: u64,
    #[cfg(feature = "function-blocks")]
    pub blocks_reads: std::cell::Cell<u64>,
    #[cfg(feature = "function-blocks")]
    pub blocks_contexts: std::cell::Cell<u64>,
    #[cfg(feature = "function-blocks")]
    pub blocks_cross_run_contexts: std::cell::Cell<u64>,
    pages: Vec<Option<Box<Page>>>,
    pub live: usize,
    pub peak: usize,
    pub installations: u64,
    pub promoted: u64,
    pub general: u64,
}
struct Page {
    row: [u32; 512],
    count: usize,
}
impl Directory {
    pub fn get(&self, id: u32) -> Option<usize> {
        let page = self.pages.get(id as usize / 512)?.as_ref()?;
        page.row[id as usize % 512]
            .checked_sub(1)
            .map(|x| x as usize)
    }
    fn insert(&mut self, id: u32, index: u32) {
        let i = id as usize / 512;
        if self.pages.len() <= i {
            self.pages.resize_with(i + 1, || None);
        }
        let page = self.pages[i].get_or_insert_with(|| {
            Box::new(Page {
                row: [0; 512],
                count: 0,
            })
        });
        debug_assert_eq!(page.row[id as usize % 512], 0);
        page.row[id as usize % 512] = index + 1;
        page.count += 1;
        self.live += 1;
        self.peak = self.peak.max(self.live);
        self.installations += 1;
    }
    fn remove(&mut self, id: u32) {
        let p = &mut self.pages[id as usize / 512];
        let page = p.as_mut().unwrap();
        debug_assert_ne!(page.row[id as usize % 512], 0);
        page.row[id as usize % 512] = 0;
        page.count -= 1;
        if page.count == 0 {
            *p = None;
        }
        self.live -= 1;
    }
}
impl Graph {
    pub(super) fn writer(&self, id: u32) -> Option<&Writer> {
        self.functions
            .get(self.writer_dir.get(id)?)?
            .writer
            .as_deref()
    }
    pub(super) fn detach_origin(&mut self, index: usize) -> Option<Box<Writer>> {
        let previous = self.functions[index].origin.take()?;
        let Some(mut writer) = self.functions[index].writer.take() else {
            self.release_reference(previous);
            return None;
        };
        self.writer_dir.remove(previous.cell);
        if self.parent_users.get(&previous.cell).copied().unwrap_or(0) > 0 {
            // A client explicitly retained this producer as a parent. Its
            // original parent is already retained; transfer that hold.
            #[cfg(feature = "function-blocks")]
            let values = {
                self.writer_dir.blocks_promoted += u64::from(writer.values.is_packed());
                std::mem::take(&mut writer.values).unpack(
                    self.program
                        .function_blocks
                        .get(writer.template)
                        .and_then(Option::as_ref),
                )
            };
            #[cfg(not(feature = "function-blocks"))]
            let values = std::mem::take(&mut writer.values);
            let c = Cell {
                template: writer.template,
                parent: writer.parent.take(),
                values,
                facts: Default::default(),
                completed: writer.completed,
                completed_prefix: writer.prefix,
                recipe_depth: writer.depth,
                started_epoch: writer.epoch,
            };
            self.cells.insert(previous.cell, c);
            self.stats.cells_created += 1;
            self.writer_dir.promoted += 1;
        } else if let Some(p) = writer.parent {
            self.release_reference(p);
        }
        Some(writer)
    }
    pub(super) fn inline_writer(&mut self, index: usize, cell: u32, op: usize, frame: Cell) {
        let prior = self.detach_origin(index);
        self.install_writer(
            index,
            FactRef {
                cell,
                op: op as u32,
            },
            frame,
            prior,
        );
    }
    pub(super) fn install_writer(
        &mut self,
        index: usize,
        origin: FactRef,
        frame: Cell,
        prior: Option<Box<Writer>>,
    ) {
        #[cfg(feature = "function-blocks")]
        let mut prior = prior;
        if let Some(p) = frame.parent {
            self.retain_reference(p);
        }
        #[cfg(feature = "function-blocks")]
        let values = {
            let reuse = prior.as_mut().and_then(|w| w.values.take_wide());
            let reuse_len = reuse.as_ref().map_or(0, |v| v.len());
            let before = frame.values.len();
            let plan = self
                .program
                .function_blocks
                .get(frame.template)
                .and_then(Option::as_ref)
                .filter(|p| {
                    self.writer_dir.blocks_enabled
                        && frame.completed
                        && frame.facts.is_empty()
                        && p.op == origin.op as usize
                });
            let values = if plan.is_some() {
                crate::function_blocks::Parameters::pack(
                    frame.values,
                    plan,
                    &self.program.templates[frame.template],
                    frame.parent.is_some(),
                    reuse,
                )
            } else {
                crate::function_blocks::Parameters::Ordinary(frame.values)
            };
            if values.is_packed() {
                self.writer_dir.blocks_installed += 1;
                self.writer_dir.blocks_slots += values.stored_slots() as u64;
                self.writer_dir.blocks_omitted += (before - values.stored_slots()) as u64;
                if values.wide_len() > 0 {
                    if values.wide_len() == reuse_len {
                        self.writer_dir.wide_reuses += 1;
                    } else {
                        self.writer_dir.wide_allocations += 1;
                    }
                }
            } else if self.writer_dir.blocks_enabled {
                self.writer_dir.blocks_fallback += 1;
            }
            values
        };
        #[cfg(not(feature = "function-blocks"))]
        let values = frame.values;
        let state = Writer {
            template: frame.template,
            parent: frame.parent,
            values,
            epoch: frame.started_epoch,
            depth: frame.recipe_depth,
            completed: frame.completed,
            prefix: frame.completed_prefix,
        };
        let writer = if let Some(mut b) = prior {
            *b = state;
            b
        } else {
            Box::new(state)
        };
        self.writer_dir.insert(origin.cell, index as u32);
        let f = &mut self.functions[index];
        f.writer = Some(writer);
        f.origin = Some(origin);
    }
    #[cfg(feature = "function-blocks")]
    pub fn configure_function_blocks(&mut self, enabled: bool) {
        self.writer_dir.blocks_enabled = enabled;
    }
    #[cfg(feature = "function-blocks")]
    pub(super) fn own_function_block(&self, template: usize, values: &Slots) -> bool {
        if !self.writer_dir.blocks_enabled {
            return false;
        }
        let Some(plan) = self
            .program
            .function_blocks
            .get(template)
            .and_then(Option::as_ref)
        else {
            return false;
        };
        // Source tuple values already exist in the matched binding. Snapshot
        // only the statically demanded ports, instead of retaining its whole
        // constructor producer until the function version is replaced.
        if plan.slots.len() > 4 || !plan.execution_inputs.iter().all(|s| values.contains_key(s)) {
            return false;
        }
        // Local continuation projections can demand additional query inputs.
        if plan.slots.iter().any(|(s, _)| {
            !self.program.templates[template].defs.contains_key(s) && !values.contains_key(s)
        }) {
            return false;
        }
        true
    }
    #[cfg(feature = "function-blocks")]
    pub fn function_blocks_report(&self) -> J {
        use crate::function_blocks::Parameters;
        let mut live = 0usize;
        let mut slots = 0usize;
        let mut borrowed = 0usize;
        let mut ordinary_slots = 0usize;
        let mut heap_capacity = 0usize;
        let mut widths = BTreeMap::<usize, usize>::new();
        let mut wide_bytes = 0usize;
        let mut templates = BTreeMap::<usize, usize>::new();
        let mut kinds = BTreeMap::<String, usize>::new();
        for f in &self.functions {
            if let Some(w) = &f.writer
                && let Parameters::Ordinary(v) = &w.values
            {
                ordinary_slots += v.len();
                heap_capacity += v.capacity() * std::mem::size_of::<(usize, Value)>();
            }
            if let Some(w) = &f.writer
                && w.values.is_packed()
            {
                let p = self.program.function_blocks[w.template].as_ref().unwrap();
                live += 1;
                slots += w.values.stored_slots();
                borrowed += usize::from(w.parent.is_some());
                *widths.entry(w.values.stored_slots()).or_default() += 1;
                wide_bytes += w.values.wide_len() * 8;
                *templates.entry(w.template).or_default() += 1;
                let kind = self.merge_kinds[p.table]
                    .clone()
                    .unwrap_or_else(|| "unspecified".into());
                *kinds.entry(kind).or_default() += 1;
            }
        }
        json!({"enabled":self.writer_dir.blocks_enabled,
            "certified_templates":self.program.function_blocks.iter().filter(|x|x.is_some()).count(),
            "installed":self.writer_dir.blocks_installed,"fallback":self.writer_dir.blocks_fallback,
            "captured_slots":self.writer_dir.blocks_slots,"omitted_snapshot_slots":self.writer_dir.blocks_omitted,"promoted_for_retained_parent":self.writer_dir.blocks_promoted,
            "parent_links_avoided":self.writer_dir.parent_links_avoided,"wide_buffer_allocations":self.writer_dir.wide_allocations,"wide_buffer_reuses":self.writer_dir.wide_reuses,
            "slot_reads":self.writer_dir.blocks_reads.get(),"context_requests":self.writer_dir.blocks_contexts.get(),
            "earlier_run_context_requests":self.writer_dir.blocks_cross_run_contexts.get(),
            "live":live,"live_slots":slots,"borrowed_parent_blocks":borrowed,"live_widths":widths,"live_by_template":templates,"live_by_merge_contract":kinds,
            "ordinary_live_slots":ordinary_slots,"parameter_heap_capacity_bytes":heap_capacity+wide_bytes,"packed_port_heap_bytes":wide_bytes,
            "all_writer_headers_bytes":self.writer_dir.live*std::mem::size_of::<Writer>(),
            "ordinary_parameters_header_bytes":std::mem::size_of::<Slots>(),
            "parameters_header_bytes":std::mem::size_of::<Parameters>(),
            "ordinary_slot_bytes":std::mem::size_of::<(usize,Value)>()})
    }
    #[cfg(feature = "function-blocks")]
    pub(super) fn function_block_slot(&self, cell: u32, w: &Writer, i: usize) -> Result<Value> {
        use crate::function_blocks::Parameters;
        if let Parameters::Ordinary(values) = &w.values {
            return self.slot_fields(cell, w.template, w.parent, values, i);
        }
        self.writer_dir
            .blocks_reads
            .set(self.writer_dir.blocks_reads.get() + 1);
        if let Some(v) = w
            .values
            .get(self.program.function_blocks[w.template].as_ref(), i)
        {
            if self.sealed_input_cell == Some(cell) {
                self.sealed_reads.set(self.sealed_reads.get() + 1);
                return Ok(v);
            }
            return Ok(canon(&v, &self.parents));
        }
        if let Some(a) = self
            .program
            .known_results
            .static_slots
            .get(w.template)
            .and_then(|x| x.get(i))
            .and_then(Option::as_ref)
        {
            return self.argument(cell, a);
        }
        let t = &self.program.templates[w.template];
        if let (Some(parent), Some(&column)) = (w.parent, t.parent_columns.get(&i)) {
            return self.fact_column(parent, column);
        }
        if let Some(&op) = t.defs.get(&i)
            && let Op::Copy(_, a) = &t.ops[op]
        {
            return self.argument(cell, a);
        }
        Err("unready function block port".into())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "function-blocks")]
    #[test]
    fn function_ports_keep_proposals_across_updates_union_and_retirement() {
        let table = json!({"name":"attr","schema":[{"kind":"eq","name":"E"},{"kind":"i64","name":"i64"}],"constructor":false,"virtual_fact":false,"internal":false,"merge":{"var":"new"}});
        let mut p:Program=serde_json::from_value(json!({"multi_stage":crate::multistage::MultiStage::default(),"entry_schedule":crate::entry_schedule::Plan::default(),"tables":[table],"templates":[],"rules":[],"sets":{},"control":[],"initial":[],"table_templates":[],"artifact":{}})).unwrap();
        p.templates.push(Template {
            name: "writer".into(),
            sorts: vec![
                json!({"kind":"eq"}),
                json!({"kind":"i64"}),
                json!({"kind":"i64"}),
                json!({"kind":"i64"}),
                json!({"kind":"String"}),
            ],
            defs: [(2, 0)].into_iter().collect(),
            ops: std::rc::Rc::new(vec![
                Op::Copy(2, Arg::Slot(1)),
                Op::Set(0, vec![Arg::Slot(0)], Arg::Slot(2)),
            ]),
            parent_columns: Default::default(),
        });
        p.rules.push(Rule {
            constructor_only: false,
            template: 0,
            query: Default::default(),
            inputs: std::rc::Rc::new(vec![0, 1, 3, 4]),
            seminaive: true,
            direct: None,
        });
        // Slot 3 is not in the output tuple, but a later local continuation
        // needs this source query column. Slot 4 is dead after completion.
        p.multi_stage.routes.insert(
            (0, 1, 0, 0),
            std::rc::Rc::new(crate::multistage::Continuation {
                producer: 0,
                consumer: 0,
                anchor: 0,
                reads: vec![crate::multistage::LocalRead {
                    atom: 0,
                    table: 0,
                    columns: vec![Arg::Slot(3), Arg::Slot(1)],
                    effect: None,
                    assignments: vec![],
                }],
            }),
        );
        p.function_blocks = crate::function_blocks::derive(&p, &p.multi_stage);
        assert_eq!(p.function_blocks[0].as_ref().unwrap().omitted, vec![4]);
        let mut g = Graph::new(p).unwrap();
        g.configure_function_blocks(true);
        g.merge_programs_enabled = false;
        let key = [Value::Eq(20)];
        let frame = |v, e| Cell {
            template: 0,
            parent: None,
            values: [
                (0, Value::Eq(20)),
                (1, Value::Int(v)),
                (3, Value::Int(123)),
                (4, Value::Text(std::sync::Arc::new("unused".into()))),
            ]
            .into_iter()
            .collect(),
            facts: Default::default(),
            completed: true,
            completed_prefix: 2,
            recipe_depth: 3,
            started_epoch: e,
        };
        g.epoch = 1;
        let i = g.set_function_canonical(0, &key, Value::Int(9)).unwrap();
        g.inline_writer(i, 100, 1, frame(9, 1));
        assert_eq!(g.slot(100, 3).unwrap(), Value::Int(123));
        let old = FactRef { cell: 100, op: 1 };
        g.retain_reference(old);
        g.epoch = 2;
        g.set_function_canonical(0, &key, Value::Int(7)).unwrap();
        g.inline_writer(i, 101, 1, frame(7, 2));
        assert_eq!(g.slot(100, 2).unwrap(), Value::Int(9));
        assert_eq!(g.slot(100, 3).unwrap(), Value::Int(123));
        assert_eq!(g.slot(101, 2).unwrap(), Value::Int(7));
        assert_eq!(g.writer_dir.blocks_promoted, 1);
        g.union(10, 20).unwrap();
        g.repair().unwrap();
        assert_eq!(g.slot(101, 0).unwrap(), Value::Eq(10));
        // A rebuild/custom merge can update the row without making this RHS's
        // original proposal equal to that current value.
        g.set_function_canonical(0, &[Value::Eq(10)], Value::Int(3))
            .unwrap();
        assert_eq!(g.slot(101, 2).unwrap(), Value::Int(7));
        assert_eq!(g.function(i).unwrap().values.last(), Some(&Value::Int(3)));
        let current = FactRef { cell: 101, op: 1 };
        g.retain_reference(current);
        g.function_origin(i, None);
        assert_eq!(g.slot(101, 2).unwrap(), Value::Int(7));
        g.release_reference(old);
        g.release_reference(current);
        g.collect_garbage();
        assert!(g.slot(100, 1).is_err());
        assert!(g.slot(101, 1).is_err());
    }
    #[test]
    fn replacing_a_retained_writer_preserves_the_old_immutable_slots() {
        let table = serde_json::json!({"name":"attr","schema":["i64","i64"],"constructor":false,"virtual_fact":false,"internal":false,"merge":{"var":"new"}});
        let p=serde_json::from_value(serde_json::json!({"multi_stage":crate::multistage::MultiStage::default(),"entry_schedule":crate::entry_schedule::Plan::default(),"tables":[table],"templates":[],"rules":[],"sets":{},"control":[],"initial":[],"table_templates":[],"artifact":{}})).unwrap();
        let mut g = Graph::new(p).unwrap();
        let key = [Value::Int(3)];
        let i = g.set_function_canonical(0, &key, Value::Int(1)).unwrap();
        let frame = |n| Cell {
            values: [(0, Value::Int(n))].into_iter().collect(),
            parent: None,
            template: 0,
            facts: Default::default(),
            completed: true,
            completed_prefix: 3,
            recipe_depth: 2,
            started_epoch: 4,
        };
        let a = FactRef { cell: 10, op: 2 };
        g.inline_writer(i, 10, 2, frame(7));
        g.retain_reference(a);
        assert_eq!(
            g.continuation_context(Dependency::Function(i))
                .unwrap()
                .depth,
            2
        );
        g.inline_writer(i, 11, 2, frame(8));
        assert_eq!(g.slot(10, 0).unwrap(), Value::Int(7));
        assert_eq!(g.slot(11, 0).unwrap(), Value::Int(8));
        g.release_reference(a);
        g.collect_garbage();
        assert!(g.slot(10, 0).is_err());
        assert_eq!(g.writer_dir.live, 1);
    }
}
