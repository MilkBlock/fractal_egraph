//! Constructor knowledge carried by live recipe instances, not a global memo.
use crate::program::{Arg, Call, Op, Program, Result, array};
use serde::{Deserialize, Serialize};
use serde_json::{Value as J, json};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum Source {
    Query(usize),
    Effect(usize),
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Plans {
    pub static_slots: Vec<Vec<Option<Arg>>>,
    pub static_skip: Vec<Rc<Vec<bool>>>,
    pub values: BTreeMap<usize, Rc<Vec<Option<Arg>>>>,
    pub rules: BTreeMap<usize, Vec<Option<Source>>>,
    pub queries: BTreeSet<usize>,
    pub remember: BTreeMap<usize, BTreeSet<usize>>,
}
fn normalize(a: &Arg, copies: &BTreeMap<usize, Arg>) -> Arg {
    let mut a = a.clone();
    while let Arg::Slot(s) = a {
        if let Some(b) = copies.get(&s) {
            a = b.clone()
        } else {
            break;
        }
    }
    a
}
impl Plans {
    pub fn parse(p: &Program, raw: &J) -> Result<Self> {
        if raw.is_null() {
            if matches!(
                std::env::var("CELL_CPU_KNOWN_RESULTS").as_deref(),
                Ok("on" | "observe" | "1" | "value" | "static")
            ) {
                return Err("known results require re-emitting the CPU artifact".into());
            }
            return Ok(Self::default());
        }
        let mut plans = vec![];
        let mut result = Self::default();
        for (rid, r) in p.rules.iter().enumerate() {
            let ops = &p.templates[r.template].ops;
            let copies = ops
                .iter()
                .filter_map(|e| {
                    if let Op::Copy(dst, a) = e {
                        Some((*dst, a.clone()))
                    } else {
                        None
                    }
                })
                .collect::<BTreeMap<_, _>>();
            let norm = |aa: &[Arg]| aa.iter().map(|a| normalize(a, &copies)).collect::<Vec<_>>();
            let mut rows = vec![];
            for (i, a) in r.query.iter().enumerate() {
                if let Call::Table(t) = a.call {
                    rows.push((Source::Query(i), t, norm(&a.args)));
                }
            }
            for (i, e) in ops.iter().enumerate() {
                match e {
                    Op::Let(dst, Call::Table(t), aa) if p.tables[*t].virtual_fact => {
                        let mut aa = norm(aa);
                        aa.push(normalize(&Arg::Slot(*dst), &copies));
                        rows.push((Source::Effect(i), *t, aa));
                    }
                    Op::Set(t, aa, value) => {
                        let mut aa = norm(aa);
                        aa.push(normalize(value, &copies));
                        rows.push((Source::Effect(i), *t, aa));
                    }
                    _ => (),
                }
            }
            for (op, e) in ops.iter().enumerate() {
                let Op::Let(_, Call::Table(t), aa) = e else {
                    continue;
                };
                if !p.tables[*t].virtual_fact {
                    continue;
                }
                let key = norm(aa);
                for (source, table, values) in &rows {
                    if matches!(source,Source::Effect(i) if *i>=op) {
                        continue;
                    }
                    if table == t && values[..values.len() - 1] == key {
                        let (kind, index) = match source {
                            Source::Query(i) => {
                                result.queries.insert(rid);
                                ("query", i)
                            }
                            Source::Effect(i) => {
                                result.remember.entry(rid).or_default().insert(*i);
                                ("effect", i)
                            }
                        };
                        plans.push(json!({"rule":rid,"operation":op,"source_kind":kind,"source_index":index}));
                        result
                            .rules
                            .entry(rid)
                            .or_insert_with(|| vec![None; ops.len()])[op] = Some(*source);
                        if matches!(source, Source::Query(_))
                            || matches!(source,Source::Effect(i) if matches!(ops[*i],Op::Let(..)))
                        {
                            Rc::make_mut(
                                result
                                    .values
                                    .entry(rid)
                                    .or_insert_with(|| Rc::new(vec![None; ops.len()])),
                            )[op] = values.last().cloned();
                        }
                        break;
                    }
                }
            }
        }
        let mut static_aliases = vec![];
        let deleted = p
            .rules
            .iter()
            .flat_map(|r| p.templates[r.template].ops.iter())
            .filter_map(|e| {
                if let Op::Hide(t, _, true) = e {
                    Some(*t)
                } else {
                    None
                }
            })
            .collect::<BTreeSet<_>>();
        for (rid, r) in p.rules.iter().enumerate() {
            let t = &p.templates[r.template];
            let mut aliases = BTreeMap::new();
            let mut known: BTreeMap<(usize, Vec<Arg>), (Arg, J)> = BTreeMap::new();
            let mut slots = vec![None; t.sorts.len()];
            let mut skip = vec![false; t.ops.len()];
            for (i, a) in r.query.iter().enumerate() {
                if let Call::Table(t) = a.call
                    && p.tables[t].constructor
                    && !deleted.contains(&t)
                {
                    known
                        .entry((t, a.args[..a.args.len() - 1].to_vec()))
                        .or_insert((
                            a.args.last().unwrap().clone(),
                            p.artifact["recipe"]["rules"][rid]["query"][i]["args"]
                                .as_array()
                                .unwrap()
                                .last()
                                .unwrap()
                                .clone(),
                        ));
                }
            }
            for (i, e) in t.ops.iter().enumerate() {
                match e {
                    Op::Copy(dst, a) => {
                        aliases.insert(*dst, normalize(a, &aliases));
                    }
                    Op::Hide(t, _, true) => known.retain(|(table, _), _| table != t),
                    Op::Let(dst, Call::Table(t), aa) if p.tables[*t].constructor => {
                        let key = (*t, aa.iter().map(|a| normalize(a, &aliases)).collect());
                        if let Some((value, j)) = known.get(&key) {
                            slots[*dst] = Some(value.clone());
                            skip[i] = true;
                            aliases.insert(*dst, value.clone());
                            static_aliases
                                .push(json!({"rule":rid,"operation":i,"dst":dst,"value":j}));
                        } else {
                            known.insert(key, (Arg::Slot(*dst), json!({"slot":dst})));
                        }
                    }
                    _ => (),
                }
            }
            result.static_slots.push(slots);
            result.static_skip.push(Rc::new(skip));
        }
        let mut expected = json!({"schema":"eggfuse-known-results/v1","recipe_sha256":p.artifact["recipe"]["sha256"],"plans":plans,"static_aliases":static_aliases,
            "contract":"live canonical FactRef with identical full constructor key; dirty domains fall back; never skip set/union/guard"});
        if raw.get("static_aliases").is_none() {
            if std::env::var("CELL_CPU_KNOWN_RESULTS").as_deref() == Ok("static") {
                return Err("static aliases require re-emitting the CPU artifact".into());
            }
            expected.as_object_mut().unwrap().remove("static_aliases");
        }
        if *raw != expected {
            return Err("known result projections differ from typed recipe".into());
        }
        // Reject non-array payloads even when a future compiler has no plans.
        array(&raw["plans"])?;
        Ok(result)
    }
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Stats {
    pub static_eliminated: u64,
    pub static_observed_equal:u64,
    pub static_observed_other:u64,
    pub value_eligible: u64,
    pub value_reuses: u64,
    pub observed_same_value: u64,
    pub observed_other_value: u64,
    pub deletion_guard_misses: u64,
    pub requests: u64,
    pub planned: u64,
    pub query_sources: u64,
    pub effect_sources: u64,
    pub available: u64,
    pub unavailable: u64,
    pub dirty: u64,
    pub mismatched: u64,
    pub expired: u64,
    pub validated: u64,
    pub avoided_lookups: u64,
    pub observed_same_witness: u64,
    pub observed_other_witness: u64,
    pub retained_result_writes: u64,
    pub opportunities_by_table: BTreeMap<String, u64>,
}
