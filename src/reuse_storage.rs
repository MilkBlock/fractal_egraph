//! Exact, acyclic row sharing for immutable baked bodies. Never a weak-dominate rewrite.
use super::*;
use crate::saturated_rule_composition::Row;
#[derive(Clone, Serialize, Deserialize)]
pub enum StoredState {
    Full(SaturatedRuleComposition),
    /// Values/ports/scope are identical to base; row indexes refer to base rows.
    Delta {
        base: usize,
        shared: Vec<usize>,
        added: Vec<Row>,
        hidden: Vec<usize>,
        local_ids: Vec<String>,
    },
}
impl StoredState {
    pub fn expand(
        &self,
        previous: &[SaturatedRuleComposition],
    ) -> Result<SaturatedRuleComposition> {
        match self {
            Self::Full(s) => {
                s.validate()?;
                Ok(s.clone())
            }
            Self::Delta {
                base,
                shared,
                added,
                hidden,
                local_ids,
            } => {
                let b = previous
                    .get(*base)
                    .ok_or("storage base must precede its dependent")?;
                let mut s = b.clone();
                s.rows = shared
                    .iter()
                    .map(|i| b.rows.get(*i).cloned().ok_or("unknown shared row".into()))
                    .collect::<Result<Vec<_>>>()?;
                s.rows.extend(added.iter().cloned());
                s.subsumed_rows = hidden.clone();
                s.local_ids = local_ids.clone();
                s.validate()?;
                Ok(s)
            }
        }
    }
    pub fn select(
        s: &SaturatedRuleComposition,
        previous: &[SaturatedRuleComposition],
    ) -> Result<Self> {
        let full = Self::Full(s.clone());
        let mut cost = serde_json::to_vec(&full)?.len();
        let mut best = full;
        for (base, b) in previous.iter().enumerate() {
            if s.version != b.version
                || s.scope != b.scope
                || s.values != b.values
                || s.ports != b.ports
            {
                continue;
            }
            let rows: BTreeMap<&Row, usize> =
                b.rows.iter().enumerate().map(|(i, r)| (r, i)).collect();
            let mut shared = vec![];
            let mut added = vec![];
            // Preserve row order: only a common prefix can currently be shared.
            // No search for arbitrary embeddings and no dropped visibility bits.
            let prefix = s.rows.iter().take_while(|r| rows.contains_key(*r)).count();
            if prefix == 0 {
                continue;
            }
            for r in &s.rows[..prefix] {
                shared.push(rows[r]);
            }
            added.extend_from_slice(&s.rows[prefix..]);
            let candidate = Self::Delta {
                base,
                shared,
                added,
                hidden: s.subsumed_rows.clone(),
                local_ids: s.local_ids.clone(),
            };
            let bytes = serde_json::to_vec(&candidate)?.len();
            if bytes < cost {
                let restored = candidate.expand(previous)?;
                if serde_json::to_value(&restored)? != serde_json::to_value(s)? {
                    return Err("storage reconstruction mismatch".into());
                }
                cost = bytes;
                best = candidate;
            }
        }
        Ok(best)
    }
}

/// Immutable virtual row view. A shared row is borrowed from its owner, without
/// flattening the reference chain. This is baked-library storage, not tier0 rows.
pub struct SharedBodies {
    states: Vec<StoredState>,
}
impl SharedBodies {
    pub fn new(states: Vec<StoredState>) -> Result<Self> {
        let mut previous = vec![];
        for s in &states {
            previous.push(s.expand(&previous)?);
        }
        Ok(Self { states })
    }
    pub fn row_count(&self, id: usize) -> Option<usize> {
        match self.states.get(id)? {
            StoredState::Full(s) => Some(s.rows.len()),
            StoredState::Delta { shared, added, .. } => Some(shared.len() + added.len()),
        }
    }
    pub fn row(&self, mut id: usize, mut row: usize) -> Option<&Row> {
        loop {
            match self.states.get(id)? {
                StoredState::Full(s) => return s.rows.get(row),
                StoredState::Delta {
                    base,
                    shared,
                    added,
                    ..
                } => {
                    if row >= shared.len() {
                        return added.get(row - shared.len());
                    }
                    row = shared[row];
                    id = *base;
                }
            }
        }
    }
    pub fn visible(&self, id: usize, row: usize) -> bool {
        if self.row_count(id).is_none_or(|n| row >= n) {
            return false;
        }
        match &self.states[id] {
            StoredState::Full(s) => !s.subsumed_rows.contains(&row),
            StoredState::Delta { hidden, .. } => !hidden.contains(&row),
        }
    }
}
