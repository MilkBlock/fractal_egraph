//! Optional tracing for rule matches produced by the core query engine.
//!
//! A [`RuleMatchEvent`] records the logical substitution that survives query
//! planning at the point where a match is about to enqueue its actions. Query
//! planners may eliminate body-only variables, so this is not an exact set of
//! physical table-row witnesses. It also intentionally does not claim that the
//! actions changed the database: mutations are staged and only acquire a
//! committed/no-op outcome later, while tables are merged.
//!
//! `TraceSession::with_dependencies()` additionally records exact keyed-table
//! witnesses and propagates per-lane causes to actual insertion/merge decisions.
//! It is a diagnostic mode for pure relational/constructor rules, not a complete
//! equality proof tracer: unavailable keys and virtual-table reads remain gaps.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering},
};

use crate::{Value, Variable};

/// An engine-local, read-only expansion of a planned query into source row keys.
/// Registration is diagnostic metadata, not proof that the query rewrite is sound.
#[derive(Clone, Debug)]
pub enum SourceReadKey {
    Named(Arc<str>),
    Constant(Value),
}
#[derive(Clone, Debug)]
pub struct SourceRead {
    pub table: crate::TableId,
    pub source_span: Option<Arc<str>>,
    pub keys: Vec<SourceReadKey>,
}
#[derive(Clone, Debug)]
pub struct SourceReadPlan {
    pub reads: Vec<SourceRead>,
    pub visible_bindings: std::collections::BTreeSet<Arc<str>>,
    pub summary_table: crate::TableId,
}

/// A variable binding in a completed logical rule match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleMatchBinding {
    /// The query-local variable id.
    pub variable: Variable,
    /// The source name, when the rule builder assigned one.
    pub name: Option<Arc<str>>,
    /// The runtime value. It is meaningful only inside the traced database
    /// execution; it is not a stable wire-format identifier.
    pub value: Value,
}

/// One completed logical match of a core rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleMatchEvent {
    /// Monotonic within one [`TraceSession`]. Parallel execution does not make
    /// this id a causal ordering between workers.
    pub event_id: u64,
    /// Human-readable rule description supplied by the rule builder.
    pub rule: Arc<str>,
    /// The logical substitution retained at the query/action boundary.
    /// Variables eliminated by the query planner are not present.
    pub bindings: Vec<RuleMatchBinding>,
    /// True only in dependency mode when every explicit LHS atom was resolved
    /// to a unique concrete row by its complete key while the database was read-only.
    /// This does not assert coverage of union proofs, external-function reads,
    /// or complete semantic causes. Normal logical tracing keeps this false.
    pub physical_witness_complete: bool,
}

/// Whether the vectorized action program kept or filtered a matched lane.
///
/// `Survived` still does not mean that the database changed: every staged
/// mutation may be redundant when tables are committed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleActionOutcome {
    Survived,
    Filtered,
    /// Trusted optimizer certified the RHS redundant before execution.
    SkippedRedundant,
}

/// The action-program outcome corresponding to one [`RuleMatchEvent`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleActionOutcomeEvent {
    pub event_id: u64,
    pub match_event_id: u64,
    pub outcome: RuleActionOutcome,
}

