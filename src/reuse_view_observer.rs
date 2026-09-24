//! Diagnostics for actually matched native summary rows and committed effects.
//! Learned signatures predict future grafts; they are NOT rewrite certificates.
use super::*;
use egglog::{TraceBatch, WriteOutcome};
#[derive(Default)]
pub(super) struct Observer {
    pub views: BTreeMap<String, (Key, Vec<usize>)>,
    pub rules: BTreeMap<String, String>,
    pub known: BTreeSet<String>,
    instances: BTreeSet<(String, Vec<NativeValue>)>,
    broken: BTreeSet<(String, Vec<NativeValue>)>,
    reuse: usize,
    novel: usize,
    external: usize,
    incomplete: usize,
    evidence: BTreeMap<String, Value>,
}
impl Observer {
    pub fn consume(&mut self, eg: &EGraph, batch: TraceBatch) -> Result<()> {
        let changed: BTreeSet<_> = batch
            .writes
            .iter()
            .filter(|w| w.outcome == WriteOutcome::Inserted && w.rebuild_of.is_none())
            .map(|w| w.match_event_id)
            .chain(
                batch
                    .unions
                    .iter()
                    .filter(|u| u.displaced.is_some())
                    .map(|u| u.match_event_id),
            )
            .collect();
        let mut reads: BTreeMap<u64, Vec<_>> = BTreeMap::new();
        for r in &batch.reads {
            reads.entry(r.match_event_id).or_default().push(r);
        }
        // Union can alias previously distinct instances. Re-key them using the
        // current engine before counting new instances or repeated break episodes.
        let canon = |items: BTreeSet<(String, Vec<NativeValue>)>| -> BTreeSet<_> {
            items
                .into_iter()
                .map(|(name, key)| {
                    let schema = eg.get_function(&name).unwrap().schema();
                    (
                        name,
                        key.iter()
                            .zip(&schema.input)
                            .map(|(v, s)| eg.get_canonical_value(*v, s))
                            .collect(),
                    )
                })
                .collect()
        };
        self.instances = canon(std::mem::take(&mut self.instances));
        self.broken = canon(std::mem::take(&mut self.broken));
        for m in &batch.matches {
            let Some(rows) = reads.get(&m.event_id) else {
                continue;
            };
            for read in rows {
                let Some(name) = read.table_name.as_deref() else {
                    continue;
                };
                let Some((shape, arities)) = self.views.get(name) else {
                    continue;
                };
                if !m.physical_witness_complete {
                    self.incomplete += 1;
                    continue;
                }
                let schema = eg.get_function(name).unwrap().schema();
                let key: Vec<_> = read
                    .key
                    .iter()
                    .zip(&schema.input)
                    .map(|(v, s)| eg.get_canonical_value(*v, s))
                    .collect();
                if key.len() != shape.values.len() {
                    return Err("summary witness arity mismatch".into());
                }
                self.reuse += 1;
                self.instances.insert((name.into(), key.clone()));
                if !changed.contains(&m.event_id) {
                    continue;
                }
                let footprint: BTreeSet<_> = shape
                    .atoms
                    .iter()
                    .zip(arities)
                    .map(|((op, cols), arity)| {
                        (
                            op.clone(),
                            cols[..*arity].iter().map(|i| key[*i]).collect::<Vec<_>>(),
                        )
                    })
                    .collect();
                let mut outside = vec![];
                let mut symbols: BTreeMap<(String, NativeValue), usize> = BTreeMap::new();
                let mut aliases = vec![];
                for ((sort, _), v) in shape.values.iter().zip(&key) {
                    let n = symbols.len();
                    aliases.push(*symbols.entry((sort.clone(), *v)).or_insert(n));
                }
                for r in rows {
                    let Some(op) = r.table_name.as_deref() else {
                        continue;
                    };
                    if op == name {
                        continue;
                    }
                    let Some(f) = eg.get_function(op) else {
                        continue;
                    };
                    let raw: Vec<_> = r
                        .key
                        .iter()
                        .zip(&f.schema().input)
                        .map(|(v, s)| eg.get_canonical_value(*v, s))
                        .collect();
                    if footprint.contains(&(op.to_string(), raw.clone())) {
                        continue;
                    }
                    let wiring = raw
                        .iter()
                        .zip(&f.schema().input)
                        .map(|(v, s)| {
                            let n = symbols.len();
                            *symbols.entry((s.name().to_string(), *v)).or_insert(n)
                        })
                        .collect::<Vec<_>>();
                    outside.push((
                        op.to_string(),
                        f.schema()
                            .input
                            .iter()
                            .map(|s| s.name().to_string())
                            .collect::<Vec<_>>(),
                        wiring,
                    ));
                }
                if outside.is_empty() {
                    continue;
                }
                outside.sort();
                outside.dedup();
                self.external += 1;
                let rule = self
                    .rules
                    .get(m.rule.as_ref())
                    .cloned()
                    .unwrap_or_else(|| m.rule.to_string());
                let signature = serde_json::to_string(&(
                    &shape.values,
                    &shape.atoms,
                    &aliases,
                    rule,
                    &outside,
                ))?;
                let unexpected = !self.known.contains(&signature);
                if unexpected && self.broken.insert((name.into(), key.clone())) {
                    self.novel += 1;
                }
                // The new observation predicts later executions. It never changes
                // the native rule, skips ripen, or marks a new body saturated.
                self.known.insert(signature.clone());
                let entry=self.evidence.entry(signature).or_insert_with(||json!({"first_event":m.event_id,"view":name,"rule":m.rule.to_string(),"unexpected_signature":unexpected,"external_reads":outside,"committed_effect":true,"occurrences":0}));
                entry["occurrences"] = json!(entry["occurrences"].as_u64().unwrap() + 1);
            }
        }
        Ok(())
    }
    pub fn report(&self) -> Value {
        json!({"reuse_count":self.reuse,"unexpected_break_count":self.novel,"committed_external_grafts":self.external,"instances":self.instances.len(),"incomplete_witnesses":self.incomplete,"events":self.evidence.values().collect::<Vec<_>>(),"scope":"native summary-row reuse and committed external grafts; novelty is an observed typed rule/wiring signature, not a semantic certificate; one break per instance until re-Bake"})
    }
}
