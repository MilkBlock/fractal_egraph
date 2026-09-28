//! Verified storage recipes for already committed rows. Keys, row IDs, timestamps
//! and merge semantics remain native. The base is immutable; mutations use an
//! ordinary dense append-only overlay plus tombstones.
use super::*;
use crate::WrappedTable;
use super::cell_rows::{CellRows, CellSegment, NativeCellBatch, NativeCellStats};
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet, HashMap},
};

#[derive(Debug)]
pub struct NativeRowTemplate {
    rows: Vec<Box<[usize]>>,
    slots: usize,
}
impl NativeRowTemplate {
    pub fn new(rows: Vec<Vec<usize>>) -> Result<Arc<Self>, String> {
        if rows.is_empty() || rows.iter().any(Vec::is_empty) {
            return Err("empty virtual template".into());
        }
        let used: BTreeSet<_> = rows.iter().flatten().copied().collect();
        if used.iter().copied().ne(0..used.len()) {
            return Err("template slots must be contiguous from zero".into());
        }
        Ok(Arc::new(Self {
            rows: rows.into_iter().map(Vec::into_boxed_slice).collect(),
            slots: used.len(),
        }))
    }
    pub fn row_slots(&self,row:usize)->&[usize]{&self.rows[row]}
    pub fn slots(&self) -> usize {
        self.slots
    }
    pub fn rows(&self) -> usize {
        self.rows.len()
    }
}
#[derive(Debug)]
struct BindingMatrix {
    template: Arc<NativeRowTemplate>,
    values: Box<[Value]>,
}
#[derive(Clone, Debug)]
pub struct NativeRowInstance {
    matrix: Arc<BindingMatrix>,
    index: u32,
}
impl NativeRowInstance {
    pub fn new(template: Arc<NativeRowTemplate>, binding: Vec<Value>) -> Result<Arc<Self>, String> {
        Ok(Self::batch(template, vec![binding])?.pop().unwrap())
    }
    pub fn batch(
        template: Arc<NativeRowTemplate>,
        bindings: Vec<Vec<Value>>,
    ) -> Result<Vec<Arc<Self>>, String> {
        let n = bindings.len();
        if n > u32::MAX as usize {
            return Err("too many native instance rows".into());
        }
        if bindings.iter().any(|b| b.len() != template.slots) {
            return Err("virtual binding arity mismatch".into());
        }
        let matrix = Arc::new(BindingMatrix {
            template,
            values: bindings
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        });
        Ok((0..n)
            .map(|i| {
                Arc::new(Self {
                    matrix: matrix.clone(),
                    index: i as u32,
                })
            })
            .collect())
    }
    pub fn row(&self, row: usize) -> Option<Vec<Value>> {
        let start = self.index as usize * self.matrix.template.slots;
        Some(
            self.matrix
                .template
                .rows
                .get(row)?
                .iter()
                .map(|s| self.matrix.values[start + *s])
                .collect(),
        )
    }
}
#[derive(Clone)]
pub struct VirtualRowRequest {
    pub key: Vec<Value>,
    pub instance: Arc<NativeRowInstance>,
    pub template_row: usize,
}
#[derive(Clone, Debug, Default)]
pub struct NativeVirtualStats {
    pub shared_index_bytes: Vec<(usize, usize)>,
    pub virtual_rows: usize,
    pub direct_base_rows: usize,
    pub overlay_rows: usize,
    pub tombstones: usize,
    pub rebuild_rows_examined: usize,
    pub rebuild_rows_changed: usize,
    pub dense_equivalent_bytes: usize,
    pub descriptor_bytes: usize,
    pub direct_bytes: usize,
    pub metadata_bytes: usize,
    pub local_header_bytes: usize,
    pub binding_headers: Vec<(usize, usize)>,
    pub template_headers: Vec<(usize, usize)>,
    /// Process-local allocation identity and payload bytes; deduplicate across tables.
    pub instances: Vec<(usize, usize)>,
    pub templates: Vec<(usize, usize)>,
}
const DIRECT: u32 = u32::MAX;
#[derive(Clone, Copy)]
struct Recipe {
    instance: u32,
    row: u32,
    tail: u32,
}
#[derive(Clone, Copy)]
struct BindingRef {
    matrix: u32,
    index: u32,
}
// Fixed physical addressing granule, not a search or admission budget.
const PAGE_ROWS: usize = 256;
#[derive(Default)]
struct Page {
    // Dense pages need no per-row descriptors. Mixed pages preserve row IDs.
    recipes: Vec<Recipe>,
    virtual_bits: [u64; PAGE_ROWS / 64],
    virtual_prefix: [u16; PAGE_ROWS / 64],
    direct: Vec<Value>,
    len: usize,
}
#[derive(Default)]
struct Base {
    cells: Option<CellRows>,
    pages: Vec<Page>,
    len: usize,
    width: usize,
    instances: Vec<BindingRef>,
    matrices: Vec<Arc<BindingMatrix>>,
    tails: Vec<Box<[Value]>>,
}
impl Base {
    fn recipe(&self, n: usize) -> Recipe {
        let page = &self.pages[n / PAGE_ROWS];
        let offset = n % PAGE_ROWS;
        if page.recipes.is_empty() { return Recipe { instance: DIRECT, row: offset as u32, tail: 0 }; }
        let word = offset / 64;
        let bit = offset % 64;
        let rank = page.virtual_prefix[word] as usize
            + (page.virtual_bits[word] & ((1u64 << bit) - 1)).count_ones() as usize;
        if page.virtual_bits[word] & (1u64 << bit) != 0 { page.recipes[rank] }
        else { Recipe { instance: DIRECT, row: (offset - rank) as u32, tail: 0 } }
    }
    fn direct_row(&self, n: usize, offset: u32) -> &[Value] {
        let start = offset as usize * self.width;
        &self.pages[n / PAGE_ROWS].direct[start..start + self.width]
    }
    fn push(&mut self, recipe: Option<Recipe>, values: Option<&[Value]>) {
        if self.len % PAGE_ROWS == 0 { self.pages.push(Page::default()); }
        let page = self.pages.last_mut().unwrap();
        if page.len % 64 == 0 { page.virtual_prefix[page.len / 64] = page.recipes.len() as u16; }
        if let Some(r) = recipe {
            page.virtual_bits[page.len / 64] |= 1u64 << (page.len % 64);
            page.recipes.push(r);
        } else {
            if let Some(values) = values { page.direct.extend_from_slice(values); }
            else { page.direct.resize(page.direct.len() + self.width, Value::new_const(0)); }
        }
        page.len += 1;
        self.len += 1;
    }
    fn finish(&mut self) {
        for page in &mut self.pages {
            page.direct.shrink_to_fit();
            page.recipes.shrink_to_fit();
        }
        self.pages.shrink_to_fit();
    }
    fn view(&self, id: u32) -> (&NativeRowTemplate, &[Value]) {
        let r = self.instances[id as usize];
        let m = &self.matrices[r.matrix as usize];
        let start = r.index as usize * m.template.slots;
        (&m.template, &m.values[start..start + m.template.slots])
    }
    fn instance(&self, id: u32) -> NativeRowInstance {
        let r = self.instances[id as usize];
        NativeRowInstance {
            matrix: self.matrices[r.matrix as usize].clone(),
            index: r.index,
        }
    }
}
#[derive(Clone, Default)]
struct DeadRows { bits: fixedbitset::FixedBitSet, count: usize }
impl DeadRows {
    fn contains(&self, row: &RowId) -> bool { self.bits.contains(row.index()) }
    fn insert(&mut self, row: RowId) {
        if row.index() >= self.bits.len() { self.bits.grow(row.index() + 1); }
        if !self.bits.contains(row.index()) { self.bits.insert(row.index()); self.count += 1; }
    }
    fn len(&self) -> usize { self.count }
}
#[derive(Clone)]
pub(super) struct VirtualRows {
    base: Arc<Base>,
    dead: DeadRows,
    rebuild_rows_examined: usize,
    rebuild_rows_changed: usize,
}
impl VirtualRows {
    fn prefix(&self) -> usize {
        self.base.len
    }
    fn value(&self, row: RowId, col: usize) -> Option<Value> {
        if self.dead.contains(&row) {
            return None;
        }
        if row.index() >= self.base.len { return None; }
        if let Some(c)=&self.base.cells { return c.value(row.index(),col); }
        let r = self.base.recipe(row.index());
        if r.instance == DIRECT {
            return self.base.direct_row(row.index(), r.row).get(col).copied();
        }
        let (template, binding) = self.base.view(r.instance);
        let pattern = &template.rows[r.row as usize];
        if col < pattern.len() {
            Some(binding[pattern[col]])
        } else {
            self.base.tails[r.tail as usize]
                .get(col - pattern.len())
                .copied()
        }
    }
    fn row(&self, row: RowId) -> Option<Cow<'_, [Value]>> {
        if self.dead.contains(&row) {
            return None;
        }
        if row.index() >= self.base.len { return None; }
        if let Some(c)=&self.base.cells { return Some(c.row(row.index())); }
        let r = self.base.recipe(row.index());
        if r.instance == DIRECT {
            return Some(Cow::Borrowed(self.base.direct_row(row.index(), r.row)));
        }
        let (template, binding) = self.base.view(r.instance);
        let mut values = template.rows[r.row as usize]
            .iter()
            .map(|s| binding[*s])
            .collect::<Vec<_>>();
        values.extend_from_slice(&self.base.tails[r.tail as usize]);
        Some(Cow::Owned(values))
    }
}
impl SortedWritesTable {
    pub(super) fn physical_len(&self) -> usize {
        self.virtual_rows.as_ref().map_or(0, VirtualRows::prefix) + self.data.data.len()
    }
    pub(super) fn next_row_id(&self) -> RowId {
        RowId::from_usize(self.physical_len())
    }
    pub(super) fn stale_count(&self) -> usize {
        self.data.stale_rows + self.virtual_rows.as_ref().map_or(0, |v| v.dead.len())
    }
    pub(super) fn row_values(&self, row: RowId) -> Option<Cow<'_, [Value]>> {
        if let Some(v) = &self.virtual_rows {
            if row.index() < v.prefix() {
                return v.row(row);
            }
            return self
                .data
                .get_row(RowId::from_usize(row.index() - v.prefix()))
                .map(Cow::Borrowed);
        }
        self.data.get_row(row).map(Cow::Borrowed)
    }
    pub(super) fn row_value(&self, row: RowId, col: usize) -> Option<Value> {
        if let Some(v) = &self.virtual_rows {
            if row.index() < v.prefix() {
                return v.value(row, col);
            }
        }
        self.row_values(row)?.get(col).copied()
    }
    pub(super) fn virtual_key_matches(&self, row: RowId, key: &[Value]) -> bool {
        let v = self.virtual_rows.as_ref().unwrap();
        if row.index() >= v.prefix() {
            return self
                .data
                .get_row(RowId::from_usize(row.index() - v.prefix()))
                .is_some_and(|r| &r[..key.len()] == key);
        }
        if v.dead.contains(&row) {
            return false;
        }
        if v.base.cells.is_some() { return self.row_values(row).is_some_and(|r|r[..key.len()]==*key); }
        let r = v.base.recipe(row.index());
        if r.instance == DIRECT {
            return &v.base.direct_row(row.index(), r.row)[..key.len()] == key;
        }
        let (template, binding) = v.base.view(r.instance);
        template.rows[r.row as usize][..key.len()]
            .iter()
            .zip(key)
            .all(|(slot, value)| binding[*slot] == *value)
    }
    pub(super) fn copy_virtual_row(&self, row: RowId, out: &mut Vec<Value>) -> bool {
        out.clear();
        let v = self.virtual_rows.as_ref().unwrap();
        if row.index() >= v.prefix() {
            if let Some(r) = self
                .data
                .get_row(RowId::from_usize(row.index() - v.prefix()))
            {
                out.extend_from_slice(r);
                return true;
            }
            return false;
        }
        if v.dead.contains(&row) {
            return false;
        }
        if let Some(c)=&v.base.cells { c.copy_row(row.index(),out); return true; }
        let r = v.base.recipe(row.index());
        if r.instance == DIRECT {
            out.extend_from_slice(v.base.direct_row(row.index(), r.row));
            return true;
        }
        let (template, binding) = v.base.view(r.instance);
        out.reserve(self.n_columns);
        out.extend(
            template.rows[r.row as usize]
                .iter()
                .map(|slot| binding[*slot]),
        );
        out.extend_from_slice(&v.base.tails[r.tail as usize]);
        true
    }
    fn virtual_append(&mut self, row: &[Value]) -> RowId {
        let start = self.virtual_rows.as_ref().unwrap().prefix();
        let local = self.data.add_row(row);
        RowId::from_usize(start + local.index())
    }
    fn virtual_remove(&mut self, row: RowId) {
        let v = self.virtual_rows.as_mut().unwrap();
        if row.index() < v.prefix() {
            v.dead.insert(row);
        } else {
            self.data
                .set_stale(RowId::from_usize(row.index() - v.prefix()));
        }
    }
    pub(super) fn storage_lookup(&self,key:&[Value])->Option<RowId> {
        let direct=get_entry(key,self.n_keys,&self.hash,|r|if self.virtual_rows.is_some(){self.virtual_key_matches(r,key)}else{self.data.get_row(r).is_some_and(|v|v[..self.n_keys]==*key)});
        direct.or_else(|| self.virtual_rows.as_ref().and_then(|v|v.base.cells.as_ref().and_then(|c|c.lookup(key,|n|!v.dead.contains(&RowId::from_usize(n))).map(RowId::from_usize))))
    }
    pub(super) fn is_cell_row(&self,id:RowId)->bool {
        self.virtual_rows.as_ref().is_some_and(|v|id.index()<v.prefix() && v.base.cells.as_ref().is_some_and(|c|c.is_cell(id.index())))
    }
    pub(super) fn is_virtual_key(&self,key:&[Value])->bool {
        self.storage_lookup(key).is_some_and(|id|self.virtual_rows.as_ref().is_some_and(|v|id.index()<v.prefix() && (v.base.cells.as_ref().map_or_else(||v.base.recipe(id.index()).instance!=DIRECT,|c|c.is_cell(id.index())))))
    }
    pub(super) fn pack_rows(&mut self, requests: &[VirtualRowRequest]) -> Result<(), String> {
        if self.cell_mode { return Err("use cell compaction for a compact-cell table".into()); }
        // Validate the entire request before changing any storage.
        let mut selected = BTreeMap::new();
        for request in requests {
            let payload = request
                .instance
                .row(request.template_row)
                .ok_or("unknown template row")?;
            if request.key.len() != self.n_keys
                || payload.len() < self.n_keys
                || payload.len() > self.n_columns
                || payload[..self.n_keys] != request.key
            {
                return Err("virtual row schema/key mismatch".into());
            }
            let id = get_entry(&request.key, self.n_keys, &self.hash, |r| {
                self.row_values(r)
                    .is_some_and(|v| v[..self.n_keys] == request.key)
            })
            .ok_or("virtualization requires an existing fact")?;
            let actual = self.row_values(id).ok_or("stale virtual row")?;
            if actual[..payload.len()] != payload {
                return Err("virtual payload differs from the committed row".into());
            }
            selected.entry(id).or_insert(request);
        }
        if selected.is_empty() {
            return Ok(());
        }
        self.virtual_rows = Some(self.encode_virtual_base(&selected, false)?);
        self.data = NativeRows::Owned(Rows::new(RowBuffer::new(self.n_columns)));
        // Row positions and all values are unchanged. Keep hash, offsets, row
        // producers and table generation: packing is NOT a semantic insertion.
        Ok(())
    }
    fn encode_virtual_base(
        &self,
        selected: &BTreeMap<RowId, &VirtualRowRequest>,
        compact: bool,
    ) -> Result<VirtualRows, String> {
        let mut base = Base { width: self.n_columns, ..Default::default() };
        let mut dead = DeadRows::default();
        let mut instances = HashMap::new();
        let mut matrices = HashMap::new();
        let mut tails = BTreeMap::new();
        for n in 0..self.physical_len() {
            let id = RowId::from_usize(n);
            let Some(actual) = self.row_values(id) else {
                if !compact {
                    dead.insert(id);
                    base.push(None, None);
                }
                continue;
            };
            // Existing ownership wins on overlaps; this is storage ownership,
            // never a claim about the fact's producer or semantic identity.
            let old = self
                .virtual_rows
                .as_ref()
                .filter(|v| n < v.prefix())
                .and_then(|v| {
                    let r = v.base.recipe(n);
                    (r.instance != DIRECT).then(|| (v.base.instance(r.instance), r.row as usize))
                });
            let recipe = old.or_else(|| {
                selected
                    .get(&id)
                    .map(|r| ((*r.instance).clone(), r.template_row))
            });
            if let Some((instance, row)) = recipe {
                let width = instance.matrix.template.rows[row].len();
                let next = base.instances.len() as u32;
                let instance_id = *instances
                    .entry((Arc::as_ptr(&instance.matrix) as usize, instance.index))
                    .or_insert_with(|| {
                        let index = base.matrices.len() as u32;
                        let matrix = *matrices
                            .entry(Arc::as_ptr(&instance.matrix) as usize)
                            .or_insert_with(|| {
                                base.matrices.push(instance.matrix.clone());
                                index
                            });
                        base.instances.push(BindingRef {
                            matrix,
                            index: instance.index,
                        });
                        next
                    });
                let suffix = actual[width..].to_vec();
                let next = base.tails.len() as u32;
                let tail = *tails.entry(suffix.clone()).or_insert_with(|| {
                    base.tails.push(suffix.into_boxed_slice());
                    next
                });
                base.push(Some(Recipe {
                    instance: instance_id,
                    row: row.try_into().map_err(|_| "template row exceeds u32")?,
                    tail,
                }), None);
            } else {
                base.push(None, Some(&actual));
            }
        }
        base.finish();
        Ok(VirtualRows {
            base: Arc::new(base),
            dead,
            rebuild_rows_examined: self
                .virtual_rows
                .as_ref()
                .map_or(0, |v| v.rebuild_rows_examined),
            rebuild_rows_changed: self
                .virtual_rows
                .as_ref()
                .map_or(0, |v| v.rebuild_rows_changed),
        })
    }
    fn compact_virtual(&mut self) {
        if self.virtual_rows.as_ref().is_some_and(|v|v.base.cells.is_some()) { self.materialize_virtual(); return; }
        let data = self
            .encode_virtual_base(&BTreeMap::new(), true)
            .expect("previously validated virtual rows");
        self.virtual_rows = Some(data);
        self.data = NativeRows::Owned(Rows::new(RowBuffer::new(self.n_columns)));
        self.hash.clear();
        self.offsets.clear();
        self.generation = self.generation.inc();
        for n in 0..self.physical_len() {
            let id = RowId::from_usize(n);
            let row = self.row_values(id).unwrap().into_owned();
            let (shard, hc) = hash_code(self.hash.shard_data(), &row, self.n_keys);
            self.hash.mut_shards()[shard.index()].insert_unique(
                hc,
                TableEntry {
                    hashcode: hc,
                    row: id,
                },
                TableEntry::hashcode,
            );
            if let Some(col) = self.sort_by {
                let value = row[col.index()];
                if self.offsets.last().is_none_or(|(v, _)| *v != value) {
                    self.offsets.push((value, id));
                }
            }
        }
    }
    pub(super) fn materialize_virtual(&mut self) -> bool {
        if self.virtual_rows.is_none() {
            return false;
        }
        if self.virtual_rows.as_ref().is_some_and(|v|v.base.cells.is_some()){self.cell_fallbacks+=1;}
        let mut rows=(0..self.physical_len()).filter_map(|i|self.row_values(RowId::from_usize(i)).map(|r|r.into_owned())).collect::<Vec<_>>();
        if let Some(col)=self.sort_by {rows.sort_by_key(|r|r[col.index()]);}
        let mut data=Rows::new(RowBuffer::new(self.n_columns));
        for row in rows {data.add_row(&row);}
        self.virtual_rows = None;
        self.data = NativeRows::Owned(data);
        self.hash.clear();
        self.offsets.clear();
        self.generation = self.generation.inc();
        for i in 0..self.data.data.len() {
            let id = RowId::from_usize(i);
            let row = self.data.get_row(id).unwrap();
            let (shard, hc) = hash_code(self.hash.shard_data(), row, self.n_keys);
            self.hash.mut_shards()[shard.index()].insert_unique(
                hc,
                TableEntry {
                    hashcode: hc,
                    row: id,
                },
                TableEntry::hashcode,
            );
            if let Some(col) = self.sort_by {
                let value = row[col.index()];
                if self.offsets.last().is_none_or(|(v, _)| *v != value) {
                    self.offsets.push((value, id));
                }
            }
        }
        true
    }
    pub(super) fn virtual_stats(&self) -> Option<NativeVirtualStats> {
        let v = self.virtual_rows.as_ref()?;
        if v.base.cells.is_some() { return Some(self.cell_virtual_stats()); }
        let mut instances = BTreeMap::new();
        let mut templates = BTreeMap::new();
        let mut binding_headers = BTreeMap::new();
        let mut template_headers = BTreeMap::new();
        for i in &v.base.matrices {
            binding_headers.insert(
                Arc::as_ptr(i) as usize,
                std::mem::size_of::<BindingMatrix>() + 2 * std::mem::size_of::<usize>(),
            );
            template_headers.insert(
                Arc::as_ptr(&i.template) as usize,
                std::mem::size_of::<NativeRowTemplate>()
                    + 2 * std::mem::size_of::<usize>()
                    + i.template.rows.capacity() * std::mem::size_of::<Box<[usize]>>(),
            );
            instances.insert(
                Arc::as_ptr(i) as usize,
                i.values.len() * std::mem::size_of::<Value>(),
            );
            templates.insert(
                Arc::as_ptr(&i.template) as usize,
                i.template
                    .rows
                    .iter()
                    .map(|r| r.len() * std::mem::size_of::<usize>())
                    .sum(),
            );
        }
        let virtual_rows = (0..v.prefix()).filter(|i| v.base.recipe(*i).instance != DIRECT && !v.dead.contains(&RowId::from_usize(*i))).count();
        Some(NativeVirtualStats {
            shared_index_bytes: vec![],
            virtual_rows,
            direct_base_rows: v.prefix() - v.dead.len() - virtual_rows,
            overlay_rows: self.data.data.len() - self.data.stale_rows,
            tombstones: self.stale_count(),
            rebuild_rows_examined: v.rebuild_rows_examined,
            rebuild_rows_changed: v.rebuild_rows_changed,
            dense_equivalent_bytes: self.physical_len()
                * self.n_columns
                * std::mem::size_of::<Value>(),
            descriptor_bytes: v.base.pages.iter().map(|p| p.recipes.capacity() * std::mem::size_of::<Recipe>()).sum(),
            local_header_bytes: std::mem::size_of::<Base>()
                + 2 * std::mem::size_of::<usize>()
                + v.base.instances.capacity() * std::mem::size_of::<BindingRef>()
                + v.base.matrices.capacity() * std::mem::size_of::<Arc<BindingMatrix>>()
                + v.base.pages.capacity() * std::mem::size_of::<Page>() + v.dead.bits.as_slice().len() * std::mem::size_of::<usize>()
                + v.base.tails.capacity() * std::mem::size_of::<Box<[Value]>>(),
            binding_headers: binding_headers.into_iter().collect(),
            template_headers: template_headers.into_iter().collect(),
            direct_bytes: v.base.pages.iter().map(|p| p.direct.capacity() * std::mem::size_of::<Value>()).sum::<usize>()
                + self.data.data.len() * self.n_columns * std::mem::size_of::<Value>(),
            metadata_bytes: v
                .base
                .tails
                .iter()
                .map(|r| r.len() * std::mem::size_of::<Value>())
                .sum(),
            instances: instances.into_iter().collect(),
            templates: templates.into_iter().collect(),
        })
    }
    pub(super) fn merge_virtual(&mut self, exec: &mut ExecutionState) -> TableChange {
        self.pending_state.total_removals.swap(0, Ordering::Relaxed);
        let removals: Vec<_> = self
            .pending_state
            .pending_removals
            .iter()
            .flat_map(|(_, q)| std::iter::from_fn(move || q.pop()))
            .collect();
        let mut removed = false;
        for batch in removals {
            batch.for_each(|key| {
                let (shard, hc) = hash_code(self.hash.shard_data(), key, self.n_keys);
                let id = self.storage_lookup(key);
                if let Some(id) = id {
                    if !self.is_cell_row(id) {
                        self.hash.mut_shards()[shard.index()].find_entry(hc,|e|e.row==id).unwrap().remove();
                    }
                    self.pending_state.invalidate_origin(
                        key,
                        crate::trace::OriginInvalidationReason::RemovedOrRebuilt,
                    );
                    self.virtual_remove(id);
                    removed = true;
                }
            });
        }
        self.pending_state.total_rows.swap(0, Ordering::Relaxed);
        let batches: Vec<_> = self
            .pending_state
            .pending_rows
            .iter()
            .flat_map(|(_, q)| std::iter::from_fn(move || q.pop()))
            .collect();
        let mut added = false;
        let mut merged = Vec::new();
        for batch in batches {
            for (incoming, cause) in batch.traced_rows() {
                let key = &incoming[..self.n_keys];
                let old = self.storage_lookup(key);
                let outcome;
                let row = if let Some(id) = old {
                    let current = self.row_values(id).unwrap().into_owned();
                    merged.clear();
                    if !(self.merge)(exec, &current, incoming, &mut merged) {
                        self.pending_state.finish(
                            cause,
                            incoming,
                            &current,
                            crate::trace::WriteOutcome::Deduplicated,
                            self.n_keys,
                        );
                        continue;
                    }
                    outcome = crate::trace::WriteOutcome::Updated;
                    merged.as_slice()
                } else {
                    outcome = crate::trace::WriteOutcome::Inserted;
                    incoming
                };
                let id = self.virtual_append(row);
                if let Some(col) = self.sort_by {
                    let value = incoming[col.index()];
                    if let Some((largest, _)) = self.offsets.last() {
                        assert!(value >= *largest, "virtual write violates timestamp order");
                    }
                    if self.offsets.last().is_none_or(|(v, _)| *v != value) {
                        self.offsets.push((value, id));
                    }
                }
                let (shard, hc) = hash_code(self.hash.shard_data(), key, self.n_keys);
                if let Some(old) = old.filter(|id|!self.is_cell_row(*id)) {
                    self.virtual_remove(old);
                    self.hash.mut_shards()[shard.index()]
                        .find_mut(hc, |e| e.row == old)
                        .unwrap()
                        .row = id;
                } else {
                    if let Some(old)=old {self.virtual_remove(old);}
                    self.hash.mut_shards()[shard.index()].insert_unique(
                        hc,
                        TableEntry {
                            hashcode: hc,
                            row: id,
                        },
                        TableEntry::hashcode,
                    );
                }
                self.pending_state
                    .finish(cause, incoming, row, outcome, self.n_keys);
                added = true;
            }
        }
        // Reuse the existing native stale-row housekeeping policy. No new
        // template selection threshold or candidate budget is introduced.
        if self.stale_count() > cmp::max(16, self.physical_len() / 2) {
            self.compact_virtual();
        }
        TableChange { removed, added }
    }
    pub(super) fn rebuild_virtual(
        &mut self,
        table_id: TableId,
        table: &WrappedTable,
        next_ts: Value,
        _exec: &mut ExecutionState,
    ) -> bool {
        if self.to_rebuild.is_empty() {
            return false;
        }
        let Some(rebuilder) = table.rebuilder(&self.to_rebuild) else {
            return false;
        };
        let mut selected = BTreeSet::new();
        if let Some(col)=rebuilder.hint_col(){
            let recent=self.subset_tracker.recent_updates(table_id,table);let mut rows=crate::TaggedRowBuffer::new(1);
            table.scan_project(recent.as_ref(),&[col],Offset::new(0),usize::MAX,&[],&mut rows);
            let dirty=rows.iter().map(|(_,r)|r[0]).collect::<BTreeSet<_>>();
            if let Some(candidates)=self.compact_affected(&dirty){selected.extend(candidates);}
            else {self.refresh_rebuild_index();for value in dirty{if let Some(rows)=self.rebuild_index.get_subset(&value){rows.offsets(|id|{selected.insert(id);});}}}
        } else {selected.extend((0..self.physical_len()).map(RowId::from_usize));}
        let mut buffer = self.new_buffer();
        let mut changed = false;
        let mut examined = 0;
        let mut rewritten = 0;
        for id in selected {
            let Some(before) = self.row_values(id) else {
                continue;
            };
            examined += 1;
            let mut after = before.to_vec();
            for col in &self.to_rebuild {
                after[col.index()] = rebuilder.rebuild_val(after[col.index()]);
            }
            if after == *before {
                continue;
            }
            buffer.stage_remove(&before[..self.n_keys]);
            if let Some(col) = self.sort_by {
                after[col.index()] = next_ts;
            }
            if let Some(cause) = self.rebuild_origin(id, &after) {
                buffer.stage_insert_with_cause(&after, cause);
            } else {
                buffer.stage_insert(&after);
            }
            changed = true;
            rewritten += 1;
        }
        let stats = self.virtual_rows.as_mut().unwrap();
        stats.rebuild_rows_examined += examined;
        stats.rebuild_rows_changed += rewritten;
        changed
    }
}

