//! A separately addressable Unit row can borrow a previous row's immutable
//! fields. The directory checks its original key until ordered repair rekeys it.
use super::*;
use crate::view_plan::Write;
use std::rc::Rc;

#[derive(Default, serde::Serialize)]
pub(super) struct Stats {
    pub eligible_misses: u64,
    pub unequal_cert: u64,
    pub source_miss: u64,
    pub direct_sources: u64,
    pub compact: u64,
    pub promoted: u64,
    pub lost: u64,
    pub deleted: u64,
    pub identity_switches: u64,
    pub fast_merges: u64,
    pub peak: u64,
}
struct Alias {
    table: usize,
    hash: u64,
    source: Rc<Vec<Value>>,
    columns: Rc<Vec<usize>>,
    identity: bool,
}
impl Alias {
    fn field(&self, col: usize) -> &Value {
        &self.source[if self.identity {
            col
        } else {
            self.columns[col]
        }]
    }
}
pub(super) struct Units {
    pub plans: Vec<HashMap<usize, Write>>,
    pub owned: Vec<bool>,
    pub counts: Vec<usize>,
    aliases: HashMap<usize, Alias>,
    directory: HashMap<u64, smallvec::SmallVec<[usize; 1]>>,
    pub stats: Stats,
}
impl Units {
    pub fn new(p: &Program) -> Self {
        let mut plans = vec![HashMap::default(); p.templates.len()];
        let mut owned = vec![false; p.tables.len()];
        if cfg!(feature = "view-storage") {
            for block in p.view_blocks.iter() {
                owned[block.view] = true;
                for write in &block.writes {
                    plans[p.rules[write.rule].template].insert(write.op, write.clone());
                }
            }
        }
        Self {
            plans,
            owned,
            counts: vec![0; p.tables.len()],
            aliases: Default::default(),
            directory: Default::default(),
            stats: Default::default(),
        }
    }
    pub fn lookup(&self, t: usize, key: &[Value]) -> Option<usize> {
        if self.counts[t] == 0 {
            return None;
        }
        self.directory
            .get(&hashkey(t, key))?
            .iter()
            .copied()
            .find(|i| {
                let a = &self.aliases[i];
                a.table == t
                    && a.columns.len() == key.len()
                    && key.iter().enumerate().all(|(col, k)| a.field(col) == k)
            })
    }
    #[cfg(feature = "view-consumers")]
    pub fn payload(
        &self,
        i: usize,
        epoch: u64,
        parents: &Parents,
    ) -> Option<crate::view_plan::Tuple> {
        let a = self.aliases.get(&i)?;
        // A cold/rekeyed payload only crosses the barrier after every port is canonical.
        if (0..a.columns.len()).any(|c| canon(a.field(c), parents) != *a.field(c)) {
            return None;
        }
        Some(crate::view_plan::Tuple {
            witness: Dependency::Function(i),
            epoch,
            fields: a.source.clone(),
            columns: (!a.identity).then(|| a.columns.clone()),
        })
    }
    pub fn values(&self, i: usize, parents: &Parents, value: &Value) -> Option<Rc<Vec<Value>>> {
        let a = self.aliases.get(&i)?;
        let mut out = Vec::with_capacity(a.columns.len() + 1);
        out.extend((0..a.columns.len()).map(|col| canon(a.field(col), parents)));
        out.push(canon(value, parents));
        Some(Rc::new(out))
    }
    pub fn install(
        &mut self,
        t: usize,
        key: &[Value],
        i: usize,
        source: Rc<Vec<Value>>,
        columns: Rc<Vec<usize>>,
    ) {
        let hash = hashkey(t, key);
        self.directory.entry(hash).or_default().push(i);
        self.aliases.insert(
            i,
            Alias {
                table: t,
                hash,
                source,
                columns,
                identity: false,
            },
        );
        self.counts[t] += 1;
        self.stats.compact += 1;
        self.stats.peak = self.stats.peak.max(self.aliases.len() as u64);
    }
    pub fn remove(&mut self, i: usize) -> bool {
        let Some(a) = self.aliases.remove(&i) else {
            return false;
        };
        self.counts[a.table] -= 1;
        let ids = self.directory.get_mut(&a.hash).unwrap();
        ids.retain(|v| *v != i);
        if ids.is_empty() {
            self.directory.remove(&a.hash);
        }
        true
    }
    pub fn change_block(&mut self, j: usize, row: &Rc<Vec<Value>>) -> bool {
        let Some(a) = self.aliases.get_mut(&j) else {
            return false;
        };
        self.stats.identity_switches += u64::from(!a.identity);
        a.identity = true;
        a.source = row.clone();
        true
    }
    pub fn report(&self) -> serde_json::Value {
        let mut r = serde_json::json!(self.stats);
        r["still_compact"] = self.aliases.len().into();
        r
    }
}

impl Graph {
    /// A missing key may share a source payload, but still receives its own
    /// logical function index and birth epoch, including before union repair.
    pub(super) fn try_view_projection(
        &mut self,
        t: usize,
        i: usize,
        key: &[Value],
        site: Option<(u32, usize, Option<usize>)>,
    ) -> Result<bool> {
        let Some((cell, op, source)) = site else {
            return Ok(false);
        };
        let Some(plan) = self.units.plans[self.cells[&cell].template]
            .get(&op)
            .cloned()
        else {
            return Ok(false);
        };
        let Some(&(x, y)) = self.pending_unions.last() else {
            return Ok(false);
        };
        self.units.stats.eligible_misses += 1;
        let old = self.arguments(cell, &plan.old)?;
        let after_union = |a: &Value, b: &Value| {
            a == b
                || matches!((a,b),(Value::Eq(m),Value::Eq(n)) if (*m==x && *n==y)||(*m==y && *n==x))
        };
        if x == y || old.len() != key.len() || !old.iter().zip(key).all(|(a, b)| after_union(a, b))
        {
            self.units.stats.unequal_cert += 1;
            return Ok(false);
        }
        let matches_source =
            |r: &Row| r.table == t && r.values.split_last().is_some_and(|(_, k)| k == old);
        let direct = source.and_then(|i| self.function(i)).filter(matches_source);
        self.units.stats.direct_sources += u64::from(direct.is_some());
        let row = direct.or_else(|| {
            self.function_index(t, &old)
                .and_then(|i| self.function(i))
                .filter(matches_source)
        });
        if let Some(row) = row
            && plan.cols.iter().zip(key).all(|(c, k)| row.values[*c] == *k)
        {
            self.units.install(t, key, i, row.values, plan.cols);
            Ok(true)
        } else {
            self.units.stats.source_miss += 1;
            Ok(false)
        }
    }
}
