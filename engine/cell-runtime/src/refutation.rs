//! Exact, online negative receipts for marker-anchored residual queries.
//! These are NOT RipSat completeness certificates. A receipt is admitted only
//! after an unrestricted, unsuccessful residual search, and retains every read
//! range that justified that failure. Hashes choose supersets, never truth.
use crate::{
    graph::Graph,
    query::{QueryIndexes, ReadStamp},
    value::Value,
};
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Operand {
    Bound(Value),
    Variable(usize),
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Operator {
    Table(usize),
    Primitive(String, String),
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    // Alpha-normalized residual conjunction; RHS and ruleset do not affect
    // existence on the same immutable visible snapshot. Atom order is retained.
    pub atoms: Vec<(Operator, Vec<Operand>)>,
}
#[derive(Default, Clone)]
pub struct Evidence {
    pub reads: Vec<ReadStamp>,
    pub delta_restrictions: u64,
    pub query_reads: u64,
}
struct Receipt {
    evidence: Evidence,
    candidates: u64,
    rule: usize,
    epoch: u64,
}
#[derive(Default, serde::Serialize)]
pub struct Stats {
    pub lookups: u64,
    pub hits: u64,
    pub admitted: u64,
    pub invalidated: u64,
    pub delta_incomplete: u64,
    pub positive_searches: u64,
    pub certification_candidates: u64,
    /// Historical search work attached to hits; NOT measured work saved.
    pub prior_search_candidates_on_hits: u64,
    pub peak_receipts: usize,
    pub peak_read_ranges: usize,
    pub per_rule_hits: HashMap<usize, u64>,
    pub stronger_query_hits: u64,
    pub later_epoch_hits: u64,
    pub other_ruleset_hits: u64,
    pub continuation_entries: u64,
    pub max_continuation_depth: u32,
    pub containment_checks: u64,
    pub proof_index_nodes: u64,
    pub index_only_failures: u64,
}
pub struct Refutation {
    pub enabled: bool,
    pub multistage: bool,
    receipts: HashMap<Key, Receipt>,
    shapes: HashMap<Vec<Operator>, ProofIndex>,
    read_ranges: usize,
    published_revision: u64,
    pub stats: Stats,
}
impl Refutation {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            multistage: std::env::var("CELL_CPU_REFUTATION_SCOPE").as_deref() == Ok("multistage"),
            receipts: HashMap::new(),
            shapes: HashMap::new(),
            read_ranges: 0,
            published_revision: 0,
            stats: Stats::default(),
        }
    }
    pub fn reject(
        &mut self,
        rule: usize,
        key: &Key,
        indexes: &QueryIndexes,
        graph: &Graph,
    ) -> Option<Evidence> {
        self.stats.lookups += 1;
        if let Some(r) = self.receipts.get(key) {
            if r.evidence
                .reads
                .iter()
                .all(|s| indexes.valid_read(graph, s))
            {
                self.stats.hits += 1;
                *self.stats.per_rule_hits.entry(rule).or_default() += 1;
                self.stats.prior_search_candidates_on_hits += r.candidates;
                self.record_origin(rule, r.rule, r.epoch, graph);
                return Some(self.receipts[key].evidence.clone());
            }
            self.remove(key);
            self.stats.invalidated += 1;
        }
        if self.multistage {
            // A negative subprogram also refutes a stronger later query. Only
            // ordered contiguous fragments are indexed; no subset/permutation
            // search or representatives. Not finding a proof always falls back.
            let ops = key.atoms.iter().map(|a| a.0.clone()).collect::<Vec<_>>();
            for start in 0..ops.len() {
                for end in start + 2..=ops.len() {
                    let Some(index) = self.shapes.get(&ops[start..end]) else {
                        continue;
                    };
                    let values = key.atoms[start..end]
                        .iter()
                        .flat_map(|a| a.1.iter())
                        .collect::<Vec<_>>();
                    let mut nodes = 0;
                    let mut checks = 0;
                    let negative = index.find(&values, 0, &mut vec![], &mut nodes, &mut |k| {
                        checks += 1;
                        k != key
                            && self.receipts.get(k).is_some_and(|r| {
                                r.evidence
                                    .reads
                                    .iter()
                                    .all(|s| indexes.valid_read(graph, s))
                            })
                    });
                    self.stats.proof_index_nodes += nodes;
                    self.stats.containment_checks += checks;
                    if let Some(negative) = negative {
                        debug_assert!(entails(&key.atoms[start..end], &negative.atoms));
                        let r = &self.receipts[negative];
                        let evidence = r.evidence.clone();
                        let origin = (r.rule, r.epoch, r.candidates);
                        self.stats.hits += 1;
                        self.stats.stronger_query_hits += 1;
                        *self.stats.per_rule_hits.entry(rule).or_default() += 1;
                        self.stats.prior_search_candidates_on_hits += origin.2;
                        self.record_origin(rule, origin.0, origin.1, graph);
                        return Some(evidence);
                    }
                }
            }
        }
        None
    }
    fn record_origin(&mut self, rule: usize, origin: usize, epoch: u64, graph: &Graph) {
        if epoch < graph.epoch {
            self.stats.later_epoch_hits += 1;
        }
        let rs = &graph.program.artifact["recipe"]["rules"];
        if rs[rule]["metadata"]["source_set"] != rs[origin]["metadata"]["source_set"] {
            self.stats.other_ruleset_hits += 1;
        }
    }
    pub fn finish(
        &mut self,
        rule: usize,
        epoch: u64,
        key: Key,
        evidence: Evidence,
        candidates: u64,
        found: bool,
    ) {
        self.stats.certification_candidates += candidates;
        if found {
            self.stats.positive_searches += 1;
        } else if evidence.delta_restrictions > 0 {
            // "No new match in this delta partition" is not "no match".
            self.stats.delta_incomplete += 1;
        } else if self.multistage && evidence.query_reads < 2 {
            // A one-read failure is already represented by an empty index/key
            // lookup. Do not replicate it once for every consumer conjunction.
            self.stats.index_only_failures += 1;
        } else {
            if self.multistage && !self.receipts.contains_key(&key) {
                let shape = key.atoms.iter().map(|a| a.0.clone()).collect();
                self.shapes
                    .entry(shape)
                    .or_default()
                    .insert(&tokens(&key), key.clone());
            }
            self.read_ranges += evidence.reads.len();
            if let Some(old) = self.receipts.insert(
                key,
                Receipt {
                    evidence,
                    candidates,
                    rule,
                    epoch,
                },
            ) {
                self.read_ranges -= old.evidence.reads.len();
            }
            self.stats.admitted += 1;
            self.stats.peak_receipts = self.stats.peak_receipts.max(self.receipts.len());
            self.stats.peak_read_ranges = self.stats.peak_read_ranges.max(self.read_ranges);
        }
    }
    pub fn publish(&mut self, indexes: &QueryIndexes, graph: &Graph) {
        if self.published_revision == indexes.revision {
            return;
        }
        self.published_revision = indexes.revision;
        // Reclaim stale evidence at the published snapshot, including hidden
        // markers. Cached references do not pin graph frames or provider chains.
        let removed = self
            .receipts
            .iter()
            .filter(|(_, r)| {
                !r.evidence
                    .reads
                    .iter()
                    .all(|s| indexes.valid_read(graph, s))
            })
            .map(|(k, _)| k.clone())
            .collect::<Vec<_>>();
        self.stats.invalidated += removed.len() as u64;
        for key in removed {
            self.remove(&key);
        }
    }
    fn remove(&mut self, key: &Key) {
        if let Some(old) = self.receipts.remove(key) {
            self.read_ranges -= old.evidence.reads.len();
        }
        let shape = key.atoms.iter().map(|a| a.0.clone()).collect::<Vec<_>>();
        if let Some(index) = self.shapes.get_mut(&shape) {
            index.remove(&tokens(key));
            if index.empty() {
                self.shapes.remove(&shape);
            }
        }
    }
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"enabled":self.enabled,"multistage_scope":self.multistage,"stats":self.stats,
            "live_receipts":self.receipts.len(),
            "live_read_ranges":self.read_ranges,
            "scope":"online complete query negatives from marker or validated multi-stage recipe entries; not baked Cell closure or static impossibility"})
    }
}

