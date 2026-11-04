//! Free Join subatoms at a shared delta entry. Plans are derived from the
//! complete typed LHS, before a pivot has been matched or point steps removed.
use crate::program::{Arg, Call, Program};
use crate::value::Value;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) enum Source {
    Cover(usize),
    Literal(Value),
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) struct Projection {
    pub table: usize,
    pub columns: Vec<(usize, Source)>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Step {
    pub atom: usize,
    pub projection: usize,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Plan {
    schema: String,
    recipe_sha256: serde_json::Value,
    pub projections: Vec<Projection>,
    pub entries: Vec<Vec<Vec<Step>>>,
}
impl Plan {
    pub fn derive(p: &Program) -> Self {
        let mut ids = BTreeMap::new();
        let mut projections = vec![];
        let mut entries = Vec::with_capacity(p.rules.len());
        for rule in &p.rules {
            let mut pivots = vec![vec![]; rule.query.len()];
            if rule.seminaive && rule.direct.is_none() {
                for (pivot, driver) in rule.query.iter().enumerate() {
                    if !matches!(driver.call, Call::Table(_)) {
                        continue;
                    }
                    for (atom, other) in rule.query.iter().enumerate() {
                        let Call::Table(table) = other.call else {
                            continue;
                        };
                        if atom == pivot {
                            continue;
                        }
                        let mut columns = vec![];
                        let mut shared = false;
                        for (col, arg) in other.args.iter().enumerate() {
                            match arg {
                                Arg::Literal(v) => columns.push((col, Source::Literal(v.clone()))),
                                Arg::Slot(_) => {
                                    if let Some(i) = driver.args.iter().position(|x| x == arg) {
                                        columns.push((col, Source::Cover(i)));
                                        shared = true;
                                    }
                                }
                            }
                        }
                        if !shared {
                            continue;
                        }
                        let projection = Projection { table, columns };
                        let index = *ids.entry(projection.clone()).or_insert_with(|| {
                            let i = projections.len();
                            projections.push(projection);
                            i
                        });
                        pivots[pivot].push(Step {
                            atom,
                            projection: index,
                        });
                    }
                    // Prefer the most constrained subatom, with a stable atom-order tie break.
                    pivots[pivot].sort_by_key(|s| {
                        std::cmp::Reverse(projections[s.projection].columns.len())
                    });
                }
            }
            entries.push(pivots)
        }
        Self {
            schema: "eggfuse-entry-free-join/v1".into(),
            recipe_sha256: p.artifact["recipe"]["sha256"].clone(),
            projections,
            entries,
        }
    }
}