impl SortedWritesTable {
    pub(super) fn install_cells(&mut self, table:TableId, batches:&[Arc<NativeCellBatch>])->Result<(),String> {
        let mut selected=BTreeSet::new();let mut segments=vec![];
        for batch in batches {
            for (row,spec) in batch.rows.iter().enumerate().filter(|(_,r)|r.table==table) {
                let width=batch.template.row_slots(spec.template_row).len();
                if width+spec.tail.len()!=self.n_columns || self.sort_by.is_some_and(|c|c.index()<width) {return Err("cell metadata does not describe a native row".into());}
                for i in 0..batch.count(){
                    let expected=batch.row(i,row);let found=self.storage_lookup(&expected[..self.n_keys]).ok_or("cell fact missing")?;
                    if self.is_cell_row(found) || self.row_values(found).as_deref()!=Some(expected.as_slice()) || !selected.insert(found) {return Err("cell fact changed or multiple owners".into());}
                }
                segments.push(CellSegment::Cell{batch:batch.clone(),row,hidden:vec![]});
            }
        }
        if selected.is_empty(){return Ok(());}
        if let Some(v)=&self.virtual_rows {if let Some(c)=&v.base.cells {
            for (p,s) in c.segments.iter().enumerate(){if let CellSegment::Cell{batch,row,..}=s {
                let hidden=(0..s.len()).filter(|i|v.dead.contains(&RowId::from_usize(c.starts[p]+i))).map(|i|i as u32).collect::<Vec<_>>();
                if hidden.len()<s.len(){segments.push(CellSegment::Cell{batch:batch.clone(),row:*row,hidden});}
            }}
        }}
        let mut direct=BTreeMap::<Value,Vec<Value>>::new();
        for i in 0..self.physical_len(){let id=RowId::from_usize(i);
            if selected.contains(&id)||self.is_cell_row(id){continue;}
            if let Some(row)=self.row_values(id){let stamp=self.sort_by.map_or(Value::new_const(0),|c|row[c.index()]);direct.entry(stamp).or_default().extend_from_slice(&row);}
        }
        for (_,rows) in direct {segments.push(CellSegment::Direct{values:rows.into_boxed_slice(),width:self.n_columns});}
        if let Some(c)=self.sort_by{segments.sort_by_key(|s|s.value(0,c.index()).unwrap());}
        let mut cells=CellRows::new(segments)?;
        let mut uniforms=vec![None;self.n_columns];let mut first=true;
        for i in 0..self.physical_len(){if let Some(row)=self.row_values(RowId::from_usize(i)){
            for (c,v) in row.iter().enumerate(){if first{uniforms[c]=Some(*v);}else if uniforms[c]!=Some(*v){uniforms[c]=None;}}
            first=false;
        }}cells.uniforms=uniforms;
        let mut dead=DeadRows::default();self.offsets.clear();
        let mut hash=ShardedHashTable::with_shards(self.hash.shard_data().n_shards());
        for (p,s) in cells.segments.iter().enumerate(){
            if let CellSegment::Cell{hidden,..}=s {for i in hidden{dead.insert(RowId::from_usize(cells.starts[p]+*i as usize));}}
            else {for i in 0..s.len(){let r=s.row(i);let id=RowId::from_usize(cells.starts[p]+i);let(shard,hc)=hash_code(hash.shard_data(),&r,self.n_keys);hash.mut_shards()[shard.index()].insert_unique(hc,TableEntry{hashcode:hc,row:id},TableEntry::hashcode);}}
            if let Some(col)=self.sort_by{let stamp=s.value(0,col.index()).unwrap();if self.offsets.last().is_none_or(|(v,_)|*v!=stamp){self.offsets.push((stamp,RowId::from_usize(cells.starts[p])));}}
        }
        let len=cells.len;
        self.virtual_rows=Some(VirtualRows{base:Arc::new(Base{cells:Some(cells),len,width:self.n_columns,..Default::default()}),dead,rebuild_rows_examined:0,rebuild_rows_changed:0});
        self.cell_mode=true;self.hash=hash;self.data=NativeRows::Owned(Rows::new(RowBuffer::new_empty(self.n_columns)));
        self.rebuild_index=Index::new(self.to_rebuild.clone(),ColumnIndex::new());
        self.generation=self.generation.inc();Ok(())
    }
    pub(super) fn cell_constraint(&self,c:&Constraint)->Subset {
        if let Constraint::EqConst{col,val}=c {if let Some(cells)=self.virtual_rows.as_ref().and_then(|v|v.base.cells.as_ref()) {
            if cells.uniforms.get(col.index())==Some(&Some(*val)) && (0..self.data.data.len()).all(|i|self.data.get_row(RowId::from_usize(i)).is_none_or(|r|r[col.index()]==*val)) {return self.all();}
        }}
        let mut result=Subset::empty();
        for i in 0..self.physical_len(){let id=RowId::from_usize(i);let Some(row)=self.row_values(id) else{continue};
            let keep=match c {
                Constraint::Eq{l_col,r_col}=>row[l_col.index()]==row[r_col.index()],
                Constraint::EqConst{col,val}=>row[col.index()]==*val,
                Constraint::LtConst{col,val}=>row[col.index()]<*val,
                Constraint::LeConst{col,val}=>row[col.index()]<=*val,
                Constraint::GtConst{col,val}=>row[col.index()]>*val,
                Constraint::GeConst{col,val}=>row[col.index()]>=*val,
            };if keep{result.add_row_sorted(id);}
        }result
    }
    pub(super) fn cell_stats(&self)->Option<NativeCellStats>{
        let Some(v)=self.virtual_rows.as_ref().filter(|v|v.base.cells.is_some()) else {return self.cell_mode.then(||NativeCellStats{materialized_fallbacks:self.cell_fallbacks,native_hash_rows:self.hash.entry_count(),..Default::default()});};let c=v.base.cells.as_ref().unwrap();
        let mut groups=BTreeMap::new();let mut rows=0;
        for (p,s) in c.segments.iter().enumerate(){if let CellSegment::Cell{batch,..}=s{
            groups.insert(Arc::as_ptr(batch) as usize,batch);
            rows+=(0..s.len()).filter(|i|!v.dead.contains(&RowId::from_usize(c.starts[p]+i))).count();
        }}
        Some(NativeCellStats{materialized_fallbacks:self.cell_fallbacks,cells:groups.values().map(|b|b.count()).sum(),projections:c.segments.len(),rows,native_hash_rows:self.hash.entry_count(),projection_bytes:c.segments.capacity()*std::mem::size_of::<CellSegment>()+c.starts.capacity()*std::mem::size_of::<usize>()+c.uniforms.capacity()*std::mem::size_of::<Option<Value>>()+c.segments.iter().map(|s|match s{CellSegment::Cell{hidden,..}=>hidden.capacity()*std::mem::size_of::<u32>(),_=>0}).sum::<usize>(),bindings:groups.iter().map(|(id,b)|(*id,b.binding_bytes())).collect(),binding_indexes:groups.iter().map(|(id,b)|(*id,b.index_bytes())).collect(),bodies:groups.iter().map(|(id,b)|(*id,b.header_bytes())).collect(),codes:groups.values().flat_map(|b|[(Arc::as_ptr(&b.full_tables) as *const TableId as usize,b.full_tables.len()*std::mem::size_of::<TableId>()+2*std::mem::size_of::<usize>()),(Arc::as_ptr(&b.template) as usize,std::mem::size_of::<NativeRowTemplate>()+2*std::mem::size_of::<usize>()+b.template.rows.capacity()*std::mem::size_of::<Box<[usize]>>()+b.template.rows.iter().map(|r|r.len()*std::mem::size_of::<usize>()).sum::<usize>())]).collect()})
    }
    fn cell_virtual_stats(&self)->NativeVirtualStats{
        let v=self.virtual_rows.as_ref().unwrap();let c=v.base.cells.as_ref().unwrap();let stats=self.cell_stats().unwrap();
        let mut templates=BTreeMap::new();
        for (id,bytes) in &stats.codes {templates.insert(*id,*bytes);}
        for s in &c.segments {if let CellSegment::Cell{batch,..}=s{templates.insert(Arc::as_ptr(&batch.template) as usize,batch.template.rows.iter().map(|r|r.len()*std::mem::size_of::<usize>()).sum());}}
        let mut template_headers=BTreeMap::new();for seg in &c.segments {if let CellSegment::Cell{batch,..}=seg {template_headers.insert(Arc::as_ptr(&batch.template) as usize,std::mem::size_of::<NativeRowTemplate>()+2*std::mem::size_of::<usize>()+batch.template.rows.capacity()*std::mem::size_of::<Box<[usize]>>());}}
        NativeVirtualStats{shared_index_bytes:stats.binding_indexes,virtual_rows:stats.rows,direct_base_rows:v.prefix()-v.dead.len()-stats.rows,overlay_rows:self.data.data.len()-self.data.stale_rows,tombstones:self.stale_count(),rebuild_rows_examined:v.rebuild_rows_examined,rebuild_rows_changed:v.rebuild_rows_changed,dense_equivalent_bytes:self.physical_len()*self.n_columns*std::mem::size_of::<Value>(),descriptor_bytes:stats.projection_bytes,local_header_bytes:std::mem::size_of::<Base>()+v.dead.bits.as_slice().len()*std::mem::size_of::<usize>(),direct_bytes:c.segments.iter().map(|s|match s{CellSegment::Direct{values,..}=>values.len()*std::mem::size_of::<Value>(),_=>0}).sum::<usize>()+self.data.data.len()*self.n_columns*std::mem::size_of::<Value>(),metadata_bytes:0,instances:stats.bindings,templates:templates.into_iter().collect(),binding_headers:stats.bodies,template_headers:template_headers.into_iter().collect()}
    }
}

