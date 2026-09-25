//! Boundary-slot canonicalization for Graft attachment space.
//!
//! `RipSatEgraph` canonicalization compares complete saturated bodies. This
//! module canonicalizes a different object: which symmetric RipenInput/Apply
//! boundary slots a Grafting coarse rule attaches to. It never unions tier0
//! values, does not erase producer identity, and does not certify body closure.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::coarse_smooth::RelativeBinding;
use crate::reuse_graph::Graft;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SlotDescriptor {
    pub sort: String,
    pub role: String,
    #[serde(default)]
    pub upper_degree: usize,
    #[serde(default)]
    pub lower_degree: usize,
    /// Relative alias class. Equal classes may be permuted only with an equal
    /// structural descriptor; concrete tier0 values are intentionally absent.
    #[serde(default)]
    pub alias_class: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraftCandidate {
    pub rule: String,
    pub slots: Vec<SlotDescriptor>,
    /// Boundary slots touched by this Grafting coarse rule.
    pub targets: BTreeSet<usize>,
    pub upper: BTreeSet<usize>,
    pub lower: BTreeSet<usize>,
    pub binding: Vec<RelativeBinding>,
    /// Operator/effect names that the candidate contributes to its residual.
    #[serde(default)]
    pub residual_ops: Vec<String>,
    /// The concrete event is retained outside the canonical key. This is only a
    /// structural witness and must never become semantic identity.
    #[serde(default)]
    pub witness: Option<GraftWitness>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraftWitness {
    pub event: u64,
    pub instance: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OrbitSignature {
    pub rule: String,
    /// Counts of symmetric slot classes, retaining role/sort/degree structure.
    pub slot_classes: Vec<(SlotDescriptor, usize)>,
    pub target_classes: Vec<(SlotDescriptor, usize)>,
    pub upper_classes: Vec<(SlotDescriptor, usize)>,
    pub lower_classes: Vec<(SlotDescriptor, usize)>,
    /// Relative bindings after External slots are mapped to slot classes.
    pub binding: Vec<String>,
    pub residual_ops: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraftOrbit {
    pub signature: OrbitSignature,
    pub representative: usize,
    pub members: Vec<usize>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SlottedCanonReport {
    pub candidates: usize,
    pub orbits: usize,
    pub compression_ratio: f64,
    pub orbit_sizes: Vec<usize>,
    pub ambiguous_classes: usize,
    pub scope: String,
}

#[derive(Clone, Debug, Default)]
pub struct SlottedCanon {
    candidates: Vec<GraftCandidate>,
    signatures: BTreeMap<OrbitSignature, usize>,
    orbits: Vec<GraftOrbit>,
}
impl SlottedCanon {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, candidate: GraftCandidate) -> usize {
        let signature = signature(&candidate);
        if let Some(orbit) = self.signatures.get(&signature).copied() {
            let member = self.candidates.len();
            self.candidates.push(candidate);
            self.orbits[orbit].members.push(member);
            return orbit;
        }
        let member = self.candidates.len();
        self.candidates.push(candidate);
        let orbit = self.orbits.len();
        self.signatures.insert(signature.clone(), orbit);
        self.orbits.push(GraftOrbit {
            signature,
            representative: member,
            members: vec![member],
        });
        orbit
    }

    pub fn orbit(&self, id: usize) -> Option<&GraftOrbit> {
        self.orbits.get(id)
    }
    pub fn candidate(&self, id: usize) -> Option<&GraftCandidate> {
        self.candidates.get(id)
    }
    pub fn orbits(&self) -> &[GraftOrbit] {
        &self.orbits
    }
    pub fn report(&self) -> SlottedCanonReport {
        let candidates = self.candidates.len();
        let orbits = self.orbits.len();
        let ambiguous_classes = self.orbits.iter().filter(|o| o.members.len() > 1).count();
        SlottedCanonReport {
            candidates,
            orbits,
            compression_ratio: if candidates == 0 {
                1.0
            } else {
                orbits as f64 / candidates as f64
            },
            orbit_sizes: self.orbits.iter().map(|o| o.members.len()).collect(),
            ambiguous_classes,
            scope: "Graft attachment orbit canonicalization; no tier0 union, no RipSatEgraph closure proof".into(),
        }
    }
}

/// Make a structural candidate from a runtime Graft and a separately supplied
/// boundary description. Runtime facts are deliberately omitted from identity.
pub fn from_graft(
    graft: &Graft,
    rule: impl Into<String>,
    slots: Vec<SlotDescriptor>,
    binding: Vec<RelativeBinding>,
    residual_ops: Vec<String>,
) -> GraftCandidate {
    GraftCandidate {
        rule: rule.into(),
        slots,
        targets: graft.upper.union(&graft.lower).copied().collect(),
        upper: graft.upper.clone(),
        lower: graft.lower.clone(),
        binding,
        residual_ops,
        witness: Some(GraftWitness {
            event: graft.event.id,
            instance: graft.instance.0,
        }),
    }
}

fn class_key(slot: &SlotDescriptor) -> SlotDescriptor {
    slot.clone()
}
fn class_counts(
    slots: &[SlotDescriptor],
    selected: &BTreeSet<usize>,
) -> Vec<(SlotDescriptor, usize)> {
    let mut counts: BTreeMap<SlotDescriptor, usize> = BTreeMap::new();
    for index in selected {
        if let Some(slot) = slots.get(*index) {
            *counts.entry(class_key(slot)).or_default() += 1;
        }
    }
    counts.into_iter().collect()
}
fn all_counts(slots: &[SlotDescriptor]) -> Vec<(SlotDescriptor, usize)> {
    class_counts(slots, &(0..slots.len()).collect())
}
fn binding_shape(binding: &[RelativeBinding], slots: &[SlotDescriptor]) -> Vec<String> {
    binding
        .iter()
        .map(|b| match b {
            RelativeBinding::External { slot } => slots
                .get(*slot)
                .map(|s| format!("External:{:?}", class_key(s)))
                .unwrap_or_else(|| "External:invalid".into()),
            RelativeBinding::ParentPort { parent, output } => {
                format!("ParentPort:{parent}:{output}")
            }
        })
        .collect()
}
fn signature(candidate: &GraftCandidate) -> OrbitSignature {
    let mut residual_ops = candidate.residual_ops.clone();
    residual_ops.sort();
    residual_ops.dedup();
    OrbitSignature {
        rule: candidate.rule.clone(),
        slot_classes: all_counts(&candidate.slots),
        target_classes: class_counts(&candidate.slots, &candidate.targets),
        upper_classes: class_counts(&candidate.slots, &candidate.upper),
        lower_classes: class_counts(&candidate.slots, &candidate.lower),
        binding: binding_shape(&candidate.binding, &candidate.slots),
        residual_ops,
    }
}
