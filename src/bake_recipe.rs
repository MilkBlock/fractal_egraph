use crate::{
    coarse_smooth::{Apply, Comb, CombKind, RelativeBinding, canonical_effect, symbol},
    pipeline::Result,
    saturated_rule_composition::SaturatedRuleComposition,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
pub struct RipSatEgraph;
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
        Ok(Self)
    }
}

pub fn local_recipe(applies: &[Apply], ids: &[usize]) -> Result<Vec<Comb>> {
    if applies.len() != ids.len() {
        return Err("recipe occurrence arity mismatch".into());
    }
    let slots: BTreeMap<_, _> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    if slots.len() != ids.len() {
        return Err("duplicate recipe occurrence".into());
    }
    let mut out = vec![];
    let mut names = BTreeMap::new();
    let mut support: Vec<BTreeSet<crate::coarse_smooth::Effect>> = vec![];
    for (i, a) in applies.iter().enumerate() {
        let parents: Vec<_> = a
            .parents
            .iter()
            .filter_map(|p| slots.get(p).copied())
            .collect();
        if parents.iter().any(|p| *p >= i) {
            return Err("recipe is not topological".into());
        }
        let mut external = a.external.clone();
        let mut binding = vec![];
        for (j, b) in a.binding.iter().enumerate() {
            let local = match b {
                RelativeBinding::ParentPort { parent, output } => {
                    let p = a.parents.get(*parent).ok_or("bad source parent")?;
                    slots.get(p).map(|s| RelativeBinding::ParentPort {
                        parent: parents.iter().position(|x| x == s).unwrap(),
                        output: *output,
                    })
                }
                _ => None,
            };
            binding.push(if let Some(b) = local {
                b
            } else {
                let v = *a.wanted.get(j).ok_or("bad source binding")?;
                let slot = external.iter().position(|x| *x == v).unwrap_or_else(|| {
                    external.push(v);
                    external.len() - 1
                });
                RelativeBinding::External { slot }
            });
        }
        let mut provided: BTreeSet<_> = parents
            .iter()
            .flat_map(|p| support[*p].iter().cloned())
            .collect();
        let mut external_facts = a.external_facts.clone();
        for e in &a.required {
            if !provided.contains(e) && !external_facts.contains(e) {
                external_facts.push(e.clone());
            }
        }
        let coarse = parents.is_empty()
            || !external_facts.is_empty()
            || binding
                .iter()
                .any(|b| matches!(b, RelativeBinding::External { .. }));
        let aliases = a
            .wanted
            .iter()
            .chain(&a.outputs)
            .chain(&external)
            .map(|v| symbol(*v, &mut names))
            .collect();
        let mut effect = |e| canonical_effect(e, &mut names);
        out.push(Comb {
            kind: if coarse {
                CombKind::CoarseRuleComposition
            } else {
                CombKind::SmoothRuleComposition
            },
            rule: a.rule.clone(),
            input_roles: a.input_roles.clone(),
            output_roles: a.output_roles.clone(),
            parents,
            binding,
            aliases,
            input_count: a.wanted.len(),
            output_count: a.outputs.len(),
            external_count: external.len(),
            requirements: a.required.iter().map(&mut effect).collect(),
            effects: a.produced.iter().map(&mut effect).collect(),
            external_facts: external_facts.iter().map(&mut effect).collect(),
        });
        provided.extend(a.produced.iter().cloned());
        support.push(provided);
    }
    Ok(out)
}