impl SortedWritesTable {
    pub(super) fn compact_affected(&self,dirty:&BTreeSet<Value>)->Option<Vec<RowId>> {
        let v=self.virtual_rows.as_ref()?;let c=v.base.cells.as_ref()?;
        let mut rows=c.affected(&self.to_rebuild,dirty);
        for i in 0..self.data.data.len(){if self.data.get_row(RowId::from_usize(i)).is_some_and(|r|self.to_rebuild.iter().any(|col|dirty.contains(&r[col.index()]))) {rows.push(RowId::from_usize(v.prefix()+i));}}
        Some(rows)
    }
}

impl SortedWritesTable {
    pub(super) fn validate_cells(&self,table:TableId,cells:&[Arc<NativeCellBatch>])->Result<(),String>{
        let mut seen=BTreeSet::new();
        for b in cells {for (i,r) in b.rows.iter().enumerate().filter(|(_,r)|r.table==table) {
            let w=b.template.row_slots(r.template_row).len();
            if w+r.tail.len()!=self.n_columns||w<self.n_keys||self.sort_by.is_some_and(|c|c.index()<w){return Err("unsupported compact-cell row layout".into());}
            for n in 0..b.count(){let row=b.row(n,i);let key=&row[..self.n_keys];let id=self.storage_lookup(key).ok_or("cell fact missing")?;
                if self.is_cell_row(id)||!seen.insert(id)||self.row_values(id).as_deref()!=Some(row.as_slice()){return Err("cell fact is not uniquely owned".into());}
            }
        }}Ok(())
    }
}
