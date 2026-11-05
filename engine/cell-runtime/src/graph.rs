//! Rows are temporary views of shared recipes, never a constructor row store.
use crate::program::*;
use crate::value::{self, Parents, Value, canon, exported, root};
use rustc_hash::FxHashMap as HashMap;
use serde::{Deserialize, Serialize};
use serde_json::{Value as J, json};
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};

#[cfg(feature = "sparse-cells")]
pub type RowFields = std::rc::Rc<[Value]>;
#[cfg(not(feature = "sparse-cells"))]
pub type RowFields = std::rc::Rc<Vec<Value>>;
pub(crate) fn row_fields(v: Vec<Value>) -> RowFields {
    #[cfg(feature = "sparse-cells")]
    {
        v.into()
    }
    #[cfg(not(feature = "sparse-cells"))]
    {
        std::rc::Rc::new(v)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct FactRef {
    pub cell: u32,
    pub op: u32,
}
/// Copied fields only need source-owner identities for containment candidates.
/// Provider projection additionally needs atom/operation coordinates.

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FactState {
    #[cfg(feature = "receipt-slots")]
    #[serde(skip)]
    port_slot: u32,
    // Canonicalized on read, like an explicit Unit-function key. Reconstructed
    // from the checked recipe on checkpoint restore; never trusted serialized data.

    // Snapshot of the ordinary recipe's fields. The union subscriber clears
    // it before any access to a changed representative. Never checkpoint it.
    #[serde(skip)]
    decoded: Option<(usize, RowFields)>,
    #[serde(default)]
    pub entry: Option<FactRef>,
    pub hidden: bool,
    pub epoch: u64,
    pub born: u64,
    pub hash: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Cell {
    #[serde(default)]
    pub completed_prefix: usize,
    #[serde(default)]
    pub started_epoch: u64,
    #[serde(default)]
    pub completed: bool,

    pub template: usize,
    pub parent: Option<FactRef>,
    pub values: crate::slots::Slots,
    pub facts: crate::slots::FrameMap<u32, FactState>,
    #[serde(default)]
    pub recipe_depth: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Function {
    #[cfg(feature = "sparse-cells")]
    #[serde(skip)]
    writer: Option<Box<writer_frames::Writer>>,
    #[cfg(feature = "receipt-slots")]
    #[serde(skip)]
    port_slot: u32,
    #[serde(skip)]
    decoded: Option<RowFields>,
    pub table: usize,
    pub key: Vec<Value>,
    pub value: Value,
    pub epoch: u64,
    pub live: bool,
    #[serde(default)]
    pub origin: Option<FactRef>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Dependency {
    Fact(FactRef),
    Function(usize),
}
#[cfg(not(feature = "sparse-cells"))]
type UseSet = BTreeSet<Dependency>;

#[cfg(feature = "sparse-cells")]
#[path = "reverse_uses.rs"]
mod reverse_uses;
#[cfg(feature = "sparse-cells")]
use reverse_uses::UseSet;
#[derive(Clone, Debug)]
pub struct Row {
    pub table: usize,
    pub values: RowFields,
    pub hidden: bool,
    pub epoch: u64,
    pub witness: Dependency,
}
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct Stats {
    #[serde(default)]
    pub merge_program_calls: u64,
    #[serde(default)]
    pub scalar_merge_calls: u64,
    #[serde(default)]
    pub scalar_unchanged: u64,
    #[serde(default)]
    pub scalar_old_projection: u64,
    #[serde(default)]
    pub merge_program_operations: u64,
    #[serde(default)]
    pub merge_program_constructs: u64,
    pub constructor_requests: u64,
    pub constructor_reused: u64,
    pub logical_facts_created: u64,
    pub function_updates: u64,
    pub merge_calls: u64,
    #[serde(default)]
    pub key_operations: u64,
    #[serde(default)]
    pub merge_port_reads: u64,
    #[serde(default)]
    pub merge_contract_calls: BTreeMap<String, u64>,
    pub unions: u64,
    pub repaired_dependencies: u64,
    #[serde(default)]
    pub decoded_rows: u64,
    pub residual_candidates: u64,
    pub bindings: u64,
    #[serde(default)]
    pub handoff_attempts: u64,
    #[serde(default)]
    pub handoff_missing: u64,
    #[serde(default)]
    pub handoff_stale: u64,
    #[serde(default)]
    pub handoff_lets: u64,
    #[serde(default)]
    pub handoff_sets: u64,
    #[serde(default)]
    pub handoff_noop_unions: u64,
    #[serde(default)]
    pub handoff_deep: u64,
    #[serde(default)]
    pub handoff_closed: u64,
    #[serde(default)]
    pub handoff_closed_fallback: u64,
    #[serde(default)]
    pub register_seeds: u64,
    #[serde(default)]
    pub register_answers: u64,
    #[serde(default)]
    pub single_writes: u64,
    #[serde(default)]
    pub transient_writes: u64,
    pub direct_bindings: u64,
    #[serde(default)]
    pub direct_candidates: u64,
    pub cells_created: u64,
    #[serde(default)]
    pub rhs_frames_avoided: u64,
    pub empty_frames_released: u64,
    pub native_table_reads: u64,
    pub native_table_writes: u64,
    pub native_free_join_calls: u64,
    #[serde(default)]
    pub delivery_events: u64,
    #[serde(default)]
    pub distinct_delivery_events: u64,
    #[serde(default)]
    pub union_requests: u64,
    #[serde(default)]
    pub repair_key_hash_changes: u64,
    #[serde(default)]
    pub repair_collisions: u64,
}

#[cfg(feature = "sparse-cells")]
#[path = "cell_arena.rs"]
pub mod arena;
#[cfg(feature = "sparse-cells")]
#[path = "scalar_keys.rs"]
mod scalar_keys;
/// All lanes share the same Run barrier. The last three are timing controls.

#[cfg(feature = "sparse-cells")]
#[path = "writer_frames.rs"]
mod writer_frames;
pub struct Graph {
    #[cfg(feature = "sparse-cells")]
    visibility_tainted: Vec<bool>,
    #[cfg(feature = "sparse-cells")]
    active_origin: Option<usize>,
    #[cfg(feature = "sparse-cells")]
    active_rhs: Option<(u32, Cell)>,
    #[cfg(feature = "sparse-cells")]
    writer_dir: writer_frames::Directory,
    #[cfg(feature = "keeper-proof")]
    pub(crate) proof: crate::prefix_proof::Proof,
    #[cfg(feature = "rhs-work-audit")]
    pub(crate) rhs_births: crate::rhs_work::Births,
    units: unit_ports::Units,
    pub known_stats: crate::known_results::Stats,
    pub merge_programs_enabled: bool,
    scalar_merge: bool,
    merge_kinds: Vec<Option<String>>,
    equal_keeps_old: Vec<bool>,
    pub key_lookup_calls: std::cell::Cell<u64>,
    pub program: Program,

    #[cfg(not(feature = "sparse-cells"))]
    pub cells: rustc_hash::FxHashMap<u32, Cell>,
    #[cfg(feature = "sparse-cells")]
    pub cells: arena::Cells,
    pub functions: Vec<Function>,
    pub parents: Parents,
    pub epoch: u64,
    pub changed: bool,
    pub stats: Stats,
    pub pending_unions: Vec<(u64, u64)>,

    pub(crate) changes: crate::change_batch::ChangeBatch,
    pub anchors: Vec<(String, String, u64)>,
    pub next_cell: u32,
    pub query_versions: Vec<u64>,
    // Transient commit-boundary receipts, never part of a checkpoint.
    #[cfg(feature = "receipt-slots")]
    pub(crate) retired_port_slots: HashMap<Dependency, u32>,
    pub receipt_tracking: bool,
    pub receipt_forwards: HashMap<Dependency, Dependency>,

    operators: Vec<BTreeSet<FactRef>>,
    function_operators: Vec<BTreeSet<usize>>,
    #[cfg(not(feature = "sparse-cells"))]
    intern: HashMap<u64, BTreeSet<FactRef>>,
    #[cfg(feature = "sparse-cells")]
    intern: HashMap<u64, crate::tiny_set::TinySet<FactRef>>,
    #[cfg(not(feature = "sparse-cells"))]
    function_keys: Vec<HashMap<Vec<Value>, usize>>,
    #[cfg(feature = "sparse-cells")]
    function_keys: Vec<scalar_keys::FunctionKeys>,
    uses: rustc_hash::FxHashMap<u64, UseSet>,
    #[cfg(not(feature = "sparse-cells"))]
    dependencies: HashMap<Dependency, BTreeSet<u64>>,
    #[cfg(feature = "sparse-cells")]
    dependencies: HashMap<Dependency, smallvec::SmallVec<[u64; 3]>>,
    dirty: BTreeSet<Dependency>,
    dirty_markers: BTreeSet<FactRef>,
    dirty_facts: Vec<BTreeSet<FactRef>>,
    deletion_epochs: Vec<u64>,
    parent_users: rustc_hash::FxHashMap<u32, usize>,
    garbage: BTreeSet<u32>,
}
#[cfg(feature = "rhs-locals")]
#[path = "local_rhs_exec.rs"]
pub(crate) mod local_rhs_exec;
#[path = "unit_ports.rs"]
mod unit_ports;
fn hashkey(table: usize, key: &[Value]) -> u64 {
    let mut h = rustc_hash::FxHasher::default();
    table.hash(&mut h);
    key.hash(&mut h);
    h.finish()
}
impl Graph {
    #[cfg(feature = "receipt-slots")]
    pub(crate) fn current_port_slot(&self, w: Dependency) -> Option<u32> {
        match w {
            Dependency::Fact(f) => self
                .cells
                .get(&f.cell)?
                .facts
                .get(&f.op)
                .map(|f| f.port_slot),
            Dependency::Function(i) => self.functions.get(i).map(|f| f.port_slot),
        }
    }
    #[cfg(feature = "receipt-slots")]
    pub(crate) fn previous_port_slot(&self, w: Dependency) -> u32 {
        let id = self.current_port_slot(w).unwrap_or(0);
        if id == 0 && !self.retired_port_slots.is_empty() {
            self.retired_port_slots.get(&w).copied().unwrap_or(0)
        } else {
            id
        }
    }
    #[cfg(feature = "receipt-slots")]
    pub(crate) fn set_port_slot(&mut self, w: Dependency, id: u32) {
        match w {
            Dependency::Fact(f) => {
                self.cells
                    .get_mut(&f.cell)
                    .unwrap()
                    .facts
                    .get_mut(&f.op)
                    .unwrap()
                    .port_slot = id
            }
            Dependency::Function(i) => self.functions[i].port_slot = id,
        }
    }
    #[cfg(feature = "storage-metrics")]
    pub fn storage_sample(&self) -> J {
        let mut arity = [0usize; 5];
        for c in self.cells.values() {
            arity[c.facts.len().min(4)] += 1;
        }
        let mut buckets = [0usize; 4];
        for set in self.intern.values() {
            buckets[set.len().min(3)] += 1;
        }
        #[cfg(feature = "sparse-cells")]
        let writers = json!({"live":self.writer_dir.live,"peak":self.writer_dir.peak,"installed":self.writer_dir.installations,"promoted":self.writer_dir.promoted,"from_general_rhs":self.writer_dir.general});
        #[cfg(not(feature = "sparse-cells"))]
        let writers = J::Null;
        json!({"embedded_writers":writers,"full_cells":self.cells.len(),"cell_fact_arity_0_1_2_3_4plus":arity,"intern_sizes_0_1_2_3plus":buckets,"intern_headers":self.intern.len(),"forward_dependency_headers":self.dependencies.len(),"reverse_dependency_headers":self.uses.len(),"dependency_edges":self.dependencies.values().map(|v|v.len()).sum::<usize>(),"live_functions":self.functions.iter().filter(|f|f.live).count(),"function_slots":self.functions.len(),"cell_id_high_water":self.next_cell})
    }
    /// Follow only storage relocation and proved same-key coalescence in this
    /// commit. Equal result classes alone never establish a row receipt.
    /// A receipt's key was checked by its query/action. Keys only change by
    /// canonicalization; receipt forwarding preserves that same-key relation.
    /// Check liveness/visibility here without decoding the recipe again.
    pub fn receipt_visible(&self, w: Dependency, table: usize) -> bool {
        match w {
            Dependency::Fact(f) => {
                self.live_table(w) == Some(table)
                    && self
                        .cells
                        .get(&f.cell)
                        .and_then(|c| c.facts.get(&f.op))
                        .is_some_and(|s| !s.hidden)
            }
            Dependency::Function(i) => self
                .functions
                .get(i)
                .is_some_and(|f| f.live && f.table == table),
        }
    }
    pub fn resolve_receipt(&self, mut w: Dependency) -> Dependency {
        loop {
            if let Some(next) = self.receipt_forwards.get(&w) {
                w = *next;
            } else {
                return w;
            }
        }
    }

    pub fn new(program: Program) -> Result<Self> {
        let n = program.tables.len();
        let initial = program.initial.clone();

        let merge_kinds = program
            .tables
            .iter()
            .enumerate()
            .map(|(t, _)| {
                program.artifact["merge_contracts"]["tables"][t]["kind"]
                    .as_str()
                    .map(str::to_owned)
            })
            .collect();
        let equal_keeps_old = program
            .tables
            .iter()
            .enumerate()
            .map(|(t, v)| {
                if program.artifact["merge_contracts"].is_null() {
                    v.merge["call"]["table"].is_string()
                } else {
                    program.artifact["merge_contracts"]["tables"][t]["equal_inputs"] == "return_old"
                }
            })
            .collect();
        #[cfg(feature = "sparse-cells")]
        let typed_keys = program
            .tables
            .iter()
            .map(scalar_keys::FunctionKeys::new)
            .collect();
        let mut g = Self {
            #[cfg(feature = "sparse-cells")]
            visibility_tainted: vec![false; n],
            #[cfg(feature = "sparse-cells")]
            active_origin: None,
            #[cfg(feature = "sparse-cells")]
            active_rhs: None,
            #[cfg(feature = "sparse-cells")]
            writer_dir: Default::default(),
            #[cfg(feature = "rhs-work-audit")]
            rhs_births: Default::default(),
            #[cfg(feature = "keeper-proof")]
            proof: Default::default(),
            units: unit_ports::Units::new(&program),
            merge_kinds,
            equal_keeps_old,

            key_lookup_calls: std::cell::Cell::new(0),

            program,
            cells: Default::default(),
            functions: vec![],
            parents: Default::default(),
            epoch: 0,
            changed: false,
            stats: Stats::default(),

            known_stats: Default::default(),
            scalar_merge: cfg!(feature = "sparse-cells"),
            merge_programs_enabled: true,
            pending_unions: vec![],

            changes: crate::change_batch::ChangeBatch::new(n),
            anchors: vec![],
            next_cell: 1,
            query_versions: vec![0; n],
            receipt_tracking: false,
            #[cfg(feature = "receipt-slots")]
            retired_port_slots: HashMap::default(),
            receipt_forwards: HashMap::default(),

            operators: vec![BTreeSet::new(); n],
            function_operators: vec![BTreeSet::new(); n],
            intern: HashMap::default(),
            #[cfg(not(feature = "sparse-cells"))]
            function_keys: vec![Default::default(); n],
            #[cfg(feature = "sparse-cells")]
            function_keys: typed_keys,
            uses: Default::default(),
            dependencies: Default::default(),
            dirty: BTreeSet::new(),
            dirty_markers: BTreeSet::new(),
            dirty_facts: vec![BTreeSet::new(); n],
            deletion_epochs: vec![0; n],
            parent_users: Default::default(),
            garbage: BTreeSet::new(),
        };
        let mut ids = BTreeMap::new();
        for r in &initial {
            let table = g
                .program
                .tables
                .iter()
                .position(|t| r["table"] == t.name)
                .ok_or("unknown initial operator")?;
            let vals = array(&r["values"])?
                .iter()
                .map(|v| Self::import(v, &mut ids))
                .collect::<Result<Vec<_>>>()?;
            if vals.len() != g.program.tables[table].schema.len() {
                return Err("initial arity".into());
            }
            let key = vals[..vals.len() - 1].to_vec();
            let val = vals.last().unwrap().clone();
            if g.program.tables[table].virtual_fact {
                let template = g.program.table_templates[table];
                let cell = g.frame(template, (0..vals.len()).zip(vals).collect())?;
                g.activate(
                    FactRef { cell, op: 0 },
                    table,
                    &key,
                    val,
                    r["subsumed"].as_bool().unwrap_or(false),
                )?;
            } else {
                g.set_function(table, key, val)?;
            }
        }
        g.anchors = ids
            .into_iter()
            .map(|((sort, old), id)| (sort, old, id))
            .collect();
        g.repair()?;
        g.changed = false;
        Ok(g)
    }
    fn import(j: &J, ids: &mut BTreeMap<(String, String), u64>) -> Result<Value> {
        if j.get("key").is_some() {
            return Ok(Value::Key(eggfuse_structural_key::Key::from_wire(
                serde_json::from_value(j["key"].clone()).map_err(|e| e.to_string())?,
            )?));
        }
        if let Some(eq) = j["eq"].as_str() {
            let next = ids.len() as u64 + 1;
            if next >= 1u64 << 32 {
                return Err("initial identity capacity".into());
            }
            return Ok(Value::Eq(
                *ids.entry((string(&j["sort"])?.into(), eq.into()))
                    .or_insert(next),
            ));
        }
        if let Some(xs) = j["elements"].as_array() {
            let mut xs = xs
                .iter()
                .map(|x| Self::import(x, ids))
                .collect::<Result<Vec<_>>>()?;
            xs.sort();
            xs.dedup();
            return Ok(Value::Set(xs.into()));
        }
        Ok(match string(&j["sort"])? {
            "String" => Value::Text(string(&j["value"])?.to_owned().into()),
            "f64" => Value::Float(j["value"].as_i64().ok_or("f64 bits")?),
            _ => Value::Int(
                j["value"]
                    .as_i64()
                    .or_else(|| j["value"].as_str().and_then(|s| s.parse().ok()))
                    .ok_or("initial scalar")?,
            ),
        })
    }
    fn reserve_cell_id(&mut self) -> Result<u32> {
        let id = self.next_cell;
        self.next_cell = id.checked_add(1).ok_or("Cell identity exhausted")?;
        Ok(id)
    }
    fn new_cell(
        &self,
        template: usize,
        mut values: crate::slots::Slots,
        parent: Option<FactRef>,
    ) -> Cell {
        let columns = &self.program.templates[template].parent_columns;
        let parent = parent.filter(|_| !columns.is_empty());
        // The owned control retains the very same parents, with a copy of
        // each projected parameter available in the existing owned slot map.
        if parent.is_some() {
            for slot in columns.keys() {
                values.remove(slot);
            }
        }

        Cell {
            template,
            values,
            parent,

            completed_prefix: 0,
            started_epoch: self.epoch,
            completed: false,

            facts: Default::default(),
            recipe_depth: 0,
        }
    }
    fn store_cell(&mut self, id: u32, cell: Cell) {
        if let Some(parent) = cell.parent {
            self.retain_reference(parent);
        }
        self.cells.insert(id, cell);
        self.stats.cells_created += 1;
    }
    pub fn frame(&mut self, template: usize, values: BTreeMap<usize, Value>) -> Result<u32> {
        self.linked_frame(template, values, None)
    }
    pub fn linked_frame(
        &mut self,
        template: usize,
        values: BTreeMap<usize, Value>,
        parent: Option<FactRef>,
    ) -> Result<u32> {
        self.linked_slots(template, values.into_iter().collect(), parent)
    }
    pub(crate) fn linked_slots(
        &mut self,
        template: usize,
        values: crate::slots::Slots,
        parent: Option<FactRef>,
    ) -> Result<u32> {
        let id = self.reserve_cell_id()?;
        let frame = self.new_cell(template, values, parent);
        self.store_cell(id, frame);
        Ok(id)
    }
    /// Run the read-only prefix against live canonical constructor rows. Keep
    /// the ID sequence even on a full hit since later eclass IDs encode it.
    pub(crate) fn start_constructor_rhs(
        &mut self,
        template: usize,
        values: crate::slots::Slots,
        parent: Option<FactRef>,
        aliases: Option<&[bool]>,
        #[cfg(feature = "structural-depth")] mut receipts: Option<&mut [Option<Dependency>]>,
    ) -> Result<Option<(u32, usize)>> {
        let id = self.reserve_cell_id()?;
        let mut frame = self.new_cell(template, values, parent);
        let ops = self.program.templates[template].ops.clone();
        for (op, instruction) in ops.iter().enumerate() {
            // slot_in resolves the validated alias in either the temporary
            // frame or the same frame after its first constructor miss.
            if self.known_stats.elide_static(aliases, op) {
                self.known_stats.lazy_prefix_eliminated += 1;
                continue;
            }
            let (dst, table, args) = match instruction {
                Op::Copy(..) => continue, // slot_in resolves copy definitions on demand
                Op::Let(dst, Call::Table(t), args) => (*dst, *t, args),
                _ => return Err("unexpected operation in constructor-only RHS".into()),
            };
            let key = args
                .iter()
                .map(|a| self.argument_in(id, &frame, a))
                .collect::<Result<Vec<_>>>()?;
            self.stats.constructor_requests += 1;
            if let Some(row) = self.lookup_canonical(table, &key)? {
                self.stats.constructor_reused += 1;
                #[cfg(feature = "structural-depth")]
                if let Some(ref mut rs) = receipts {
                    rs[op] = Some(row.witness);
                }
                frame.values.insert(dst, row.values.last().unwrap().clone());
            } else {
                self.store_cell(id, frame);
                self.insert_constructor(id, op, table, &key, dst)?;
                #[cfg(feature = "structural-depth")]
                if let Some(ref mut rs) = receipts {
                    rs[op] = Some(Dependency::Fact(FactRef {
                        cell: id,
                        op: op as u32,
                    }));
                }
                return Ok(Some((id, op + 1)));
            }
        }
        self.stats.rhs_frames_avoided += 1;
        Ok(None)
    }
    pub(crate) fn apply_write(
        &mut self,
        template: usize,
        values: crate::slots::Slots,
        parent: Option<FactRef>,
        write: &crate::row_kernel::Write,
    ) -> Result<(u32, usize, Vec<Value>, bool)> {
        let id = self.reserve_cell_id()?;
        self.stats.single_writes += 1;
        let before = self.stats.function_updates;
        #[cfg(feature="sparse-cells")]
        if parent.is_none() && let Some(ports)=&write.ports {
            let (key,value,dst)=ports.project(&values,&self.parents)?;
            let i=self.set_function_canonical(write.table,std::slice::from_ref(&key),Value::Int(value))?;
            let changed=self.stats.function_updates!=before;
            if changed{
                let mut frame=self.new_cell(template,values,None);
                if let Some(dst)=dst{frame.values.insert(dst,Value::Int(value));}
                self.keep_writer(i,id,write.op,frame);
            }else{self.stats.rhs_frames_avoided+=1;self.stats.transient_writes+=1;}
            return Ok((id,i,vec![key],changed));
        }
        let mut frame = self.new_cell(template, values, parent);
        for (dst, name, sort, args) in &write.prefix {
            let args = args
                .iter()
                .map(|a| self.argument_in(id, &frame, a))
                .collect::<Result<smallvec::SmallVec<[Value; 4]>>>()?;
            let v = value::primitive(name, &args, sort, self.program.key_nodes())?
                .ok_or_else(|| format!("RHS primitive failed: {name}"))?;
            frame.values.insert(*dst, v);
        }
        let key = write
            .key
            .iter()
            .map(|a| self.argument_in(id, &frame, a))
            .collect::<Result<Vec<_>>>()?;
        let value = self.argument_in(id, &frame, &write.value)?;
        let i=self.set_function_canonical(write.table,&key,value)?;
        let changed=self.stats.function_updates!=before;
        if changed {
            self.keep_writer(i,id,write.op,frame);
        } else {
            self.stats.rhs_frames_avoided += 1;
            self.stats.transient_writes += 1;
        }
        Ok((id, i, key, changed))
    }
    fn keep_writer(&mut self,i:usize,id:u32,op:usize,mut frame:Cell){
        frame.completed=true;
        frame.completed_prefix=op+1;
        #[cfg(feature="sparse-cells")]
        if i<u32::MAX as usize {self.inline_writer(i,id,op,frame);return;}
        self.store_cell(id,frame);
        self.function_origin(i,Some(FactRef{cell:id,op:op as u32}));
    }
    pub fn argument(&self, cell: u32, a: &Arg) -> Result<Value> {
        match a {
            Arg::Literal(v) => Ok(canon(v, &self.parents)),
            Arg::Slot(i) => self.slot(cell, *i),
        }
    }
    pub fn slot(&self, cell: u32, i: usize) -> Result<Value> {
        if let Some(c) = self.rhs_frame(cell) {
            return self.slot_in(cell, c, i);
        }
        #[cfg(feature = "sparse-cells")]
        if let Some(w) = self.writer(cell) {
            return self.slot_fields(cell, w.template, w.parent, &w.values, i);
        }
        Err("expired Cell reference".into())
    }
    #[inline]
    pub(crate) fn rhs_frame(&self, id: u32) -> Option<&Cell> {
        #[cfg(feature = "sparse-cells")]
        if let Some((current, c)) = &self.active_rhs
            && *current == id
        {
            return Some(c);
        }
        self.cells.get(&id)
    }
    #[inline]
    pub(crate) fn rhs_frame_mut(&mut self, id: u32) -> Option<&mut Cell> {
        #[cfg(feature = "sparse-cells")]
        if let Some((current, c)) = &mut self.active_rhs
            && *current == id
        {
            return Some(c);
        }
        self.cells.get_mut(&id)
    }
    #[cfg(feature = "sparse-cells")]
    pub(crate) fn begin_transient(
        &mut self,
        template: usize,
        values: crate::slots::Slots,
        parent: Option<FactRef>,
    ) -> Result<u32> {
        debug_assert!(self.active_rhs.is_none());
        #[cfg(feature = "sparse-cells")]
        {
            self.active_origin = None;
        }
        let id = self.reserve_cell_id()?;
        self.active_rhs = Some((id, self.new_cell(template, values, parent)));
        Ok(id)
    }
    #[cfg(feature = "sparse-cells")]
    pub(crate) fn promote_active(&mut self) {
        if let Some((id, c)) = self.active_rhs.take() {
            self.store_cell(id, c);
        }
    }
    #[cfg(feature = "sparse-cells")]
    fn promote_owner(&mut self, id: u32) {
        if self
            .active_rhs
            .as_ref()
            .is_some_and(|(active, _)| *active == id)
        {
            self.promote_active();
        }
    }
    fn argument_in(&self, cell: u32, c: &Cell, a: &Arg) -> Result<Value> {
        match a {
            Arg::Literal(v) => Ok(canon(v, &self.parents)),
            Arg::Slot(i) => self.slot_in(cell, c, *i),
        }
    }
    fn slot_in(&self, cell: u32, c: &Cell, i: usize) -> Result<Value> {
        self.slot_fields(cell, c.template, c.parent, &c.values, i)
    }
    fn slot_fields(
        &self,
        cell: u32,
        template: usize,
        parent: Option<FactRef>,
        values: &crate::slots::Slots,
        i: usize,
    ) -> Result<Value> {
        if let Some(v) = values.get(&i) {
            return Ok(canon(v, &self.parents));
        }
        let arg = |a: &Arg| match a {
            Arg::Literal(v) => Ok(canon(v, &self.parents)),
            Arg::Slot(s) => self.slot_fields(cell, template, parent, values, *s),
        };
        if let Some(a) = self
            .program
            .known_results
            .static_slots
            .get(template)
            .and_then(|s| s.get(i))
            .and_then(Option::as_ref)
        {
            return arg(a);
        }
        let t = &self.program.templates[template];
        if let (Some(p), Some(&col)) = (parent, t.parent_columns.get(&i)) {
            return self.fact_column(p, col);
        }
        let op = &t.ops[*t.defs.get(&i).ok_or("unready slot")?];
        match op {
            Op::Copy(_, a) => arg(a),
            Op::Let(_, Call::Table(t), _) if self.program.tables[*t].virtual_fact => {
                Ok(if self.program.tables[*t].constructor {
                    Value::Eq(root(&self.parents, ((cell as u64) << 32) | (i as u64 + 1)))
                } else {
                    Value::Int(0)
                })
            }
            _ => Err("unready computed/function slot".into()),
        }
    }
    fn fact_column(&self, f: FactRef, col: usize) -> Result<Value> {
        // Recipe parameters are immutable even after this fact is hidden/deleted.
        // Derived Cells can keep their binding without copying the parent's fields.
        let template = self.cells.get(&f.cell).map(|c| c.template);
        #[cfg(feature = "sparse-cells")]
        let template = template.or_else(|| self.writer(f.cell).map(|w| w.template));
        match self.program.templates[template.ok_or("expired parent Cell")?]
            .ops
            .get(f.op as usize)
            .ok_or("invalid retained fact position")?
        {
            Op::Let(dst, Call::Table(_), args) => {
                if col == args.len() {
                    self.slot(f.cell, *dst)
                } else {
                    self.argument(f.cell, args.get(col).ok_or("parent column")?)
                }
            }
            Op::Set(_, args, v) => self.argument(
                f.cell,
                if col == args.len() {
                    v
                } else {
                    args.get(col).ok_or("parent column")?
                },
            ),
            _ => Err("invalid parent fact".into()),
        }
    }
    pub fn arguments(&self, cell: u32, aa: &[Arg]) -> Result<Vec<Value>> {
        aa.iter().map(|a| self.argument(cell, a)).collect()
    }
    pub fn fact(&self, f: FactRef) -> Result<Option<Row>> {
        let Some(c) = self.cells.get(&f.cell) else {
            return Ok(None);
        };
        let Some(s) = c.facts.get(&f.op) else {
            return Ok(None);
        };
        if let Some((table, values)) = &s.decoded {
            return Ok(Some(Row {
                table: *table,
                values: values.clone(),
                hidden: s.hidden,
                epoch: s.epoch,
                witness: Dependency::Fact(f),
            }));
        }

        let arguments = |aa: &[Arg]| {
            aa.iter()
                .map(|a| self.argument_in(f.cell, c, a))
                .collect::<Result<Vec<_>>>()
        };
        let (table, values) = {
            match self.program.templates[c.template]
                .ops
                .get(f.op as usize)
                .ok_or("invalid live fact position")?
            {
                Op::Let(dst, Call::Table(t), aa) => {
                    let mut vals = arguments(aa)?;
                    vals.push(self.slot_in(f.cell, c, *dst)?);
                    (*t, vals)
                }
                Op::Set(t, aa, v) => {
                    let mut vals = arguments(aa)?;
                    vals.push(self.argument_in(f.cell, c, v)?);
                    (*t, vals)
                }
                _ => return Err("invalid fact recipe position".into()),
            }
        };
        // Every argument/slot path already returns a canonical value, including
        // recursive provider projections and literal/container arguments.

        Ok(Some(Row {
            table,
            values: row_fields(values),
            hidden: s.hidden,
            epoch: s.epoch,
            witness: Dependency::Fact(f),
        }))
    }
    fn shared_fields(key: &[Value], v: &Value) -> RowFields {
        #[cfg(feature = "sparse-cells")]
        {
            key.iter()
                .cloned()
                .chain(std::iter::once(v.clone()))
                .collect()
        }
        #[cfg(not(feature = "sparse-cells"))]
        {
            let mut fields = Vec::with_capacity(key.len() + 1);
            fields.extend_from_slice(key);
            fields.push(v.clone());
            std::rc::Rc::new(fields)
        }
    }
    fn function_index(&self, t: usize, key: &[Value]) -> Option<usize> {
        self.function_keys[t]
            .get(key)
            .copied()
            .or_else(|| self.units.lookup(t, key))
    }
    #[cfg(feature = "view-consumers")]
    pub(crate) fn published_view(&self, i: usize) -> Option<crate::view_plan::Tuple> {
        let f = self.functions.get(i).filter(|f| f.live)?;
        if let Some(fields) = &f.decoded {
            return Some(crate::view_plan::Tuple {
                witness: Dependency::Function(i),
                epoch: f.epoch,
                fields: fields.clone(),
                columns: None,
            });
        }
        self.units
            .payload(i, f.epoch, &self.parents)
            .or_else(|| self.function(i).map(|r| crate::view_plan::Tuple::flat(&r)))
    }
    #[cfg(feature = "view-storage")]
    pub(crate) fn has_view_plan(&self, t: usize) -> bool {
        self.units.owned[t]
    }
    pub(crate) fn unit_stats(&self) -> serde_json::Value {
        self.units.report()
    }
    pub fn function(&self, i: usize) -> Option<Row> {
        let f = self.functions.get(i)?;
        if !f.live {
            return None;
        }
        let values = f
            .decoded
            .clone()
            .or_else(|| {
                if f.key.is_empty() && self.units.counts[f.table] > 0 {
                    self.units.values(i, &self.parents, &f.value)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| {
                let mut values = Vec::with_capacity(f.key.len() + 1);
                values.extend(f.key.iter().map(|v| canon(v, &self.parents)));
                values.push(canon(&f.value, &self.parents));
                row_fields(values)
            });

        Some(Row {
            table: f.table,
            values,
            hidden: false,
            epoch: f.epoch,
            witness: Dependency::Function(i),
        })
    }
    pub fn row(&self, w: Dependency) -> Result<Option<Row>> {
        match w {
            Dependency::Fact(f) => self.fact(f),
            Dependency::Function(i) => Ok(self.function(i)),
        }
    }
    pub(crate) fn visible_epoch(&self, w: Dependency) -> Option<u64> {
        match w {
            Dependency::Fact(f) => self
                .cells
                .get(&f.cell)?
                .facts
                .get(&f.op)
                .filter(|s| !s.hidden)
                .map(|s| s.epoch),
            Dependency::Function(i) => self.functions.get(i).filter(|f| f.live).map(|f| f.epoch),
        }
    }
    /// Delivery only needs liveness and the table, not the decoded binding.
    pub fn live_table(&self, w: Dependency) -> Option<usize> {
        match w {
            Dependency::Function(i) => self.functions.get(i).filter(|f| f.live).map(|f| f.table),
            Dependency::Fact(f) => {
                let c = self.cells.get(&f.cell)?;
                c.facts.get(&f.op)?;

                match self.program.templates[c.template].ops.get(f.op as usize)? {
                    Op::Let(_, Call::Table(t), _) | Op::Set(t, _, _) => Some(*t),
                    _ => None,
                }
            }
        }
    }
    pub fn origin(&self, w: Dependency) -> Option<FactRef> {
        match w {
            Dependency::Fact(f) => Some(f),
            Dependency::Function(i) => self
                .functions
                .get(i)
                .filter(|f| f.live)
                .and_then(|f| f.origin),
        }
    }
    fn function_origin(&mut self, index: usize, origin: Option<FactRef>) {
        if self.functions[index].origin == origin {
            return;
        }
        #[cfg(feature = "sparse-cells")]
        {
            self.detach_origin(index);
        }
        #[cfg(not(feature = "sparse-cells"))]
        if let Some(old) = self.functions[index].origin.take() {
            *self
                .parent_users
                .get_mut(&old.cell)
                .expect("function producer reference") -= 1;
            self.garbage.insert(old.cell);
        }
        if let Some(new) = origin {
            *self.parent_users.entry(new.cell).or_default() += 1;
        }
        self.functions[index].origin = origin;
    }
    pub(crate) fn dependency_table(&self, w: Dependency) -> Option<usize> {
        match w {
            Dependency::Function(i) => self.functions.get(i).map(|f| f.table),
            Dependency::Fact(f) => {
                let c = self.cells.get(&f.cell)?;

                match self.program.templates[c.template].ops.get(f.op as usize)? {
                    Op::Let(_, Call::Table(t), _) | Op::Set(t, _, _) => Some(*t),
                    _ => None,
                }
            }
        }
    }
    pub fn witnesses(&self, table: usize) -> Vec<Dependency> {
        if self.program.tables[table].virtual_fact {
            self.operators[table]
                .iter()
                .copied()
                .map(Dependency::Fact)
                .collect()
        } else {
            self.function_operators[table]
                .iter()
                .copied()
                .map(Dependency::Function)
                .collect()
        }
    }
    /// A query index contains references, never copies of virtual constructor rows.
    /// `uses` already follows unions for rebuild; a bound identity can also narrow
    /// a query. It is a superset (any column, including container members), so the
    /// matcher must still check all columns and visibility on the decoded row.
    pub fn witness_count(&self, table: usize) -> usize {
        if self.program.tables[table].virtual_fact {
            self.operators[table].len()
        } else {
            self.function_operators[table].len()
        }
    }
    fn bound_uses(&self, bound: &[Option<Value>]) -> Option<&UseSet> {
        #[cfg(feature = "sparse-cells")]
        static EMPTY: UseSet = UseSet::Empty;
        #[cfg(not(feature = "sparse-cells"))]
        static EMPTY: UseSet = UseSet::new();
        bound
            .iter()
            .filter_map(|v| match v {
                Some(Value::Eq(id)) => {
                    Some(self.uses.get(&root(&self.parents, *id)).unwrap_or(&EMPTY))
                }
                _ => None,
            })
            .min_by_key(|uses| uses.len())
    }
    pub fn candidate_count(&self, table: usize, bound: &[Option<Value>]) -> usize {
        let all = self.witness_count(table);
        self.bound_uses(bound)
            .map_or(all, |uses| all.min(uses.len()))
    }
    pub fn candidates(&self, table: usize, bound: &[Option<Value>]) -> Vec<Dependency> {
        if let Some(uses) = self.bound_uses(bound) {
            if uses.len() < self.witness_count(table) {
                let rows: Vec<_> = uses
                    .iter()
                    .copied()
                    .filter(|w| match w {
                        Dependency::Fact(f) => {
                            self.cells.get(&f.cell).is_some_and(|c| {
                                match &self.program.templates[c.template].ops[f.op as usize] {
                                    Op::Let(_, Call::Table(t), _) | Op::Set(t, _, _) => *t == table,
                                    _ => false,
                                }
                            })
                        }
                        Dependency::Function(i) => {
                            self.functions[*i].live && self.functions[*i].table == table
                        }
                    })
                    .collect();
                #[cfg(feature = "sparse-cells")]
                let rows = {
                    let mut sorted = rows;
                    sorted.sort_unstable();
                    sorted
                };
                return rows;
            }
        }
        self.witnesses(table)
    }
    pub fn lookup(&self, table: usize, key: &[Value]) -> Result<Option<Row>> {
        let key = key
            .iter()
            .map(|v| canon(v, &self.parents))
            .collect::<Vec<_>>();
        self.lookup_canonical(table, &key)
    }
    pub(crate) fn lookup_canonical(&self, table: usize, key: &[Value]) -> Result<Option<Row>> {
        #[cfg(feature = "family-overlap")]
        let clock = crate::family_overlap::clock();
        let result = self.lookup_canonical_inner(table, key);
        #[cfg(feature = "rhs-work-audit")]
        if let Ok(Some(r)) = &result {
            self.rhs_births.read(r.witness);
        }
        #[cfg(feature = "family-overlap")]
        crate::family_overlap::physical(table, 0, clock, || {
            key.iter().cloned().enumerate().collect()
        });
        result
    }
    #[inline]
    fn lookup_canonical_inner(&self, table: usize, key: &[Value]) -> Result<Option<Row>> {
        self.key_lookup_calls.set(self.key_lookup_calls.get() + 1);
        if !self.program.tables[table].virtual_fact {
            return Ok(self
                .function_index(table, key)
                .and_then(|i| self.function(i)));
        }
        let h = hashkey(table, key);
        if self.dirty_facts[table].is_empty() {
            // Matching observes a repaired snapshot. Avoid allocating/copying
            // a set on every keyed read; retain the original FactRef order.
            for &f in self.intern.get(&h).into_iter().flatten() {
                if let Some(r) = self.fact(f)? {
                    if r.table == table && r.values[..key.len()] == *key {
                        return Ok(Some(r));
                    }
                }
            }
            return Ok(None);
        }
        #[cfg(not(feature = "sparse-cells"))]
        let mut refs = self.intern.get(&h).cloned().unwrap_or_default();
        #[cfg(feature = "sparse-cells")]
        let mut refs: BTreeSet<_> = self.intern.get(&h).into_iter().flatten().copied().collect();
        // A merge callback may request a constructor whose old key is dirty. Include
        // only affected references until repair catches up, not all graph rows.
        refs.extend(self.dirty_facts[table].iter().copied());
        for f in refs {
            if let Some(r) = self.fact(f)? {
                if r.table == table && r.values[..key.len()] == *key {
                    return Ok(Some(r));
                }
            }
        }
        Ok(None)
    }
    #[cfg(not(feature = "sparse-cells"))]
    fn refs(v: &Value, out: &mut BTreeSet<u64>) {
        match v {
            Value::Eq(i) => {
                out.insert(*i);
            }
            Value::Set(xs) => {
                for x in xs.iter() {
                    Self::refs(x, out)
                }
            }
            _ => {}
        }
    }
    #[cfg(not(feature = "sparse-cells"))]
    fn subscribe(&mut self, d: Dependency, values: &[Value]) {
        if let Some(table) = self.dependency_table(d) {
            self.query_versions[table] += 1;
        }
        let mut refs = BTreeSet::new();
        for v in values {
            Self::refs(v, &mut refs);
        }
        if refs.iter().any(|v| self.parents.contains_key(v)) {
            refs = refs.into_iter().map(|id| root(&self.parents, id)).collect();
        }
        if self.dependencies.get(&d) == Some(&refs) {
            return;
        }
        let old = self.dependencies.remove(&d).unwrap_or_default();
        for id in old {
            let id = root(&self.parents, id);
            if !refs.contains(&id)
                && let Some(uses) = self.uses.get_mut(&id)
            {
                uses.remove(&d);
            }
        }
        for &id in &refs {
            self.uses.entry(id).or_default().insert(d);
        }
        self.dependencies.insert(d, refs);
    }
    #[cfg(feature = "sparse-cells")]
    fn subscribe(&mut self, d: Dependency, values: &[Value]) {
        type Refs = smallvec::SmallVec<[u64; 3]>;
        fn collect(v: &Value, p: &Parents, out: &mut Refs) {
            match v {
                Value::Eq(i) => out.push(root(p, *i)),
                Value::Set(xs) => {
                    for x in xs.iter() {
                        collect(x, p, out);
                    }
                }
                _ => (),
            }
        }
        if let Some(t) = self.dependency_table(d) {
            self.query_versions[t] += 1
        }
        let mut refs = Refs::new();
        for v in values {
            collect(v, &self.parents, &mut refs);
        }
        refs.sort_unstable();
        refs.dedup();
        if self.dependencies.get(&d).is_some_and(|old| *old == refs) {
            return;
        }
        let mut old = self.dependencies.remove(&d).unwrap_or_default();
        for id in &mut old {
            *id = root(&self.parents, *id);
        }
        for id in &old {
            if !refs.contains(id)
                && let Some(bucket) = self.uses.get_mut(id)
            {
                bucket.remove(&d);
                if bucket.is_empty() {
                    self.uses.remove(id);
                }
            }
        }
        for &id in &refs {
            if !old.contains(&id) {
                self.uses.entry(id).or_default().insert(d);
            }
        }
        if !refs.is_empty() {
            self.dependencies.insert(d, refs);
        }
    }
    fn activate(
        &mut self,
        f: FactRef,
        t: usize,
        key: &[Value],
        v: Value,
        hidden: bool,
    ) -> Result<()> {
        #[cfg(feature = "keeper-proof")]
        if let Some(pair) = self.proof.expects(f) {
            let old = self.fact(pair.keeper)?.ok_or("keeper missing at birth")?;
            if old.table != t || old.hidden {
                return Err("keeper table or visibility differs".into());
            }
            self.proof.at_birth(
                pair,
                self.epoch,
                key,
                &old.values[..old.values.len() - 1],
                &self.parents,
                &self.pending_unions,
            )?;
        }
        #[cfg(feature = "sparse-cells")]
        if hidden {
            self.visibility_tainted[t] = true;
        }
        let h = hashkey(t, key);
        let vs = Self::shared_fields(key, &v);
        #[cfg(feature = "sparse-cells")]
        self.promote_owner(f.cell);
        let facts = &mut self.cells.get_mut(&f.cell).unwrap().facts;
        #[cfg(feature = "sparse-cells")]
        facts.reserve_first();
        facts.insert(
            f.op,
            FactState {
                #[cfg(feature = "receipt-slots")]
                port_slot: 0,
                decoded: Some((t, vs.clone())),

                entry: None,
                hidden,
                epoch: self.epoch,
                born: self.epoch,
                hash: h,
            },
        );
        self.operators[t].insert(f);
        self.intern.entry(h).or_default().insert(f);
        self.subscribe(Dependency::Fact(f), &vs);
        self.changes.touch(t, Dependency::Fact(f));
        self.stats.logical_facts_created += 1;

        self.changed = true;
        Ok(())
    }
    /// Preflight the complete switched RHS on the sealed tuple. Reserve an ID
    /// and enqueue unions only after every row and its full canonical key pass.
    #[cfg(feature = "iterative-bake")]
    pub(crate) fn finish_handoff(
        &mut self,
        template: usize,
        inputs: &crate::slots::Slots,
        transfers: &[crate::result_handoff::Input],
    ) -> Result<Option<(u32, Vec<Option<Vec<Value>>>, Vec<Option<Dependency>>, u64)>> {
        let ops = self.program.templates[template].ops.clone();
        let mut receipts = vec![None; ops.len()];
        let mut keys = vec![None; ops.len()];
        let mut suffix = crate::slots::Slots::default();
        let mut unions = vec![];
        let arg = |a: &Arg, s: &crate::slots::Slots| -> Result<Value> {
            match a {
                Arg::Slot(i) => s
                    .get(i)
                    .or_else(|| inputs.get(i))
                    .map(|v| canon(v, &self.parents))
                    .ok_or_else(|| "unbound completed slot".into()),
                Arg::Literal(v) => Ok(canon(v, &self.parents)),
            }
        };
        let mut transfers_iter = transfers.iter();
        let mut lets = 0;
        let mut sets = 0;
        let mut deep = 0;
        let mut noop = 0;
        for (op, instruction) in ops.iter().enumerate() {
            let (table, aa) = match instruction {
                Op::Copy(dst, a) => {
                    let v = arg(a, &suffix)?;
                    suffix.insert(*dst, v);
                    continue;
                }
                #[cfg(feature = "structural-depth")]
                Op::Union(a, b) => {
                    unions.push((arg(a, &suffix)?, arg(b, &suffix)?));
                    continue;
                }
                Op::Let(_, Call::Table(t), aa) | Op::Set(t, aa, _) => (*t, aa),
                _ => return Err("unexpected instruction in completed block".into()),
            };
            let Some(input) = transfers_iter.next() else {
                return Ok(None);
            };
            let Some(f) = input.fact else { return Ok(None) };
            if input.op != op {
                return Err("invalid completed block order".into());
            }
            let key = aa
                .iter()
                .map(|a| arg(a, &suffix))
                .collect::<Result<Vec<_>>>()?;
            let Some(row) = self.live_handoff(f, table)? else {
                return Ok(None);
            };
            let Some((value, fields)) = row.values.split_last() else {
                return Ok(None);
            };
            if fields != key {
                return Ok(None);
            }
            match instruction {
                Op::Let(dst, ..) => {
                    suffix.insert(*dst, value.clone());
                    lets += 1;
                }
                Op::Set(_, _, a) => {
                    let v = arg(a, &suffix)?;
                    if !matches!((value, &v), (Value::Eq(_), Value::Eq(_))) {
                        return Err("non-equality constructor result".into());
                    }
                    if input.equal_output && value == &v {
                        noop += 1;
                    } else {
                        unions.push((value.clone(), v));
                    }
                    sets += 1;
                }
                _ => unreachable!(),
            }
            receipts[op] = Some(row.witness);
            keys[op] = Some(key);
            deep += u64::from(input.back > 1);
        }
        let cell = self.reserve_cell_id()?;
        let count = unions.len() as u64;
        for (a, b) in unions {
            self.enqueue_union(a, b)?
        }
        self.stats.rhs_frames_avoided += 1;
        self.stats.handoff_closed += 1;
        self.stats.handoff_noop_unions += noop;
        self.stats.handoff_attempts += lets + sets;
        self.stats.handoff_lets += lets;
        self.stats.handoff_sets += sets;
        self.stats.handoff_deep += deep;
        Ok(Some((cell, keys, receipts, count)))
    }
    #[cfg(feature = "structural-depth")]
    pub(crate) fn checked_constructor(
        &self,
        f: FactRef,
        table: usize,
        key: &[Value],
    ) -> Result<Option<Row>> {
        Ok(self
            .live_handoff(f, table)?
            .filter(|r| r.values.split_last().is_some_and(|(_, k)| k == key)))
    }
    #[cfg(feature = "iterative-bake")]
    fn live_handoff(&self, f: FactRef, table: usize) -> Result<Option<Row>> {
        if !self.dirty_facts[table].is_empty() {
            return Ok(None);
        }
        Ok(self
            .row(self.resolve_receipt(Dependency::Fact(f)))?
            .filter(|r| r.table == table && !r.hidden))
    }
    #[cfg(feature = "iterative-bake")]
    pub(crate) fn handoff_row(
        &self,
        f: FactRef,
        cell: u32,
        table: usize,
        args: &[Arg],
    ) -> Result<Option<Row>> {
        let Some(row) = self.live_handoff(f, table)? else {
            return Ok(None);
        };
        if row.values.len() != args.len() + 1 {
            return Ok(None);
        }
        for (a, v) in args.iter().zip(row.values.iter()) {
            if self.argument(cell, a)? != *v {
                return Ok(None);
            }
        }
        Ok(Some(row))
    }
    pub fn construct(
        &mut self,
        cell: u32,
        op: usize,
        t: usize,
        aa: &[Arg],
        dst: usize,
    ) -> Result<Value> {
        self.construct_inner(cell, op, t, aa, dst, None, None)
    }
    pub fn construct_visible(
        &mut self,
        cell: u32,
        op: usize,
        t: usize,
        aa: &[Arg],
        dst: usize,
        visible: &mut bool,
    ) -> Result<Value> {
        self.construct_inner(cell, op, t, aa, dst, Some(visible), None)
    }
    pub fn construct_recorded(
        &mut self,
        cell: u32,
        op: usize,
        t: usize,
        aa: &[Arg],
        dst: usize,
        receipt: &mut Option<Dependency>,
        visible: &mut bool,
    ) -> Result<Value> {
        self.construct_inner(cell, op, t, aa, dst, Some(visible), Some(receipt))
    }
    fn construct_inner(
        &mut self,
        cell: u32,
        op: usize,
        t: usize,
        aa: &[Arg],
        dst: usize,
        visible: Option<&mut bool>,
        receipt: Option<&mut Option<Dependency>>,
    ) -> Result<Value> {
        self.stats.constructor_requests += 1;
        let key = self.arguments(cell, aa)?;
        let row = self.lookup_canonical(t, &key)?;
        if let Some(row) = row {
            if let Some(receipt) = receipt {
                *receipt = Some(row.witness);
            }
            if let Some(visible) = visible {
                *visible = !row.hidden;
            }
            self.stats.constructor_reused += 1;

            let value = row.values.last().unwrap().clone();
            self.rhs_frame_mut(cell)
                .unwrap()
                .values
                .insert(dst, value.clone());
            return Ok(value);
        }
        let value = self.insert_constructor(cell, op, t, &key, dst)?;

        if let Some(receipt) = receipt {
            *receipt = Some(Dependency::Fact(FactRef {
                cell,
                op: op as u32,
            }));
        }
        if let Some(visible) = visible {
            *visible = true;
        }
        Ok(value)
    }
    fn insert_constructor(
        &mut self,
        cell: u32,
        op: usize,
        t: usize,
        key: &[Value],
        dst: usize,
    ) -> Result<Value> {
        let value = self.slot(cell, dst)?;
        self.activate(
            FactRef {
                cell,
                op: op as u32,
            },
            t,
            key,
            value.clone(),
            false,
        )?;
        Ok(value)
    }

    pub fn dynamic_construct(&mut self, t: usize, args: Vec<Value>) -> Result<Value> {
        if let Some(row) = self.lookup(t, &args)? {
            return Ok(row.values.last().unwrap().clone());
        }
        let dst = args.len();
        let cell = self.frame(
            self.program.table_templates[t],
            args.into_iter().enumerate().collect(),
        )?;
        self.construct(
            cell,
            0,
            t,
            &(0..dst).map(Arg::Slot).collect::<Vec<_>>(),
            dst,
        )
    }
    pub fn enqueue_union(&mut self, a: Value, b: Value) -> Result<()> {
        self.stats.union_requests += 1;
        let (Value::Eq(a), Value::Eq(b)) = (a, b) else {
            return Err("union requires equality values".into());
        };
        self.pending_unions.push((a, b));

        Ok(())
    }
    fn union(&mut self, a: u64, b: u64) -> Result<()> {
        let a = root(&self.parents, a);
        let b = root(&self.parents, b);
        if a == b {
            return Ok(());
        }
        let (lo, hi) = (a.min(b), a.max(b));

        self.parents.insert(hi, lo);
        let mut affected = self.uses.remove(&hi).unwrap_or_default();
        // Only displaced identities change row fields. Re-stamping the already
        // canonical side can re-run an unchanged LHS and create extra custom
        // merge nodes (e.g. a repeated default ExtractedExpr write).
        self.mark_affected(&affected);
        #[cfg(not(feature = "sparse-cells"))]
        {
            affected.extend(self.uses.get(&lo).into_iter().flatten().copied());
            self.uses.insert(lo, affected);
        }
        #[cfg(feature = "sparse-cells")]
        if !affected.is_empty() {
            let keep = self.uses.entry(lo).or_default();
            if keep.len() < affected.len() {
                std::mem::swap(keep, &mut affected);
            }
            for &dependency in affected.iter() {
                keep.insert(dependency);
            }
        }
        self.stats.unions += 1;
        self.changed = true;
        Ok(())
    }
    /// Complete visible scalar equality incidences from the existing reverse
    /// dependency index, including non-root argument positions.

    fn mark_affected(&mut self, affected: &UseSet) {
        for d in affected.iter() {
            let table = self.dependency_table(*d);
            if let Dependency::Function(i) = d {
                self.functions[*i].decoded = None;
            }
            if let Dependency::Fact(f) = d
                && let Some(state) = self
                    .cells
                    .get_mut(&f.cell)
                    .and_then(|c| c.facts.get_mut(&f.op))
            {
                state.decoded = None;
            }
            {
                self.dirty.insert(*d);
            }
            if let Dependency::Fact(f) = d {
                if let Some(table) = table {
                    self.dirty_facts[table].insert(*f);
                }
            }
        }
    }

    pub fn set(&mut self, cell: u32, op: usize, t: usize, aa: &[Arg], v: &Arg) -> Result<()> {
        self.set_inner(cell, op, t, aa, v, None, None, None)
    }
    pub fn set_visible(
        &mut self,
        cell: u32,
        op: usize,
        t: usize,
        aa: &[Arg],
        v: &Arg,
        visible: &mut bool,
    ) -> Result<()> {
        self.set_inner(cell, op, t, aa, v, Some(visible), None, None)
    }
    pub fn set_recorded(
        &mut self,
        cell: u32,
        op: usize,
        t: usize,
        aa: &[Arg],
        v: &Arg,
        receipt: &mut Option<Dependency>,
        visible: &mut bool,
    ) -> Result<()> {
        self.set_inner(cell, op, t, aa, v, Some(visible), Some(receipt), None)
    }
    pub(crate) fn set_inner(
        &mut self,
        cell: u32,
        op: usize,
        t: usize,
        aa: &[Arg],
        v: &Arg,
        visible: Option<&mut bool>,
        receipt: Option<&mut Option<Dependency>>,
        source: Option<usize>,
    ) -> Result<()> {
        let key = self.arguments(cell, aa)?;
        let value = self.argument(cell, v)?;
        if !self.program.tables[t].virtual_fact {
            let before = self.stats.function_updates;
            let index = self.set_function_from(t, &key, value, Some((cell, op, source)))?;
            if let Some(receipt) = receipt {
                *receipt = Some(Dependency::Function(index));
            }
            if self.stats.function_updates != before {
                #[cfg(feature = "sparse-cells")]
                if self.active_rhs.as_ref().is_some_and(|(id, _)| *id == cell) {
                    self.active_origin = Some(index);
                }
                self.function_origin(
                    index,
                    Some(FactRef {
                        cell,
                        op: op as u32,
                    }),
                );
            }
            if let Some(visible) = visible {
                *visible = true;
            }
            return Ok(());
        }
        if let Some(old) = self.lookup(t, &key)? {
            if let Some(receipt) = receipt {
                *receipt = Some(old.witness);
            }
            if let Some(visible) = visible {
                *visible = !old.hidden
                    && (!self.program.tables[t].constructor || old.values.last() == Some(&value));
            }
            if self.program.tables[t].constructor {
                self.enqueue_union(old.values.last().unwrap().clone(), value)?
            } else if old.values.last() != Some(&value) {
                return Err("relation output conflict".into());
            }
        } else {
            if let Some(receipt) = receipt {
                *receipt = Some(Dependency::Fact(FactRef {
                    cell,
                    op: op as u32,
                }));
            }
            if let Some(visible) = visible {
                *visible = true;
            }
            self.activate(
                FactRef {
                    cell,
                    op: op as u32,
                },
                t,
                &key,
                value,
                false,
            )?;
        }
        Ok(())
    }
    fn merge_expression(&mut self, e: &J, old: &Value, new: &Value) -> Result<Value> {
        // Native ResolvedMergeFn::Function returns old on identical arguments.
        // Primitive merges do NOT have this shortcut: (+ old new) must execute.
        if let Some(v) = e["var"].as_str() {
            return match v {
                "old" => Ok(old.clone()),
                "new" => Ok(new.clone()),
                _ => Err("unknown merge variable".into()),
            };
        }
        if !e["literal"].is_null() {
            return value::literal(e);
        }
        let aa = array(&e["args"])?
            .iter()
            .map(|a| self.merge_expression(a, old, new))
            .collect::<Result<Vec<_>>>()?;
        if let Some(name) = e["call"]["table"].as_str() {
            let t = self
                .program
                .tables
                .iter()
                .position(|t| t.name == name)
                .ok_or("merge operator")?;
            if !self.program.tables[t].virtual_fact {
                self.stats.merge_port_reads += 1;
                return self.read_merge_port(t, &aa);
            }
            self.dynamic_construct(t, aa)
        } else {
            if e["call"]["primitive"]
                .as_str()
                .is_some_and(|n| n.starts_with("cell-key-"))
            {
                self.stats.key_operations += 1;
            }
            value::primitive(
                string(&e["call"]["primitive"])?,
                &aa,
                string(&e["call"]["output"])?,
                self.program.key_nodes(),
            )?
            .ok_or("merge arithmetic failed".into())
        }
    }
    fn merge_value(&mut self, t: usize, old: Value, new: Value) -> Result<Value> {
        self.stats.merge_calls += 1;
        if let Some(kind) = &self.merge_kinds[t] {
            if let Some(n) = self.stats.merge_contract_calls.get_mut(kind) {
                *n += 1;
            } else {
                self.stats.merge_contract_calls.insert(kind.clone(), 1);
            }
        }
        if self.scalar_merge
            && let Some(plan) = self.program.physical.merges.get(t).copied().flatten()
        {
            use crate::physical_plan::ScalarMerge;
            let keep_old = match (plan, &old, &new) {
                (ScalarMerge::Old, _, _) => {
                    self.stats.scalar_old_projection += 1;
                    true
                }
                (ScalarMerge::Min, Value::Int(a), Value::Int(b)) => a <= b,
                (ScalarMerge::Max, Value::Int(a), Value::Int(b)) => a >= b,
                _ => return Err("unexpected scalar merge value".into()),
            };
            self.stats.scalar_merge_calls += 1;
            self.stats.scalar_unchanged += u64::from(keep_old);
            return Ok(if keep_old { old } else { new });
        }
        let expr = &self.program.tables[t].merge;
        if old == new && self.equal_keeps_old[t] {
            return Ok(old);
        }
        if expr.is_null() {
            return if old == new {
                Ok(old)
            } else {
                Err(format!(
                    "conflicting function {} without merge",
                    self.program.tables[t].name
                ))
            };
        }
        if self.merge_programs_enabled {
            let m = self
                .program
                .merge_programs
                .merges
                .get(&t)
                .cloned()
                .ok_or("missing compiled merge")?;
            self.stats.merge_program_calls += 1;
            if m.operations.is_empty() {
                return Ok(match &m.result {
                    Arg::Slot(0) => old,
                    Arg::Slot(1) => new,
                    Arg::Literal(v) => v.clone(),
                    _ => return Err("merge result slot".into()),
                });
            }
            let mut values = Vec::with_capacity(2 + m.operations.len());
            values.extend([old, new]);
            let operand = |a: &Arg, values: &Vec<Value>| match a {
                Arg::Slot(s) => values[*s].clone(),
                Arg::Literal(v) => v.clone(),
            };
            for op in &m.operations {
                let Op::Let(dst, call, args) = op else {
                    return Err("invalid merge instruction".into());
                };
                let args = args.iter().map(|a| operand(a, &values)).collect::<Vec<_>>();
                let value = match call {
                    Call::Table(t) if self.program.tables[*t].virtual_fact => {
                        self.stats.merge_program_constructs += 1;
                        self.dynamic_construct(*t, args)?
                    }
                    Call::Table(t) => {
                        self.stats.merge_port_reads += 1;
                        self.read_merge_port(*t, &args)?
                    }
                    Call::Primitive(name, sort) => {
                        if name.starts_with("cell-key-") {
                            self.stats.key_operations += 1;
                        }
                        value::primitive(name, &args, sort, self.program.key_nodes())?
                            .ok_or("merge arithmetic failed")?
                    }
                };
                if *dst != values.len() {
                    return Err("merge SSA order".into());
                }
                values.push(value);
                self.stats.merge_program_operations += 1;
            }
            Ok(operand(&m.result, &values))
        } else {
            let expr = expr.clone();
            self.merge_expression(&expr, &old, &new)
        }
    }
    fn read_merge_port(&mut self, t: usize, args: &[Value]) -> Result<Value> {
        let key = args
            .iter()
            .map(|v| canon(v, &self.parents))
            .collect::<Vec<_>>();
        let mut result: Option<Value> = None;
        // During repair the ordinary key directory may still contain displaced
        // keys. Resolve the complete canonical port, including pending pure
        // lattice collisions; never pick an arbitrary stale entry.
        for i in self.function_operators[t].iter().copied() {
            let Some(row) = self.function(i) else {
                continue;
            };
            if row.values[..key.len()] != key {
                continue;
            }
            let value = row.values.last().unwrap().clone();
            if let Some(old) = result.as_ref() {
                if *old == value {
                    continue;
                }
                let e = &self.program.tables[t].merge;
                let Some(op) = e["call"]["primitive"]
                    .as_str()
                    .filter(|op| matches!(*op, "min" | "max" | "or" | "and" | "cell-key-min"))
                else {
                    return Err("ambiguous non-lattice merge read".into());
                };
                let a = e["args"].as_array().ok_or("invalid lattice merge")?;
                if a.len() != 2
                    || !((a[0]["var"] == "old" && a[1]["var"] == "new")
                        || (a[0]["var"] == "new" && a[1]["var"] == "old"))
                {
                    return Err("ambiguous merge port requires a pure lattice contract".into());
                }
                result = value::primitive(
                    op,
                    &[old.clone(), value],
                    self.program.tables[t].schema.last().unwrap()["name"]
                        .as_str()
                        .unwrap(),
                    self.program.key_nodes(),
                )?;
            } else {
                result = Some(value);
            }
        }
        result.ok_or_else(|| format!("unready merge port {}", self.program.tables[t].name))
    }
    pub fn set_function(&mut self, t: usize, key: Vec<Value>, value: Value) -> Result<()> {
        self.set_function_inner(t, key, value).map(|_| ())
    }
    fn set_function_inner(&mut self, t: usize, key: Vec<Value>, value: Value) -> Result<usize> {
        let key = key
            .into_iter()
            .map(|v| canon(&v, &self.parents))
            .collect::<Vec<_>>();
        let value = canon(&value, &self.parents);
        self.set_function_canonical(t, &key, value)
    }
    fn set_function_canonical(&mut self, t: usize, key: &[Value], value: Value) -> Result<usize> {
        self.set_function_from(t, key, value, None)
    }
    fn set_function_from(
        &mut self,
        t: usize,
        key: &[Value],
        value: Value,
        site: Option<(u32, usize, Option<usize>)>,
    ) -> Result<usize> {
        let index = self.function_index(t, key);
        if let Some(i) = index {
            let old = canon(&self.functions[i].value, &self.parents);
            let next = self.merge_value(t, old.clone(), value)?;
            if next != old {
                self.functions[i].decoded = Some(Self::shared_fields(key, &next));
                #[cfg(feature = "slot-transfer-audit")]
                crate::transfer_audit::effective(self.epoch, t, i, key, Some(&old), &next)?;
                self.functions[i].value = next;
                self.functions[i].epoch = self.epoch;
                self.changed = true;
                self.stats.function_updates += 1;
                self.changes.touch(t, Dependency::Function(i));
                // A scalar result change preserves every subscribed eclass.
                // Publish the row version without rebuilding its reverse uses.
                if !matches!(old, Value::Eq(_) | Value::Set(_))
                    && !matches!(self.functions[i].value, Value::Eq(_) | Value::Set(_))
                {
                    self.query_versions[t] += 1;
                } else {
                    let row = self.function(i).unwrap();
                    self.subscribe(row.witness, &row.values);
                }
            }
            Ok(i)
        } else {
            let i = self.functions.len();
            #[cfg(feature = "slot-transfer-audit")]
            crate::transfer_audit::effective(self.epoch, t, i, key, None, &value)?;
            let compact = if cfg!(feature = "view-storage")
                && self.units.owned[t]
                && value == Value::Int(0)
            {
                self.try_view_projection(t, i, key, site)?
            } else {
                false
            };
            let (stored_key, decoded) = if compact {
                (Vec::new(), None)
            } else {
                #[cfg(feature = "sparse-cells")]
                self.function_keys[t].insert(key, i);
                #[cfg(not(feature = "sparse-cells"))]
                self.function_keys[t].insert(key.to_vec(), i);
                (key.to_vec(), Some(Self::shared_fields(key, &value)))
            };
            self.functions.push(Function {
                #[cfg(feature = "sparse-cells")]
                writer: None,
                #[cfg(feature = "receipt-slots")]
                port_slot: 0,
                decoded,
                table: t,
                key: stored_key,
                value,
                epoch: self.epoch,
                live: true,
                origin: None,
            });
            self.function_operators[t].insert(i);
            self.changed = true;
            self.stats.function_updates += 1;
            self.changes.touch(t, Dependency::Function(i));
            if compact {
                // Unit contributes no equality dependency.
                self.subscribe(Dependency::Function(i), key);
            } else {
                let row = self.function(i).unwrap();
                self.subscribe(row.witness, &row.values);
            }
            Ok(i)
        }
    }
    fn retire(&mut self, f: FactRef) -> Result<()> {
        if let Some(r) = self.fact(f)? {
            self.changes.retire(r.table, Dependency::Fact(f));
            self.query_versions[r.table] += 1;
            let s = self
                .cells
                .get_mut(&f.cell)
                .unwrap()
                .facts
                .remove(&f.op)
                .unwrap();
            #[cfg(feature = "receipt-slots")]
            if s.port_slot != 0 {
                self.retired_port_slots
                    .insert(Dependency::Fact(f), s.port_slot);
            }
            self.operators[r.table].remove(&f);
            if let Some(b) = self.intern.get_mut(&s.hash) {
                b.remove(&f);
                #[cfg(feature = "sparse-cells")]
                if b.is_empty() {
                    self.intern.remove(&s.hash);
                }
            }
            self.subscribe(Dependency::Fact(f), &[]);
            self.garbage.insert(f.cell);
        }
        Ok(())
    }
    #[cfg(feature = "sparse-cells")]
    pub(crate) fn has_visibility_changes(&self, t: usize) -> bool {
        self.visibility_tainted[t]
    }
    pub fn hide(&mut self, t: usize, key: &[Value], delete: bool) -> Result<bool> {
        #[cfg(feature = "sparse-cells")]
        {
            self.visibility_tainted[t] = true;
        }
        if delete {
            self.deletion_epochs[t] = self.epoch;
        }
        let row = self.lookup(t, key)?;
        let Some(row) = row else {
            return if delete {
                Ok(false)
            } else {
                let v = self.dynamic_construct(t, key.to_vec())?;
                let _ = v;
                self.hide(t, key, false)?;
                Ok(false)
            };
        };
        match row.witness {
            Dependency::Fact(f) => {
                let born = self.cells[&f.cell].facts[&f.op].born;
                if delete {
                    self.retire(f)?;
                    self.changed = true;
                } else {
                    let s = self
                        .cells
                        .get_mut(&f.cell)
                        .unwrap()
                        .facts
                        .get_mut(&f.op)
                        .unwrap();
                    if !s.hidden {
                        self.query_versions[t] += 1;
                        #[cfg(feature = "receipt-slots")]
                        if s.port_slot != 0 {
                            self.retired_port_slots
                                .insert(row.witness, std::mem::take(&mut s.port_slot));
                        }
                        s.hidden = true;
                        s.epoch = self.epoch;
                        self.changed = true;
                        self.changes.touch(t, row.witness);
                    }
                }
                Ok(born < self.epoch)
            }
            Dependency::Function(i) => {
                if !delete {
                    return Err("function subsume unsupported".into());
                }
                #[cfg(feature = "receipt-slots")]
                if self.functions[i].port_slot != 0 {
                    self.retired_port_slots.insert(
                        Dependency::Function(i),
                        std::mem::take(&mut self.functions[i].port_slot),
                    );
                }
                self.functions[i].live = false;
                self.changes.retire(t, row.witness);
                self.function_origin(i, None);
                self.query_versions[t] += 1;
                self.function_operators[t].remove(&i);
                if self.units.remove(i) {
                    self.units.stats.deleted += 1;
                } else {
                    self.function_keys[t].remove(key);
                }
                self.subscribe(row.witness, &[]);
                self.changed = true;
                Ok(true)
            }
        }
    }
    pub fn repair(&mut self) -> Result<()> {
        loop {
            if !self.pending_unions.is_empty() {
                let pairs = std::mem::take(&mut self.pending_unions);
                for (a, b) in pairs {
                    self.union(a, b)?;
                }
                continue;
            }

            let Some(d) = self
                .dirty
                .pop_first()
                .or_else(|| self.dirty_markers.pop_first().map(Dependency::Fact))
            else {
                break;
            };
            if let Dependency::Fact(f) = d {
                if let Some(table) = self.dependency_table(d) {
                    self.dirty_facts[table].remove(&f);
                }
            }
            self.stats.repaired_dependencies += 1;

            match d {
                Dependency::Fact(f) => {
                    let Some(r) = self.fact(f)? else { continue };
                    let key = &r.values[..r.values.len() - 1];
                    let h = hashkey(r.table, key);
                    let old = self.cells[&f.cell].facts[&f.op].hash;
                    self.stats.repair_key_hash_changes += u64::from(h != old);
                    if let Some(bucket) = self.intern.get_mut(&old) {
                        bucket.remove(&f);
                        #[cfg(feature = "sparse-cells")]
                        if bucket.is_empty() {
                            self.intern.remove(&old);
                        }
                    }
                    let candidates = self.intern.get(&h).cloned().unwrap_or_default();
                    let mut found = None;
                    for &other in candidates.iter() {
                        if let Some(row) = self.fact(other)? {
                            if row.table == r.table && row.values[..key.len()] == *key {
                                found = Some(row);
                                break;
                            }
                        }
                    }
                    if let Some(other) = found {
                        self.stats.repair_collisions += 1;
                        if self.program.tables[r.table].constructor {
                            self.enqueue_union(
                                r.values.last().unwrap().clone(),
                                other.values.last().unwrap().clone(),
                            )?;
                        }
                        if let Dependency::Fact(keep) = other.witness {
                            self.query_versions[r.table] += 1;
                            let target = self
                                .cells
                                .get_mut(&keep.cell)
                                .unwrap()
                                .facts
                                .get_mut(&keep.op)
                                .unwrap();
                            #[cfg(feature = "receipt-slots")]
                            if r.hidden && target.port_slot != 0 {
                                self.retired_port_slots
                                    .insert(other.witness, std::mem::take(&mut target.port_slot));
                            }
                            target.hidden |= r.hidden;
                            target.epoch = self.epoch;
                            self.changes.touch(r.table, other.witness);
                        }
                        if self.receipt_tracking {
                            self.receipt_forwards
                                .insert(Dependency::Fact(f), other.witness);
                        }
                        self.retire(f)?;
                    } else {
                        self.intern.entry(h).or_default().insert(f);
                        let s = self
                            .cells
                            .get_mut(&f.cell)
                            .unwrap()
                            .facts
                            .get_mut(&f.op)
                            .unwrap();

                        s.decoded = Some((r.table, r.values.clone()));
                        s.hash = h;
                        s.epoch = self.epoch;
                        self.subscribe(d, &r.values);
                        self.changes.touch(r.table, d);
                    }
                }
                Dependency::Function(i) => {
                    let Some(row) = self.function(i) else {
                        continue;
                    };
                    let key = row.values[..row.values.len() - 1].to_vec();
                    let compressed = self.units.remove(i);
                    let oldkey = &self.functions[i].key;
                    if !compressed && self.function_keys[row.table].get(oldkey) == Some(&i) {
                        self.function_keys[row.table].remove(oldkey);
                    }
                    if let Some(j) = self.function_index(row.table, &key) {
                        if j != i {
                            if compressed {
                                self.units.stats.lost += 1;
                            }

                            // The key directory selected the exact survivor. Its Unit
                            // value and canonical key keep all reverse subscriptions valid.
                            let shared_unit = cfg!(feature = "view-storage")
                                && self.units.owned[row.table]
                                && row.values.last() == Some(&Value::Int(0))
                                && self.functions[j].value == Value::Int(0)
                                && (self.functions[j].key == key
                                    || self.units.change_block(j, &row.values));
                            if shared_unit {
                                self.functions[j].decoded = Some(row.values.clone());
                                self.units.stats.fast_merges += 1;
                            } else {
                                let old = canon(&self.functions[j].value, &self.parents);
                                #[cfg(feature = "slot-transfer-audit")]
                                let before = old.clone();
                                let next = self.merge_value(
                                    row.table,
                                    old,
                                    row.values.last().unwrap().clone(),
                                )?;
                                #[cfg(feature = "slot-transfer-audit")]
                                if before != next {
                                    crate::transfer_audit::effective(
                                        self.epoch,
                                        row.table,
                                        j,
                                        &key,
                                        Some(&before),
                                        &next,
                                    )?;
                                }
                                self.functions[j].decoded = None;
                                self.functions[j].value = next;
                            }
                            self.functions[j].epoch = self.epoch;
                            #[cfg(feature = "receipt-slots")]
                            if self.functions[i].port_slot != 0 {
                                self.retired_port_slots.insert(
                                    Dependency::Function(i),
                                    std::mem::take(&mut self.functions[i].port_slot),
                                );
                            }
                            self.functions[i].live = false;
                            self.changes.retire(row.table, d);
                            if self.receipt_tracking {
                                self.receipt_forwards
                                    .insert(Dependency::Function(i), Dependency::Function(j));
                            }
                            self.function_origin(i, None);
                            self.function_operators[row.table].remove(&i);
                            self.subscribe(d, &[]);
                            if shared_unit {
                                self.query_versions[row.table] += 1;
                            } else {
                                let target = self.function(j).unwrap();
                                self.subscribe(target.witness, &target.values);
                            }
                            self.changes.touch(row.table, Dependency::Function(j));
                            continue;
                        }
                    }
                    if compressed {
                        self.units.stats.promoted += 1;
                    }
                    self.functions[i].decoded = Some(row.values.clone());
                    self.functions[i].key = key.clone();
                    self.functions[i].value = row.values.last().unwrap().clone();
                    self.functions[i].epoch = self.epoch;
                    #[cfg(feature = "sparse-cells")]
                    self.function_keys[row.table].insert(&key, i);
                    #[cfg(not(feature = "sparse-cells"))]
                    self.function_keys[row.table].insert(key, i);
                    self.subscribe(d, &row.values);
                    self.changes.touch(row.table, d);
                }
            }
        }

        Ok(())
    }

    pub fn release_empty(&mut self, cell: u32) {
        #[cfg(feature = "sparse-cells")]
        if self.active_rhs.as_ref().is_some_and(|(id, _)| *id == cell) {
            #[cfg(feature = "sparse-cells")]
            if let Some(&n) = self.parent_users.get(&cell)
                && n > 0
            {
                if n == 1
                    && let Some(index) = self.active_origin
                    && index < u32::MAX as usize
                    && let Some(f) = self.functions[index].origin
                    && f.cell == cell
                {
                    self.parent_users.remove(&cell);
                    let (_, frame) = self.active_rhs.take().unwrap();
                    self.install_writer(index, f, frame, None);
                    self.writer_dir.general += 1;
                    return;
                }
                self.promote_active();
                return;
            }
            self.active_rhs = None;
            self.stats.rhs_frames_avoided += 1;
            return;
        }
        // Only the newly executed frame is retired here. Ancestors are kept
        // until the Run boundary: later bindings in this Run may still use them.
        self.retire_empty(cell);
    }
    fn retire_empty(&mut self, cell: u32) {
        if self.parent_users.get(&cell).copied().unwrap_or(0) == 0
            && self.cells.get(&cell).is_some_and(|c| c.facts.is_empty())
        {
            if let Some(c) = self.cells.remove(&cell) {
                if let Some(p) = c.parent {
                    self.release_reference(p);
                }
            }
            self.parent_users.remove(&cell);
            self.stats.empty_frames_released += 1;
        }
    }
    pub fn collect_garbage(&mut self) {
        while let Some(cell) = self.garbage.pop_first() {
            self.retire_empty(cell);
        }
    }
    pub fn export(&self) -> Result<J> {
        let mut rows = vec![];
        for t in 0..self.program.tables.len() {
            for w in self.witnesses(t) {
                if let Some(r) = self.row(w)? {
                    rows.push(json!({"table":self.program.tables[t].name,"values":r.values.iter().zip(&self.program.tables[t].schema).map(|(v,s)|exported(v,s)).collect::<Vec<_>>(),"subsumed":r.hidden}));
                }
            }
        }
        let anchors=self.anchors.iter().map(|(sort,old,id)|json!({"sort":sort,"old":old,"now":format!("Cell({})",root(&self.parents,*id))})).collect::<Vec<_>>();
        Ok(json!({"rows":rows,"anchors":anchors}))
    }
    /// Stream diagnostics without allocating a second complete row graph.
    pub fn write_export(&self, writer: impl std::io::Write) -> Result<()> {
        use serde::ser::{SerializeSeq, SerializeStruct};
        struct Rows<'a>(&'a Graph);
        impl Serialize for Rows<'_> {
            fn serialize<S: serde::Serializer>(
                &self,
                serializer: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                let mut seq = serializer.serialize_seq(None)?;
                for table in 0..self.0.program.tables.len() {
                    for witness in self.0.witnesses(table) {
                        if let Some(row) = self.0.row(witness).map_err(serde::ser::Error::custom)? {
                            let table = &self.0.program.tables[row.table];
                            seq.serialize_element(&json!({"table":table.name,"values":row.values.iter().zip(&table.schema).map(|(v,s)|exported(v,s)).collect::<Vec<_>>(),"subsumed":row.hidden}))?;
                        }
                    }
                }
                seq.end()
            }
        }
        struct Export<'a>(&'a Graph);
        impl Serialize for Export<'_> {
            fn serialize<S: serde::Serializer>(
                &self,
                serializer: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                let mut result = serializer.serialize_struct("Graph", 2)?;
                result.serialize_field("rows", &Rows(self.0))?;
                let anchors=self.0.anchors.iter().map(|(sort,old,id)|json!({"sort":sort,"old":old,"now":format!("Cell({})",root(&self.0.parents,*id))})).collect::<Vec<_>>();
                result.serialize_field("anchors", &anchors)?;
                result.end()
            }
        }
        serde_json::to_writer(writer, &Export(self)).map_err(|e| e.to_string())
    }
}

#[path = "producer.rs"]
pub(crate) mod producer;
