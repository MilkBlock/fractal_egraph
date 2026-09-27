//! Verified storage recipes for already committed rows. Keys, row IDs, timestamps
//! and merge semantics remain native. The base is immutable; mutations use an
//! ordinary dense append-only overlay plus tombstones.
use super::*;
use crate::WrappedTable;
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
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
#[derive(Default)]
struct Base {
    rows: Vec<Recipe>,
    instances: Vec<BindingRef>,
    matrices: Vec<Arc<BindingMatrix>>,
    direct: Vec<Box<[Value]>>,
    tails: Vec<Box<[Value]>>,
}
impl Base {
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
#[derive(Clone)]
pub(super) struct VirtualRows {
    base: Arc<Base>,
    dead: HashSet<RowId>,
    rebuild_rows_examined: usize,
    rebuild_rows_changed: usize,
}
impl VirtualRows {
    fn prefix(&self) -> usize {
        self.base.rows.len()
    }
    fn value(&self, row: RowId, col: usize) -> Option<Value> {
        if self.dead.contains(&row) {
            return None;
        }
        let r = self.base.rows.get(row.index())?;
        if r.instance == DIRECT {
            return self.base.direct.get(r.row as usize)?.get(col).copied();
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
        let r = self.base.rows.get(row.index())?;
        if r.instance == DIRECT {
            return Some(Cow::Borrowed(&self.base.direct[r.row as usize]));
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
        let r = v.base.rows[row.index()];
        if r.instance == DIRECT {
            return &v.base.direct[r.row as usize][..key.len()] == key;
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
        let r = v.base.rows[row.index()];
        if r.instance == DIRECT {
            out.extend_from_slice(&v.base.direct[r.row as usize]);
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
    pub(super) fn is_virtual_key(&self, key: &[Value]) -> bool {
        let Some(v) = &self.virtual_rows else {
            return false;
        };
        get_entry(key, self.n_keys, &self.hash, |id| {
            self.virtual_key_matches(id, key)
        })
        .is_some_and(|id| id.index() < v.prefix() && v.base.rows[id.index()].instance != DIRECT)
    }
    pub(super) fn pack_rows(&mut self, requests: &[VirtualRowRequest]) -> Result<(), String> {
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
        let mut base = Base::default();
        let mut dead = HashSet::new();
        let mut instances = HashMap::new();
        let mut matrices = HashMap::new();
        let mut tails = BTreeMap::new();
        for n in 0..self.physical_len() {
            let id = RowId::from_usize(n);
            let Some(actual) = self.row_values(id) else {
                if !compact {
                    dead.insert(id);
                    base.rows.push(Recipe {
                        instance: DIRECT,
                        row: 0,
                        tail: 0,
                    });
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
                    let r = v.base.rows[n];
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
                base.rows.push(Recipe {
                    instance: instance_id,
                    row: row.try_into().map_err(|_| "template row exceeds u32")?,
                    tail,
                });
            } else {
                let row = base.direct.len() as u32;
                base.direct.push(actual.into_owned().into_boxed_slice());
                base.rows.push(Recipe {
                    instance: DIRECT,
                    row,
                    tail: 0,
                });
            }
        }
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
        let mut data = Rows::new(RowBuffer::new(self.n_columns));
        for i in 0..self.physical_len() {
            if let Some(row) = self.row_values(RowId::from_usize(i)) {
                data.add_row(&row);
            }
        }
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
        let virtual_rows = v
            .base
            .rows
            .iter()
            .enumerate()
            .filter(|(i, r)| r.instance != DIRECT && !v.dead.contains(&RowId::from_usize(*i)))
            .count();
        Some(NativeVirtualStats {
            virtual_rows,
            direct_base_rows: v.prefix() - v.dead.len() - virtual_rows,
            overlay_rows: self.data.data.len() - self.data.stale_rows,
            tombstones: self.stale_count(),
            rebuild_rows_examined: v.rebuild_rows_examined,
            rebuild_rows_changed: v.rebuild_rows_changed,
            dense_equivalent_bytes: self.physical_len()
                * self.n_columns
                * std::mem::size_of::<Value>(),
            descriptor_bytes: v.base.rows.capacity() * std::mem::size_of::<Recipe>(),
            local_header_bytes: std::mem::size_of::<Base>()
                + 2 * std::mem::size_of::<usize>()
                + v.base.instances.capacity() * std::mem::size_of::<BindingRef>()
                + v.base.matrices.capacity() * std::mem::size_of::<Arc<BindingMatrix>>()
                + v.base.direct.capacity() * std::mem::size_of::<Box<[Value]>>()
                + v.base.tails.capacity() * std::mem::size_of::<Box<[Value]>>(),
            binding_headers: binding_headers.into_iter().collect(),
            template_headers: template_headers.into_iter().collect(),
            direct_bytes: v
                .base
                .direct
                .iter()
                .map(|r| r.len() * std::mem::size_of::<Value>())
                .sum::<usize>()
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
                let id = get_entry(key, self.n_keys, &self.hash, |r| {
                    self.row_values(r)
                        .is_some_and(|row| &row[..self.n_keys] == key)
                });
                if let Some(id) = id {
                    self.hash.mut_shards()[shard.index()]
                        .find_entry(hc, |e| e.row == id)
                        .unwrap()
                        .remove();
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
                let old = get_entry(key, self.n_keys, &self.hash, |r| {
                    self.row_values(r)
                        .is_some_and(|row| &row[..self.n_keys] == key)
                });
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
                if let Some(old) = old {
                    self.virtual_remove(old);
                    self.hash.mut_shards()[shard.index()]
                        .find_mut(hc, |e| e.row == old)
                        .unwrap()
                        .row = id;
                } else {
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
        self.refresh_rebuild_index();
        let mut selected = BTreeSet::new();
        if let Some(col) = rebuilder.hint_col() {
            let recent = self.subset_tracker.recent_updates(table_id, table);
            let mut dirty = crate::TaggedRowBuffer::new(1);
            table.scan_project(
                recent.as_ref(),
                &[col],
                Offset::new(0),
                usize::MAX,
                &[],
                &mut dirty,
            );
            for (_, row) in dirty.iter() {
                if let Some(rows) = self.rebuild_index.get_subset(&row[0]) {
                    rows.offsets(|id| {
                        selected.insert(id);
                    });
                }
            }
        } else {
            selected.extend((0..self.physical_len()).map(RowId::from_usize));
        }
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
