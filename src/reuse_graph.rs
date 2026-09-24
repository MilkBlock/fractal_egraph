//! Phase 1: shared RipSatEgraphs, enclosure identities and event accounting.
//! This is an in-memory contract layer, not a planner or an online Bake runtime.
use crate::{
    coarse_smooth::Comb,
    pipeline::Result,
    saturated_rule_composition::{self as state, Comparison, SaturatedRuleComposition},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

macro_rules! id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
        pub struct $name(pub usize);
    };
}
id!(RipSatId);
id!(EnclosureId);
id!(InstanceId);

/// The legacy export DTO also holds initial/unfinished graphs. This wrapper
/// admits only a completed native export. Reports are trusted local artifacts,
/// not cryptographic certificates or independent proofs of rewrite soundness.
#[derive(Clone, Debug, Serialize)]
#[serde(transparent)]
pub struct RipSatEgraph(SaturatedRuleComposition);
impl RipSatEgraph {
    pub fn from_native_export(graph: SaturatedRuleComposition, report: &Value) -> Result<Self> {
        graph.validate()?;
        if report["ripen"]["state"] != "Saturated"
            || report["ripen"]["updated"] != false
            || report["checks"] != "passed"
            || !report["native_stop_reason"].is_null()
            || report["saturated_rule_composition"]["status"] != "exported"
            || report["saturated_rule_composition"]["values"].as_u64()
                != Some(graph.values.len() as u64)
            || report["saturated_rule_composition"]["facts"].as_u64()
                != Some(graph.rows.len() as u64)
            || graph.scope["observation_only"] == true
        {
            return Err("RipSatEgraph requires a completed native saturation export".into());
        }
        Ok(Self(graph))
    }
    pub fn graph(&self) -> &SaturatedRuleComposition {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Interface {
    /// Entries point to value slots in the canonical stored body. Two different
    /// ports may refer to the same value; port degree does not collapse aliases.
    pub upper: Vec<usize>,
    pub lower: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EnclosureDescriptor {
    pub body: RipSatId,
    pub interface: Interface,
    /// Reuses the existing normalized Comb vocabulary. Parent indices are LOCAL
    /// to this topologically ordered recipe, not global trace/instance IDs.
    /// Alias/effect symbols share ONE namespace across all recipe steps.
    /// Full rule text retains its guards; this module does not execute or prove
    /// the recipe. Native replay validation is required before runtime reuse.
    pub origin: Vec<Comb>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RuntimeValue {
    pub sort: String,
    pub id: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RuntimeFact {
    pub run: u64,
    pub table: String,
    pub id: u64,
    pub version: u64,
}
#[derive(Clone, Debug, Serialize)]
pub struct EnclosureInstance {
    pub enclosure: EnclosureId,
    pub run: u64,
    pub revision: u64,
    pub upper: Arc<[RuntimeValue]>,
    pub lower: Arc<[RuntimeValue]>,
    pub fact_slots: BTreeMap<usize, RuntimeFact>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RuntimeEvent {
    pub run: u64,
    pub id: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Graft {
    /// Identifies a committed/applied operation within the instance's run.
    pub event: RuntimeEvent,
    /// Shared causal episode for follow-on applications of ONE unexpected break.
    pub episode: u64,
    pub instance: InstanceId,
    pub revision: u64,
    pub rule: String,
    pub upper: BTreeSet<usize>,
    pub lower: BTreeSet<usize>,
    /// Actual matched row slots, not transitively reachable ancestor nodes.
    pub inner_facts: BTreeMap<usize, RuntimeFact>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Connectivity {
    pub upper: usize,
    pub lower: usize,
    pub inner_nodes_used: usize,
}
impl Graft {
    pub fn connectivity(&self) -> Connectivity {
        Connectivity {
            upper: self.upper.len(),
            lower: self.lower.len(),
            inner_nodes_used: self.inner_facts.values().collect::<BTreeSet<_>>().len(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ReuseKind {
    BodyStorage,
    InternalDerivation,
    QueryPlan,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission {
    InitialBake,
    RuntimeSupplement,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Metrics {
    pub reuse_count: usize,
    pub reuse_by_kind: BTreeMap<ReuseKind, usize>,
    pub unexpected_break_count: usize,
    pub new_enclosure_count: usize,
}
#[derive(Debug)]
pub struct InternedBody {
    pub id: RipSatId,
    /// Incoming value slots -> canonical stored value slots.
    pub value_map: Vec<usize>,
    pub inserted: bool,
}

#[derive(Default)]
pub struct ReuseGraph {
    bodies: Vec<RipSatEgraph>,
    features: Vec<features::Features>,
    enclosures: Vec<Arc<EnclosureDescriptor>>,
    enclosure_index: BTreeMap<Arc<EnclosureDescriptor>, EnclosureId>,
    instances: Vec<EnclosureInstance>,
    instance_index:
        BTreeMap<(EnclosureId, u64, Arc<[RuntimeValue]>, Arc<[RuntimeValue]>), InstanceId>,
    reuse_events: BTreeMap<(InstanceId, u64, ReuseKind), u64>,
    break_episodes: BTreeSet<(InstanceId, u64)>,
    graft_events: BTreeMap<(InstanceId, u64), (Graft, bool)>,
    metrics: Metrics,
}
impl ReuseGraph {
    /// Exact comparison, not weak dominance or a hash-only equality decision.
    pub fn intern_body(&mut self, body: RipSatEgraph) -> Result<InternedBody> {
        for (i, old) in self.bodies.iter().enumerate() {
            if let Comparison::Equivalent { value_map, .. } =
                state::compare(body.graph(), old.graph())?
            {
                return Ok(InternedBody {
                    id: RipSatId(i),
                    value_map,
                    inserted: false,
                });
            }
        }
        let id = RipSatId(self.bodies.len());
        let value_map = (0..body.graph().values.len()).collect();
        self.features.push(features::Features::bake(&body));
        self.bodies.push(body);
        Ok(InternedBody {
            id,
            value_map,
            inserted: true,
        })
    }
    pub fn features(&self, id: RipSatId) -> Option<&features::Features> {
        self.features.get(id.0)
    }
    pub fn body(&self, id: RipSatId) -> Option<&RipSatEgraph> {
        self.bodies.get(id.0)
    }
    pub fn enclosure(&self, id: EnclosureId) -> Option<&EnclosureDescriptor> {
        self.enclosures.get(id.0).map(Arc::as_ref)
    }
    pub fn instance(&self, id: InstanceId) -> Option<&EnclosureInstance> {
        self.instances.get(id.0)
    }
    pub fn metrics(&self) -> &Metrics {
        &self.metrics
    }

    pub fn register_enclosure(
        &mut self,
        mut descriptor: EnclosureDescriptor,
        admission: Admission,
    ) -> Result<EnclosureId> {
        let body = self
            .body(descriptor.body)
            .ok_or("unknown RipSatEgraph")?
            .graph();
        if descriptor
            .interface
            .upper
            .iter()
            .chain(&descriptor.interface.lower)
            .any(|v| *v >= body.values.len())
        {
            return Err("interface references an unknown body value slot".into());
        }
        if descriptor
            .interface
            .upper
            .iter()
            .chain(&descriptor.interface.lower)
            .any(|v| body.values[*v].literal.is_some())
        {
            return Err("a fixed literal is not a variable interface port".into());
        }
        // One namespace preserves aliases/effect identity across recipe steps.
        let mut names = BTreeMap::new();
        for step in &mut descriptor.origin {
            step.aliases = step
                .aliases
                .iter()
                .map(|v| crate::coarse_smooth::symbol(*v, &mut names))
                .collect();
            for effects in [
                &mut step.requirements,
                &mut step.effects,
                &mut step.external_facts,
            ] {
                for e in effects {
                    *e = crate::coarse_smooth::canonical_effect(e, &mut names);
                }
            }
        }
        if descriptor.origin.is_empty() {
            return Err("enclosure needs an origin recipe".into());
        }
        for (i, step) in descriptor.origin.iter().enumerate() {
            if step.rule.trim().is_empty()
                || (step.parents.is_empty()
                    && step.kind != crate::coarse_smooth::CombKind::CoarseRuleComposition)
                || step.parents.iter().any(|p| *p >= i)
                || step.input_roles.len() != step.input_count
                || step.output_roles.len() != step.output_count
                || step.binding.len() != step.input_count
                || step.aliases.len() != step.input_count + step.output_count + step.external_count
                || step.parents.iter().collect::<BTreeSet<_>>().len() != step.parents.len()
            {
                return Err("invalid local origin recipe".into());
            }
            for binding in &step.binding {
                use crate::coarse_smooth::RelativeBinding::*;
                let valid = match binding {
                    External { slot } => *slot < step.external_count,
                    ParentPort { parent, output } => step
                        .parents
                        .get(*parent)
                        .is_some_and(|p| *output < descriptor.origin[*p].output_count),
                };
                if !valid {
                    return Err("origin binding references an unknown port".into());
                }
            }
        }
        if let Some(id) = self.enclosure_index.get(&descriptor) {
            return Ok(*id);
        }
        let id = EnclosureId(self.enclosures.len());
        let descriptor = Arc::new(descriptor);
        self.enclosure_index.insert(descriptor.clone(), id);
        self.enclosures.push(descriptor);
        if admission == Admission::RuntimeSupplement {
            self.metrics.new_enclosure_count += 1;
        }
        Ok(id)
    }
    /// Values must already be canonicalized by the caller in this run/version.
    /// This checks shape/types/required aliases, not primitive guards or replay.
    pub fn instantiate(
        &mut self,
        enclosure: EnclosureId,
        run: u64,
        upper: Vec<RuntimeValue>,
        lower: Vec<RuntimeValue>,
    ) -> Result<InstanceId> {
        let d = self.enclosure(enclosure).ok_or("unknown enclosure")?;
        let body = self.body(d.body).unwrap().graph();
        if upper.len() != d.interface.upper.len() || lower.len() != d.interface.lower.len() {
            return Err("interface binding arity mismatch".into());
        }
        let mut aliases = BTreeMap::new();
        for (slot, value) in d
            .interface
            .upper
            .iter()
            .chain(&d.interface.lower)
            .zip(upper.iter().chain(&lower))
        {
            if body.values[*slot].sort != value.sort {
                return Err("interface binding sort mismatch".into());
            }
            if aliases.insert(*slot, value).is_some_and(|old| old != value) {
                return Err("required interface alias not preserved".into());
            }
        }
        let upper: Arc<[RuntimeValue]> = upper.into();
        let lower: Arc<[RuntimeValue]> = lower.into();
        let key = (enclosure, run, upper.clone(), lower.clone());
        if let Some(id) = self.instance_index.get(&key) {
            return Ok(*id);
        }
        let id = InstanceId(self.instances.len());
        self.instance_index.insert(key, id);
        self.instances.push(EnclosureInstance {
            enclosure,
            run,
            revision: 0,
            upper,
            lower,
            fact_slots: BTreeMap::new(),
        });
        Ok(id)
    }
    /// Register a caller-verified canonical row/version, without manufacturing
    /// a native fact or claiming that a template slot was its actual producer.
    pub fn bind_fact_slot(
        &mut self,
        id: InstanceId,
        revision: u64,
        slot: usize,
        fact: RuntimeFact,
    ) -> Result<()> {
        let instance = self.current(id, revision)?;
        let body = self
            .body(self.enclosure(instance.enclosure).unwrap().body)
            .unwrap()
            .graph();
        if fact.run != instance.run || body.rows.get(slot).is_none_or(|r| r.op != fact.table) {
            return Err("fact does not match the instance run or body slot".into());
        }
        if instance
            .fact_slots
            .get(&slot)
            .is_some_and(|old| old != &fact)
        {
            return Err("advance the instance revision before rebinding a fact slot".into());
        }
        if instance
            .fact_slots
            .values()
            .any(|old| old.table == fact.table && old.id == fact.id && old.version != fact.version)
        {
            return Err("conflicting versions of the same actual fact".into());
        }
        self.instances[id.0].fact_slots.insert(slot, fact);
        Ok(())
    }
    pub fn advance_revision(&mut self, id: InstanceId) -> Result<u64> {
        let instance = self.instances.get_mut(id.0).ok_or("unknown instance")?;
        instance.revision = instance
            .revision
            .checked_add(1)
            .ok_or("instance revision overflow")?;
        instance.fact_slots.clear();
        Ok(instance.revision)
    }
    fn current(&self, id: InstanceId, revision: u64) -> Result<&EnclosureInstance> {
        let instance = self.instance(id).ok_or("unknown instance")?;
        if instance.revision != revision {
            return Err("stale instance revision".into());
        }
        Ok(instance)
    }
    /// Call only after actual validated reuse. A filter hit or weak-dominate
    /// witness is deliberately not an event kind supported by this method.
    pub fn record_applied_reuse(
        &mut self,
        instance: InstanceId,
        revision: u64,
        event: RuntimeEvent,
        kind: ReuseKind,
    ) -> Result<bool> {
        if self.current(instance, revision)?.run != event.run {
            return Err("event belongs to another run".into());
        }
        let key = (instance, event.id, kind);
        if let Some(old) = self.reuse_events.get(&key) {
            if *old != revision {
                return Err("reuse event reassigned to another revision".into());
            }
            return Ok(false);
        }
        self.reuse_events.insert(key, revision);
        self.metrics.reuse_count += 1;
        *self.metrics.reuse_by_kind.entry(kind).or_default() += 1;
        Ok(true)
    }
    /// `unexpected` is supplied by the future interface validator, not inferred
    /// from degree. Subsequent events in an episode remain stored but count once.
    pub fn observe_graft(&mut self, graft: Graft, unexpected: bool) -> Result<bool> {
        let instance = self.current(graft.instance, graft.revision)?;
        if instance.run != graft.event.run {
            return Err("event belongs to another run".into());
        }
        if graft.rule.trim().is_empty()
            || (graft.upper.is_empty() && graft.lower.is_empty() && graft.inner_facts.is_empty())
        {
            return Err("graft needs a rule and an observed contact".into());
        }
        let d = self.enclosure(instance.enclosure).unwrap();
        let body = self.body(d.body).unwrap().graph();
        if graft.upper.iter().any(|p| *p >= d.interface.upper.len())
            || graft.lower.iter().any(|p| *p >= d.interface.lower.len())
            || graft.inner_facts.keys().any(|p| *p >= body.rows.len())
        {
            return Err("graft references an unknown interface/fact slot".into());
        }
        if graft
            .inner_facts
            .iter()
            .any(|(slot, fact)| instance.fact_slots.get(slot) != Some(fact))
        {
            return Err("graft reads a fact not bound in the current instance revision".into());
        }
        let key = (graft.instance, graft.event.id);
        if let Some(old) = self.graft_events.get(&key) {
            if old != &(graft, unexpected) {
                return Err("conflicting graft evidence for the same event".into());
            }
            return Ok(false);
        }
        if unexpected && self.break_episodes.insert((graft.instance, graft.episode)) {
            self.metrics.unexpected_break_count += 1;
        }
        self.graft_events.insert(key, (graft, unexpected));
        Ok(true)
    }
    /// Inspection snapshot only. Event-index restoration is a later persistence
    /// task; this is not advertised as a resumable Bake library format.
    pub fn report(&self) -> Value {
        json!({"schema":"reuse-graph-foundation/v1","scope":"phase-1 in-memory contracts and accounting; no runtime planner feedback",
            "rip_sat_egraphs":self.bodies,"enclosures":self.enclosures,"instances":self.instances,
            "grafts":self.graft_events.values().map(|(g,u)|json!({"graft":g,"unexpected":u,"connectivity":g.connectivity()})).collect::<Vec<_>>(),"metrics":self.metrics})
    }
}

#[path = "reuse_features.rs"]
pub mod features;
#[path = "reuse_library.rs"]
pub mod library;
#[path = "reuse_planner.rs"]
pub mod planner;
#[path = "reuse_storage.rs"]
pub mod storage;
