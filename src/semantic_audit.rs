//! Observability diagnostics, not a new Cell equivalence relation.
//! Static reachability reports risks. Only a native distinguishing query is a
//! counterexample; agreement on finitely many queries never certifies reuse.
use crate::pipeline::Result;
use egglog::{
    EGraph,
    ast::{Action, Command, Expr, Fact},
};
use serde_json::{Value as Json, json};
use std::collections::{BTreeMap, BTreeSet};

fn calls(e: &Expr, out: &mut BTreeSet<String>) {
    if let Expr::Call(_, name, args) = e {
        out.insert(name.clone());
        for arg in args {
            calls(arg, out);
        }
    }
}
fn facts(fs: &[Fact], out: &mut BTreeSet<String>) {
    for f in fs {
        match f {
            Fact::Eq(_, a, b) => {
                calls(a, out);
                calls(b, out);
            }
            Fact::Fact(e) => calls(e, out),
        }
    }
}

#[derive(Default)]
struct Flow {
    label: String,
    ruleset: Option<String>,
    reads: BTreeSet<String>,
    writes: BTreeSet<String>,
    effects: BTreeSet<String>,
}
fn actions(xs: &[Action], f: &mut Flow) {
    for a in xs {
        // Calls in actions can be lookups as well as materializations. Retain
        // both possibilities rather than silently missing RHS dependencies.
        let mut cs = BTreeSet::new();
        match a {
            Action::Set(_, name, args, value) => {
                f.writes.insert(name.clone());
                f.effects.insert("function_update".into());
                for a in args {
                    calls(a, &mut cs);
                }
                calls(value, &mut cs);
            }
            Action::Change(_, change, name, args) => {
                f.writes.insert(name.clone());
                f.effects.insert(format!("{change:?}").to_lowercase());
                for a in args {
                    calls(a, &mut cs);
                }
            }
            Action::Union(_, a, b) => {
                calls(a, &mut cs);
                calls(b, &mut cs);
                f.effects.insert("union_rebuild".into());
            }
            Action::Expr(_, e) | Action::Let(_, _, e) => calls(e, &mut cs),
            Action::Panic(..) => {
                f.effects.insert("panic".into());
            }
        }
        f.reads.extend(cs.clone());
        f.writes.extend(cs);
    }
}

