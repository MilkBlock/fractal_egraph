//! Shared, typed matching spines over immutable analyzed closures.
//!
//! These markers certify a *symbolic projection*, not a live Cell instance.
//! The device's row generations and missing-key watches remain authoritative.
use crate::{Result, Template};
use egglog::cell_assist::{CellActionSink, CellOperand};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Term {
    Slot(usize),
    Literal(String),
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Atom {
    function: String,
    sorts: Vec<String>,
    terms: Vec<Term>,
}
#[derive(Default)]
struct Rows {
    data: Vec<Vec<usize>>,
    by_function: BTreeMap<String, Vec<usize>>,
    by_key: BTreeMap<(String, Vec<usize>), Vec<usize>>,
    // A literal may have multiple symbolic identities. Do not pick one.
    literals: BTreeMap<(String, String), Vec<usize>>,
    sorts: Vec<String>,
    literal_text: Vec<Option<String>>,
    markers: BTreeSet<String>,
}
#[derive(Clone)]
struct Witness {
    cell: usize,
    slots: Vec<Option<usize>>,
}
struct Marker {
    witnesses: Vec<Witness>,
}
#[derive(Default, Clone, Serialize)]
pub(crate) struct Work {
    pub requests: usize,
    pub prefix_reuses: usize,
    pub markers_built: usize,
    pub anchor_row_checks: usize,
    pub keyed_row_checks: usize,
    pub key_lookups: usize,
    pub stored_witnesses: usize,
}
pub(crate) struct Index {
    rows: Vec<Rows>,
    nodes: BTreeMap<(Option<usize>, Atom), usize>,
    markers: Vec<Marker>,
    pub work: Work,
}
impl Index {
    pub fn new(templates: &[Template]) -> Result<Self> {
        let mut rows = vec![];
        for t in templates {
            let mut r = Rows {
                markers: t.markers.clone(),
                ..Rows::default()
            };
            for (i, v) in t.state["values"]
                .as_array()
                .ok_or("missing Cell values")?
                .iter()
                .enumerate()
            {
                let sort = v["sort"]
                    .as_str()
                    .ok_or("missing Cell value sort")?
                    .to_owned();
                let lit = v["literal"].as_str().map(str::to_owned);
                if let Some(lit) = &lit {
                    r.literals
                        .entry((sort.clone(), lit.clone()))
                        .or_default()
                        .push(i);
                }
                r.sorts.push(sort);
                r.literal_text.push(lit);
            }
            let hidden: BTreeSet<_> = t.state["subsumed_rows"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_u64())
                .collect();
            for (i, row) in t.state["rows"]
                .as_array()
                .ok_or("missing Cell rows")?
                .iter()
                .enumerate()
            {
                if hidden.contains(&(i as u64)) {
                    continue;
                }
                let op = row["op"]
                    .as_str()
                    .ok_or("missing Cell operator")?
                    .to_owned();
                if r.markers.contains(&op) {
                    continue;
                }
                let columns: Vec<_> = row["args"]
                    .as_array()
                    .ok_or("missing Cell arguments")?
                    .iter()
                    .chain([&row["result"]])
                    .map(|v| v.as_u64().map(|i| i as usize).ok_or("invalid Cell value"))
                    .collect::<std::result::Result<_, _>>()?;
                if columns.is_empty() || columns.iter().any(|v| *v >= r.sorts.len()) {
                    return Err("Cell row outside value table".into());
                }
                let key = columns[..columns.len() - 1].to_vec();
                let id = r.data.len();
                r.by_key.entry((op.clone(), key)).or_default().push(id);
                r.by_function.entry(op).or_default().push(id);
                r.data.push(columns);
            }
            rows.push(r);
        }
        Ok(Self {
            rows,
            nodes: BTreeMap::new(),
            markers: vec![],
            work: Work::default(),
        })
    }

    /// Alpha-normalized prefix markers retain concrete symbolic row identities,
    /// so extending a spine cannot mix bindings from different alternatives.
    pub fn alternatives(
        &mut self,
        templates: &[Template],
        q: &CellActionSink,
        parts: &[usize],
    ) -> Vec<(usize, Vec<[u32; 2]>)> {
        use egglog_numeric_id::NumericId;
        self.work.requests += 1;
        let mut names = BTreeMap::new();
        let mut parent = None;
        for &part in parts {
            let a = &q.atoms[part];
            let terms = a
                .columns
                .iter()
                .map(|o| match o {
                    CellOperand::Slot(s) => {
                        let next = names.len();
                        Term::Slot(*names.entry(*s).or_insert(next))
                    }
                    CellOperand::Constant { text, .. } => Term::Literal(text.clone()),
                })
                .collect();
            let atom = Atom {
                function: a.function.clone(),
                sorts: a.sorts.clone(),
                terms,
            };
            let key = (parent, atom.clone());
            let id = if let Some(&id) = self.nodes.get(&key) {
                self.work.prefix_reuses += 1;
                id
            } else {
                let mut witnesses = vec![];
                if let Some(p) = parent {
                    for old in &self.markers[p].witnesses {
                        let r = &self.rows[old.cell];
                        if r.markers.contains(&atom.function) {
                            continue;
                        }
                        // Fully keyed suffix: no scan of the Cell's unrelated rows.
                        let mut keys = vec![vec![]];
                        for (term, sort) in
                            atom.terms[..atom.terms.len() - 1].iter().zip(&atom.sorts)
                        {
                            let values: Vec<_> = match term {
                                Term::Slot(s) => {
                                    old.slots.get(*s).copied().flatten().into_iter().collect()
                                }
                                Term::Literal(lit) => r
                                    .literals
                                    .get(&(sort.clone(), lit.clone()))
                                    .cloned()
                                    .unwrap_or_default(),
                            };
                            keys = keys
                                .into_iter()
                                .flat_map(|key| {
                                    values.iter().map(move |v| {
                                        let mut k = key.clone();
                                        k.push(*v);
                                        k
                                    })
                                })
                                .collect();
                        }
                        for key in keys {
                            self.work.key_lookups += 1;
                            if let Some(rows) = r.by_key.get(&(atom.function.clone(), key)) {
                                for row in rows {
                                    self.work.keyed_row_checks += 1;
                                    if let Some(w) = bind(r, &atom, &r.data[*row], old, names.len())
                                    {
                                        witnesses.push(w);
                                    }
                                }
                            }
                        }
                    }
                } else {
                    for (cell, r) in self.rows.iter().enumerate() {
                        if let Some(rows) = r.by_function.get(&atom.function) {
                            for row in rows {
                                self.work.anchor_row_checks += 1;
                                if let Some(w) = bind(
                                    r,
                                    &atom,
                                    &r.data[*row],
                                    &Witness {
                                        cell,
                                        slots: vec![],
                                    },
                                    names.len(),
                                ) {
                                    witnesses.push(w);
                                }
                            }
                        }
                    }
                }
                let id = self.markers.len();
                self.work.markers_built += 1;
                self.work.stored_witnesses += witnesses.len();
                self.markers.push(Marker { witnesses });
                self.nodes.insert(key, id);
                id
            };
            parent = Some(id);
        }
        let Some(id) = parent else {
            return vec![];
        };
        let mut result = BTreeSet::new();
        for w in &self.markers[id].witnesses {
            let t = &templates[w.cell];
            let mut conditions = vec![[0, 0]; q.slots().len()];
            let mut seen = BTreeMap::new();
            // Restore original slot order, including its choice of alias representative.
            for (&original, &local) in &names {
                if let Some(v) = w.slots[local] {
                    conditions[original] = if let Some(lit) = t.literals.get(&v) {
                        [2, lit.rep()]
                    } else {
                        [1, *seen.entry(v).or_insert(original) as u32]
                    };
                }
            }
            result.insert((t.id, conditions));
        }
        result.into_iter().collect()
    }
}
fn bind(r: &Rows, a: &Atom, row: &[usize], old: &Witness, width: usize) -> Option<Witness> {
    if row.len() != a.terms.len() || a.sorts.len() != row.len() {
        return None;
    }
    let mut next = old.clone();
    next.slots.resize(width, None);
    for ((term, sort), &v) in a.terms.iter().zip(&a.sorts).zip(row) {
        if r.sorts[v] != *sort {
            return None;
        }
        match term {
            Term::Slot(s) => {
                if next.slots[*s].is_some_and(|old| old != v) {
                    return None;
                }
                next.slots[*s] = Some(v);
            }
            Term::Literal(lit) if r.literal_text[v].as_ref() != Some(lit) => return None,
            _ => (),
        }
    }
    Some(next)
}
