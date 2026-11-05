//! Stage-local proposals read sealed slots. Only the last proposal at a
//! canonical receipt can change its bundle, including a last empty proposal.
use super::*;

struct Proposed {
    rule: usize,
    emitter: usize,
    table: usize,
    raw: Dependency,
    ordinal: u64,
    sources: SmallVec<[Option<Dependency>; 2]>,
}
#[derive(Default)]
struct Bundles {
    // Receipt handle zero denotes the empty bundle.
    sealed: Vec<Packet>,
    free: Vec<u32>,
    anchors: usize,
    references: usize,
}
impl Bundles {
    fn sealed(&self, id: u32) -> Option<&Packet> {
        id.checked_sub(1).map(|i| &self.sealed[i as usize])
    }
    fn release(&mut self, id: u32) {
        let old = &mut self.sealed[id as usize - 1];
        self.references -= old.ports.len();
        *old = Packet::default();
        self.anchors -= 1;
        self.free.push(id);
    }
    fn set(&mut self, old: u32, p: Packet) -> Result<u32> {
        if p.ports.is_empty() {
            if old != 0 {
                self.release(old);
            }
            return Ok(0);
        }
        let count = p.ports.len();
        if old != 0 {
            let slot = &mut self.sealed[old as usize - 1];
            self.references = self.references - slot.ports.len() + count;
            *slot = p;
            return Ok(old);
        }
        let id = match self.free.pop() {
            Some(id) => {
                self.sealed[id as usize - 1] = p;
                id
            }
            None => {
                let id = u32::try_from(self.sealed.len() + 1)
                    .map_err(|_| "structural slot handles exhausted")?;
                self.sealed.push(p);
                id
            }
        };
        self.anchors += 1;
        self.references += count;
        Ok(id)
    }
}
#[derive(Default, Serialize)]
pub(super) struct Metrics {
    pub gathered_sources: u64,
    pub evaluated: u64,
    pub committed: u64,
    pub unchanged: u64,
    pub retired: u64,
    pub repaired_packets: u64,
}
#[derive(Default)]
pub(super) struct Slots {
    bundles: Bundles,
    proposals: Map<Dependency, Proposed>,
    changes: Vec<(Dependency, u32, Packet)>,
    pub metrics: Metrics,
}
impl Slots {
    pub fn get(&self, g: &Graph, w: Dependency) -> Option<&Packet> {
        self.bundles.sealed(g.previous_port_slot(w))
    }
    pub fn capture(
        &mut self,
        g: &Graph,
        focus: &focus::Focus,
        r: usize,
        known: &QueryFacts,
        effects: &[Option<Dependency>],
        stats: &mut Stats,
        profile: bool,
    ) {
        let receipt = |s| Depth::source(g, r, s, known, effects);
        for (emitter, op, table, sources) in focus.outputs(r) {
            let Some(anchor) = receipt(Source::Effect(op)) else {
                continue;
            };
            stats.captured += 1;
            let p = self.proposals.entry(anchor).or_insert_with(|| Proposed {
                rule: r,
                emitter,
                table,
                raw: anchor,
                ordinal: 0,
                sources: SmallVec::new(),
            });
            if profile {
                stats.census.overwritten += u64::from(p.ordinal != 0);
            }
            p.ordinal = stats.captured;
            p.rule = r;
            p.table = table;
            p.emitter = emitter;
            p.sources.clear();
            p.sources.extend(sources.iter().map(|s| receipt(*s)));
            self.metrics.gathered_sources += sources.len() as u64;
        }
    }
    pub fn publish(
        &mut self,
        g: &mut Graph,
        focus: &focus::Focus,
        stats: &mut Stats,
        profile: bool,
    ) -> Result<()> {
        // Preserve last RHS order even when its proposal will turn out empty.
        for &source in g.receipt_forwards.keys() {
            if let Some(p) = self.proposals.remove(&source) {
                let target = g.resolve_receipt(source);
                let entry = self.proposals.entry(target);
                if profile {
                    stats.census.redirected += 1;
                    stats.census.collisions += u64::from(matches!(
                        entry,
                        std::collections::hash_map::Entry::Occupied(_)
                    ));
                }
                match entry {
                    std::collections::hash_map::Entry::Vacant(v) => {
                        v.insert(p);
                    }
                    std::collections::hash_map::Entry::Occupied(mut o) => {
                        if p.ordinal > o.get().ordinal {
                            o.insert(p);
                        }
                    }
                }
            }
        }
        // Evaluate all surviving programs against the same previous Run.
        // A winner cannot read another winner's future suffix.
        for (w, p) in self.proposals.drain() {
            if !g.receipt_visible(w, p.table) {
                stats.stale_exports += 1;
                continue;
            }
            self.metrics.evaluated += 1;
            let mut next = focus.assemble(p.rule, p.emitter, p.raw, &p.sources, |src| {
                self.bundles.sealed(g.previous_port_slot(src))
            });
            stats.saturated_packets += u64::from(next.ports.len() == BUDGET);
            let mut repaired = false;
            for x in &mut next.ports {
                let resolved = g.resolve_receipt(x.w);
                repaired |= resolved != x.w;
                x.w = resolved;
            }
            let before = next.ports.len();
            next.ports
                .retain(|x| x.w != w && g.receipt_visible(x.w, x.t));
            self.metrics.repaired_packets += u64::from(repaired || before != next.ports.len());
            let old = g.current_port_slot(w).unwrap();
            let prior = self
                .bundles
                .sealed(old)
                .map_or(&[][..], |p| p.ports.as_slice());
            if profile {
                stats.census.sealed.observe(prior, &next.ports);
            }
            stats.published += u64::from(!next.ports.is_empty());
            for port in &next.ports {
                stats.first_exports[port.path.len as usize] += 1;
            }
            if prior == next.ports.as_slice() {
                self.metrics.unchanged += 1;
                continue;
            }
            self.changes.push((w, old, next));
        }
        for (w, old, next) in self.changes.drain(..) {
            let handle = self.bundles.set(old, next)?;
            if handle != old {
                g.set_port_slot(w, handle);
            }
            self.metrics.committed += 1;
        }
        for (_, id) in g.retired_port_slots.drain() {
            self.bundles.release(id);
            self.metrics.retired += 1;
            if profile {
                stats.census.retired_nonempty += 1;
            }
        }
        stats.peak_anchors = stats.peak_anchors.max(self.bundles.anchors);
        stats.peak_refs = stats.peak_refs.max(self.bundles.references);
        Ok(())
    }
}
