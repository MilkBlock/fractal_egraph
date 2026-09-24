//! Owned native query runtime: caches complete fragment bindings, not sampled witnesses.
//! Native user-rule execution is unchanged. Plans are never shared across engines.
use super::*;
use egglog::{
    CompiledFactQuery, EGraph, Value as NativeValue,
    ast::{Expr, Literal},
};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Query {
    pub values: Vec<(String, Option<String>)>,
    pub atoms: Vec<(String, Vec<usize>)>,
    pub projection: Vec<usize>,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    values: Vec<(String, Option<String>)>,
    atoms: Vec<(String, Vec<usize>)>,
}
struct Cached {
    compiled: CompiledFactQuery,
    rows: Option<Arc<Vec<Vec<NativeValue>>>>,
}
#[derive(Default, Serialize)]
pub struct Stats {
    pub compilations: usize,
    pub native_scans: usize,
    pub result_hits: usize,
    pub fragment_joins: usize,
    pub invalidations: usize,
}
pub struct Engine {
    eg: EGraph,
    cache: BTreeMap<Key, Cached>,
    baked: BTreeSet<Key>,
    boundary_demand: BTreeMap<Key, (usize, BTreeSet<usize>)>,
    pub stats: Stats,
}
impl Default for Engine {
    fn default() -> Self {
        Self {
            eg: EGraph::default(),
            cache: BTreeMap::new(),
            baked: BTreeSet::new(),
            boundary_demand: BTreeMap::new(),
            stats: Stats::default(),
        }
    }
}
impl Query {
    fn key(&self, atoms: &[(String, Vec<usize>)]) -> Result<(Key, Vec<usize>)> {
        let mut old_to_new = BTreeMap::new();
        let mut back = vec![];
        let mut rows = vec![];
        for (op, args) in atoms {
            let mut columns = vec![];
            for v in args {
                if *v >= self.values.len() {
                    return Err("unknown query value".into());
                }
                let next = old_to_new.len();
                let i = *old_to_new.entry(*v).or_insert_with(|| {
                    back.push(*v);
                    next
                });
                columns.push(i);
            }
            rows.push((op.clone(), columns));
        }
        Ok((
            Key {
                values: back.iter().map(|v| self.values[*v].clone()).collect(),
                atoms: rows,
            },
            back,
        ))
    }
}
impl Engine {
    pub fn install_library(&mut self, library: &super::library::Library) -> Result<()> {
        for q in library.fragments() {
            let (key, _) = q.key(&q.atoms)?;
            self.baked.insert(key);
        }
        Ok(())
    }
    /// All mutation goes through this owned engine. Invalidate before executing,
    /// including failed commands with partial effects and push/pop rollback.
    pub fn execute(&mut self, source: &str) -> Result<()> {
        let commands = self.eg.parse_program(None, source)?;
        // Declarations and rollback may invalidate backend rule handles, so free
        // them BEFORE execution; ordinary actions/schedules retain compiled plans.
        let structural = commands.iter().any(|c| {
            !matches!(
                c,
                egglog::ast::Command::Action(_)
                    | egglog::ast::Command::RunSchedule(_)
                    | egglog::ast::Command::Check(..)
            )
        });
        if structural {
            self.clear();
        } else {
            for c in self.cache.values_mut() {
                c.rows = None;
            }
        }
        self.stats.invalidations += 1;
        self.eg.run_program(commands)?;
        Ok(())
    }
    fn clear(&mut self) {
        for (_, c) in std::mem::take(&mut self.cache) {
            self.eg.free_fact_query(c.compiled);
        }
    }
    fn scan(&mut self, key: Key) -> Result<Arc<Vec<Vec<NativeValue>>>> {
        if !self.cache.contains_key(&key) {
            let mut parser = egglog::ast::Parser::default();
            let values = key
                .values
                .iter()
                .map(|(s, l)| -> Result<_> {
                    let literal = l
                        .as_ref()
                        .map(|text| -> Result<Literal> {
                            match parser.get_expr_from_string(None, text)? {
                                Expr::Lit(_, l) => Ok(l),
                                _ => Err("query constant must be a literal".into()),
                            }
                        })
                        .transpose()?;
                    Ok((s.clone(), literal))
                })
                .collect::<Result<Vec<_>>>()?;
            let compiled = self.eg.compile_fact_query(
                &values,
                &key.atoms,
                &(0..values.len()).collect::<Vec<_>>(),
            )?;
            self.cache.insert(
                key.clone(),
                Cached {
                    compiled,
                    rows: None,
                },
            );
            self.stats.compilations += 1;
        }
        let c = self.cache.get_mut(&key).unwrap();
        if let Some(rows) = &c.rows {
            self.stats.result_hits += 1;
            return Ok(rows.clone());
        }
        let rows: BTreeSet<_> = self.eg.run_fact_query(&c.compiled)?.into_iter().collect();
        let rows = Arc::new(rows.into_iter().collect::<Vec<_>>());
        c.rows = Some(rows.clone());
        self.stats.native_scans += 1;
        Ok(rows)
    }
    /// Reuse the longest ALREADY cached atom prefix. No enumeration of arbitrary
    /// subgraphs. Every shared variable is joined, so hidden binding alternatives
    /// and cross-fragment equalities cannot disappear through projection.
    pub fn query(&mut self, q: &Query) -> Result<Vec<Vec<NativeValue>>> {
        let (full, back) = q.key(&q.atoms)?;
        if q.projection.iter().any(|v| !back.contains(v)) {
            return Err("unbound query projection".into());
        }
        let mut split = None;
        if !self.cache.contains_key(&full) {
            for n in (2..q.atoms.len()).rev() {
                let (key, vars) = q.key(&q.atoms[..n])?;
                if self.cache.contains_key(&key) || self.baked.contains(&key) {
                    split = Some((n, key, vars));
                    break;
                }
            }
        }
        let (rows, vars) = if let Some((n, left, lvars)) = split {
            let (right, rvars) = q.key(&q.atoms[n..])?;
            let demand = self.boundary_demand.entry(left.clone()).or_default();
            demand.0 += 1;
            for (i, v) in lvars.iter().enumerate() {
                if rvars.contains(v) || q.projection.contains(v) {
                    demand.1.insert(i);
                }
            }
            let left = self.scan(left)?;
            let right = self.scan(right)?;
            let shared: Vec<_> = lvars
                .iter()
                .enumerate()
                .filter_map(|(i, v)| rvars.iter().position(|r| r == v).map(|j| (i, j)))
                .collect();
            let mut index: BTreeMap<Vec<NativeValue>, Vec<&Vec<NativeValue>>> = BTreeMap::new();
            for row in right.iter() {
                index
                    .entry(shared.iter().map(|(_, j)| row[*j]).collect())
                    .or_default()
                    .push(row);
            }
            let extra: Vec<_> = rvars
                .iter()
                .enumerate()
                .filter(|(_, v)| !lvars.contains(v))
                .map(|(i, v)| (i, *v))
                .collect();
            let mut result = BTreeSet::new();
            for a in left.iter() {
                let key: Vec<_> = shared.iter().map(|(i, _)| a[*i]).collect();
                for b in index.get(&key).into_iter().flatten() {
                    let mut row = a.clone();
                    row.extend(extra.iter().map(|(i, _)| b[*i]));
                    result.insert(row);
                }
            }
            let mut vars = lvars;
            vars.extend(extra.iter().map(|(_, v)| *v));
            self.stats.fragment_joins += 1;
            (Arc::new(result.into_iter().collect::<Vec<_>>()), vars)
        } else {
            (self.scan(full)?, back)
        };
        let projection: Vec<_> = q
            .projection
            .iter()
            .map(|v| vars.iter().position(|x| x == v).unwrap())
            .collect();
        Ok(rows
            .iter()
            .map(|r| projection.iter().map(|i| r[*i]).collect())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect())
    }
    /// Observed exposed columns, shared across alpha-renamed queries and instances.
    /// These are next-Bake interface suggestions; no automatic semantic rewrite.
    pub fn boundary_demands(&self) -> Value {
        json!(self.boundary_demand.iter().map(|(key,(uses,ports))|json!({"atoms":key.atoms,"typed_values":key.values,"uses":uses,"required_columns":ports})).collect::<Vec<_>>())
    }
    /// Validation path: native whole-query join, with no fragment substitution.
    pub fn reference(&mut self, q: &Query) -> Result<Vec<Vec<NativeValue>>> {
        let (key, back) = q.key(&q.atoms)?;
        if q.projection.iter().any(|v| !back.contains(v)) {
            return Err("unbound query projection".into());
        }
        let rows = self.scan(key)?;
        let p: Vec<_> = q
            .projection
            .iter()
            .map(|v| back.iter().position(|x| x == v).unwrap())
            .collect();
        Ok(rows
            .iter()
            .map(|r| p.iter().map(|i| r[*i]).collect())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect())
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        self.clear();
    }
}
