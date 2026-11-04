//! A fused Free Join stage over the Cell query indexes. Subatoms project the
//! shared columns. A probe returns a witness subset for the next stage.
use super::*;
use smallvec::SmallVec;
use std::collections::HashMap;

type Rows = Rc<Vec<Dependency>>;
pub(super) struct Subtrie {
    pub atom: usize,
    pub rows: Rows,
}
pub(super) struct Group {
    pub cover: SmallVec<[Dependency; 2]>,
    pub probes: SmallVec<[Subtrie; 2]>,
}

#[derive(Default, serde::Serialize)]
pub(super) struct Stats {
    stages: u64,
    cover_rows: u64,
    key_groups: u64,
    probes: u64,
    bucket_rows_examined: u64,
    bucket_rows_materialized: u64,
    rejected_cover_rows: u64,
    pub subtrie_uses: u64,
}

pub(super) struct FreeJoin {
    pub enabled: bool,
    pub stats: Stats,
    plans: Vec<Vec<Vec<Subatom>>>,
}

struct Subatom {
    atom: usize,
    table: usize,
    // (shared slot, column in the follower, column in the cover)
    links: Vec<(usize, usize, usize)>,
}
struct Probe {
    atom: usize,
    table: usize,
    selectors: SmallVec<[(usize, u64); 4]>,
    projection: SmallVec<[(usize, usize); 4]>,
}

fn subatoms(query: &[Atom], cover: usize) -> Vec<Subatom> {
    if !matches!(query[cover].call, Call::Table(_)) {
        return Vec::new();
    }
    let mut probes = Vec::new();
    for (atom, follower) in query.iter().enumerate() {
        let Call::Table(table) = follower.call else {
            continue;
        };
        if atom == cover {
            continue;
        }
        let links = follower
            .args
            .iter()
            .enumerate()
            .filter_map(|(col, arg)| {
                let Arg::Slot(slot) = arg else { return None };
                query[cover]
                    .args
                    .iter()
                    .position(|v| v == arg)
                    .map(|source| (*slot, col, source))
            })
            .collect::<Vec<_>>();
        if !links.is_empty() {
            probes.push(Subatom { atom, table, links });
        }
    }
    probes
}

impl FreeJoin {
    pub fn new(p: &Program) -> Self {
        let enabled = std::env::var("CELL_CPU_FREE_JOIN").as_deref() == Ok("1");
        let plans = if enabled {
            p.rules
                .iter()
                .map(|r| (0..r.query.len()).map(|i| subatoms(&r.query, i)).collect())
                .collect()
        } else {
            Vec::new()
        };
        Self {
            enabled,
            plans,
            stats: Stats::default(),
        }
    }

    pub fn partition(
        &mut self,
        g: &Graph,
        index: &mut QueryIndexes,
        rid: usize,
        query: &[Atom],
        cover: usize,
        remaining: &[bool],
        env: &QueryEnv,
        rows: &[Dependency],
    ) -> Result<Option<SmallVec<[Group; 2]>>> {
        if rows.is_empty() {
            return Ok(None);
        }
        let Call::Table(table) = query[cover].call else {
            return Ok(None);
        };
        let mut columns = SmallVec::<[usize; 4]>::new();
        let mut probes = SmallVec::<[Probe; 2]>::new();
        for sub in &self.plans[rid][cover] {
            if !remaining[sub.atom] {
                continue;
            }
            let mut projection = SmallVec::new();
            for &(slot, col, pos) in &sub.links {
                if env.values[slot].is_some() {
                    continue;
                }
                let key_col = if let Some(i) = columns.iter().position(|&c| c == pos) {
                    i
                } else {
                    columns.push(pos);
                    columns.len() - 1
                };
                projection.push((col, key_col));
            }
            if !projection.is_empty() {
                let mut selectors = query[sub.atom]
                    .args
                    .iter()
                    .enumerate()
                    .filter_map(|(col, arg)| {
                        operand(arg, env, g).map(|v| (col, index.fingerprint(&v)))
                    })
                    .collect::<SmallVec<[_; 4]>>();
                for (col, _) in &mut projection {
                    let source_col = *col;
                    *col = selectors.len();
                    selectors.push((source_col, 0));
                }
                index.ensure(g, sub.table)?;
                probes.push(Probe {
                    atom: sub.atom,
                    table: sub.table,
                    selectors,
                    projection,
                });
            }
        }
        if probes.is_empty() {
            return Ok(None);
        }
        index.ensure(g, table)?;
        self.stats.stages += 1;
        self.stats.cover_rows += rows.len() as u64;
        let mut positions = HashMap::new();
        let mut groups = SmallVec::<[Group; 2]>::new();
        // If the prefix and the current bindings already determine the cover's
        // functional key, there is no exact group reuse to amortize a hash map.
        let cache = rows.len() > 1
            && query[cover].args[..query[cover].args.len() - 1]
                .iter()
                .enumerate()
                .any(|(col, a)| {
                    matches!(a,Arg::Slot(slot) if env.values[*slot].is_none())
                        && !columns.contains(&col)
                });
        for &row in rows {
            // The cover may come from an ordinary table scan, including hidden
            // rows. Only visible rows have a current query-index entry.
            let Some(values) = index.hashes(table, row) else {
                continue;
            };
            let key = columns
                .iter()
                .map(|&col| values[col])
                .collect::<SmallVec<[_; 4]>>();
            let group = if let Some(cached) = cache.then(|| positions.get(&key).copied()).flatten()
            {
                cached
            } else {
                self.stats.key_groups += 1;
                let mut children = SmallVec::new();
                for probe in &mut probes {
                    for &(pos, i) in &probe.projection {
                        probe.selectors[pos].1 = key[i];
                    }
                    self.stats.probes += 1;
                    let Some(candidates) = index.probe(
                        probe.table,
                        &probe.selectors,
                        &mut self.stats.bucket_rows_examined,
                        &mut self.stats.bucket_rows_materialized,
                    ) else {
                        break;
                    };
                    children.push(Subtrie {
                        atom: probe.atom,
                        rows: candidates,
                    });
                }
                let result = if children.len() == probes.len() {
                    let i = groups.len();
                    groups.push(Group {
                        cover: SmallVec::new(),
                        probes: children,
                    });
                    Some(i)
                } else {
                    None
                };
                if cache {
                    positions.insert(key, result);
                }
                result
            };
            if let Some(i) = group {
                groups[i].cover.push(row)
            } else {
                self.stats.rejected_cover_rows += 1;
            }
        }
        Ok(Some(groups))
    }
}
