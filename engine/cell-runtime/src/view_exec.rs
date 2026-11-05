//! Both input deltas are consumed at each member's original watermark.
//! The producer and the self-writers still commit at their original RHS ordinal.
#[cfg(feature = "family-port-trial")]
#[path = "family_port_trial.rs"]
mod trial;
use super::*;
use crate::{
    entry_schedule::Scheduler,
    slots::Slots,
    view_plan::{Comparison, Join, Member, Port, Tuple},
};
use rustc_hash::FxHashMap;

#[derive(Default)]
struct Directory {
    rows: FxHashMap<usize, Tuple>,
    keys: FxHashMap<Value, BTreeSet<usize>>,
    epochs: BTreeSet<(u64, usize)>,
}
impl Directory {
    fn update(&mut self, column: usize, i: usize, next: Option<Tuple>) {
        if let Some(old) = self.rows.remove(&i) {
            if next.as_ref().is_none_or(|r| r.epoch != old.epoch) {
                self.epochs.remove(&(old.epoch, i));
            }
            let key = old.port(column);
            if next.as_ref().is_none_or(|r| r.port(column) != key) {
                let bucket = self.keys.get_mut(key).unwrap();
                bucket.remove(&i);
                if bucket.is_empty() {
                    self.keys.remove(key);
                }
            }
        }
        if let Some(row) = next {
            self.keys
                .entry(row.port(column).clone())
                .or_default()
                .insert(i);
            self.epochs.insert((row.epoch, i));
            self.rows.insert(i, row);
        }
    }
}
#[derive(Default, serde::Serialize)]
struct Counts {
    builds: u64,
    buffered_calls: u64,
    driver_delta: u64,
    peer_delta: u64,
    candidate_visits: u64,
    joined_pairs: u64,
    comparison_dispatches: u64,
    outputs: u64,
    epoch_skips: u64,
    bucket_rows_materialized: u64,
    #[serde(skip)]
    search_s: f64,
}
struct Group {
    plan: Join,
    view: Option<usize>,
    counts: Counts,
    stamp: Option<(u64, u64)>,
    streams: Vec<Option<Vec<Binding>>>,
    by_rule: Vec<u64>,
}

