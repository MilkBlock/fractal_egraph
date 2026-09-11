//! Conditional eventual periodicity of a supplied symbolic macro.
//! This is an offline summary analysis, not a runtime saturation certificate.
use crate::effect_program::Summary;
use serde_json::{Value, json};

pub struct Orbit {
    /// State zero already includes the caller-supplied startup prefix.
    pub states: Vec<Summary>,
    /// First recurrent joint state, and number of macro applications per cycle.
    pub cycle: Option<(usize, usize)>,
    pub unresolved: Option<String>,
}

/// Keep binding/focus variants even when their graph effects coincide.
/// A repeat is conditional on the accumulated preconditions of that state.
pub fn discover(startup: Summary, step: &Summary, limit: usize) -> Orbit {
    assert!(limit > 0);
    let mut result = Orbit {
        states: vec![startup.normalized()],
        cycle: None,
        unresolved: None,
    };
    loop {
        let next = match result.states.last().unwrap().then(step) {
            Ok(next) => next,
            Err(reason) => {
                result.unresolved = Some(reason);
                return result;
            }
        };
        if let Some(first) = result
            .states
            .iter()
            .position(|s| s.same_effect_and_focus(&next))
        {
            result.cycle = Some((first, result.states.len() - first));
            return result;
        }
        if result.states.len() == limit {
            result.unresolved =
                Some("state budget reached; growth or recurrence remains unknown".into());
            return result;
        }
        result.states.push(next);
    }
}

impl Orbit {
    pub fn json(&self) -> Value {
        let mut representatives: Vec<&Summary> = vec![];
        let classes: Vec<usize> = self
            .states
            .iter()
            .map(|state| {
                if let Some(i) = representatives
                    .iter()
                    .position(|r| r.same_graph_effect(state))
                {
                    i
                } else {
                    representatives.push(state);
                    representatives.len() - 1
                }
            })
            .collect();
        let trigger = self.cycle.map(|(first, period)| {
            let stationary = classes[first..].iter().all(|c| *c == classes[first]);
            json!({
                "prefix_applications_after_startup": first,
                "joint_period": period,
                "state_contract": self.states[first].json(),
                "graph_effect_stationary": stationary,
                "status": "conditional_symbolic_cycle",
                "scope": "supplied macro and summary semantics only; no unmodeled interleavings"
            })
        });
        json!({
            "trigger": trigger,
            "joint_states": self.states.iter().map(Summary::json).collect::<Vec<_>>(),
            "effect_class_by_joint_state": classes,
            "unresolved": self.unresolved,
            "growing_fractal_family": "not inferred by finite-state recurrence"
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{binding_program::Term, effect_program::Fact};
    use std::collections::BTreeMap;

    fn interface() -> BTreeMap<String, Term> {
        BTreeMap::from([(
            "focus".into(),
            Term::new("T", "Pair", vec![port(0), port(1)]),
        )])
    }
    fn port(i: usize) -> Term {
        Term::new("T", format!("in:{i}"), vec![])
    }
    fn permutation() -> Summary {
        let mut step = Summary::identity(interface());
        let after = Term::new("T", "Pair", vec![port(1), port(0)]);
        step.record_nodes(&after, false);
        step.record_equality(step.entry["focus"].clone(), after.clone(), false);
        step.exit.insert("focus".into(), after);
        step.normalized()
    }
    #[test]
    fn dirty_prefix_then_stationary_effect_with_two_binding_phases() {
        let orbit = discover(Summary::identity(interface()), &permutation(), 8);
        assert_eq!(orbit.cycle, Some((1, 2)));
        assert_eq!(
            orbit.json()["effect_class_by_joint_state"],
            json!([0, 1, 1])
        );
        assert_eq!(orbit.json()["trigger"]["graph_effect_stationary"], true);
    }
    #[test]
    fn coarse_startup_retains_external_fact_and_equality_conditions() {
        let mut startup = Summary::identity(interface());
        startup
            .requires
            .insert(Fact::Relation("External".into(), vec![port(0)]));
        startup.record_equality(port(0), port(1), true);
        let mut step = permutation();
        step.requires
            .insert(Fact::Relation("Ready".into(), vec![port(0)]));
        let orbit = discover(startup, &step, 8);
        let (first, _) = orbit.cycle.unwrap();
        let trigger = &orbit.states[first];
        assert!(
            trigger
                .requires
                .contains(&Fact::Relation("External".into(), vec![port(0)]))
        );
        // Both phases must be enabled; observing the first phase alone is insufficient.
        for i in 0..2 {
            assert!(
                trigger
                    .requires
                    .contains(&Fact::Relation("Ready".into(), vec![port(i)]))
            );
        }
        assert!(trigger.equivalent_terms(&port(0), &port(1)));
        assert!(!trigger.required_equalities.is_empty());
    }
    #[test]
    fn startup_can_discharge_the_coarse_requirement() {
        let mut startup = Summary::identity(interface());
        let ready = Fact::Relation("Ready".into(), vec![port(0)]);
        startup.adds.insert(ready.clone());
        let mut next = Summary::identity(interface());
        next.requires.insert(ready.clone());
        let composed = startup.then(&next).unwrap();
        assert!(!composed.requires.contains(&ready));
        assert!(composed.adds.contains(&ready));
    }
    #[test]
    fn committed_symbolic_equality_discharges_a_later_guard() {
        let mut startup = Summary::identity(interface());
        startup.record_equality(port(0), port(1), false);
        let mut next = Summary::identity(interface());
        next.record_equality(port(0), port(1), true);
        assert!(startup.then(&next).unwrap().required_equalities.is_empty());
    }
    #[test]
    fn unresolved_external_binding_does_not_produce_a_trigger() {
        let mut next = permutation();
        next.requires
            .insert(Fact::Relation("Outside".into(), vec![port(9)]));
        let orbit = discover(Summary::identity(interface()), &next, 8);
        assert!(orbit.cycle.is_none());
        assert!(orbit.unresolved.unwrap().contains("unbound interface port"));
    }
    #[test]
    fn nested_composition_preserves_context_without_inventing_a_write() {
        let nested = Term::new("T", "Wrap", vec![interface()["focus"].clone()]);
        let startup = Summary::identity(BTreeMap::from([("focus".into(), nested)]));
        let next = permutation();
        let combined = startup.then_at(&next, "focus", &[0]).unwrap();
        assert_eq!(combined.exit["focus"].args[0], next.exit["focus"]);
        assert!(
            !combined
                .adds
                .contains(&Fact::Node(combined.exit["focus"].clone()))
        );
        assert!(startup.then_at(&next, "focus", &[1]).is_err());
    }
    #[test]
    fn stationary_effect_does_not_erase_an_evolving_binding() {
        let entry = BTreeMap::from([("focus".into(), port(0))]);
        let mut step = Summary::identity(entry.clone());
        step.exit
            .insert("focus".into(), Term::new("T", "Next", vec![port(0)]));
        // Deliberately pure interface evolution; all graph effects coincide.
        let orbit = discover(Summary::identity(entry), &step, 5);
        assert!(orbit.cycle.is_none());
        assert!(orbit.unresolved.is_some());
        assert_eq!(
            orbit.json()["effect_class_by_joint_state"],
            json!([0, 0, 0, 0, 0])
        );
    }
}