/// Supplied tables are potential differences, not differences discovered by
/// this static pass. All declared rules are considered, including currently
/// unscheduled rules. Paths are an overapproximation, not executable witnesses.
pub fn risks(source: &str, potential_tables: &[&str]) -> Result<Json> {
    let commands = EGraph::default().parse_program(None, source)?;
    let mut tables = BTreeSet::new();
    let mut flows = vec![];
    let mut unknowns = BTreeSet::new();
    for (index, c) in commands.iter().enumerate() {
        match c {
            Command::Constructor { name, .. }
            | Command::Function { name, .. }
            | Command::Relation { name, .. } => {
                tables.insert(name.clone());
            }
            Command::Datatype { variants, .. } => {
                tables.extend(variants.iter().map(|v| v.name.clone()));
            }
            Command::Datatypes { .. } => {
                unknowns.insert("datatype* needs expanded declarations".to_string());
            }
            Command::Include(..) | Command::UserDefined(..) => {
                unknowns.insert("unexpanded include or external command".to_string());
            }
            _ => {}
        }
        let mut f = Flow {
            label: format!("command:{index}"),
            ..Flow::default()
        };
        match c {
            Command::Rule { rule } => {
                f.label = format!(
                    "rule:{index}:{}",
                    rule.name.chars().take(100).collect::<String>()
                );
                f.ruleset = Some(rule.ruleset.clone());
                facts(&rule.body, &mut f.reads);
                actions(&rule.head.0, &mut f);
                if rule.naive {
                    unknowns.insert("naive rules may call stateful primitives".into());
                }
            }
            Command::Rewrite(rs, r, subsume) => {
                f.ruleset = Some(rs.clone());
                calls(&r.lhs, &mut f.reads);
                facts(&r.conditions, &mut f.reads);
                calls(&r.rhs, &mut f.reads);
                calls(&r.rhs, &mut f.writes);
                f.effects.insert("union_rebuild".into());
                if *subsume {
                    f.effects.insert("subsume".into());
                    calls(&r.lhs, &mut f.writes);
                }
            }
            Command::BiRewrite(..) => {
                unknowns.insert("bi-rewrite requires directional lowering".into());
            }
            Command::Function {
                name,
                merge: Some(e),
                ..
            } => {
                f.label = format!("merge:{name}");
                f.reads.insert(name.clone());
                calls(e, &mut f.reads);
                calls(e, &mut f.writes);
                f.writes.insert(name.clone());
                f.effects.insert("function_merge".into());
            }
            Command::Action(a) => actions(std::slice::from_ref(a), &mut f),
            Command::Check(_, fs) => {
                facts(fs, &mut f.reads);
                f.effects.insert("check".into());
            }
            Command::Extract(_, e, _) => {
                calls(e, &mut f.reads);
                f.effects.insert("extract".into());
                unknowns.insert("extraction through globals and external extractors needs explicit observations".into());
            }
            _ => {}
        }
        if !f.reads.is_empty() || !f.effects.is_empty() {
            flows.push(f);
        }
    }
    for f in &mut flows {
        f.reads.retain(|x| tables.contains(x));
        f.writes.retain(|x| tables.contains(x));
        if f.effects.contains("union_rebuild") {
            // A changed equivalence can change keys, trigger merge functions
            // and alter joins even without a direct read of the source table.
            // All-table propagation is conservative; no typed proof is claimed.
            f.writes.extend(tables.clone());
        }
    }
    let roots = potential_tables
        .iter()
        .filter(|x| tables.contains(**x))
        .map(|x| x.to_string())
        .collect::<BTreeSet<_>>();
    let mut paths: BTreeMap<String, Vec<String>> =
        roots.iter().map(|x| (x.clone(), vec![x.clone()])).collect();
    let mut reached = BTreeSet::new();
    loop {
        let mut changed = false;
        for (i, f) in flows.iter().enumerate() {
            if reached.contains(&i) {
                continue;
            }
            if let Some(read) = f.reads.iter().find(|x| paths.contains_key(*x)) {
                let mut path = paths[read].clone();
                path.push(f.label.clone());
                for target in &f.writes {
                    paths.entry(target.clone()).or_insert_with(|| {
                        let mut p = path.clone();
                        p.push(target.clone());
                        p
                    });
                }
                reached.insert(i);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let readers = flows.iter().enumerate().filter(|(i, _)| reached.contains(i)).map(|(_, f)| {
        json!({"consumer":f.label,"ruleset":f.ruleset,"direct_reads":f.reads.intersection(&roots).collect::<Vec<_>>(),
            "reads":f.reads,"effects":f.effects,"writes":f.writes})
    }).collect::<Vec<_>>();
    Ok(json!({
        "schema":"semantic-observability/v1",
        "status":if readers.is_empty() {"unknown"} else {"potentially_observable"},
        "difference_evidence":"policy_risk_only_not_measured_graph_difference",
        "potentially_different_tables":roots,
        "consumers":readers, "propagation_paths":paths,
        "cell_replacement":"not_certified",
        "final_output_equivalence":"not_checked_by_this_run",
        "warnings":["Stable selection does not imply equal intermediate graphs.",
                    "Do not reuse a Cell solely because extraction matched.",
                    "A dependency path is a risk, not a demonstrated output difference."],
        "scope":"all declared rules; schedule-insensitive, conservative RHS and union/rebuild propagation",
        "unmodeled":unknowns,
        "next_check":"Run the same permitted continuation on both concrete states and compare explicit queries."
    }))
}

fn flat(commands: &[Command]) -> Result {
    for c in commands {
        match c {
            Command::Sort { .. } | Command::Datatype { .. } | Command::Datatypes { .. }
            | Command::Constructor { .. } | Command::Relation { .. } | Command::Function { .. }
            | Command::AddRuleset(..) | Command::UnstableCombinedRuleset(..)
            | Command::Rule { .. } | Command::Rewrite(..) | Command::BiRewrite(..)
            | Command::Action(..) | Command::RunSchedule(..) | Command::Check(..) => {}
            _ => return Err(format!("continuation audit requires flat native programs with explicit check observations; unsupported: {c}").into()),
        }
    }
    Ok(())
}

/// Concrete differential execution, not a bounded solver or a global proof.
/// `check` commands in the continuation are non-mutating probes on clones.
/// Failed queries do not stop later probes; real execution errors do.
/// Run and Saturate boundaries are passed to egglog without rescheduling.
pub fn compare_continuation(left: &str, right: &str, continuation: &str) -> Result<Json> {
    // Install separate native states. Policy-bearing sources need their exact
    // native helper environment, rather than silently running with plain rules.
    let prepare = |source: &str| -> Result<EGraph> {
        if source.starts_with("; egg-layout structural-keys/") {
            return Err("continuation CLI accepts plain native replay sources; key-policy states must use compare_states with their installed environment".into());
        }
        let mut eg = EGraph::default();
        let commands = eg.parse_program(None, source)?;
        flat(&commands)?;
        eg.run_program(commands)?;
        Ok(eg)
    };
    compare_states(prepare(left)?, prepare(right)?, continuation)
}

/// Can also compare already prepared structural-key environments. The caller
/// supplies their exact continuation, including any key-maintenance steps.
pub fn compare_states(mut left: EGraph, mut right: EGraph, continuation: &str) -> Result<Json> {
    let commands = left.parse_program(None, continuation)?;
    flat(&commands)?;
    let mut observations = vec![];
    let mut executed = vec![];
    let mut errors = vec![];
    for (index, command) in commands.into_iter().enumerate() {
        if matches!(command, Command::Check(..)) {
            let probe = |eg: &EGraph| -> std::result::Result<bool, String> {
                match eg.clone().run_program(vec![command.clone()]) {
                    Ok(_) => Ok(true),
                    Err(egglog::Error::CheckError(..)) => Ok(false),
                    Err(e) => Err(e.to_string()),
                }
            };
            let a = probe(&left);
            let b = probe(&right);
            let differs = matches!((&a,&b),(Ok(x),Ok(y)) if x!=y);
            if a.is_err() || b.is_err() {
                errors.push(
                    json!({"command":index,"left":a.as_ref().err(),"right":b.as_ref().err()}),
                );
            }
            observations.push(json!({"command":index,"query":command.to_string(),"left":a.ok(),"right":b.ok(),"distinguishes":differs}));
        } else {
            let a = left.run_program(vec![command.clone()]);
            let b = right.run_program(vec![command.clone()]);
            executed.push(command.to_string());
            if a.is_err() || b.is_err() {
                errors.push(json!({"command":index,"left":a.err().map(|e|e.to_string()),"right":b.err().map(|e|e.to_string())}));
                break;
            }
        }
    }
    let counterexample = observations
        .iter()
        .find(|x| x["distinguishes"] == true)
        .cloned();
    Ok(json!({
        "schema":"semantic-continuation/v1",
        "status":if counterexample.is_some() {"counterexample"} else if !errors.is_empty() || observations.is_empty() {"unknown"} else {"no_counterexample_in_checked_continuation"},
        "cell_replacement":if counterexample.is_some() {"rejected_for_this_continuation"}else{"not_certified"},
        "counterexample":counterexample, "observations":observations, "errors":errors,
        "executed_commands":executed,
        "scope":"these two concrete states and these queries under exactly this continuation",
        "all_continuations_proved":false
    }))
}
