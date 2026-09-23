//! Opt-in exhaustive saturated-family composition. No consumer witness or Top-K.
use crate::{
    pipeline::{Result, write_json},
    saturated_rule_composition::{
        Comparison, Row, SaturatedRuleComposition as State, Vertex, compare,
    },
};
use egglog::{
    EGraph, Value,
    ast::{Action, Command},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::Path,
    time::Instant,
};
#[derive(Deserialize, Serialize)]
pub struct Seed {
    pub parameters: usize,
    pub facts: Vec<String>,
    pub result: String,
}
struct Family {
    eg: EGraph,
    state: State,
    terms: Vec<String>,
    parameters: Vec<String>,
}
struct Environment {
    base: EGraph,
    sort: String,
    ops: Vec<String>,
    rulesets: Vec<String>,
    scope: serde_json::Value,
}
fn environment(source: &str) -> Result<Environment> {
    if source.contains("__Cc") {
        return Err("__Cc names are reserved by the CC experiment".into());
    }
    let mut eg = EGraph::default();
    let raw = eg.parse_program(None, source)?;
    let mut commands = vec![];
    let mut sort = None;
    let mut ops = vec![];
    let mut rulesets = BTreeSet::new();
    for (i, cmd) in raw.into_iter().enumerate() {
        let mut cmd = crate::visual_rule::normalize(cmd, i);
        match &mut cmd {
   Command::Datatype{name,variants,..}=>{
    if sort.replace(name.clone()).is_some(){return Err("CC experiment supports one equality datatype".into())}
    if variants.iter().flat_map(|v|&v.types).any(|t|t!=name&&!matches!(t.as_str(),"i64"|"String"|"bool")){return Err("unsupported CC constructor field sort".into())}
    ops.extend(variants.iter().map(|v|v.name.clone()));
    let extra=eg.parse_program(None,"(datatype Temporary (__CcParameter i64) (__CcNode i64))")?;
    let Command::Datatype{variants:extra,..}=&extra[0] else {unreachable!()};variants.extend(extra.clone());
   }
   Command::Rule{rule}=>{
    if rule.head.0.iter().any(|a|!matches!(a,Action::Expr(..)|Action::Let(..)|Action::Union(..))){return Err("CC experiment requires positive constructor/union rules".into())}
    rulesets.insert(rule.ruleset.clone());
   }
   Command::AddRuleset(..)=>{},
   _=>return Err("rules file must contain datatype and positive rules only; put coarse facts in seeds JSON".into()),
  }
        commands.push(cmd);
    }
    let sort = sort.ok_or("missing datatype")?;
    eg.run_program(commands)?;
    Ok(Environment {
        base: eg,
        sort,
        ops,
        rulesets: rulesets.into_iter().collect(),
        scope: json!({"cc_rules":source,"interface":"fixed external parameter ports and current result; private allocation rows excluded"}),
    })
}
fn eval(eg: &mut EGraph, text: &str) -> Result<Value> {
    let e = egglog::ast::Parser::default().get_expr_from_string(None, text)?;
    Ok(eg.eval_expr(&e)?.1)
}
fn saturate(eg: &mut EGraph, env: &Environment) -> Result<usize> {
    let mut n = 0;
    loop {
        n += 1;
        let mut changed = false;
        for r in &env.rulesets {
            changed |= eg.step_rules(r)?.updated;
        }
        if !changed {
            return Ok(n);
        }
    }
}
fn snapshot(
    eg: &mut EGraph,
    env: &Environment,
    parameters: Vec<String>,
    result: &str,
) -> Result<Family> {
    let mut values = vec![];
    let mut terms = vec![];
    let mut ids = BTreeMap::new();
    let mut vertex = |sort: &egglog::ArcSort, v: Value| -> Result<usize> {
        let v = eg.get_canonical_value(v, sort);
        let key = (sort.name().to_string(), v);
        if let Some(i) = ids.get(&key) {
            return Ok(*i);
        }
        let literal = match sort.name() {
            "i64" => Some(eg.value_to_base::<i64>(v).to_string()),
            "String" => Some(serde_json::to_string(
                &eg.value_to_base::<egglog::sort::S>(v).to_string(),
            )?),
            "bool" => Some(eg.value_to_base::<bool>(v).to_string()),
            _ => None,
        };
        let term = if let Some(l) = &literal {
            l.clone()
        } else {
            eg.extract_value_to_string(sort, v)?.0
        };
        let i = values.len();
        ids.insert(key, i);
        values.push(Vertex {
            sort: sort.name().to_string(),
            literal,
        });
        terms.push(term);
        Ok(i)
    };
    let mut rows = vec![];
    for op in &env.ops {
        let f = eg.get_function(op).unwrap();
        let sorts: Vec<_> = f
            .schema()
            .input
            .iter()
            .chain(std::iter::once(&f.schema().output))
            .cloned()
            .collect();
        let mut raw = vec![];
        eg.function_for_each(op, |r| raw.push(r.vals.to_vec()))?;
        for row in raw {
            let mut v = row
                .into_iter()
                .zip(&sorts)
                .map(|(v, s)| vertex(s, v))
                .collect::<Result<Vec<_>>>()?;
            let result = v.pop().unwrap();
            rows.push(Row {
                op: op.clone(),
                args: v,
                result,
            });
        }
    }
    drop(vertex);
    let math = eg.get_arcsort_by(|s| s.name() == env.sort);
    let mut ports = BTreeMap::new();
    for (name, term) in parameters
        .iter()
        .enumerate()
        .map(|(i, t)| (format!("p{i}"), t.as_str()))
        .chain(std::iter::once(("result".into(), result)))
    {
        let v = eval(eg, term)?;
        let key = (env.sort.clone(), eg.get_canonical_value(v, &math));
        let id = *ids.entry(key).or_insert_with(|| {
            let i = values.len();
            values.push(Vertex {
                sort: env.sort.clone(),
                literal: None,
            });
            terms.push(term.into());
            i
        });
        ports.insert(name, id);
    }
    rows.sort();
    rows.dedup();
    let state = State {
        version: 2,
        local_ids: vec![],
        scope: env.scope.clone(),
        values,
        rows,
        ports,
        subsumed_rows: vec![],
    };
    Ok(Family {
        eg: eg.clone(),
        state,
        terms,
        parameters,
    })
}
fn intern(f: Family, families: &mut Vec<Family>) -> Result<(usize, bool)> {
    for (i, old) in families.iter().enumerate() {
        if matches!(
            compare(&f.state, &old.state)?,
            Comparison::Equivalent { .. }
        ) {
            return Ok((i, false));
        }
    }
    let i = families.len();
    families.push(f);
    Ok((i, true))
}
/// Enumerate existing-class assignments plus canonical fresh-variable partitions.
fn bindings(
    n: usize,
    k: usize,
    prefix: &mut Vec<usize>,
    fresh: usize,
    f: &mut impl FnMut(&[usize], usize) -> Result,
) -> Result {
    if prefix.len() == k {
        return f(prefix, fresh);
    }
    for choice in 0..=n + fresh {
        prefix.push(choice);
        bindings(n, k, prefix, fresh + usize::from(choice == n + fresh), f)?;
        prefix.pop();
    }
    Ok(())
}
fn merge(
    left: &Family,
    right: &Family,
    binding: &[usize],
    fresh: usize,
    env: &Environment,
) -> Result<(Family, usize, bool)> {
    let mut eg = left.eg.clone();
    let destinations: Vec<_> = left
        .state
        .values
        .iter()
        .enumerate()
        .filter(|(_, v)| v.sort == env.sort)
        .map(|(i, _)| left.terms[i].clone())
        .collect();
    let mut parameters = left.parameters.clone();
    let mut targets = destinations;
    for _ in 0..fresh {
        let t = format!("(__CcParameter {})", parameters.len());
        eval(&mut eg, &t)?;
        parameters.push(t.clone());
        targets.push(t);
    }
    let mut right_parameters = vec![];
    for i in 0..right.parameters.len() {
        let id = right.state.ports[&format!("p{i}")];
        if !right_parameters.contains(&id) {
            right_parameters.push(id)
        }
    }
    let mut terms = vec![String::new(); right.state.values.len()];
    for (&v, &target) in right_parameters.iter().zip(binding) {
        terms[v] = targets[target].clone();
    }
    let mut allocation = eg.get_size("__CcNode");
    for (i, v) in right.state.values.iter().enumerate() {
        if terms[i].is_empty() {
            terms[i] = if let Some(l) = &v.literal {
                l.clone()
            } else {
                let t = format!("(__CcNode {allocation})");
                allocation += 1;
                eval(&mut eg, &t)?;
                t
            };
        }
    }
    for r in &right.state.rows {
        let call = format!(
            "({}{})",
            r.op,
            r.args
                .iter()
                .map(|i| format!(" {}", terms[*i]))
                .collect::<String>()
        );
        eg.parse_and_run_program(None, &format!("(union {} {call})", terms[r.result]))?;
    }
    let result = &terms[right.state.ports["result"]];
    let before = snapshot(&mut eg, env, parameters.clone(), result)?;
    let sweeps = saturate(&mut eg, env)?;
    let after = snapshot(&mut eg, env, parameters, result)?;
    let changed = !matches!(
        compare(&before.state, &after.state)?,
        Comparison::Equivalent { .. }
    );
    Ok((after, sweeps, changed))
}
#[derive(Serialize, Default)]
struct Generation {
    generation: usize,
    pairs: usize,
    bindings: usize,
    new_families: usize,
    changed: usize,
    max_parameter_classes: usize,
    max_provider_classes: usize,
    seconds: f64,
}
pub fn run(rules: &Path, seeds: &Path, out: &Path, generations: usize) -> Result {
    let existed = out.exists();
    let result = execute(rules, seeds, out, generations);
    if let Err(e) = &result {
        if !existed && out.is_dir() {
            let mut progress = std::fs::read(out.join("progress.json"))
                .ok()
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                .unwrap_or(json!({}));
            progress["status"] = json!("error");
            progress["reason"] = json!(e.to_string());
            write_json(&out.join("progress.json"), &progress)?;
        }
    }
    result
}
fn execute(rules: &Path, seeds: &Path, out: &Path, generations: usize) -> Result {
    if out.exists() {
        return Err("CC output directory must be new".into());
    }
    let source = std::fs::read_to_string(rules)?;
    let seeds: Vec<Seed> = serde_json::from_slice(&std::fs::read(seeds)?)?;
    let env = environment(&source)?;
    std::fs::create_dir_all(out.join("families"))?;
    std::fs::write(out.join("rules.egg"), &source)?;
    write_json(&out.join("seeds.json"), &seeds)?;
    let mut csv = std::fs::File::create(out.join("generations.csv"))?;
    writeln!(
        csv,
        "generation,pairs,bindings,new_families,changed,max_parameter_classes,max_provider_classes,seconds"
    )?;
    let mut families = vec![];
    for (seed_index, seed) in seeds.into_iter().enumerate() {
        write_json(
            &out.join("progress.json"),
            &json!({"status":"saturating_seed","seed":seed_index}),
        )?;
        let mut eg = env.base.clone();
        let parameters: Vec<_> = (0..seed.parameters)
            .map(|i| format!("(__CcParameter {i})"))
            .collect();
        for (i, t) in parameters.iter().enumerate() {
            eg.parse_and_run_program(None, &format!("(let p{i} {t})"))?;
        }
        for fact in seed.facts {
            let commands = eg.parse_program(None, &fact)?;
            if commands
                .iter()
                .any(|c| !matches!(c, Command::Action(Action::Expr(..) | Action::Union(..))))
            {
                return Err("seed facts may only construct terms or union them".into());
            }
            eg.run_program(commands)?;
        }
        let result = eval(&mut eg, &seed.result)?;
        let sort = eg.get_arcsort_by(|s| s.name() == env.sort);
        let result = eg.extract_value_to_string(&sort, result)?.0;
        saturate(&mut eg, &env)?;
        let f = snapshot(&mut eg, &env, parameters, &result)?;
        intern(f, &mut families)?;
    }
    for (i, f) in families.iter().enumerate() {
        write_json(&out.join(format!("families/{i}.json")), &f.state)?;
    }
    let initial = families.len();
    let mut seen = BTreeSet::new();
    let mut reports = vec![];
    let mut log = std::io::BufWriter::new(std::fs::File::create(out.join("merges.jsonl"))?);
    for generation in 1..=generations {
        let started = Instant::now();
        let width = families.len();
        let mut report = Generation {
            generation,
            ..Default::default()
        };
        for left in 0..width {
            for right in 0..width {
                if !seen.insert((left, right)) {
                    continue;
                }
                report.pairs += 1;
                let n = families[left]
                    .state
                    .values
                    .iter()
                    .filter(|v| v.sort == env.sort)
                    .count();
                let k = (0..families[right].parameters.len())
                    .map(|i| families[right].state.ports[&format!("p{i}")])
                    .collect::<BTreeSet<_>>()
                    .len();
                report.max_parameter_classes = report.max_parameter_classes.max(k);
                report.max_provider_classes = report.max_provider_classes.max(n);
                bindings(n, k, &mut vec![], 0, &mut |binding, fresh| {
                    write_json(
                        &out.join("progress.json"),
                        &json!({"status":"running","generation":generation,"left":left,"right":right,"binding":binding,"completed_merges":report.bindings}),
                    )?;
                    let (f, sweeps, changed) =
                        merge(&families[left], &families[right], binding, fresh, &env)?;
                    let facts = f.state.rows.len();
                    let (id, new) = intern(f, &mut families)?;
                    report.bindings += 1;
                    report.new_families += usize::from(new);
                    report.changed += usize::from(changed);
                    if new {
                        write_json(
                            &out.join(format!("families/{id}.json")),
                            &families[id].state,
                        )?;
                    }
                    writeln!(
                        log,
                        "{}",
                        json!({"generation":generation,"left":left,"right":right,"binding":binding,"fresh":fresh,"family":id,"new":new,"sweeps":sweeps,"changed":changed,"facts":facts})
                    )?;
                    log.flush()?;
                    Ok(())
                })?;
            }
        }
        report.seconds = started.elapsed().as_secs_f64();
        writeln!(
            csv,
            "{},{},{},{},{},{},{},{:.9}",
            report.generation,
            report.pairs,
            report.bindings,
            report.new_families,
            report.changed,
            report.max_parameter_classes,
            report.max_provider_classes,
            report.seconds
        )?;
        csv.flush()?;
        reports.push(report);
        write_json(
            &out.join("summary.json"),
            &json!({"status":"running","initial_families":initial,"families":families.len(),"generations":reports}),
        )?;
    }
    write_json(
        &out.join("summary.json"),
        &json!({"status":"complete","initial_families":initial,"families":families.len(),"generations":reports,"scope":"all new ordered family pairs per generation; all parameter-to-class/fresh assignments including aliases; no inference cutoff; constructor-only experiment, not production discovery"}),
    )?;
    write_json(&out.join("progress.json"), &json!({"status":"complete"}))?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binding_counts_include_aliases() {
        let mut n = 0;
        bindings(3, 2, &mut vec![], 0, &mut |_, _| {
            n += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(n, 17);
        let mut n = 0;
        bindings(3, 3, &mut vec![], 0, &mut |_, _| {
            n += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(n, 77);
    }
}
