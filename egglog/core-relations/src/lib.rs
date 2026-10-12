#[macro_use]
#[cfg(test)]
pub(crate) mod table_shortcuts;
#[macro_use]
pub(crate) mod action;
pub(crate) mod base_values;
pub(crate) mod common;
pub(crate) mod containers;
pub(crate) mod dependency_graph;
pub(crate) mod free_join;
pub(crate) mod hash_index;
#[cfg(feature = "join-work-profile")]
pub(crate) mod join_work_profile;
pub(crate) mod offsets;
pub(crate) mod parallel_heuristics;
pub(crate) mod pool;
pub(crate) mod query;
#[cfg(feature = "table-read-profile")]
pub mod read_profile;
pub(crate) mod row_buffer;
pub(crate) mod table;
pub mod trace;

pub(crate) mod table_spec;
pub(crate) mod uf;

#[cfg(test)]
mod tests;

pub use action::{ExecutionState, MergeVal, QueryEntry, WriteVal};
pub use base_values::{BaseValue, BaseValueId, BaseValuePrinter, BaseValues, Boxed};
pub use common::Value;
pub use containers::{ContainerRebuildSummary, ContainerValue, ContainerValueId, ContainerValues};
pub use free_join::{
    AtomId, CounterId, Database, ExternalFunction, ExternalFunctionId, TableId, Variable,
    make_external_func, plan::PlanStrategy,
};
pub use hash_index::TupleIndex;
pub use offsets::{OffsetRange, RowId, Subset, SubsetRef};
pub use pool::{Pool, PoolSet, Pooled};
pub use query::{
    CachedPlan, QueryBuilder, QueryError, RuleBuilder, RuleId, RuleSet, RuleSetBuilder,
};
pub use row_buffer::TaggedRowBuffer;
pub use table::virtual_rows::{
    NativeRowInstance, NativeRowTemplate, NativeVirtualStats, VirtualRowRequest,
};
pub use table::{MergeFn, SortedWritesTable};
pub use table_spec::{
    ColumnId, Constraint, MutationBuffer, Offset, Rebuilder, Row, Table, TableChange, TableSpec,
    TableVersion, WrappedTable,
};
pub use trace::{
    OriginInvalidation, OriginInvalidationReason, RowReadEvent, RuleActionOutcome,
    RuleActionOutcomeEvent, RuleMatchBinding, RuleMatchEvent, TraceBatch, TraceCause, TraceSession,
    WriteEvent, WriteOutcome,
};
pub use uf::DisplacedTable;

use egglog_numeric_id as numeric_id;
use egglog_union_find as union_find;

pub use table::cell_rows::{NativeCellBatch, NativeCellRow, NativeCellStats};

/// Read-only probe-path counters (profiling only; relaxed atomics, no behaviour change).
pub mod probe_counters {
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub static PROBES: AtomicUsize = AtomicUsize::new(0);
    pub static SPARSE_COLUMN: AtomicUsize = AtomicUsize::new(0);
    pub static CACHED_TUPLE: AtomicUsize = AtomicUsize::new(0);
    pub static CACHED_COLUMN: AtomicUsize = AtomicUsize::new(0);
    /// multi-column probe whose subset is not dense/large -> group_by_key every time
    pub static DYNAMIC_TUPLE: AtomicUsize = AtomicUsize::new(0);
    /// single-column probe whose subset is not dense/large -> per-trie-node cache
    pub static DYNAMIC_COLUMN: AtomicUsize = AtomicUsize::new(0);
    /// cached index actually had to be (re)built on this access
    pub static INDEX_REBUILD: AtomicUsize = AtomicUsize::new(0);
    /// cached index was already fresh
    pub static INDEX_HIT: AtomicUsize = AtomicUsize::new(0);
    /// rows visited by index (re)construction
    pub static INDEX_REBUILD_ROWS: AtomicUsize = AtomicUsize::new(0);
    /// index rebuilds that had to rebuild the whole table (major version bump)
    pub static INDEX_REBUILD_FULL: AtomicUsize = AtomicUsize::new(0);
    /// distinct key values handed out by a prober (`Prober::for_each` iterations)
    pub static PROBED_KEYS: AtomicUsize = AtomicUsize::new(0);
    /// rows reachable through probes: the sum of every subset a prober hands out,
    /// i.e. every row that still has to pass the remaining constraints. This is the
    /// fan-out the join actually pays for, as opposed to `matches` (rule firings).
    /// Equal to `ENUM_ROWS + LOOKUP_ROWS`; kept so a single counter tells the whole story.
    pub static CANDIDATE_ROWS: AtomicUsize = AtomicUsize::new(0);
    /// the `Prober::for_each` half of `CANDIDATE_ROWS`: enumerating a whole index,
    /// one subset per distinct key
    pub static ENUM_ROWS: AtomicUsize = AtomicUsize::new(0);
    /// single-key lookups (`Prober::get_subset`), which are NOT counted as probes:
    /// one prober is built per stage and then reused for every row of the cover
    pub static LOOKUP_CALLS: AtomicUsize = AtomicUsize::new(0);
    /// the `Prober::get_subset` half of `CANDIDATE_ROWS`
    pub static LOOKUP_ROWS: AtomicUsize = AtomicUsize::new(0);

    pub fn bump(c: &AtomicUsize) {
        c.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot() -> Vec<(&'static str, usize)> {
        let g = |c: &AtomicUsize| c.load(Ordering::Relaxed);
        vec![
            ("probes", g(&PROBES)),
            ("sparse_column", g(&SPARSE_COLUMN)),
            ("cached_tuple", g(&CACHED_TUPLE)),
            ("cached_column", g(&CACHED_COLUMN)),
            ("dynamic_tuple (group_by_key)", g(&DYNAMIC_TUPLE)),
            ("dynamic_column (per-node cache)", g(&DYNAMIC_COLUMN)),
            ("cached_index_rebuild", g(&INDEX_REBUILD)),
            ("cached_index_hit", g(&INDEX_HIT)),
            ("cached_index_rebuild_rows", g(&INDEX_REBUILD_ROWS)),
            ("cached_index_rebuild_full", g(&INDEX_REBUILD_FULL)),
            ("probed_keys", g(&PROBED_KEYS)),
            ("candidate_rows", g(&CANDIDATE_ROWS)),
            ("  candidate_rows enumerated (for_each)", g(&ENUM_ROWS)),
            ("  lookup calls (get_subset)", g(&LOOKUP_CALLS)),
            ("  candidate_rows from lookups", g(&LOOKUP_ROWS)),
        ]
    }
}
