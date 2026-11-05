//! Offline admission from paired ordinary and context discovery profiles.
//! Evidence selects complete matchers, so it need not certify future cardinalities.
use crate::{
    checksum::digest,
    program::{Arg, Call, Program, Result, Rule},
};
use serde_json::{Value as J, json};
use std::collections::{BTreeMap, BTreeSet};

// Color variable incidences and repeated-table occurrences. Sorted multisets
// make the performance hint independent of names and conjunction order.
fn family(p: &Program, r: &Rule) -> String {
    let labels = r
        .query
        .iter()
        .map(|a| match &a.call {
            Call::Table(i) => {
                let t = &p.tables[*i];
                json!([
                    "table",
                    t.constructor,
                    t.schema.iter().map(|s| &s["kind"]).collect::<Vec<_>>()
                ])
            }
            Call::Primitive(n, s) => json!(["primitive", n, s]),
        })
        .collect::<Vec<_>>();
    let mut colors = labels.iter().map(digest).collect::<Vec<_>>();
    for _ in 0..3 {
        let mut vars = BTreeMap::<usize, Vec<(&str, usize)>>::new();
        let mut tables = BTreeMap::<usize, Vec<&str>>::new();
        for (i, atom) in r.query.iter().enumerate() {
            if let Call::Table(t) = atom.call {
                tables.entry(t).or_default().push(&colors[i]);
            }
            for (column, a) in atom.args.iter().enumerate() {
                if let Arg::Slot(s) = a {
                    vars.entry(*s).or_default().push((&colors[i], column));
                }
            }
        }
        for uses in vars.values_mut() {
            uses.sort_unstable()
        }
        for uses in tables.values_mut() {
            uses.sort_unstable()
        }
        colors = r
            .query
            .iter()
            .enumerate()
            .map(|(i, atom)| {
                let table = match atom.call {
                    Call::Table(t) => Some(&tables[&t]),
                    _ => None,
                };
                let ports = atom
                    .args
                    .iter()
                    .map(|a| match a {
                        Arg::Slot(s) => json!(vars[s]),
                        Arg::Literal(v) => json!(["literal", v]),
                    })
                    .collect::<Vec<_>>();
                digest(&json!([labels[i], table, ports]))
            })
            .collect();
    }
    colors.sort_unstable();
    digest(&json!(colors))
}
fn counters(report: &J) -> Result<BTreeMap<usize, (u64, u64, u64)>> {
    let mut rows = BTreeMap::new();
    for r in report["query_profile"]
        .as_array()
        .ok_or("requires CELL_CPU_PROFILE=1")?
    {
        let n = |k: &str| r[k].as_u64().ok_or("invalid query counter");
        if rows
            .insert(
                n("rule")? as usize,
                (n("candidates")?, n("bindings")?, n("calls")?),
            )
            .is_some()
        {
            return Err("duplicate query profile row".into());
        }
    }
    Ok(rows)
}
pub fn bake(p: &Program, base: &J, wide: &J) -> Result<J> {
    let source = p.artifact["sha256"]
        .as_str()
        .ok_or("missing training digest")?;
    if p.artifact.get("context_admission").is_some() {
        return Err("discovery requires an unfiltered training program".into());
    }
    for r in [base, wide] {
        if r["program_sha256"] != source || r["status"] != "complete" {
            return Err("profile does not match completed training program".into());
        }
    }
    if base["local_context"]["enabled"] != false || wide["local_context"]["general"] != true {
        return Err("expected context-off and general discovery reports".into());
    }
    let schedule = |r: &J| -> Result<Vec<J>> {
        Ok(r["steps"]
            .as_array()
            .ok_or("missing Run trace")?
            .iter()
            .map(|s| {
                json!([
                    s["path"],
                    s["iterations"],
                    s["ruleset"],
                    s["epoch"],
                    s["logical_matches"],
                    s["executed_bindings"],
                    s["changed"]
                ])
            })
            .collect())
    };
    let (before, after) = (counters(base)?, counters(wide)?);
    if before.keys().ne(after.keys()) || schedule(base)? != schedule(wide)? {
        return Err("unpaired query schedules".into());
    }
    let mut evidence = BTreeMap::<String, (u64, u64, u64)>::new();
    for (i, (original, bindings, calls)) in before {
        let (visited, new_bindings, new_calls) = after[&i];
        if i >= p.rules.len() || (bindings, calls) != (new_bindings, new_calls) {
            return Err("unpaired rule answers".into());
        }
        if p.context_cuts[i].is_empty() {
            continue;
        }
        let stats = &wide["local_context"]["rules"][i.to_string()];
        let n = |k: &str| stats[k].as_u64().unwrap_or(0);
        let e = evidence.entry(family(p, &p.rules[i])).or_default();
        e.0 += original;
        e.1 += visited;
        // Per-root validation and index probes cost work even without a decoded row.
        e.2 += n("root_reuses") + n("port_probes") + n("reverse_groups");
    }
    let winners = evidence
        .iter()
        .filter(|(_, (old, new, maintenance))| *old > new.saturating_add(*maintenance))
        .map(|(key, _)| key)
        .collect::<Vec<_>>();
    Ok(
        json!({"schema":"eggfuse-context-admission/v1","training_program":source,"families":winners,"evidence":evidence}),
    )
}

pub fn install(p: &Program, mut policy: J) -> Result<J> {
    if policy["schema"] != "eggfuse-context-admission/v1" {
        return Err("invalid context admission schema".into());
    }
    let winners = policy["families"]
        .as_array()
        .ok_or("missing context families")?
        .iter()
        .map(|s| {
            s.as_str()
                .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
                .ok_or("invalid context family")
        })
        .collect::<std::result::Result<BTreeSet<_>, _>>()?;
    // A second fit may widen a previously filtered target.
    let all_cuts = p
        .artifact
        .get("context_admission")
        .map(|_| crate::local_context::derive_cuts(p));
    let cuts = all_cuts.as_ref().unwrap_or(&p.context_cuts);
    let ids = p
        .rules
        .iter()
        .enumerate()
        .filter(|(i, _)| !cuts[*i].is_empty())
        .filter_map(|(i, r)| winners.contains(family(p, r).as_str()).then_some(i))
        .collect::<Vec<_>>();
    policy["recipe"] = p.artifact["recipe"]["sha256"].clone();
    policy["rules"] = json!(ids);
    Ok(policy)
}

pub(crate) fn admitted(p: &Program, policy: &J) -> Result<Vec<bool>> {
    if policy["schema"] != "eggfuse-context-admission/v1"
        || policy["recipe"] != p.artifact["recipe"]["sha256"]
    {
        return Err("context admission belongs to another recipe".into());
    }
    let mut keep = vec![false; p.rules.len()];
    for i in policy["rules"].as_array().ok_or("missing admitted rules")? {
        let id = i
            .as_u64()
            .and_then(|v| usize::try_from(v).ok())
            .ok_or("invalid admitted rule")?;
        if id >= keep.len() || keep[id] || p.context_cuts[id].is_empty() {
            return Err("invalid admitted cut".into());
        }
        keep[id] = true;
    }
    Ok(keep)
}