/// Cooperative search admission. Accepted actions still commit and rebuild.
/// Row credits reserve worst-case writes before match/trace allocation; duplicate
/// writes can therefore stop earlier than the final live-row limit.
pub struct ExecutionControl {
    deadline: Option<web_time::Instant>,
    row_credits: Option<usize>,
    reserved: AtomicUsize,
    reason: AtomicU8,
    enabled: AtomicBool,
}
impl ExecutionControl {
    pub fn should_stop(&self) -> bool {
        if !self.enabled.load(Ordering::Relaxed) {
            return false;
        }
        if self.reason.load(Ordering::Relaxed) != 0 {
            return true;
        }
        if self.deadline.is_some_and(|d| web_time::Instant::now() >= d) {
            let _ = self
                .reason
                .compare_exchange(0, 1, Ordering::Relaxed, Ordering::Relaxed);
            return true;
        }
        false
    }
    fn admit(&self, credits: usize) -> bool {
        if !self.enabled.load(Ordering::Relaxed) {
            return true;
        }
        if self.should_stop() {
            return false;
        }
        if let Some(limit) = self.row_credits {
            if self
                .reserved
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                    n.checked_add(credits).filter(|n| *n <= limit)
                })
                .is_err()
            {
                let _ = self
                    .reason
                    .compare_exchange(0, 2, Ordering::Relaxed, Ordering::Relaxed);
                return false;
            }
        }
        true
    }
}
/// Trusted pre-RHS filter. True certifies that all semantic effects already hold.
/// Only called with complete keyed reads. The caller is responsible for validity
/// in this engine, rule environment and execution boundary. No default filtering.
pub type RedundantRhsFilter = Arc<dyn Fn(&str, &[(&str, &[Value])]) -> bool + Send + Sync>;

#[derive(Default)]
struct TraceState {
    rhs_filter: Mutex<Option<RedundantRhsFilter>>,
    skipped_rhs: Mutex<Vec<u64>>,
    next_event_id: AtomicU64,
    control: Mutex<Option<Arc<ExecutionControl>>>,
    scope_resets: Mutex<Vec<u64>>,
    dependencies_enabled: bool,
    effects_only: bool,
    has_read_plans: AtomicBool,
    read_plans: Mutex<std::collections::HashMap<Arc<str>, Arc<SourceReadPlan>>>,
    match_count: AtomicUsize,
    effect_counts: Mutex<(std::collections::HashMap<crate::TableId, usize>, bool)>,
    table_names: Mutex<std::collections::HashMap<crate::TableId, Arc<str>>>,
    writes: Mutex<Vec<WriteEvent>>,
    unions: Mutex<Vec<UnionEvent>>,
    equality_edges: Mutex<Vec<(u64, Value, Value)>>,
    invalidations: Mutex<Vec<OriginInvalidation>>,
    reads: Mutex<Vec<RowReadEvent>>,
    matches: Mutex<Vec<RuleMatchEvent>>,
    action_outcomes: Mutex<Vec<RuleActionOutcomeEvent>>,
}

/// A shareable collection session for core rule-match events.
///
/// Normal logical tracing is passed explicitly to each run. Dependency mode
/// additionally stores origin references in tables; table clones copy those
/// references, but producer IDs are returned only to the same trace session.
/// Tables which have never carried provenance retain the normal commit path.
#[derive(Clone, Default)]
pub struct TraceSession {
    state: Arc<TraceState>,
}

