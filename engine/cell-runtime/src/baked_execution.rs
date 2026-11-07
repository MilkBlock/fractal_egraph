use super::*;
use crate::slots::Slots;
use crate::stage_spine::{DirectOrigin, Started};

impl Executor {
    pub(super) fn seal_consumer_values(
        &mut self,
        rid: usize,
        direct: Option<&DirectOrigin>,
        values: Vec<Value>,
        inputs: &[usize],
    ) -> Slots {
        if let Some(trial) = self.baked_consumer.as_mut()
            && trial.plan.rule == rid
            && let Some(token) = direct
            && self
                .stage_spine
                .sealed_unit(token, &self.graph.program.iterative)
                == Some(trial.plan.unit)
        {
            trial.counts.sealed += 1;
            trial.counts.sealed_values += values.len() as u64;
            if trial.execute {
                // Stage snapshot and auxiliary analysis clients still take named slots.
                let shadow = if self.audit
                    || cfg!(any(feature = "structural-depth", feature = "family-replay"))
                {
                    trial.counts.diagnostic_shadow_maps += 1;
                    inputs.iter().copied().zip(values.iter().cloned()).collect()
                } else {
                    Slots::default()
                };
                trial.pending.insert(token.0, values);
                return shadow;
            }
            trial.counts.binding_maps_at_match += 1;
        }
        inputs.iter().copied().zip(values).collect()
    }
    pub(super) fn dedup_consumer_ports(
        &mut self,
        rid: usize,
        slots: &[usize],
        found: &mut Vec<Binding>,
    ) -> Result<bool> {
        let Some(trial) = self
            .baked_consumer
            .as_ref()
            .filter(|t| t.plan.rule == rid && !t.pending.is_empty())
        else {
            return Ok(false);
        };
        let ports = |b: &Binding| b.direct.as_ref().and_then(|d| trial.pending.get(&d.0));
        if found
            .iter()
            .any(|b| ports(b).is_none() && slots.iter().any(|s| b.values.get(s).is_none()))
        {
            return Err("incomplete binding".into());
        }
        let compare = |a: &Binding, b: &Binding| {
            let (left, right) = (ports(a), ports(b));
            for (column, slot) in slots.iter().enumerate() {
                let av = left.map_or_else(|| &a.values[slot], |p| &p[column]);
                let bv = right.map_or_else(|| &b.values[slot], |p| &p[column]);
                let order = av.cmp(bv);
                if !order.is_eq() {
                    return order;
                }
            }
            std::cmp::Ordering::Equal
        };
        found.sort_by(compare);
        let before = found.len();
        found.dedup_by(|a, b| compare(a, b).is_eq());
        self.dedup_duplicates += (before - found.len()) as u64;
        Ok(true)
    }
    pub(super) fn apply_consumer_ports(
        &mut self,
        rid: usize,
        known: &QueryFacts,
        step: Started,
        ports: Vec<Value>,
    ) -> Result<()> {
        let trial = self.baked_consumer.as_mut().unwrap();

        let plan = &trial.plan;
        let donors = step.result_inputs(&self.graph.program.iterative);
        let donor = donors.iter().find(|r| r.op == plan.op).and_then(|r| r.fact);
        let committed = self.graph.commit_baked_consumer(plan, ports, donor)?;
        #[cfg(feature = "slot-transfer-audit")]
        self.graph.consumer_audit.record(
            committed.cell,
            plan.op,
            plan.table,
            &committed.key,
            committed.witness,
        )?;
        trial.counts.direct += 1;
        trial.counts.checked_computations_reused += plan.computed.len() as u64;
        trial.counts.frame_input_maps_at_birth += u64::from(committed.born && !committed.compact);
        trial.counts.compact_births += u64::from(committed.compact);
        trial.counts.receipt_hits += u64::from(committed.from_receipt);
        trial.counts.keyed_hits += u64::from(!committed.born && !committed.from_receipt);
        let mut receipts = smallvec::SmallVec::<[Option<Dependency>; 4]>::new();
        receipts.resize(plan.op + 1, None);
        receipts[plan.op] = Some(committed.witness);
        #[cfg(feature = "structural-depth")]
        self.stage_spine
            .structural
            .capture(&self.graph, rid, known, &receipts);
        self.stage_spine
            .effects(u64::from(!committed.from_receipt), 0, 0);
        self.stage_spine.capture_keys(
            &self.graph,
            rid,
            committed.cell,
            step,
            |i| known.dependency(i),
            Some(&receipts),
            |op, _| {
                if op != plan.op {
                    return Err("unexpected consumer publication".into());
                }
                Ok(committed.key.to_vec())
            },
        )?;
        if committed.born {
            self.graph.release_empty(committed.cell);
        }
        Ok(())
    }
    #[cfg(feature = "slot-transfer-audit")]
    pub(super) fn audit_consumer_values<'a>(
        &'a self,
        rid: usize,
        b: &'a Binding,
    ) -> AuditValues<'a> {
        if let Some(t) = &self.baked_consumer
            && t.plan.rule == rid
            && let Some(d) = &b.direct
            && let Some(ports) = t.pending.get(&d.0)
        {
            return AuditValues::Ports(&self.graph.program.rules[rid].inputs, ports);
        }
        AuditValues::Named(&b.values)
    }
}
#[cfg(feature = "slot-transfer-audit")]
pub(super) enum AuditValues<'a> {
    Named(&'a Slots),
    Ports(&'a [usize], &'a [Value]),
}
#[cfg(feature = "slot-transfer-audit")]
impl serde::Serialize for AuditValues<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            Self::Named(v) => v.serialize(s),
            Self::Ports(slots, values) => s.collect_map(slots.iter().zip(values.iter())),
        }
    }
}
