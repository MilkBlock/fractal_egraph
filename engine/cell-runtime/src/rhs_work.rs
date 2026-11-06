//! Diagnostic cohort, selected by actual non-resumed closed RHS executions.
//! Replay validates the exact inputs, scheduling position and ordered
//! non-reflexive union requests. It observes normal execution without routing it.
use crate::{
    graph::{Dependency, FactRef, Graph},
    program::{Program, Result},
    slots::Slots,
    value::root,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};
use std::io::Write;
#[derive(Serialize)]
struct CohortRef<'a> {
    program: &'a serde_json::Value,
    events: &'a [Event],
}
#[derive(Serialize, Deserialize)]
struct Event {
    ordinal: u64,
    epoch: u64,
    rule: usize,
    values: Slots,
    unions: Vec<(u64, u64)>,
}
#[derive(Serialize, Deserialize)]
pub(crate) struct Cohort {
    program: String,
    events: Vec<Event>,
}
#[derive(Default, Serialize)]
struct Work {
    block_local_intermediates: u64,
    local_completions: u64,
    calls: u64,
    frames: u64,
    facts: u64,
    function_updates: u64,
    touches: u64,
    lookups: u64,
    union_requests: u64,
    nonreflexive: u64,
}
pub(crate) struct Sample {
    frames: u64,
    facts: u64,
    functions: u64,
    touches: u64,
    lookups: u64,
    queued: usize,
}
impl Sample {
    pub fn read(g: &Graph) -> Self {
        Self {
            frames: g.stats.cells_created,
            facts: g.stats.logical_facts_created,
            functions: g.stats.function_updates,
            touches: g.changes.events,
            lookups: g.key_lookup_calls.get(),
            queued: g.pending_unions.len(),
        }
    }
}
struct Birth {
    rule: usize,
    table: usize,
    root: bool,
    anchor: bool,
    seen: std::cell::Cell<bool>,
}
#[derive(Default)]
pub(crate) struct Births {
    rows: rustc_hash::FxHashMap<FactRef, Birth>,
    pub reading: bool,
}
impl Births {
    #[cfg(feature = "rhs-locals")]
    pub fn insert(&mut self, f: FactRef, rule: usize, table: usize, root: bool, anchor: bool) {
        self.rows.insert(
            f,
            Birth {
                rule,
                table,
                root,
                anchor,
                seen: false.into(),
            },
        );
    }
    pub fn read(&self, w: Dependency) {
        if self.reading
            && let Dependency::Fact(f) = w
            && let Some(birth) = self.rows.get(&f)
        {
            birth.seen.set(true)
        }
    }
}
#[derive(Default, Serialize)]
struct Lifetime {
    created: u64,
    visible_same: u64,
    visible_forwarded: u64,
    hidden: u64,
    absent: u64,
    later_rhs_lookup: u64,
    from_spine_root: u64,
    forwarded_without_later_lookup: u64,
    anchor_op: u64,
    forwarded_to_older: u64,
    older_without_later_lookup: u64,
    older_without_later_lookup_or_anchor: u64,
}
#[derive(Default)]
pub(crate) struct Ledger {
    ordinal: u64,
    digest: Sha256,
    replay: Option<VecDeque<Event>>,
    events: Vec<Event>,
    current: Option<Event>,
    work: BTreeMap<usize, Work>,
    runs: Vec<serde_json::Value>,
    wakes: u64,
    lifetimes: BTreeMap<usize, Lifetime>,
}
impl Ledger {
    pub fn load(&mut self, p: &Program, c: Cohort) -> Result<()> {
        if p.artifact["sha256"] != c.program {
            return Err("RHS cohort program mismatch".into());
        }
        if c.events.windows(2).any(|w| w[0].ordinal >= w[1].ordinal) {
            return Err("unordered RHS cohort".into());
        }
        self.replay = Some(c.events.into());
        Ok(())
    }
    pub fn begin(&mut self, g: &Graph, r: usize, v: &Slots) -> Result<Option<Sample>> {
        self.ordinal += 1;
        if let Some(queue) = &mut self.replay {
            if queue.front().is_none_or(|e| e.ordinal != self.ordinal) {
                return Ok(None);
            }
            let e = queue.pop_front().unwrap();
            if e.rule != r || e.epoch != g.epoch || &e.values != v {
                return Err(format!("RHS cohort binding mismatch at {}", self.ordinal));
            }
            self.current = Some(e);
        }
        Ok(Some(Sample::read(g)))
    }
    pub fn resumed(&self, live: bool) -> Result<()> {
        if live && self.current.is_some() {
            return Err("RHS cohort unexpectedly resumed a Spine".into());
        }
        Ok(())
    }
    pub fn closed(&mut self, g: &Graph, r: usize, v: &Slots) {
        if self.replay.is_none() {
            self.current = Some(Event {
                ordinal: self.ordinal,
                epoch: g.epoch,
                rule: r,
                values: v.clone(),
                unions: vec![],
            })
        }
    }
    #[cfg(feature = "rhs-locals")]
    pub fn locals(&mut self, before: (u64, u64), now: &crate::local_rhs::Counts) {
        if let Some(e) = &self.current {
            let w = self.work.entry(e.rule).or_default();
            w.block_local_intermediates += now.block_local_intermediates - before.0;
            w.local_completions += now.local_complete - before.1;
        }
    }
    pub fn end(&mut self, g: &Graph, before: Option<Sample>) -> Result<()> {
        let Some(mut e) = self.current.take() else {
            return Ok(());
        };
        let b = before.unwrap();
        let unions: Vec<_> = g.pending_unions[b.queued..]
            .iter()
            .copied()
            .map(|(a, b)| (root(&g.parents, a), root(&g.parents, b)))
            .filter(|(a, b)| a != b)
            .collect();
        let w = self.work.entry(e.rule).or_default();
        w.calls += 1;
        w.frames += g.stats.cells_created - b.frames;
        w.facts += g.stats.logical_facts_created - b.facts;
        w.function_updates += g.stats.function_updates - b.functions;
        w.touches += g.changes.events - b.touches;
        w.lookups += g.key_lookup_calls.get() - b.lookups;
        w.union_requests += (g.pending_unions.len() - b.queued) as u64;
        w.nonreflexive += unions.len() as u64;
        if self.replay.is_some() && e.unions != unions {
            return Err(format!("RHS cohort union disagreement at {}", e.ordinal));
        }
        e.unions = unions;
        serde_json::to_writer(&mut self.digest, &e).map_err(|e| e.to_string())?;
        if self.replay.is_none() {
            self.events.push(e);
        }
        Ok(())
    }
    pub fn observe_births(&mut self, g: &mut Graph, b: &mut Births) -> Result<()> {
        for (
            f,
            Birth {
                rule: r,
                table: t,
                root,
                anchor,
                seen,
            },
        ) in b.rows.drain()
        {
            let w = Dependency::Fact(f);
            let target = g.resolve_receipt(w);
            let life = self.lifetimes.entry(r).or_default();
            life.created += 1;
            life.later_rhs_lookup += u64::from(seen.get());
            life.from_spine_root += u64::from(root);
            life.anchor_op += u64::from(anchor);
            if g.receipt_visible(target, t) {
                if w == target {
                    life.visible_same += 1;
                } else {
                    life.visible_forwarded += 1;
                    if let Dependency::Fact(keep) = target
                        && g.fact_state(keep).is_some_and(|s|s.born < g.epoch)
                    {
                        #[cfg(feature = "keeper-proof")]
                        if !seen.get() {
                            g.proof.winner(crate::prefix_proof::Pair {
                                epoch: g.epoch,
                                rule: r,
                                source: f,
                                keeper: keep,
                            })?;
                        }
                        life.forwarded_to_older += 1;
                        life.older_without_later_lookup += u64::from(!seen.get());
                        life.older_without_later_lookup_or_anchor +=
                            u64::from(!seen.get() && !anchor);
                    }
                    life.forwarded_without_later_lookup += u64::from(!seen.get());
                }
            } else if g.live_table(target) == Some(t) {
                life.hidden += 1;
            } else {
                life.absent += 1;
            }
        }
        Ok(())
    }
    pub fn wake(&mut self) {
        self.wakes += 1;
    }
    pub fn barrier(&mut self, g: &Graph, publication: [u64; 3], deliveries: u64) {
        let mut rows = vec![];
        for (t, cs) in g.changes.tables() {
            rows.extend(cs.iter().map(|(w, c)| (t, *w, *c as u8)));
        }
        rows.sort_unstable();
        let mut digest = Sha256::new();
        serde_json::to_writer(&mut digest, &rows).unwrap();
        self.runs.push(serde_json::json!([
            g.epoch,
            format!("{:x}", digest.finalize()),
            publication,
            self.wakes,
            deliveries,
            g.stats.repaired_dependencies,
            g.stats.unions
        ]));
    }
    pub fn write(&self, g: &Graph, out: &std::path::Path) -> Result<()> {
        if self.replay.as_ref().is_some_and(|q| !q.is_empty()) {
            return Err("incomplete RHS cohort replay".into());
        }
        if self.replay.is_none() {
            let f =
                std::fs::File::create(out.join("rhs-cohort.json")).map_err(|e| e.to_string())?;
            let mut writer = std::io::BufWriter::new(f);
            serde_json::to_writer(
                &mut writer,
                &CohortRef {
                    program: &g.program.artifact["sha256"],
                    events: &self.events,
                },
            )
            .map_err(|e| e.to_string())?;
            writer.flush().map_err(|e| e.to_string())?;
        }
        std::fs::write(out.join("rhs-work.json"),serde_json::to_vec(&serde_json::json!({"program":g.program.artifact["sha256"],"cohort_sha256":format!("{:x}",self.digest.clone().finalize()),"rules":self.work,"new_intermediates":self.lifetimes,"barriers":self.runs,"applications":self.ordinal})).map_err(|e|e.to_string())?).map_err(|e|e.to_string())
    }
}