/// Owned events from a completed execution boundary. Dropping this releases
/// raw bindings, rows and action records. Session identity and row origins survive.
#[derive(Default)]
pub struct TraceBatch {
    pub skipped_rhs: Vec<u64>,
    pub matches: Vec<RuleMatchEvent>,
    pub actions: Vec<RuleActionOutcomeEvent>,
    pub writes: Vec<WriteEvent>,
    pub reads: Vec<RowReadEvent>,
    pub unions: Vec<UnionEvent>,
    pub invalidations: Vec<OriginInvalidation>,
}
impl TraceBatch {
    pub fn len(&self) -> usize {
        self.matches.len()
            + self.actions.len()
            + self.writes.len()
            + self.reads.len()
            + self.unions.len()
            + self.invalidations.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
impl TraceSession {
    pub fn set_proven_redundant_rhs_filter(&self, filter: Option<RedundantRhsFilter>) {
        *self.state.rhs_filter.lock().unwrap() = filter;
    }
    pub(crate) fn proven_redundant_rhs(
        &self,
        rule: &str,
        reads: &[RowWitness],
        complete: bool,
    ) -> bool {
        if !complete || reads.is_empty() {
            return false;
        }
        let filter = self.state.rhs_filter.lock().unwrap().clone();
        let Some(filter) = filter else { return false };
        let rows: Option<Vec<_>> = reads
            .iter()
            .map(|r| {
                r.table_name
                    .as_ref()
                    .map(|n| (n.as_ref(), r.row.as_slice()))
            })
            .collect();
        rows.is_some_and(|rows| filter(rule, &rows))
    }
    pub(crate) fn record_skipped_rhs(&self, id: u64) {
        self.state.skipped_rhs.lock().unwrap().push(id);
        self.state
            .action_outcomes
            .lock()
            .unwrap()
            .push(RuleActionOutcomeEvent {
                event_id: self.state.next_event_id.fetch_add(1, Ordering::Relaxed),
                match_event_id: id,
                outcome: RuleActionOutcome::SkippedRedundant,
            });
    }

    /// Register before execution; table IDs belong to the compiling engine.
    pub fn set_source_reads(&self, rule: Arc<str>, plan: SourceReadPlan) {
        self.state
            .read_plans
            .lock()
            .unwrap()
            .insert(rule, Arc::new(plan));
        self.state.has_read_plans.store(true, Ordering::Release);
    }
    pub(crate) fn source_reads(&self, rule: &str) -> Option<Arc<SourceReadPlan>> {
        if !self.state.has_read_plans.load(Ordering::Acquire) {
            return None;
        }
        self.state.read_plans.lock().unwrap().get(rule).cloned()
    }
    /// Arm before one native step, with remaining time and row reservation.
    /// Do not reconfigure while workers are running.
    pub fn set_execution_limits(
        &self,
        time: Option<std::time::Duration>,
        row_credits: Option<usize>,
    ) {
        *self.state.control.lock().unwrap() = if time.is_none() && row_credits.is_none() {
            None
        } else {
            Some(Arc::new(ExecutionControl {
                deadline: time.map(|d| web_time::Instant::now() + d),
                row_credits,
                reserved: AtomicUsize::new(0),
                reason: AtomicU8::new(0),
                enabled: AtomicBool::new(true),
            }))
        };
    }
    pub fn clear_execution_limits(&self) {
        self.state.control.lock().unwrap().take();
    }
    pub fn execution_control(&self) -> Option<Arc<ExecutionControl>> {
        self.state.control.lock().unwrap().clone()
    }
    pub fn admit_action(&self, credits: usize) -> bool {
        self.execution_control()
            .is_none_or(|c| c.admit(credits.max(1)))
    }
    pub fn execution_stop_reason(&self) -> Option<&'static str> {
        self.execution_control()
            .and_then(|c| match c.reason.load(Ordering::Relaxed) {
                1 => Some("time"),
                2 => Some("pending_fact_reservations"),
                _ => None,
            })
    }
    pub fn execution_was_interrupted(&self) -> bool {
        self.execution_control().is_some_and(|c| {
            c.enabled.load(Ordering::Relaxed) && c.reason.load(Ordering::Relaxed) != 0
        })
    }
    /// Rebuild must restore consistency after admitted writes, even after stop.
    pub fn enforce_execution_limits(&self, enabled: bool) {
        if let Some(c) = self.execution_control() {
            c.enabled.store(enabled, Ordering::Relaxed);
        }
    }
    /// Number of raw records buffered, excluding compact equality/scope metadata.
    pub fn buffered_event_count(&self) -> usize {
        self.state.matches.lock().unwrap().len()
            + self.state.action_outcomes.lock().unwrap().len()
            + self.state.writes.lock().unwrap().len()
            + self.state.reads.lock().unwrap().len()
            + self.state.unions.lock().unwrap().len()
            + self.state.invalidations.lock().unwrap().len()
    }
    /// Drain only after run/commit/rebuild finishes and all workers have joined.
    /// Not an atomic snapshot of concurrently executing rules. Compact committed
    /// equality edges remain available for future rebuild explanations; raw union
    /// events are returned only once. IDs and session identity are never reset.
    pub fn drain_completed(&self) -> TraceBatch {
        *self.state.effect_counts.lock().unwrap() = Default::default();
        let unions = std::mem::take(&mut *self.state.unions.lock().unwrap());
        let reset = self.scope_resets().into_iter().max();
        let mut edges = self.state.equality_edges.lock().unwrap();
        edges.extend(
            unions
                .iter()
                .filter(|e| e.displaced.is_some())
                .map(|e| (e.event_id, e.lhs, e.rhs)),
        );
        edges.retain(|(id, _, _)| reset.is_none_or(|r| *id > r));
        TraceBatch {
            skipped_rhs: std::mem::take(&mut *self.state.skipped_rhs.lock().unwrap()),
            matches: self.drain_matches(),
            actions: self.drain_action_outcomes(),
            unions,
            writes: std::mem::take(&mut *self.state.writes.lock().unwrap()),
            reads: std::mem::take(&mut *self.state.reads.lock().unwrap()),
            invalidations: std::mem::take(&mut *self.state.invalidations.lock().unwrap()),
        }
    }

    /// Create an empty trace session.
    pub fn new() -> Self {
        Self::default()
    }

    /// Opt in to diagnostic row provenance. Actions run one lane at a time and
    /// tables carrying provenance use serial commit so the observed winner is exact.
    pub fn with_dependencies() -> Self {
        Self {
            state: Arc::new(TraceState {
                dependencies_enabled: true,
                ..Default::default()
            }),
        }
    }
    /// Keep committed write/union effects, without LHS witnesses or binding history.
    pub fn with_effects() -> Self {
        Self {
            state: Arc::new(TraceState {
                dependencies_enabled: true,
                effects_only: true,
                ..Default::default()
            }),
        }
    }
    pub fn records_dependencies(&self) -> bool {
        self.state.dependencies_enabled && !self.state.effects_only
    }
    pub fn take_effect_match_count(&self) -> usize {
        self.state.match_count.swap(0, Ordering::Relaxed)
    }
    /// Record a successful scope rollback. Earlier block certificates need
    /// revalidation even when restored rows still carry historical origins.
    pub fn record_scope_reset(&self) {
        *self.state.rhs_filter.lock().unwrap() = None;
        self.state.read_plans.lock().unwrap().clear();
        self.state.has_read_plans.store(false, Ordering::Release);
        let id = self.state.next_event_id.fetch_add(1, Ordering::Relaxed);
        self.state.scope_resets.lock().unwrap().push(id);
    }
    pub fn scope_resets(&self) -> Vec<u64> {
        self.state.scope_resets.lock().unwrap().clone()
    }

    /// Find an explicit committed equality path in the current scope epoch.
    /// Missing paths stay unknown; initial/untraced equalities are not invented.
    pub fn equality_path(&self, lhs: Value, rhs: Value) -> Option<Vec<u64>> {
        use std::collections::{HashMap, VecDeque};
        if lhs == rhs {
            return Some(vec![]);
        }
        let reset = self.scope_resets().into_iter().max();
        let mut adjacency: HashMap<Value, Vec<(Value, u64)>> = HashMap::new();
        let mut edges = self.state.equality_edges.lock().unwrap().clone();
        edges.extend(
            self.union_events()
                .into_iter()
                .filter(|e| e.displaced.is_some())
                .map(|e| (e.event_id, e.lhs, e.rhs)),
        );
        for (id, lhs, rhs) in edges {
            if reset.is_some_and(|r| id < r) {
                continue;
            }
            adjacency.entry(lhs).or_default().push((rhs, id));
            adjacency.entry(rhs).or_default().push((lhs, id));
        }
        let mut queue = VecDeque::from([lhs]);
        let mut parents = HashMap::from([(lhs, (lhs, 0))]);
        while let Some(v) = queue.pop_front() {
            for &(next, event) in adjacency.get(&v).into_iter().flatten() {
                if parents.contains_key(&next) {
                    continue;
                }
                parents.insert(next, (v, event));
                if next == rhs {
                    let mut path = Vec::new();
                    let mut cur = rhs;
                    while cur != lhs {
                        let (prev, id) = parents[&cur];
                        path.push(id);
                        cur = prev;
                    }
                    path.reverse();
                    return Some(path);
                }
                queue.push_back(next);
            }
        }
        None
    }

    /// Union decisions recorded at the union-find commit point.
    pub fn union_events(&self) -> Vec<UnionEvent> {
        self.state.unions.lock().unwrap().clone()
    }
    /// Diagnostic table-name registry for interpreting committed row events.
    pub fn table_names(&self) -> Vec<(crate::TableId, Arc<str>)> {
        self.state
            .table_names
            .lock()
            .unwrap()
            .iter()
            .map(|(id, n)| (*id, n.clone()))
            .collect()
    }
    pub(crate) fn register_table_names(
        &self,
        names: impl IntoIterator<Item = (crate::TableId, Arc<str>)>,
    ) {
        self.state.table_names.lock().unwrap().extend(names);
    }
    pub fn same_session(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }
    /// Provenance invalidations, not claims that every affected row was deleted.
    pub fn origin_invalidations(&self) -> Vec<OriginInvalidation> {
        self.state.invalidations.lock().unwrap().clone()
    }
    pub fn dependencies_enabled(&self) -> bool {
        self.state.dependencies_enabled
    }
    /// Inserted/Deduplicated/Updated are emitted at SortedWritesTable commit.
    /// Unsupported is only a staging notification for a table without commit hooks.
    pub fn effect_summary(&self) -> (std::collections::HashMap<crate::TableId, usize>, bool, bool) {
        if self.state.effects_only {
            let effects = self.state.effect_counts.lock().unwrap();
            let union = self
                .state
                .unions
                .lock()
                .unwrap()
                .iter()
                .any(|u| u.displaced.is_some());
            return (effects.0.clone(), union, effects.1 || union);
        }
        let mut inserted = std::collections::HashMap::new();
        let writes = self.state.writes.lock().unwrap();
        let mut changed = false;
        for w in writes.iter() {
            changed |= matches!(w.outcome, WriteOutcome::Inserted | WriteOutcome::Updated);
            if w.outcome == WriteOutcome::Inserted && w.rebuild_of.is_none() {
                *inserted.entry(w.table).or_default() += 1;
            }
        }
        drop(writes);
        let union = self
            .state
            .unions
            .lock()
            .unwrap()
            .iter()
            .any(|u| u.displaced.is_some());
        (inserted, union, changed || union)
    }
    pub fn write_events(&self) -> Vec<WriteEvent> {
        self.state.writes.lock().unwrap().clone()
    }
    /// A producer ID is present only for a still-valid inserted row from this
    /// trace session. None means unknown/input, not proof of independence.
    pub fn row_reads(&self) -> Vec<RowReadEvent> {
        self.state.reads.lock().unwrap().clone()
    }
    pub(crate) fn record_write(
        &self,
        cause: &TraceCause,
        proposed: &[Value],
        actual: &[Value],
        outcome: WriteOutcome,
    ) -> u64 {
        let event_id = self.state.next_event_id.fetch_add(1, Ordering::Relaxed);
        if self.state.effects_only {
            let mut effects = self.state.effect_counts.lock().unwrap();
            effects.1 |= matches!(outcome, WriteOutcome::Inserted | WriteOutcome::Updated);
            if outcome == WriteOutcome::Inserted && cause.rebuild_of.is_none() {
                *effects.0.entry(cause.table).or_default() += 1;
            }
            return event_id;
        }
        self.state.writes.lock().unwrap().push(WriteEvent {
            source_span: cause.source_span.clone(),
            event_id,
            match_event_id: cause.match_event_id,
            table: cause.table,
            proposed: proposed.to_vec(),
            actual: actual.to_vec(),
            outcome,
            rebuild_of: cause.rebuild_of,
            union_dependencies: cause.union_dependencies.clone(),
        });
        event_id
    }
    pub(crate) fn record_reads(&self, match_id: u64, reads: Vec<RowWitness>, complete: bool) {
        // This is a diagnostic path. No producer is inferred from value similarity.
        if complete {
            if let Some(m) = self
                .state
                .matches
                .lock()
                .unwrap()
                .iter_mut()
                .rev()
                .find(|m| m.event_id == match_id)
            {
                m.physical_witness_complete = true;
            }
        }
        let mut out = self.state.reads.lock().unwrap();
        for r in reads {
            let origin = r
                .producer
                .filter(|p| Arc::ptr_eq(&p.trace.state, &self.state));
            let producer = origin.as_ref().map(|p| p.match_event_id);
            let producer_write = origin.as_ref().and_then(|p| p.commit_event_id);
            out.push(RowReadEvent {
                planner_summary: r.planner_summary,
                source_span: r.source_span,
                event_id: self.state.next_event_id.fetch_add(1, Ordering::Relaxed),
                match_event_id: match_id,
                table: r.table,
                table_name: r.table_name,
                key: r.key,
                row: r.row,
                producer_match_event_id: producer,
                producer_write_event_id: producer_write,
            });
        }
    }

    pub(crate) fn record_match(
        &self,
        rule: Arc<str>,
        variable_names: &crate::query::SymbolMap,
        bindings: &crate::numeric_id::DenseIdMap<Variable, Value>,
    ) -> u64 {
        let event_id = self.state.next_event_id.fetch_add(1, Ordering::Relaxed);
        if self.state.effects_only {
            self.state.match_count.fetch_add(1, Ordering::Relaxed);
            return event_id;
        }
        let source_plan = self.source_reads(&rule);
        let bindings = bindings
            .iter()
            .filter(|(variable, _)| {
                source_plan.as_ref().is_none_or(|p| {
                    variable_names
                        .vars
                        .get(variable)
                        .is_some_and(|n| p.visible_bindings.contains(n))
                })
            })
            .map(|(variable, value)| RuleMatchBinding {
                variable,
                name: variable_names.vars.get(&variable).cloned(),
                value: *value,
            })
            .collect();
        self.state.matches.lock().unwrap().push(RuleMatchEvent {
            event_id,
            rule,
            bindings,
            physical_witness_complete: false,
        });
        event_id
    }

    pub(crate) fn record_action_outcomes(
        &self,
        match_event_ids: &[u64],
        survived: impl IntoIterator<Item = bool>,
    ) {
        if self.state.effects_only {
            return;
        }
        let mut outcomes = self.state.action_outcomes.lock().unwrap();
        for (match_event_id, survived) in match_event_ids.iter().copied().zip(survived) {
            let event_id = self.state.next_event_id.fetch_add(1, Ordering::Relaxed);
            outcomes.push(RuleActionOutcomeEvent {
                event_id,
                match_event_id,
                outcome: if survived {
                    RuleActionOutcome::Survived
                } else {
                    RuleActionOutcome::Filtered
                },
            });
        }
    }

    /// Remove and return every event recorded so far.
    pub fn drain_matches(&self) -> Vec<RuleMatchEvent> {
        std::mem::take(&mut *self.state.matches.lock().unwrap())
    }

    /// Return a snapshot of every event recorded so far without clearing it.
    pub fn matches(&self) -> Vec<RuleMatchEvent> {
        self.state.matches.lock().unwrap().clone()
    }

    /// Remove and return every action-program outcome recorded so far.
    pub fn drain_action_outcomes(&self) -> Vec<RuleActionOutcomeEvent> {
        std::mem::take(&mut *self.state.action_outcomes.lock().unwrap())
    }

    /// Return action-program outcomes without clearing them.
    pub fn action_outcomes(&self) -> Vec<RuleActionOutcomeEvent> {
        self.state.action_outcomes.lock().unwrap().clone()
    }
}

/// Results of tracked writes. Unsupported is emitted before commit and means
/// the table does not implement attribution. Updated versions are deliberately
/// not credited as newly produced facts; their merge/equality lineage is incomplete.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteOutcome {
    Inserted,
    Deduplicated,
    Updated,
    Unsupported,
}
#[derive(Clone, Debug)]
pub struct WriteEvent {
    /// Source-file identity and byte range of the originating expression/action.
    /// Copied through rebuild lineage; None for low-level/unmapped actions.
    pub source_span: Option<Arc<str>>,
    /// Previous committed row version, when native rebuild transports provenance.
    pub rebuild_of: Option<u64>,
    /// Committed equality edges used to canonicalize that version.
    pub union_dependencies: Vec<u64>,
    pub event_id: u64,
    pub match_event_id: u64,
    pub table: crate::TableId,
    pub proposed: Vec<Value>,
    pub actual: Vec<Value>,
    pub outcome: WriteOutcome,
}
#[derive(Clone, Debug)]
pub struct RowReadEvent {
    /// Planner-only summary witness; source analyses should use expanded reads.
    pub planner_summary: bool,
    /// Source-file identity and byte range of the query atom before planning.
    /// This is not the row's position in a join traversal.
    pub source_span: Option<Arc<str>>,
    pub event_id: u64,
    pub match_event_id: u64,
    pub table: crate::TableId,
    pub table_name: Option<Arc<str>>,
    pub key: Vec<Value>,
    pub row: Vec<Value>,
    pub producer_match_event_id: Option<u64>,
    pub producer_write_event_id: Option<u64>,
}
/// Origin attached to a staged write, never reconstructed from a snapshot delta.
#[derive(Clone)]
pub struct TraceCause {
    pub(crate) source_span: Option<Arc<str>>,
    pub(crate) rebuild_of: Option<u64>,
    pub(crate) union_dependencies: Vec<u64>,
    pub(crate) trace: TraceSession,
    pub(crate) match_event_id: u64,
    pub(crate) table: crate::TableId,
    pub(crate) commit_event_id: Option<u64>,
}
impl TraceCause {
    pub(crate) fn finish(
        &self,
        proposed: &[Value],
        actual: &[Value],
        outcome: WriteOutcome,
    ) -> u64 {
        self.trace.record_write(self, proposed, actual, outcome)
    }
}
pub(crate) struct RowWitness {
    pub planner_summary: bool,
    pub source_span: Option<Arc<str>>,
    pub table: crate::TableId,
    pub table_name: Option<Arc<str>>,
    pub key: Vec<Value>,
    pub row: Vec<Value>,
    pub producer: Option<TraceCause>,
}
#[derive(Clone, Debug)]
pub(crate) struct TraceAtom {
    pub source_span: Option<Arc<str>>,
    pub table: crate::TableId,
    pub keys: Vec<Option<crate::action::QueryEntry>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OriginInvalidationReason {
    Updated,
    RemovedOrRebuilt,
    TableCleared,
}
#[derive(Clone, Debug)]
pub struct OriginInvalidation {
    pub event_id: u64,
    pub write_event_id: u64,
    pub reason: OriginInvalidationReason,
}
impl TraceCause {
    pub(crate) fn invalidate(&self, reason: OriginInvalidationReason) {
        if let Some(write_event_id) = self.commit_event_id {
            let event_id = self
                .trace
                .state
                .next_event_id
                .fetch_add(1, Ordering::Relaxed);
            self.trace
                .state
                .invalidations
                .lock()
                .unwrap()
                .push(OriginInvalidation {
                    event_id,
                    write_event_id,
                    reason,
                });
        }
    }
}

/// A committed union decision, distinct from a relational row insertion.
#[derive(Clone, Debug)]
pub struct UnionEvent {
    pub event_id: u64,
    pub match_event_id: u64,
    pub table: crate::TableId,
    pub lhs: Value,
    pub rhs: Value,
    pub canonical: Value,
    /// None means the two values were already equivalent at commit.
    pub displaced: Option<Value>,
}
impl TraceCause {
    pub(crate) fn finish_union(
        &self,
        lhs: Value,
        rhs: Value,
        canonical: Value,
        displaced: Option<Value>,
    ) {
        let event_id = self
            .trace
            .state
            .next_event_id
            .fetch_add(1, Ordering::Relaxed);
        self.trace.state.unions.lock().unwrap().push(UnionEvent {
            event_id,
            match_event_id: self.match_event_id,
            table: self.table,
            lhs,
            rhs,
            canonical,
            displaced,
        });
    }
}
