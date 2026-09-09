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
    atomic::{AtomicU64, Ordering},
};

use crate::{Value, Variable};

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
}

/// The action-program outcome corresponding to one [`RuleMatchEvent`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleActionOutcomeEvent {
    pub event_id: u64,
    pub match_event_id: u64,
    pub outcome: RuleActionOutcome,
}

#[derive(Default)]
struct TraceState {
    next_event_id: AtomicU64,
    dependencies_enabled: bool,
    writes: Mutex<Vec<WriteEvent>>,
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

impl TraceSession {
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
        self.state.writes.lock().unwrap().push(WriteEvent {
            event_id,
            match_event_id: cause.match_event_id,
            table: cause.table,
            proposed: proposed.to_vec(),
            actual: actual.to_vec(),
            outcome,
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
        let bindings = bindings
            .iter()
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
    pub event_id: u64,
    pub match_event_id: u64,
    pub table: crate::TableId,
    pub proposed: Vec<Value>,
    pub actual: Vec<Value>,
    pub outcome: WriteOutcome,
}
#[derive(Clone, Debug)]
pub struct RowReadEvent {
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
    pub table: crate::TableId,
    pub table_name: Option<Arc<str>>,
    pub key: Vec<Value>,
    pub row: Vec<Value>,
    pub producer: Option<TraceCause>,
}
#[derive(Clone, Debug)]
pub(crate) struct TraceAtom {
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
