//! Incremental native saturation snapshots. Reuses native compiled rules and
//! semi-naive timestamps, not only exported rows. No full-graph matching on cache hits.
use crate::pipeline::Result;
use egglog::EGraph;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub struct RipSatRuntime {
    states: Vec<EGraph>,
    sources: Vec<String>,
    transitions: BTreeMap<(usize, String), usize>,
}
impl RipSatRuntime {
    /// This low-level API accepts a finite constructor/rule program. The caller
    /// must not pass an unbounded rewrite system or untrusted schedule.
    pub fn new(source: &str) -> Result<Self> {
        let mut eg = EGraph::default();
        let commands = eg.parse_program(None, source)?;
        if commands.iter().any(|c| {
            matches!(
                c,
                egglog::ast::Command::RunSchedule(..)
                    | egglog::ast::Command::AddRuleset(..)
                    | egglog::ast::Command::UnstableCombinedRuleset(..)
                    | egglog::ast::Command::Include(..)
            )
        }) {
            return Err(
                "snapshot runtime currently requires a schedule-free default-ruleset source".into(),
            );
        }
        eg.run_program(commands)?;
        saturate(&mut eg)?;
        Ok(Self {
            states: vec![eg],
            sources: vec![source.into()],
            transitions: BTreeMap::new(),
        })
    }
    /// Only native actions may be grafted: declarations/rules cannot silently
    /// change the rule environment. State IDs are local immutable snapshot IDs.
    pub fn graft(&mut self, base: usize, actions: &str) -> Result<Value> {
        let parent = self.states.get_mut(base).ok_or("unknown RipSat state")?;
        let commands = parent.parse_program(None, actions)?;
        if commands
            .iter()
            .any(|c| !matches!(c, egglog::ast::Command::Action(_)))
        {
            return Err("graft must contain only actions in the existing rule environment".into());
        }
        let normalized = commands
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        let key = (base, normalized.clone());
        if let Some(id) = self.transitions.get(&key) {
            return Ok(json!({"state":id,"snapshot_reused":true,"native_matches":0}));
        }
        let mut eg = parent.fork_shared();
        eg.run_program(commands)?;
        let matches = saturate(&mut eg)?;
        let id = self.states.len();
        self.states.push(eg);
        self.sources.push(format!(
            "{}\n(run-schedule (saturate (run)))\n{}",
            self.sources[base], normalized
        ));
        self.transitions.insert(key, id);
        Ok(json!({"state":id,"snapshot_reused":false,"native_matches":matches}))
    }
    pub fn save_recipes(&self, path: &std::path::Path) -> Result<()> {
        // Portable recipes, NOT serialized compiled engines. Loading must rebake;
        // only same-process state handles preserve the native semi-naive frontier.
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&json!({
                "schema":"ripsat-native-snapshot-recipes/v1", "sources":self.sources,
                "transitions":self.transitions.iter().map(|((base,actions),target)|json!({"base":base,"actions":actions,"target":target})).collect::<Vec<_>>(),
                "scope":"same-binding local runtime; portable recipes require rebake, not a tier0 placement library"
            }))?,
        )?;
        Ok(())
    }
    pub fn check(&mut self, id: usize, facts: &str) -> Result<()> {
        let eg = self.states.get_mut(id).ok_or("unknown RipSat state")?;
        let c = eg.parse_program(None, facts)?;
        if c.iter()
            .any(|c| !matches!(c, egglog::ast::Command::Check(..)))
        {
            return Err("checks only".into());
        }
        eg.run_program(c)?;
        Ok(())
    }
}

// Reuse the project's existing limits; incomplete states are never admitted.
pub(crate) fn saturate(eg: &mut EGraph) -> Result<usize> {
    let limits = crate::ripen_observation::Limits::from_env()?;
    let start = std::time::Instant::now();
    let trace = egglog::TraceSession::with_effects();
    let mut matches = 0;
    loop {
        let facts = eg.get_function_names().iter().map(|n| eg.get_size(n)).sum();
        if !limits
            .reached(start.elapsed().as_secs_f64(), facts)
            .is_empty()
        {
            return Err("unfinished RipSat; not admitted".into());
        }
        trace.set_execution_limits(
            limits
                .milliseconds
                .map(|ms| std::time::Duration::from_millis(ms).saturating_sub(start.elapsed())),
            limits.facts.map(|n| n.saturating_sub(facts)),
        );
        let report = eg.step_rules_with_trace("", &trace)?;
        matches += trace.take_effect_match_count();
        let stopped = trace.execution_stop_reason().is_some();
        trace.clear_execution_limits();
        trace.drain_completed();
        if stopped {
            return Err("unfinished RipSat; not admitted".into());
        }
        if !report.updated {
            return Ok(matches);
        }
    }
}
