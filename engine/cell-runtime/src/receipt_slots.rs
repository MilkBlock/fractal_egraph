//! Stage-local proposals read sealed slots. Only the last proposal at a
//! canonical receipt can change its bundle, including a last empty proposal.
use super::*;

#[derive(Clone, Copy)]
enum ValueRef {
    Empty,
    Sealed(u32),
    Scratch(usize),
}
#[derive(Clone, Copy)]
struct Proposed {
    table: usize,
    ordinal: u64,
    value: ValueRef,
}
#[derive(Default)]
struct Bundles {
    // Receipt handle zero denotes the empty bundle.
    sealed: Vec<Packet>,
    free: Vec<u32>,
    scratch: Vec<Packet>,
    anchors: usize,
    references: usize,
}
impl Bundles {
    fn sealed(&self, id: u32) -> Option<&Packet> {
        id.checked_sub(1).map(|i| &self.sealed[i as usize])
    }
    fn ports(&self, v: ValueRef) -> &[Port] {
        match v {
            ValueRef::Empty => &[],
            ValueRef::Sealed(id) => self.sealed(id).map_or(&[], |p| p.ports.as_slice()),
            ValueRef::Scratch(i) => &self.scratch[i].ports,
        }
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
        self.references += p.ports.len();
        if old != 0 {
            let slot = &mut self.sealed[old as usize - 1];
            self.references -= slot.ports.len();
            *slot = p;
            return Ok(old);
        }
        self.anchors += 1;
        if let Some(id) = self.free.pop() {
            self.sealed[id as usize - 1] = p;
            return Ok(id);
        }
        let id = u32::try_from(self.sealed.len() + 1)
            .map_err(|_| "structural slot handles exhausted")?;
        self.sealed.push(p);
        Ok(id)
    }
}
#[derive(Default, Serialize)]
pub(super) struct Metrics {
    pub scratch_created: u64,
    pub scratch_rewritten: u64,
    pub equivalent_proposals: u64,
    pub committed: u64,
    pub unchanged: u64,
    pub retired: u64,
    pub repaired_packets: u64,
}
#[derive(Default)]
pub(super) struct Slots {
    bundles: Bundles,
    proposals: Map<Dependency, Proposed>,
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
        // Suffix reads see only sealed bundles. The writes below target
        // disjoint scratch storage until the original Run's repair barrier.
        let Bundles {
            sealed, scratch, ..
        } = &mut self.bundles;
        let read = |w| {
            g.previous_port_slot(w)
                .checked_sub(1)
                .map(|i| &sealed[i as usize])
        };
        focus.capture(g, r, known, effects, read, |w, table, p| {
            stats.captured += 1;
            stats.saturated_packets += u64::from(p.ports.len() == BUDGET);
            let ordinal = stats.captured;
            let entry = self.proposals.entry(w);
            let (old, was_pending) = match &entry {
                std::collections::hash_map::Entry::Occupied(e) => (e.get().value, true),
                _ => {
                    let id = g.previous_port_slot(w);
                    (
                        if id == 0 {
                            ValueRef::Empty
                        } else {
                            ValueRef::Sealed(id)
                        },
                        false,
                    )
                }
            };
            let prior = match old {
                ValueRef::Empty => &[][..],
                ValueRef::Sealed(i) => sealed[i as usize - 1].ports.as_slice(),
                ValueRef::Scratch(i) => scratch[i].ports.as_slice(),
            };
            if profile {
                stats.census.proposal.observe(prior, &p.ports);
                stats.census.overwritten += u64::from(was_pending);
            }
            let next = if prior == p.ports.as_slice() {
                self.metrics.equivalent_proposals += 1;
                old
            } else if p.ports.is_empty() {
                ValueRef::Empty
            } else if let ValueRef::Scratch(i) = old {
                self.metrics.scratch_rewritten += 1;
                scratch[i] = p;
                old
            } else {
                self.metrics.scratch_created += 1;
                let i = scratch.len();
                scratch.push(p);
                ValueRef::Scratch(i)
            };
            // Even an unchanged or empty write can beat an earlier redirected write.
            entry
                .and_modify(|e| {
                    e.ordinal = ordinal;
                    e.value = next;
                })
                .or_insert(Proposed {
                    table,
                    ordinal,
                    value: next,
                });
        });
    }
    pub fn publish(&mut self, g: &mut Graph, stats: &mut Stats, profile: bool) -> Result<()> {
        // Only forwarded sources can compete with another raw receipt. Move
        // them directly into the existing pending table before committing.
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
                entry
                    .and_modify(|other| {
                        if p.ordinal > other.ordinal {
                            *other = p
                        }
                    })
                    .or_insert(p);
            }
        }
        for (w, p) in self.proposals.drain() {
            if !g.receipt_visible(w, p.table) {
                stats.stale_exports += 1;
                continue;
            }
            let old = g.current_port_slot(w).unwrap();
            let raw = self.bundles.ports(p.value);
            let repair = |donor: &Port| {
                let mut port = *donor;
                port.w = g.resolve_receipt(port.w);
                (port.w != w && g.receipt_visible(port.w, port.t)).then_some(port)
            };
            let adjusted = if raw.iter().any(|x| repair(x) != Some(*x)) {
                self.metrics.repaired_packets += 1;
                Some(Packet {
                    ports: raw.iter().filter_map(repair).collect(),
                })
            } else {
                None
            };
            let next = adjusted.as_ref().map_or(raw, |p| p.ports.as_slice());
            let prior = self
                .bundles
                .sealed(old)
                .map_or(&[][..], |p| p.ports.as_slice());
            if profile {
                stats.census.sealed.observe(prior, next);
            }
            stats.published += u64::from(!next.is_empty());
            for x in next {
                stats.first_exports[x.path.len as usize] += 1;
            }
            if prior == next {
                self.metrics.unchanged += 1;
                continue;
            }
            self.metrics.committed += 1;
            let value = adjusted.unwrap_or_else(|| match p.value {
                ValueRef::Empty => Packet::default(),
                ValueRef::Sealed(id) => self.bundles.sealed(id).unwrap().clone(),
                ValueRef::Scratch(i) => std::mem::take(&mut self.bundles.scratch[i]),
            });
            let handle = self.bundles.set(old, value)?;
            if handle != old {
                g.set_port_slot(w, handle);
            }
        }
        // Keep detached slots alive until every forwarded winner has read its
        // old bundle. Retire the slot itself together with its physical row.
        for (_, id) in g.retired_port_slots.drain() {
            self.bundles.release(id);
            self.metrics.retired += 1;
            if profile {
                stats.census.retired_nonempty += 1;
            }
        }
        self.bundles.scratch.clear();
        stats.peak_anchors = stats.peak_anchors.max(self.bundles.anchors);
        stats.peak_refs = stats.peak_refs.max(self.bundles.references);
        Ok(())
    }
}
