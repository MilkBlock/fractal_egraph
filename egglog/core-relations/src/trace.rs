//! Optional tracing for rule matches produced by the core query engine.
//!
//! A [`RuleMatchEvent`] records the logical substitution that survives query
//! planning at the point where a match is about to enqueue its actions. Query
//! planners may eliminate body-only variables, so this is not an exact set of
//! physical table-row witnesses. It also intentionally does not claim that the
//! actions changed the database: mutations are staged and only acquire a
//! committed/no-op outcome later, while tables are merged.

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
    /// False until physical atom/row witnesses are captured by a deeper join
    /// tracing layer.
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
    matches: Mutex<Vec<RuleMatchEvent>>,
    action_outcomes: Mutex<Vec<RuleActionOutcomeEvent>>,
}

/// A shareable collection session for core rule-match events.
///
/// The session is passed explicitly to a traced run, so it does not become
/// part of [`crate::Database`]'s clone semantics. Calls without a trace
/// session retain the existing execution path.
#[derive(Clone, Default)]
pub struct TraceSession {
    state: Arc<TraceState>,
}

impl TraceSession {
    /// Create an empty trace session.
    pub fn new() -> Self {
        Self::default()
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
