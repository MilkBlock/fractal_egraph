//! Delay reverse subscriptions for a new intermediate whose key must change
//! under an already queued union. The fact, keyed lookup and receipt stay live.
use crate::{
    graph::FactRef,
    program::*,
    value::{Parents, Value, root},
};
use rustc_hash::FxHashSet;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn derive(p: &Program) -> Vec<Vec<bool>> {
    let mut hidden = vec![false; p.tables.len()];
    for t in &p.templates {
        for op in t.ops.iter() {
            if let Op::Hide(table, ..) = op {
                hidden[*table] = true;
            }
        }
    }
    let rule_templates: BTreeSet<_> = p.rules.iter().map(|r| r.template).collect();
    p.templates
        .iter()
        .enumerate()
        .map(|(id, t)| {
            t.ops
                .iter()
                .enumerate()
                .map(|(i, op)| {
                    let Op::Let(dst, Call::Table(table), _) = op else {
                        return false;
                    };
                    rule_templates.contains(&id)
                        && p.tables[*table].virtual_fact
                        && p.tables[*table].constructor
                        && !hidden[*table]
                        && t.ops[i + 1..].iter().any(|next| match next {
                            Op::Let(_, Call::Table(_), args) | Op::Set(_, args, _) => {
                                args.iter().any(|a| matches!(a, Arg::Slot(s) if s == dst))
                            }
                            _ => false,
                        })
                })
                .collect()
        })
        .collect()
}

/// A measured admission hint can only remove compiler-proved positions. Names
/// and typed schemas make it portable without granting any new effect authority.
pub(crate) fn admit(p: &Program, mut plan: Vec<Vec<bool>>) -> Result<Vec<Vec<bool>>> {
    let Some(raw) = p.artifact.get("deferred_incidence_admission") else {
        return Ok(plan);
    };
    if raw["schema"] != "eggfuse-deferred-incidence-admission/v1" {
        return Err("invalid deferred incidence admission schema".into());
    }
    let rows = raw["tables"].as_array().ok_or("invalid admission tables")?;
    let mut selected = BTreeSet::new();
    for row in rows {
        let name = row["name"].as_str().ok_or("invalid admission table name")?;
        if let Some((id, t)) = p.tables.iter().enumerate().find(|(_, t)| t.name == name) {
            if row["sorts"] != serde_json::json!(t.schema) || !t.constructor || !t.virtual_fact {
                return Err("deferred admission table type mismatch".into());
            }
            selected.insert(id);
        }
    }
    for (t, mask) in p.templates.iter().zip(&mut plan) {
        for (op, enabled) in t.ops.iter().zip(mask) {
            *enabled &= matches!(op, Op::Let(_,Call::Table(id),_) if selected.contains(id));
        }
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queued_prefix_is_a_guard_not_a_speculative_union() {
        let parents = Parents::default();
        let mut s = State::default();
        let f = FactRef { cell: 7, op: 1 };
        assert!(!s.admit(f, &[Value::Eq(2)], &parents, &[(9, 2)]));
        assert!(s.admit(f, &[Value::Eq(9)], &parents, &[(9, 2)]));
        assert_eq!(root(&parents, 9), 9);
        assert_eq!(s.stats.examined_union_pairs, 1);
        s.begin_repair();
        s.finish(f, 0, true);
        assert!(s.live.is_empty());
        assert_eq!(s.stats.retired_before_subscription, 1);
        assert!(!s.admit(f, &[Value::Eq(9)], &parents, &[]));
    }
}

#[derive(Default, Serialize)]
pub(crate) struct Counts {
    pub eligible_births: u64,
    pub examined_union_pairs: u64,
    pub deferred_births: u64,
    pub retired_before_subscription: u64,
    pub promoted: u64,
    pub peak_pending: usize,
    pub saved_birth_edges: u64,
    pub saved_birth_row_payloads: u64,
    pub saved_birth_values: u64,
    pub retired_by_table: BTreeMap<usize, u64>,
}

#[derive(Default)]
pub(crate) struct State {
    pub enabled: bool,
    pub payloads: bool,
    pub reads_before_repair: std::cell::Cell<u64>,
    pub repairing: bool,
    pub live: BTreeSet<FactRef>,
    scanned: usize,
    displaced: FxHashSet<u64>,
    pub stats: Counts,
}
impl State {
    pub fn admit(
        &mut self,
        f: FactRef,
        key: &[Value],
        parents: &Parents,
        pairs: &[(u64, u64)],
    ) -> bool {
        self.stats.eligible_births += 1;
        for &(a, b) in &pairs[self.scanned..] {
            let (a, b) = (root(parents, a), root(parents, b));
            if a != b {
                self.displaced.insert(a.max(b));
            }
            self.stats.examined_union_pairs += 1;
        }
        self.scanned = pairs.len();
        if !key
            .iter()
            .any(|v| matches!(v, Value::Eq(id) if self.displaced.contains(&root(parents,*id))))
        {
            return false;
        }
        self.live.insert(f);
        self.stats.deferred_births += 1;
        self.stats.peak_pending = self.stats.peak_pending.max(self.live.len());
        true
    }
    pub fn begin_repair(&mut self) {
        self.repairing = true;
        self.scanned = 0;
        self.displaced.clear();
    }
    pub fn finish(&mut self, f: FactRef, table: usize, retired: bool) {
        if self.live.remove(&f) {
            if retired {
                self.stats.retired_before_subscription += 1;
                *self.stats.retired_by_table.entry(table).or_default() += 1;
            } else {
                self.stats.promoted += 1;
            }
        }
    }
}