pub(super) struct Blocks {
    #[cfg(feature = "family-port-trial")]
    trial: Vec<Option<trial::Trial>>,
    #[cfg(feature = "family-port-trial")]
    last_unions: u64,
    #[cfg(feature = "family-port-trial")]
    union_epoch: Option<u64>,
    groups: Vec<Group>,
    rules: Vec<Option<(usize, usize)>>,
    directories: Vec<Option<(usize, Directory)>>,
    owned: Vec<bool>,
    changes: u64,
    payloads: u64,
    generic_column_updates: u64,
    maintenance_s: f64,
    profile: bool,
}
impl Blocks {
    pub fn new(g: &Graph) -> Self {
        let mut s = Self {
            #[cfg(feature = "family-port-trial")]
            trial: (0..g.program.tables.len()).map(|_| None).collect(),
            #[cfg(feature = "family-port-trial")]
            last_unions: g.stats.unions,
            #[cfg(feature = "family-port-trial")]
            union_epoch: None,
            groups: vec![],
            rules: vec![None; g.program.rules.len()],
            directories: (0..g.program.tables.len()).map(|_| None).collect(),
            owned: vec![false; g.program.tables.len()],
            changes: 0,
            payloads: 0,
            generic_column_updates: 0,
            maintenance_s: 0.,
            profile: std::env::var_os("CELL_CPU_PROFILE").is_some(),
        };
        for block in g.program.view_blocks.iter() {
            s.owned[block.view] = true;
            #[cfg(feature = "family-port-trial")]
            {
                let table = block.producer.driver;
                let only = block.producer.members[0].rule;
                let exclusive = g.program.rules.iter().enumerate().all(|(rid, r)| {
                    rid == only
                        || !r
                            .query
                            .iter()
                            .any(|a| matches!(a.call,Call::Table(t) if t==table))
                });
                if usize::BITS >= 64 && exclusive {
                    s.trial[table] = Some(trial::Trial::new(block.producer.driver_key));
                }
            }
            if cfg!(feature = "view-ports") {
                let col = block.consumers.driver_key;
                let mut dir = Directory::default();
                for w in g.witnesses(block.view) {
                    if let Dependency::Function(i) = w {
                        dir.update(col, i, g.published_view(i));
                    }
                }
                s.directories[block.view] = Some((col, dir));
            }
            for (plan, view) in [
                (&block.producer, None),
                (&block.consumers, Some(block.view)),
            ] {
                let id = s.groups.len();
                let n = plan.members.len();
                for (member, m) in plan.members.iter().enumerate() {
                    s.rules[m.rule] = Some((id, member));
                }
                s.groups.push(Group {
                    plan: plan.clone(),
                    view,
                    counts: Default::default(),
                    stamp: None,
                    streams: (0..n).map(|_| None).collect(),
                    by_rule: vec![0; n],
                });
            }
        }
        s
    }
    pub fn dedicated(&self, t: usize) -> bool {
        #[cfg(feature = "family-port-trial")]
        if self.trial[t].is_some() {
            return true;
        }
        self.directories[t].is_some()
    }
    pub fn publish_table(
        &mut self,
        g: &Graph,
        t: usize,
        changes: &FxHashMap<Dependency, crate::change_batch::Change>,
        indexed: bool,
    ) {
        #[cfg(feature = "family-port-trial")]
        {
            if self.last_unions != g.stats.unions {
                self.last_unions = g.stats.unions;
                self.union_epoch = Some(g.epoch);
            }
            if let Some(trial) = &mut self.trial[t] {
                trial.published(changes, self.union_epoch == Some(g.epoch), self.profile);
                trial.stats.generic_columns |= indexed;
            }
        }
        if !self.owned[t] {
            return;
        }
        self.changes += changes.len() as u64;
        if indexed {
            self.generic_column_updates += changes.len() as u64;
        }
        let Some((col, dir)) = &mut self.directories[t] else {
            return;
        };
        let clock = self.profile.then(Instant::now);
        for &w in changes.keys() {
            if let Dependency::Function(i) = w {
                let tuple = g.published_view(i);
                self.payloads += u64::from(tuple.is_some());
                dir.update(*col, i, tuple);
            }
        }
        if let Some(c) = clock {
            self.maintenance_s += c.elapsed().as_secs_f64();
        }
    }
    pub fn matches(
        &mut self,
        rid: usize,
        watermark: u64,
        g: &mut Graph,
        ix: &mut QueryIndexes,
        entries: &mut Scheduler,
        spine: &crate::stage_spine::Programs,
    ) -> Result<Option<Vec<Binding>>> {
        let Some((id, member)) = self.rules[rid] else {
            return Ok(None);
        };
        let group = &mut self.groups[id];
        if group.stamp == Some((g.epoch, watermark))
            && let Some(stream) = group.streams[member].take()
        {
            group.counts.buffered_calls += 1;
            return Ok(Some(stream));
        }
        let clock = self.profile.then(Instant::now);
        let dir = self.directories[group.plan.driver].as_ref().map(|(_, d)| d);
        #[cfg(feature = "family-port-trial")]
        let dir = if let Some(t) = &mut self.trial[group.plan.driver] {
            t.refresh(g, group.plan.driver, self.profile)?;
            Some(&t.directory)
        } else {
            dir
        };
        let before = group.counts.candidate_visits;
        group.build(rid, member, watermark, dir, g, ix, entries, spine)?;
        g.stats.residual_candidates += group.counts.candidate_visits - before;
        if let Some(c) = clock {
            group.counts.search_s += c.elapsed().as_secs_f64();
        }
        Ok(Some(group.streams[member].take().unwrap()))
    }
    #[cfg(feature = "family-port-trial")]
    fn trial_report(all: &[Option<trial::Trial>], g: &Graph, ix: &QueryIndexes) -> J {
        J::Array(all.iter().enumerate().filter_map(|(i,t)|{let t=t.as_ref()?;Some(json!({"table":g.program.tables[i].name,"port_rows":t.directory.rows.len(),"generic_columns":ix.has_columns(i),"cost":t.stats}))}).collect())
    }
    pub fn report(&self, g: &Graph, ix: &QueryIndexes) -> J {
        #[cfg(feature = "family-port-trial")]
        let trial = Self::trial_report(&self.trial, g, ix);
        #[cfg(not(feature = "family-port-trial"))]
        let trial = J::Null;
        json!({"producer_port_trial":trial,"dedicated_ports":cfg!(feature="view-ports"),"view_change_headers":self.changes,"port_payload_deliveries":self.payloads,
            "view_generic_column_updates":self.generic_column_updates,"port_maintenance_s":self.maintenance_s,
            "tables":g.program.view_blocks.iter().map(|b|json!({"view":g.program.tables[b.view].name,"generic_columns":ix.has_columns(b.view),"port_rows":self.directories[b.view].as_ref().map_or(0,|(_,d)|d.rows.len())})).collect::<Vec<_>>(),
            "groups":self.groups.iter().map(|v|json!({"driver":g.program.tables[v.plan.driver].name,"peer":g.program.tables[v.plan.peer].name,"rules":v.plan.members.iter().map(|m|m.rule).collect::<Vec<_>>(),"by_rule":v.by_rule,"counts":v.counts,"search_s":v.counts.search_s})).collect::<Vec<_>>()})
    }
}

