//! Whole-cell batches: one shared body, binding matrix, and per-body metadata.
//! A projected fact's RowId is segment.start + instance; no per-fact descriptor.
use super::virtual_rows::NativeRowTemplate;
use super::*;
use std::sync::OnceLock;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NativeCellRow {
    pub table: TableId,
    pub template_row: usize,
    pub tail: Vec<Value>,
}
#[derive(Debug)]
enum CellColumn {
    Uniform(Value),
    Affine(Value, i64),
    Packed {
        base: usize,
        bits: usize,
        words: Box<[u64]>,
    },
}
impl CellColumn {
    fn value(&self, i: usize) -> Value {
        match self {
            Self::Uniform(v) => *v,
            Self::Affine(base, step) => {
                Value::from_usize((base.index() as i128 + i as i128 * *step as i128) as usize)
            }
            Self::Packed { base, bits, words } => {
                let bit = i * bits;
                let offset = bit % 64;
                let mut v = words[bit / 64] >> offset;
                if offset + bits > 64 {
                    v |= words[bit / 64 + 1] << (64 - offset);
                }
                Value::from_usize(base + (v & ((1u64 << bits) - 1)) as usize)
            }
        }
    }
    fn bytes(&self) -> usize {
        match self {
            Self::Packed { words, .. } => words.len() * 8,
            _ => 0,
        }
    }
}
#[derive(Debug)]
enum CellBindings {
    Flat { values: Box<[Value]>, slots: usize },
    Columns(Box<[CellColumn]>),
}
impl CellBindings {
    fn new(rows: Vec<Vec<Value>>, slots: usize) -> Self {
        let n = rows.len();
        let mut columns = vec![];
        for slot in 0..slots {
            let base = rows[0][slot];
            if rows.iter().all(|r| r[slot] == base) {
                columns.push(CellColumn::Uniform(base));
                continue;
            }
            let step = rows[1][slot].index() as i64 - base.index() as i64;
            if rows.iter().enumerate().all(|(i, r)| {
                r[slot].index() as i128 == base.index() as i128 + i as i128 * step as i128
            }) {
                columns.push(CellColumn::Affine(base, step));
                continue;
            }
            let min = rows.iter().map(|r| r[slot].index()).min().unwrap();
            let max = rows.iter().map(|r| r[slot].index()).max().unwrap();
            let bits = (usize::BITS - (max - min).leading_zeros()) as usize;
            let mut words = vec![0u64; (n * bits + 63) / 64];
            for (i, row) in rows.iter().enumerate() {
                let v = (row[slot].index() - min) as u64;
                let bit = i * bits;
                let off = bit % 64;
                words[bit / 64] |= v << off;
                if off + bits > 64 {
                    words[bit / 64 + 1] |= v >> (64 - off);
                }
            }
            columns.push(CellColumn::Packed {
                base: min,
                bits,
                words: words.into_boxed_slice(),
            });
        }
        let bytes = columns.len() * std::mem::size_of::<CellColumn>()
            + columns.iter().map(CellColumn::bytes).sum::<usize>();
        if bytes < n * slots * std::mem::size_of::<Value>() {
            Self::Columns(columns.into_boxed_slice())
        } else {
            Self::Flat {
                values: rows
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
                slots,
            }
        }
    }
    fn value(&self, i: usize, slot: usize) -> Value {
        match self {
            Self::Flat { values, slots } => values[i * slots + slot],
            Self::Columns(cols) => cols[slot].value(i),
        }
    }
    fn bytes(&self) -> usize {
        match self {
            Self::Flat { values, .. } => values.len() * std::mem::size_of::<Value>(),
            Self::Columns(cols) => {
                cols.len() * std::mem::size_of::<CellColumn>()
                    + cols.iter().map(CellColumn::bytes).sum::<usize>()
            }
        }
    }
}
enum Instances<'a> {
    Range(std::ops::Range<u32>),
    Slice(std::iter::Copied<std::slice::Iter<'a, u32>>),
}
impl Iterator for Instances<'_> {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {
        match self {
            Self::Range(r) => r.next(),
            Self::Slice(s) => s.next(),
        }
    }
}
#[derive(Debug)]
pub struct NativeCellBatch {
    pub template: Arc<NativeRowTemplate>,
    pub full_tables: Arc<[TableId]>,
    pub rows: Vec<NativeCellRow>,
    bindings: CellBindings,
    count: usize,
    bounds: Box<[(Value, Value)]>,
    // Index cell instances, not expanded fact rows; keys stay in bindings.
    slot_orders: Vec<OnceLock<Box<[u32]>>>,
}
impl NativeCellBatch {
    pub fn new(
        template: Arc<NativeRowTemplate>,
        full_tables: Arc<[TableId]>,
        rows: Vec<NativeCellRow>,
        bindings: Vec<Vec<Value>>,
    ) -> Result<Arc<Self>, String> {
        if full_tables.len() != template.rows()
            || rows.iter().any(|r| {
                r.template_row >= full_tables.len() || full_tables[r.template_row] != r.table
            })
            || rows.is_empty()
            || bindings.is_empty()
            || bindings.len() > u32::MAX as usize
            || bindings.iter().any(|b| b.len() != template.slots())
            || rows.iter().any(|r| r.template_row >= template.rows())
        {
            return Err("invalid compact cell batch".into());
        }
        let count = bindings.len();
        let slots = template.slots();
        let bounds = (0..slots)
            .map(|s| {
                (
                    bindings.iter().map(|b| b[s]).min().unwrap(),
                    bindings.iter().map(|b| b[s]).max().unwrap(),
                )
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Ok(Arc::new(Self {
            template,
            full_tables,
            rows,
            bindings: CellBindings::new(bindings, slots),
            count,
            bounds,
            slot_orders: (0..slots).map(|_| OnceLock::new()).collect(),
        }))
    }
    pub fn count(&self) -> usize {
        self.count
    }
    pub fn value(&self, instance: usize, slot: usize) -> Value {
        self.bindings.value(instance, slot)
    }
    pub fn row_value(&self, instance: usize, row: usize, col: usize) -> Option<Value> {
        let r = &self.rows[row];
        let cols = self.template.row_slots(r.template_row);
        if col < cols.len() {
            Some(self.value(instance, cols[col]))
        } else {
            r.tail.get(col - cols.len()).copied()
        }
    }
    pub fn append_row(&self, instance: usize, row: usize, out: &mut Vec<Value>) {
        let r = &self.rows[row];
        out.extend(
            self.template
                .row_slots(r.template_row)
                .iter()
                .map(|c| self.value(instance, *c)),
        );
        out.extend_from_slice(&r.tail);
    }
    pub fn row(&self, instance: usize, row: usize) -> Vec<Value> {
        let mut out = vec![];
        self.append_row(instance, row, &mut out);
        out
    }
    fn instances_for(&self, slot: usize, value: Value) -> Instances<'_> {
        if let CellBindings::Columns(cols) = &self.bindings {
            match cols[slot] {
                CellColumn::Uniform(v) => {
                    return Instances::Range(0..if v == value { self.count as u32 } else { 0 });
                }
                CellColumn::Affine(base, step) => {
                    let delta = value.index() as i64 - base.index() as i64;
                    if step != 0 && delta % step == 0 {
                        let i = delta / step;
                        if i >= 0 && (i as usize) < self.count {
                            return Instances::Range(i as u32..i as u32 + 1);
                        }
                    }
                    return Instances::Range(0..0);
                }
                _ => {}
            }
        }
        let ids = self.slot_orders[slot].get_or_init(|| {
            let mut ids = (0..self.count as u32).collect::<Vec<_>>();
            ids.sort_unstable_by_key(|i| self.value(*i as usize, slot));
            ids.into_boxed_slice()
        });
        let lo = ids.partition_point(|i| self.value(*i as usize, slot) < value);
        let hi = ids.partition_point(|i| self.value(*i as usize, slot) <= value);
        Instances::Slice(ids[lo..hi].iter().copied())
    }
    pub fn binding_bytes(&self) -> usize {
        self.bindings.bytes()
    }
    pub fn index_bytes(&self) -> usize {
        self.slot_orders
            .iter()
            .filter_map(|s| s.get())
            .map(|s| s.len() * std::mem::size_of::<u32>())
            .sum()
    }
    pub fn header_bytes(&self) -> usize {
        self.bounds.len() * std::mem::size_of::<(Value, Value)>()
            + std::mem::size_of::<Self>()
            + 2 * std::mem::size_of::<usize>()
            + self.rows.capacity() * std::mem::size_of::<NativeCellRow>()
            + self
                .rows
                .iter()
                .map(|r| r.tail.capacity() * std::mem::size_of::<Value>())
                .sum::<usize>()
            + self.slot_orders.capacity() * std::mem::size_of::<OnceLock<Box<[u32]>>>()
    }
}
#[derive(Clone)]
pub(super) enum CellSegment {
    Cell {
        batch: Arc<NativeCellBatch>,
        row: usize,
        hidden: Vec<u32>,
    },
    Direct {
        values: Box<[Value]>,
        width: usize,
    },
}
impl CellSegment {
    pub fn len(&self) -> usize {
        match self {
            Self::Cell { batch, .. } => batch.count(),
            Self::Direct { values, width } => values.len() / width,
        }
    }
    pub fn value(&self, i: usize, col: usize) -> Option<Value> {
        match self {
            Self::Cell { batch, row, .. } => batch.row_value(i, *row, col),
            Self::Direct { values, width } => values.get(i * width + col).copied(),
        }
    }
    pub fn row(&self, i: usize) -> std::borrow::Cow<'_, [Value]> {
        match self {
            Self::Cell { batch, row, .. } => std::borrow::Cow::Owned(batch.row(i, *row)),
            Self::Direct { values, width } => {
                std::borrow::Cow::Borrowed(&values[i * width..(i + 1) * width])
            }
        }
    }
}
pub(super) struct CellRows {
    pub segments: Vec<CellSegment>,
    pub starts: Vec<usize>,
    pub len: usize,
    pub uniforms: Vec<Option<Value>>,
}
impl CellRows {
    pub fn new(segments: Vec<CellSegment>) -> Result<Self, String> {
        let mut starts = Vec::with_capacity(segments.len());
        let mut len = 0usize;
        for s in &segments {
            starts.push(len);
            len = len.checked_add(s.len()).ok_or("cell row overflow")?;
        }
        if len >= u32::MAX as usize {
            return Err("cell row-ID space exhausted".into());
        }
        Ok(Self {
            segments,
            starts,
            len,
            uniforms: vec![],
        })
    }
    fn position(&self, n: usize) -> (usize, usize) {
        let p = self.starts.partition_point(|s| *s <= n) - 1;
        (p, n - self.starts[p])
    }
    pub fn copy_row(&self, n: usize, out: &mut Vec<Value>) {
        let (p, i) = self.position(n);
        match &self.segments[p] {
            CellSegment::Cell { batch, row, .. } => batch.append_row(i, *row, out),
            CellSegment::Direct { .. } => out.extend_from_slice(&self.segments[p].row(i)),
        }
    }
    pub fn row(&self, n: usize) -> std::borrow::Cow<'_, [Value]> {
        let (p, i) = self.position(n);
        self.segments[p].row(i)
    }
    pub fn value(&self, n: usize, col: usize) -> Option<Value> {
        let (p, i) = self.position(n);
        self.segments[p].value(i, col)
    }
    pub fn is_cell(&self, n: usize) -> bool {
        let (p, _) = self.position(n);
        matches!(self.segments[p], CellSegment::Cell { .. })
    }
    pub fn lookup(&self, key: &[Value], mut live: impl FnMut(usize) -> bool) -> Option<usize> {
        for (p, segment) in self.segments.iter().enumerate() {
            let CellSegment::Cell { batch, row, .. } = segment else {
                continue;
            };
            let cols = batch.template.row_slots(batch.rows[*row].template_row);
            if key.len() > cols.len() {
                continue;
            }
            if key.iter().enumerate().any(|(i, v)| {
                let (lo, hi) = batch.bounds[cols[i]];
                *v < lo || *v > hi
            }) {
                continue;
            }

            let test = |i: usize| {
                key.iter()
                    .enumerate()
                    .all(|(c, v)| batch.value(i, cols[c]) == *v)
            };
            if key.is_empty() {
                for i in 0..batch.count() {
                    if live(self.starts[p] + i) && test(i) {
                        return Some(self.starts[p] + i);
                    }
                }
            } else {
                let column = if let CellBindings::Columns(enc) = &batch.bindings {
                    cols[..key.len()]
                        .iter()
                        .position(|s| !matches!(enc[*s], CellColumn::Uniform(_)))
                        .unwrap_or(0)
                } else {
                    0
                };
                for i in batch.instances_for(cols[column], key[column]) {
                    let n = self.starts[p] + i as usize;
                    if live(n) && test(i as usize) {
                        return Some(n);
                    }
                }
            }
        }
        None
    }
    pub fn affected(
        &self,
        cols: &[ColumnId],
        dirty: &std::collections::BTreeSet<Value>,
    ) -> Vec<RowId> {
        let mut out = vec![];
        for (p, s) in self.segments.iter().enumerate() {
            match s {
                CellSegment::Cell { batch, row, .. } => {
                    let slots = batch.template.row_slots(batch.rows[*row].template_row);
                    for c in cols {
                        if let Some(slot) = slots.get(c.index()) {
                            for v in dirty {
                                for i in batch.instances_for(*slot, *v) {
                                    out.push(RowId::from_usize(self.starts[p] + i as usize));
                                }
                            }
                        }
                    }
                }
                CellSegment::Direct { .. } => {
                    for i in 0..s.len() {
                        if cols
                            .iter()
                            .any(|c| s.value(i, c.index()).is_some_and(|v| dirty.contains(&v)))
                        {
                            out.push(RowId::from_usize(self.starts[p] + i));
                        }
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}

#[derive(Debug, Default)]
pub struct NativeCellStats {
    pub materialized_fallbacks: usize,
    pub cells: usize,
    pub projections: usize,
    pub rows: usize,
    pub native_hash_rows: usize,
    pub projection_bytes: usize,
    pub bindings: Vec<(usize, usize)>,
    pub binding_indexes: Vec<(usize, usize)>,
    pub bodies: Vec<(usize, usize)>,
    pub codes: Vec<(usize, usize)>,
}

#[cfg(test)]
mod encoding_tests {
    use super::*;
    #[test]
    fn lossless_columns_and_instance_indexes_cover_bit_boundaries() {
        for modulus in [511usize, (1usize << 31) - 1, u32::MAX as usize - 1] {
            let values = (0..300usize)
                .map(|i| {
                    vec![
                        Value::from_usize(17),
                        Value::from_usize(2000 + i * 3),
                        Value::from_usize(10000 - i * 2),
                        Value::from_usize((i * 2654435761usize) % modulus),
                    ]
                })
                .collect::<Vec<_>>();
            let t = NativeRowTemplate::new(vec![vec![0, 1, 2, 3]]).unwrap();
            let c = NativeCellBatch::new(
                t,
                vec![TableId::new(0)].into(),
                vec![NativeCellRow {
                    table: TableId::new(0),
                    template_row: 0,
                    tail: vec![],
                }],
                values.clone(),
            )
            .unwrap();
            assert!(c.binding_bytes() < 300 * 4 * std::mem::size_of::<Value>());
            for (i, row) in values.iter().enumerate() {
                assert_eq!(c.row(i, 0), *row);
            }
            for slot in 0..4 {
                for target in [values[0][slot], values[29][slot], Value::from_usize(99)] {
                    let a = c
                        .instances_for(slot, target)
                        .collect::<std::collections::BTreeSet<_>>();
                    let b = values
                        .iter()
                        .enumerate()
                        .filter(|(_, r)| r[slot] == target)
                        .map(|(i, _)| i as u32)
                        .collect();
                    assert_eq!(a, b);
                }
            }
        }
    }
}