/// Does the candidate conjunction impose at least the negative's constraints?
/// Negative variables may map to candidate variables or constants, with all
/// repeated occurrences preserving correlation. A fixed negative value cannot
/// be matched by an unconstrained candidate variable.
fn entails(candidate: &[(Operator, Vec<Operand>)], negative: &[(Operator, Vec<Operand>)]) -> bool {
    if candidate.len() != negative.len() {
        return false;
    }
    let mut map = HashMap::new();
    candidate.iter().zip(negative).all(|((co, ca), (no, na))| {
        co == no
            && ca.len() == na.len()
            && ca.iter().zip(na).all(|(c, n)| match n {
                Operand::Bound(v) => matches!(c,Operand::Bound(w) if v==w),
                Operand::Variable(i) => map.entry(*i).or_insert_with(|| c.clone()) == c,
            })
    })
}

enum Token {
    Bound(Value),
    New,
    Same(usize),
}
fn tokens(key: &Key) -> Vec<Token> {
    let mut vars = HashMap::new();
    key.atoms
        .iter()
        .flat_map(|a| &a.1)
        .map(|v| match v {
            Operand::Bound(v) => Token::Bound(v.clone()),
            Operand::Variable(i) => {
                if let Some(j) = vars.get(i) {
                    Token::Same(*j)
                } else {
                    let j = vars.len();
                    vars.insert(*i, j);
                    Token::New
                }
            }
        })
        .collect()
}
#[derive(Default)]
struct ProofIndex {
    bound: HashMap<Value, ProofIndex>,
    new: Option<Box<ProofIndex>>,
    same: BTreeMap<usize, ProofIndex>,
    leaf: Option<Key>,
}
impl ProofIndex {
    fn empty(&self) -> bool {
        self.bound.is_empty() && self.new.is_none() && self.same.is_empty() && self.leaf.is_none()
    }
    fn insert(&mut self, path: &[Token], key: Key) {
        let Some((head, tail)) = path.split_first() else {
            self.leaf = Some(key);
            return;
        };
        match head {
            Token::Bound(v) => self.bound.entry(v.clone()).or_default().insert(tail, key),
            Token::New => self
                .new
                .get_or_insert_with(Default::default)
                .insert(tail, key),
            Token::Same(i) => self.same.entry(*i).or_default().insert(tail, key),
        }
    }
    fn remove(&mut self, path: &[Token]) {
        let Some((head, tail)) = path.split_first() else {
            self.leaf = None;
            return;
        };
        match head {
            Token::Bound(v) => {
                if let Some(c) = self.bound.get_mut(v) {
                    c.remove(tail);
                    if c.empty() {
                        self.bound.remove(v);
                    }
                }
            }
            Token::New => {
                if let Some(c) = self.new.as_mut() {
                    c.remove(tail);
                    if c.empty() {
                        self.new = None;
                    }
                }
            }
            Token::Same(i) => {
                if let Some(c) = self.same.get_mut(i) {
                    c.remove(tail);
                    if c.empty() {
                        self.same.remove(i);
                    }
                }
            }
        }
    }
    fn find<'a>(
        &'a self,
        values: &[&Operand],
        i: usize,
        vars: &mut Vec<Operand>,
        nodes: &mut u64,
        valid: &mut impl FnMut(&Key) -> bool,
    ) -> Option<&'a Key> {
        *nodes += 1;
        if i == values.len() {
            return self.leaf.as_ref().filter(|k| valid(k));
        }
        if let Operand::Bound(v) = values[i] {
            if let Some(c) = self.bound.get(v) {
                if let Some(k) = c.find(values, i + 1, vars, nodes, valid) {
                    return Some(k);
                }
            }
        }
        for (j, c) in &self.same {
            if vars.get(*j) == Some(values[i]) {
                if let Some(k) = c.find(values, i + 1, vars, nodes, valid) {
                    return Some(k);
                }
            }
        }
        if let Some(c) = &self.new {
            vars.push(values[i].clone());
            let result = c.find(values, i + 1, vars, nodes, valid);
            vars.pop();
            if result.is_some() {
                return result;
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn atom(t: usize, args: Vec<Operand>) -> (Operator, Vec<Operand>) {
        (Operator::Table(t), args)
    }
    fn b(v: i64) -> Operand {
        Operand::Bound(Value::Int(v))
    }
    fn v(i: usize) -> Operand {
        Operand::Variable(i)
    }
    #[test]
    fn stronger_bindings_and_aliases_preserve_negative_implication() {
        let no = vec![atom(0, vec![b(7), v(0)]), atom(1, vec![v(0), v(1)])];
        assert!(entails(
            &vec![atom(0, vec![b(7), b(3)]), atom(1, vec![b(3), v(9)])],
            &no
        ));
        assert!(entails(
            &vec![atom(0, vec![b(7), v(5)]), atom(1, vec![v(5), v(5)])],
            &no
        ));
        assert!(!entails(
            &vec![atom(0, vec![b(8), v(5)]), atom(1, vec![v(5), v(6)])],
            &no
        ));
        assert!(!entails(
            &vec![atom(0, vec![b(7), v(5)]), atom(1, vec![v(6), v(7)])],
            &no
        ));
        assert!(!entails(
            &vec![atom(0, vec![v(7), v(5)]), atom(1, vec![v(5), v(6)])],
            &no
        ));
    }
    #[test]
    fn proof_index_matches_exact_containment_and_removal() {
        let choices = [b(0), b(1), v(0), v(1)];
        let mut keys = vec![];
        for a in &choices {
            for c in &choices {
                keys.push(Key {
                    atoms: vec![
                        atom(0, vec![a.clone(), v(2)]),
                        atom(1, vec![v(2), c.clone()]),
                    ],
                });
            }
        }
        let mut index = ProofIndex::default();
        for key in &keys {
            index.insert(&tokens(key), key.clone());
        }
        // Duplicate alpha-renamings denote the same pattern; validity concerns
        // semantic containment, not which equivalent stored key is returned.
        for mask in 0..256 {
            let args = (0..4)
                .map(|i| choices[(mask >> (i * 2)) & 3].clone())
                .collect::<Vec<_>>();
            let candidate = vec![atom(0, args[..2].to_vec()), atom(1, args[2..].to_vec())];
            let flat = candidate
                .iter()
                .flat_map(|a| a.1.iter())
                .collect::<Vec<_>>();
            let found = index.find(&flat, 0, &mut vec![], &mut 0, &mut |_| true);
            assert_eq!(
                found.is_some(),
                keys.iter().any(|k| entails(&candidate, &k.atoms))
            );
            if let Some(k) = found {
                assert!(entails(&candidate, &k.atoms));
            }
        }
        for key in &keys {
            index.remove(&tokens(key));
        }
        assert!(index.empty());
    }
}