fn delta(
    entries: &mut Scheduler,
    g: &Graph,
    rid: usize,
    atom: usize,
    watermark: u64,
) -> Result<Vec<Rc<Row>>> {
    let batch = entries.terminal_batch(g, rid, atom, watermark)?;
    Ok(batch
        .delta()
        .map(|&(_, w)| batch.packet(w).unwrap().row.clone())
        .collect())
}
fn field<'a>(port: Port, driver: &'a Tuple, peer: &'a Row) -> &'a Value {
    match port {
        Port::Driver(i) => driver.port(i),
        Port::Peer(i) => &peer.values[i],
        Port::Unit => &Value::Int(0),
    }
}
fn accepts(m: &Member, d: &Tuple, p: &Row) -> bool {
    m.tests.iter().all(|t| {
        let a = field(t.left, d, p);
        let b = field(t.right, d, p);
        match t.comparison {
            Comparison::Equal => a == b,
            Comparison::Less => a < b,
            Comparison::Greater => a > b,
            Comparison::LessEqual => a <= b,
            Comparison::GreaterEqual => a >= b,
        }
    })
}
impl Group {
    fn emit(&mut self, d: &Tuple, p: &Row, spine: &crate::stage_spine::Programs) {
        if d.port(self.plan.driver_key) != &p.values[self.plan.peer_key] {
            return;
        }
        self.counts.joined_pairs += 1;
        if let Some(branch) = &self.plan.dispatch {
            let member = match field(branch.left, d, p).cmp(field(branch.right, d, p)) {
                std::cmp::Ordering::Less => branch.less,
                std::cmp::Ordering::Equal => branch.equal,
                std::cmp::Ordering::Greater => branch.greater,
            };
            self.counts.comparison_dispatches += 1;
            self.emit_member(member, d, p, spine);
        } else {
            for i in 0..self.plan.members.len() {
                if accepts(&self.plan.members[i], d, p) {
                    self.emit_member(i, d, p, spine);
                }
            }
        }
    }
    fn emit_member(
        &mut self,
        ordinal: usize,
        d: &Tuple,
        p: &Row,
        spine: &crate::stage_spine::Programs,
    ) {
        let m = &self.plan.members[ordinal];
        let values: Slots = m
            .output
            .iter()
            .map(|&(slot, port)| (slot, field(port, d, p).clone()))
            .collect();
        let mut known = spine.alias_query_facts(m.rule);
        known.record(m.driver_atom, Some(d.witness));
        known.record(m.peer_atom, Some(p.witness));
        self.streams[ordinal].as_mut().unwrap().push(Binding {
            view_source: match (self.view, d.witness) {
                (Some(_), Dependency::Function(i)) => Some(i),
                _ => None,
            },
            #[cfg(feature = "iterative-bake")]
            direct: None,
            known,
            values,
            epoch: d.epoch.max(p.epoch),
            parent: None,
            continuation: None,
            compiled: true,
        });
    }

    fn build(
        &mut self,
        rid: usize,
        member: usize,
        watermark: u64,
        dir: Option<&Directory>,
        g: &Graph,
        ix: &mut QueryIndexes,
        entries: &mut Scheduler,
        spine: &crate::stage_spine::Programs,
    ) -> Result<()> {
        self.counts.builds += 1;
        self.stamp = None;
        for s in &mut self.streams {
            *s = Some(Vec::new());
        }
        let m = &self.plan.members[member];
        let peer = delta(entries, g, rid, m.peer_atom, watermark)?;
        let driver = if let Some(dir) = dir {
            dir.epochs
                .range((watermark, 0)..)
                .map(|&(_, i)| dir.rows[&i].clone())
                .collect::<Vec<_>>()
        } else {
            delta(entries, g, rid, m.driver_atom, watermark)?
                .iter()
                .map(|r| Tuple::flat(r))
                .collect()
        };
        self.counts.driver_delta += driver.len() as u64;
        self.counts.peer_delta += peer.len() as u64;
        // Fresh driver with any peer; fresh peer with strictly old driver.
        // This partitions the delta even when both operands arrived in one epoch.
        if !driver.is_empty() {
            ix.ensure(g, self.plan.peer)?;
        }
        for d in driver {
            let k = ix.fingerprint(d.port(self.plan.driver_key));
            if let Some(rows) = ix.probe(
                self.plan.peer,
                &[(self.plan.peer_key, k)],
                &mut 0,
                &mut self.counts.bucket_rows_materialized,
            ) {
                self.counts.candidate_visits += rows.len() as u64;
                for &w in rows.iter() {
                    if let Some(p) = g.row(w)?
                        && !p.hidden
                    {
                        self.emit(&d, &p, spine);
                    }
                }
            }
        }
        if dir.is_none() && !peer.is_empty() {
            ix.ensure(g, self.plan.driver)?;
        }
        for p in peer {
            let key = &p.values[self.plan.peer_key];
            if let Some(dir) = dir {
                if let Some(rows) = dir.keys.get(key) {
                    self.counts.candidate_visits += rows.len() as u64;
                    for &i in rows {
                        if dir.rows[&i].epoch < watermark {
                            self.emit(&dir.rows[&i], &p, spine);
                        } else {
                            self.counts.epoch_skips += 1;
                        }
                    }
                }
            } else {
                let k = ix.fingerprint(key);
                if let Some(rows) = ix.probe(
                    self.plan.driver,
                    &[(self.plan.driver_key, k)],
                    &mut 0,
                    &mut self.counts.bucket_rows_materialized,
                ) {
                    self.counts.candidate_visits += rows.len() as u64;
                    for &w in rows.iter() {
                        if g.visible_epoch(w).is_some_and(|e| e < watermark) {
                            if let Some(d) = g.row(w)? {
                                self.emit(&Tuple::flat(&d), &p, spine);
                            }
                        } else {
                            self.counts.epoch_skips += 1;
                        }
                    }
                }
            }
        }
        for (i, stream) in self.streams.iter_mut().enumerate() {
            let rows = stream.as_mut().unwrap();
            rows.sort_by(|a, b| a.values.cmp(&b.values));
            rows.dedup_by(|a, b| a.values == b.values);
            self.counts.outputs += rows.len() as u64;
            self.by_rule[i] += rows.len() as u64;
        }
        self.stamp = Some((g.epoch, watermark));
        Ok(())
    }
}
